pub mod annotation;
pub mod annotation_entry;
pub mod document;
pub mod ofd_xml;
pub mod page;
pub mod raw;
pub mod resource;
pub mod signature;

use std::sync::Arc;

use quick_xml::events::BytesStart;

use rofd_dom::{
    Color, DocMeta, OfdDocument, PageId, RawFragment, Rect, SignatureSet, TextLayoutHints,
};

use crate::error::{LoadReport, OfdError, OfdWarning, ResourceKind};
use crate::package::{EntryKind, PackageHandle, PkgEntry};
use crate::zip_util::read_all_entries;

pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// Parse a Rect from a whitespace-separated `"x y w h"` string.
///
/// Per GB/T 33190, the page-area box elements (`PhysicalBox`, `ApplicationBox`,
/// ...) carry their geometry as element **text content**
/// (e.g. `<ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>`), not attributes.
pub(crate) fn parse_rect_ws(s: &str) -> Rect {
    let n: Vec<f64> = s
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    Rect {
        x: n.first().copied().unwrap_or(0.0),
        y: n.get(1).copied().unwrap_or(0.0),
        w: n.get(2).copied().unwrap_or(0.0),
        h: n.get(3).copied().unwrap_or(0.0),
    }
}

/// Parse a color from a whitespace-separated `"r g b"` (or `"r g b a"`) string -
/// the format of the `Value` attribute on `FillColor`/`StrokeColor`/`Color`
/// (GB/T 33190). Alpha is accepted but currently dropped (`Color::Rgb`).
pub(crate) fn parse_color_value(s: &str) -> Option<Color> {
    let n: Vec<u8> = s
        .split_whitespace()
        .filter_map(|t| t.parse::<u8>().ok())
        .collect();
    match n.len() {
        3 | 4 => Some(Color::Rgb(n[0], n[1], n[2])),
        _ => None,
    }
}

/// Parse an xsd:boolean-style attribute value (`"true"`/`"false"`/`"1"`/`"0"`)
/// such as PathObject's `Stroke`/`Fill` (GB/T 33190 表35). Returns `None` on
/// anything else so an absent/garbage attribute stays "not specified".
pub(crate) fn parse_bool_value(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

pub fn parse_ofd(bytes: &[u8]) -> Result<LoadReport, OfdError> {
    let raw = read_all_entries(bytes)?;
    let mut warnings = Vec::new();
    let mut entries: Vec<PkgEntry> = Vec::with_capacity(raw.len());
    let mut ofd_xml = String::new();
    for (name, data) in &raw {
        let kind = EntryKind::of(name);
        if name == "OFD.xml" {
            ofd_xml = String::from_utf8_lossy(data).into_owned();
        }
        entries.push(PkgEntry {
            name: name.clone(),
            kind,
            bytes: Arc::new(data.clone()),
        });
    }
    let doc_root = ofd_xml::parse_doc_root(&ofd_xml)?;
    // Consumed-entry set for full-write transport filtering (spec D12).
    let mut consumed = std::collections::BTreeSet::new();
    consumed.insert("OFD.xml".to_string());
    consumed.insert(doc_root.clone());
    let mut level = document_level(&doc_root, &ofd_xml, &entries)?;
    let mut doc = OfdDocument {
        meta: level.meta,
        raw_doc_children: std::mem::take(&mut level.header.raw_children),
        raw_common_children: std::mem::take(&mut level.header.raw_common_children),
        raw_ofd_body_children: level.raw_ofd_body_children,
        max_unit_id: level.header.max_unit_id,
        ..OfdDocument::default()
    };
    for pref in &level.header.pages {
        let page_path = join(&doc_root, &pref.base_loc);
        let page_xml = entry_str(&entries, &page_path)?;
        consumed.insert(page_path.clone());
        let page = page::parse_page(pref.id.clone(), &page_xml, &level.header, &mut warnings)?;
        // Template handling: if page.template is Some, emit warning (v1 doesn't expand).
        if page.template.is_some() {
            warnings.push(OfdWarning::MissingFeature {
                feature: "Template".into(),
                entry: page_path.clone(),
            });
        }
        doc.pages.push(page);
    }
    // Resources: DocumentRes.xml / PublicRes.xml carry DrawParams, MultiMedias
    // (images), and Fonts (GB/T 33190). Paths resolve relative to the Res
    // BaseLoc (itself relative to the resource entry's own directory).
    for e in &entries {
        let name = e.name.as_str();
        if name.ends_with("DocumentRes.xml") || name.ends_with("PublicRes.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            let parsed = resource::parse_res(&xml)?;
            consumed.insert(e.name.clone());
            let res_dir = e.name.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let base_dir = if parsed.base_loc.is_empty() {
                res_dir.to_string()
            } else if res_dir.is_empty() {
                parsed.base_loc.clone()
            } else {
                format!("{res_dir}/{}", parsed.base_loc)
            };
            for (id, dp) in parsed.draw_params {
                doc.resources.draw_params.insert(id, dp);
            }
            for (id, media_file, _fmt) in parsed.multimedias {
                let path = if base_dir.is_empty() {
                    media_file
                } else {
                    format!("{base_dir}/{media_file}")
                };
                if let Some(fe) = entries.iter().find(|x| x.name == path) {
                    consumed.insert(fe.name.clone());
                    doc.resources.images.insert(id, fe.bytes.clone());
                } else {
                    warnings.push(OfdWarning::ResourceNotFound {
                        kind: ResourceKind::Image,
                        id: id.0.clone(),
                    });
                }
            }
            for (id, fref, font_file) in parsed.fonts {
                let family_name = fref.family_name.clone();
                doc.resources.fonts.insert(id.clone(), fref);
                if let Some(rel) = font_file {
                    let path = if base_dir.is_empty() {
                        rel
                    } else {
                        format!("{base_dir}/{rel}")
                    };
                    if let Some(fe) = entries.iter().find(|x| x.name == path) {
                        consumed.insert(fe.name.clone());
                        doc.resources.font_data.insert(id, fe.bytes.clone());
                    } else {
                        warnings.push(OfdWarning::FontSubstituted {
                            requested: family_name.unwrap_or_else(|| id.0.clone()),
                            used: "default".into(),
                        });
                    }
                }
            }
        }
    }
    // Resources: Font.xml entries (+ font bytes via FontFile)
    for e in &entries {
        if e.name.ends_with("/Res/Font.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            consumed.insert(e.name.clone());
            let font_dir = e.name.rsplit_once('/').map(|(d, _)| d).unwrap_or(""); // .../Res
            for (id, fref, font_file) in resource::parse_font_res(&xml)? {
                let family_name = fref.family_name.clone();
                doc.resources.fonts.insert(id.clone(), fref);
                if let Some(rel) = font_file {
                    // FontFile is relative to the Res dir.
                    let font_path = if font_dir.is_empty() {
                        rel
                    } else {
                        format!("{font_dir}/{rel}")
                    };
                    if let Some(fe) = entries.iter().find(|x| x.name == font_path) {
                        consumed.insert(fe.name.clone());
                        doc.resources.font_data.insert(id, fe.bytes.clone());
                    } else {
                        warnings.push(OfdWarning::FontSubstituted {
                            requested: family_name.unwrap_or_else(|| id.0.clone()),
                            used: "default".into(),
                        });
                    }
                }
            }
        }
    }
    // Annotations: GB/T 33190 standard path - Document.xml <Annotations> loc
    // -> Annotations.xml entry -> per-page PageAnnot files. Falls back to a
    // scan of entries ending in `/Annotation.xml` (+ warning) when the doc
    // has no annotations_loc (residual old-format files).
    // The located paths are recorded on the package for the preserving save
    // (spec 2026-11-23 §7): it rewrites the annotation entries in place,
    // under their original names.
    let mut annotations_entry: Option<String> = None;
    let mut annotation_files: Vec<(PageId, String)> = Vec::new();
    if let Some(ann_loc) = level.header.annotations_loc.as_deref() {
        // Recorded from the declaration alone: the preserving save writes
        // the entry file where Document.xml says even when the package does
        // not carry it yet.
        let document_dir = doc_root.rsplit_once('/').map_or("", |(dir, _)| dir);
        let entry_path = crate::paths::resolve_loc(ann_loc, document_dir);
        annotations_entry = Some(entry_path.clone());
        if let Some(entry_xml) = entries
            .iter()
            .find(|e| e.name == entry_path)
            .map(|e| String::from_utf8_lossy(&e.bytes).into_owned())
        {
            consumed.insert(entry_path.clone());
            let entry_dir = entry_path.rsplit_once('/').map_or("", |(d, _)| d);
            for pref in annotation_entry::parse_annotations_entry(&entry_xml)? {
                // A page's FileLoc is relative to the entry file's own
                // directory; `.` and `..` are part of a loc (GB/T 33190
                // table 2), so the path is resolved, not joined.
                let page_path = crate::paths::resolve_loc(&pref.file_loc, entry_dir);
                // Match by PageID against the parsed pages.
                if let Some(page) = doc.pages.iter().find(|p| p.id.0 == pref.page_id) {
                    if let Some(fe) = entries.iter().find(|e| e.name == page_path) {
                        consumed.insert(fe.name.clone());
                        annotation_files.push((page.id.clone(), fe.name.clone()));
                        let xml = String::from_utf8_lossy(&fe.bytes).into_owned();
                        let anns = annotation::parse_page_annot_with_warnings(
                            &xml,
                            &page.id,
                            &mut warnings,
                        )?;
                        if !anns.is_empty() {
                            doc.annotations
                                .by_page
                                .entry(page.id.clone())
                                .or_default()
                                .extend(anns);
                        }
                    }
                }
            }
        }
    } else {
        // Backward-compat fallback: scan for /Annotation.xml entries (old format
        // without a document-level Annotations.xml). Emit a warning per the
        // degraded-input policy (AGENTS.md §4.6).
        for e in &entries {
            if e.name.ends_with("/Annotation.xml") {
                warnings.push(OfdWarning::MissingFeature {
                    feature: "standard annotation entry (Annotations.xml)".into(),
                    entry: e.name.clone(),
                });
                let page_idx = e.name.split('/').find_map(|seg| {
                    seg.strip_prefix("Page_")
                        .and_then(|n| n.parse::<usize>().ok())
                });
                if let Some(idx) = page_idx {
                    if let Some(page) = doc.pages.get(idx) {
                        let xml = String::from_utf8_lossy(&e.bytes).into_owned();
                        consumed.insert(e.name.clone());
                        annotation_files.push((page.id.clone(), e.name.clone()));
                        let anns = annotation::parse_page_annot_with_warnings(
                            &xml,
                            &page.id,
                            &mut warnings,
                        )?;
                        if !anns.is_empty() {
                            doc.annotations
                                .by_page
                                .entry(page.id.clone())
                                .or_default()
                                .extend(anns);
                        }
                    }
                }
            }
        }
    }
    // Signature entries are only read, never consumed: a save carries them
    // through byte for byte and the projection is never written back.
    let (signatures, signatures_entry) = signatures_of(&ofd_xml, &entries, &mut warnings);
    doc.signatures = signatures;
    // The document carries its package (spec 2026-11-23 §17.11): a host
    // keeps no second handle, and a save reads it from here.
    doc.package = Some(PackageHandle {
        entries,
        consumed,
        document_entry: Some(doc_root),
        annotations_entry,
        annotation_files,
        signatures_declared: doc.signatures.declared,
        signatures_entry,
    });
    Ok(LoadReport::new(doc, warnings))
}

/// What the model keeps of OFD.xml and Document.xml (spec 2026-11-23 §17.11):
/// the document-level fields of [`OfdDocument`] and the page references,
/// read without a single page.
struct DocumentLevel {
    meta: DocMeta,
    header: document::DocHeader,
    raw_ofd_body_children: Vec<RawFragment>,
}

fn document_level(
    doc_root: &str,
    ofd_xml: &str,
    entries: &[PkgEntry],
) -> Result<DocumentLevel, OfdError> {
    let mut meta = ofd_xml::parse_doc_meta(ofd_xml)?;
    // Passthrough raws from OFD.xml (DocBody/DocInfo level, spec §6.2).
    let (ofd_body_raws, ofd_info_raws) = ofd_xml::parse_ofd_xml_raws(ofd_xml);
    let doc_xml = entry_str(entries, doc_root)?;
    let header = document::parse_document(&doc_xml)?;
    meta.raw_info_children = ofd_info_raws;
    Ok(DocumentLevel {
        meta,
        header,
        raw_ofd_body_children: ofd_body_raws,
    })
}

/// The projection of the signature entries, and the package path of the list
/// OFD.xml declares. Signature entries are only read, never consumed: a save
/// carries them through byte for byte and the projection is never written
/// back.
fn signatures_of(
    ofd_xml: &str,
    entries: &[PkgEntry],
    warnings: &mut Vec<OfdWarning>,
) -> (SignatureSet, Option<String>) {
    let signatures_entry =
        ofd_xml::parse_signatures_loc(ofd_xml).map(|loc| signature::resolve_loc(&loc, ""));
    // The declaration alone tells the editor and the preserving save that
    // Document.xml must not change; the signature list fills in the items.
    let mut signatures = SignatureSet {
        declared: signatures_entry.is_some(),
        items: Vec::new(),
    };
    if let Some(loc) = signatures_entry.as_deref() {
        signatures.items = signature::parse_signatures(entries, loc, warnings);
    }
    (signatures, signatures_entry)
}

/// Read again what the model keeps of OFD.xml, Document.xml and the signature
/// entries, from the package the document carries (spec 2026-11-23 §17.11):
/// `meta`, the three passthrough fragment lists, `max_unit_id` and the
/// signature list - and, on the handle, the two flags that say what OFD.xml
/// declares about signatures. Not a single page is read, and nothing else of
/// the document changes. Returns the warnings of the signature projection.
///
/// A signature appended to the package in place calls this, so that the
/// document-level fields, and the handle, are what loading the package again
/// would give.
///
/// Everything that can fail comes before the first field is written. The
/// caller - the landing in rofd-sign - has already put the new package in
/// the document's place when it calls, and on an error it puts the old one
/// back; it must then find the model untouched. Keep every fallible read
/// above the assignments.
pub fn refresh_document_level(document: &mut OfdDocument) -> Result<Vec<OfdWarning>, OfdError> {
    let Some(package) = document.package.as_ref() else {
        return Err(OfdError::Schema {
            entry: "package".into(),
            reason: "the document has no package to read from".into(),
        });
    };
    let ofd_xml = entry_str(&package.entries, "OFD.xml")?;
    let doc_root = ofd_xml::parse_doc_root(&ofd_xml)?;
    let mut warnings = Vec::new();
    let level = document_level(&doc_root, &ofd_xml, &package.entries)?;
    document.meta = level.meta;
    document.raw_doc_children = level.header.raw_children;
    document.raw_common_children = level.header.raw_common_children;
    document.raw_ofd_body_children = level.raw_ofd_body_children;
    document.max_unit_id = level.header.max_unit_id;
    let (signatures, signatures_entry) = signatures_of(&ofd_xml, &package.entries, &mut warnings);
    document.signatures = signatures;
    // What a load makes of the handle, too. A package OFD.xml does not
    // declare the signatures of stays that way, whatever was appended to it.
    if let Some(package) = document.package.as_mut() {
        package.signatures_declared = document.signatures.declared;
        package.signatures_entry = signatures_entry;
    }
    Ok(warnings)
}

/// A package the full write made, as a handle, with what reading that
/// package back warned about (spec 2026-11-23 §17.13).
#[derive(Debug)]
pub struct GeneratedPackage {
    pub package: PackageHandle,
    /// The warnings of parsing the package that was written. They are the
    /// caller's to pass on: whatever the model carries that rofd degrades
    /// when it reads it, the caller hears it from here and from nowhere else.
    pub warnings: Vec<OfdWarning>,
}

/// The package a full write of `document` produces, as a handle (spec
/// 2026-11-23 §17.11): for a document that has no package, or none that can
/// take what has to be patched into Document.xml. What the model does not
/// carry passes through from the document's own package, when it has one.
/// The package is read back to find its layout, and what that reading warns
/// about is returned with it.
pub fn generate_package(
    document: &OfdDocument,
    text_layout: &TextLayoutHints,
) -> Result<GeneratedPackage, OfdError> {
    let bytes = crate::serialize::package::write_ofd_with_text_layout(
        document,
        document.package.as_ref(),
        text_layout,
    )?;
    let LoadReport {
        document: mut parsed,
        warnings,
    } = parse_ofd(&bytes)?;
    let package = parsed.package.take().ok_or_else(|| OfdError::Schema {
        entry: "package".into(),
        reason: "a parsed document carries its package".into(),
    })?;
    Ok(GeneratedPackage { package, warnings })
}

fn entry_str(entries: &[PkgEntry], name: &str) -> Result<String, OfdError> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| String::from_utf8_lossy(&e.bytes).into_owned())
        .ok_or_else(|| OfdError::Schema {
            entry: name.into(),
            reason: "entry missing".into(),
        })
}

fn join(doc_root: &str, base_loc: &str) -> String {
    // base_loc is relative to the document's directory.
    let dir = doc_root.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    if dir.is_empty() {
        base_loc.to_string()
    } else {
        format!("{dir}/{base_loc}")
    }
}
