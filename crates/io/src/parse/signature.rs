//! Read-only projection of the `Signs/` entries (spec 2026-11-23 §8.2).
//!
//! From OFD.xml's `Signatures` declaration down to each signature's
//! placements and seal picture. The entries are only read: they never enter
//! the consumed set, so a save carries them through byte for byte, and the
//! projection is never written back.
//!
//! Real packages deviate from the textbook layout in several ways, all
//! tolerated here: relative or absolute locs, a non-numeric `MaxSignId` or
//! `Signature@ID`, several stamps sharing one ID, and a seal that exists only
//! inside the signature value. Anything unreadable costs a warning, never
//! the document.

use std::sync::Arc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use rofd_dom::{OfdWarning, PageId, Rect, SealPicture, SignatureInfo, SignatureKind, StampAnnot};

use crate::der::{self, TAG_IA5_STRING, TAG_OCTET_STRING, TAG_SEQUENCE};
use crate::package::PkgEntry;
use crate::parse::attr;

/// `loc` as a package path (no leading slash): a leading `/` means
/// package-absolute; anything else is relative to `base_dir`, the directory
/// of the file that contains the reference. `.` and `..` are resolved.
pub(crate) fn resolve_loc(loc: &str, base_dir: &str) -> String {
    crate::paths::resolve_loc(loc, base_dir)
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn entry_bytes<'a>(entries: &'a [PkgEntry], path: &str) -> Option<&'a [u8]> {
    entries
        .iter()
        .find(|entry| entry.name == path)
        .map(|entry| entry.bytes.as_slice())
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The seal picture of an ESS_Seal, when it is a PNG.
pub fn seal_picture_from_der(der: &[u8]) -> Option<SealPicture> {
    picture_in(&der::parse_tree(der)?)
}

/// ESS_Seal ::= SEQUENCE { eSealInfo SEQUENCE { header, id, property,
/// Picture SEQUENCE { type IA5String, data OCTET STRING, width, height } },
/// certificate, algorithm, signature }.
fn picture_in(seal: &der::Node<'_>) -> Option<SealPicture> {
    let picture = seal.children.first()?.children.get(3)?;
    let kind = picture.children.first()?;
    let data = picture.children.get(1)?;
    if kind.tag != TAG_IA5_STRING || data.tag != TAG_OCTET_STRING {
        return None;
    }
    if !kind.content.eq_ignore_ascii_case(b"png") {
        return None;
    }
    if !data.content.starts_with(&[0x89, b'P', b'N', b'G']) {
        return None;
    }
    let dimension = |node: &der::Node<'_>| der::unsigned_integer(node.content).unwrap_or(0) as f64;
    Some(SealPicture {
        png: Arc::new(data.content.to_vec()),
        width: picture.children.get(2).map_or(0.0, dimension),
        height: picture.children.get(3).map_or(0.0, dimension),
    })
}

/// The seal embedded in an SES signature: both SES v1 and v4 start the
/// to-be-signed part with the version and the seal.
fn embedded_seal_picture(signed_value: &[u8]) -> Option<SealPicture> {
    let root = der::parse_tree(signed_value)?;
    let seal = root.children.first()?.children.get(1)?;
    (seal.tag == TAG_SEQUENCE)
        .then(|| picture_in(seal))
        .flatten()
}

struct ListEntry {
    id: String,
    kind: SignatureKind,
    base_loc: String,
}

/// The `Signature` records of the signature list.
fn parse_signature_list(xml: &str) -> Option<Vec<ListEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().local_name().as_ref() == b"Signature" =>
            {
                let kind = match attr(&e, "Type").as_deref() {
                    Some("Sign") => SignatureKind::Sign,
                    _ => SignatureKind::Seal,
                };
                out.push(ListEntry {
                    id: attr(&e, "ID").unwrap_or_default(),
                    kind,
                    base_loc: attr(&e, "BaseLoc").unwrap_or_default(),
                });
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(out)
}

#[derive(Default)]
struct Description {
    stamps: Vec<StampAnnot>,
    /// Loc of SignedValue.dat, as written.
    signed_value_loc: Option<String>,
    /// Loc of Seal.esl, as written.
    seal_loc: Option<String>,
}

fn box_attr(value: &str) -> Option<Rect> {
    let mut numbers = value.split_whitespace();
    let mut next = || numbers.next()?.parse::<f64>().ok();
    let rect = Rect {
        x: next()?,
        y: next()?,
        w: next()?,
        h: next()?,
    };
    (numbers.next().is_none()).then_some(rect)
}

fn stamp_annot(e: &BytesStart) -> Option<StampAnnot> {
    Some(StampAnnot {
        page_id: PageId::new(attr(e, "PageRef")?),
        boundary: box_attr(&attr(e, "Boundary")?)?,
        clip: attr(e, "Clip").and_then(|clip| box_attr(&clip)),
    })
}

fn parse_signature_description(xml: &str) -> Option<Description> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut description = Description::default();
    let mut in_seal = false;
    // Which element's text the next Text event belongs to.
    let mut text_target = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"StampAnnot" => {
                    if let Some(stamp) = stamp_annot(&e) {
                        description.stamps.push(stamp);
                    }
                }
                b"Seal" => in_seal = true,
                b"SignedValue" => text_target = Some("SignedValue"),
                b"BaseLoc" if in_seal => text_target = Some("BaseLoc"),
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                let text = text.trim().to_owned();
                match text_target {
                    Some("SignedValue") => description.signed_value_loc = Some(text),
                    Some("BaseLoc") => description.seal_loc = Some(text),
                    _ => {}
                }
                text_target = None;
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"Seal" => in_seal = false,
                b"SignedValue" | b"BaseLoc" => text_target = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(description)
}

/// Project every signature the list declares into the read-only model.
pub fn parse_signatures(
    entries: &[PkgEntry],
    signatures_loc: &str,
    warnings: &mut Vec<OfdWarning>,
) -> Vec<SignatureInfo> {
    // OFD.xml sits at the package root, so its locs resolve against "".
    let list_path = resolve_loc(signatures_loc, "");
    let Some(list) = entry_bytes(entries, &list_path) else {
        warnings.push(OfdWarning::MissingFeature {
            feature: "signature list".into(),
            entry: list_path,
        });
        return Vec::new();
    };
    let Some(records) = parse_signature_list(&text_of(list)) else {
        warnings.push(OfdWarning::MissingFeature {
            feature: "signature list (unreadable XML)".into(),
            entry: list_path,
        });
        return Vec::new();
    };
    let list_dir = parent_dir(&list_path).to_owned();
    records
        .into_iter()
        .map(|record| {
            let mut info = SignatureInfo {
                id: record.id,
                kind: record.kind,
                base_loc: record.base_loc,
                stamps: Vec::new(),
                seal_picture: None,
            };
            let description_path = resolve_loc(&info.base_loc, &list_dir);
            let Some(description) = entry_bytes(entries, &description_path)
                .map(text_of)
                .and_then(|xml| parse_signature_description(&xml))
            else {
                warnings.push(OfdWarning::MissingFeature {
                    feature: "signature description".into(),
                    entry: description_path,
                });
                return info;
            };
            info.stamps = description.stamps;
            // A signature without a seal has no picture to find.
            let Some(seal_loc) = description.seal_loc.as_deref() else {
                return info;
            };
            let description_dir = parent_dir(&description_path).to_owned();
            // The standalone seal first, then the copy embedded in the
            // signature value.
            let seal_path = resolve_loc(seal_loc, &description_dir);
            if let Some(seal) = entry_bytes(entries, &seal_path) {
                match seal_picture_from_der(seal) {
                    Some(picture) => {
                        info.seal_picture = Some(picture);
                        return info;
                    }
                    None => warnings.push(OfdWarning::MissingFeature {
                        feature: "seal picture (PNG only)".into(),
                        entry: seal_path,
                    }),
                }
            }
            let value_path = description
                .signed_value_loc
                .as_deref()
                .map(|loc| resolve_loc(loc, &description_dir));
            if let Some(value_path) = value_path {
                let found = entry_bytes(entries, &value_path).and_then(embedded_seal_picture);
                match found {
                    Some(picture) => info.seal_picture = Some(picture),
                    None => warnings.push(OfdWarning::MissingFeature {
                        feature: "seal picture".into(),
                        entry: value_path,
                    }),
                }
            }
            info
        })
        .collect()
}
