//! Package helpers of the signing line (spec 2026-11-23 §7, §10.2, §17.11).
//!
//! Signing works on the entries of the package the document carries, and
//! never copies them: what it hashes it reads where it lies. A package that
//! has no signature yet receives what it needs when the first one lands,
//! because a signed document can no longer be changed: the ID reserve in
//! Document.xml, an annotation entry (naming the page annotation files the
//! package already carries), and the signature entry in OFD.xml. Those are
//! made as patches first, so that everything can be hashed after them and
//! before any part of the new signature exists; nothing is written until the
//! signature lands. Page annotation files are never among the changes.

use std::collections::BTreeSet;
use std::sync::Arc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use rofd_dom::{OfdDocument, PkgEntry};
use rofd_io::serialize::annotation::object_id_seed;
use rofd_io::serialize::annotation_entry::serialize_annotations_entry_with;
use rofd_io::serialize::patch::{with_annotations_loc, with_max_unit_id, with_signatures_loc};
use rofd_io::{EntryKind, PackageHandle};

use crate::error::SignError;
use crate::ses_encoder::check_value;
use crate::signature_xml::{escape, Reference};
use crate::verify::attr;

// ---- paths and entries ----

/// The directory part of a package path; empty for a name at the root.
pub(crate) fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

/// `relative` below `dir`.
pub(crate) fn join(dir: &str, relative: &str) -> String {
    if dir.is_empty() {
        relative.to_owned()
    } else {
        format!("{dir}/{relative}")
    }
}

/// `path` expressed relative to `dir`, through `..` when `path` does not lie
/// below it.
fn relative_to(path: &str, dir: &str) -> String {
    rofd_io::paths::relative_to(path, dir)
}

/// A loc as a package path: a leading `/` is package-absolute, anything
/// else is relative to `base_dir`, the directory of the file that holds the
/// reference; `.` and `..` are resolved.
pub(crate) fn resolve_loc(loc: &str, base_dir: &str) -> String {
    rofd_io::paths::resolve_loc(loc, base_dir)
}

/// Whether `name` lies below `dir` (a directory entry of a zip ends with a
/// slash and counts).
pub(crate) fn under(name: &str, dir: &str) -> bool {
    name.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// One entry written into a package: what a patch to an existing entry
/// replaces it with, or a new entry.
#[derive(Debug, Clone)]
pub(crate) struct Patch {
    pub name: String,
    pub kind: EntryKind,
    pub bytes: Vec<u8>,
    /// Whether parse reads the entry into the model (the full write must
    /// not carry it through a second time).
    pub consumed: bool,
}

/// Replace an entry, keeping its position, or append it.
pub(crate) fn set_entry(package: &mut PackageHandle, patch: Patch) {
    let Patch {
        name,
        kind,
        bytes,
        consumed,
    } = patch;
    match package.entries.iter_mut().find(|entry| entry.name == name) {
        Some(entry) => entry.bytes = Arc::new(bytes),
        None => package.entries.push(PkgEntry {
            name: name.clone(),
            kind,
            bytes: Arc::new(bytes),
        }),
    }
    if consumed {
        package.consumed.insert(name);
    }
}

fn entry_bytes<'a>(entries: &'a [PkgEntry], name: &str) -> Option<&'a [u8]> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.bytes.as_slice())
}

fn entry_text<'a>(entries: &'a [PkgEntry], name: &str) -> Result<&'a str, SignError> {
    let bytes = entry_bytes(entries, name).ok_or_else(|| encode(name, "the entry is missing"))?;
    std::str::from_utf8(bytes).map_err(|_| encode(name, "the entry is not UTF-8 text"))
}

fn invalid(reason: impl Into<String>) -> SignError {
    SignError::InvalidInput {
        reason: reason.into(),
    }
}

fn encode(entry: &str, reason: &str) -> SignError {
    SignError::Encode {
        reason: format!("{entry}: {reason}"),
    }
}

// ---- the signature list ----

/// One `Signature` record of the signature list, as the file writes it.
#[derive(Debug, Clone)]
pub(crate) struct ListRecord {
    pub id: String,
    /// The `Type` attribute as written; absent means `Seal`.
    pub kind: Option<String>,
    pub base_loc: String,
}

/// The signature list (`Signatures.xml`) as the package holds it.
#[derive(Debug, Default)]
pub(crate) struct SignatureList {
    pub records: Vec<ListRecord>,
    /// `MaxSignId` as written; `None` when the element is absent.
    pub max_sign_id: Option<String>,
}

fn read_signature_list(entries: &[PkgEntry], list_path: &str) -> Result<SignatureList, SignError> {
    let Some(list) = entry_bytes(entries, list_path) else {
        // Declared but not carried: there is nothing to keep.
        return Ok(SignatureList::default());
    };
    let xml =
        std::str::from_utf8(list).map_err(|_| encode(list_path, "the entry is not UTF-8 text"))?;
    parse_signature_list(xml).ok_or_else(|| encode(list_path, "the signature list cannot be read"))
}

/// `None` when the XML is unreadable: a list that cannot be read cannot be
/// rewritten either, so its signatures are left alone.
fn parse_signature_list(xml: &str) -> Option<SignatureList> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut list = SignatureList::default();
    let mut in_max_sign_id = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"Signature" => list.records.push(record(&e)),
                b"MaxSignId" => in_max_sign_id = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.name().local_name().as_ref() == b"Signature" => {
                list.records.push(record(&e))
            }
            Ok(Event::Text(t)) if in_max_sign_id => {
                list.max_sign_id = Some(t.unescape().ok()?.into_owned())
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"MaxSignId" => {
                in_max_sign_id = false
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(list)
}

fn record(e: &BytesStart) -> ListRecord {
    ListRecord {
        id: attr(e, "ID").unwrap_or_default(),
        kind: attr(e, "Type"),
        base_loc: attr(e, "BaseLoc").unwrap_or_default(),
    }
}

/// The signature list with `added` at the end and `max_sign_id` as its
/// `MaxSignId` (left out when empty). Existing records keep the attributes
/// as they were written. `MaxSignId` comes first, as the schema orders it.
pub(crate) fn signature_list_xml(
    list: &SignatureList,
    added: &ListRecord,
    max_sign_id: &str,
) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">",
    );
    if !max_sign_id.is_empty() {
        xml.push_str(&format!(
            "<ofd:MaxSignId>{}</ofd:MaxSignId>",
            escape(max_sign_id)
        ));
    }
    for record in list.records.iter().chain(std::iter::once(added)) {
        xml.push_str("<ofd:Signature");
        if !record.id.is_empty() {
            xml.push_str(&format!(" ID=\"{}\"", escape(&record.id)));
        }
        if let Some(kind) = &record.kind {
            xml.push_str(&format!(" Type=\"{}\"", escape(kind)));
        }
        xml.push_str(&format!(" BaseLoc=\"{}\"/>", escape(&record.base_loc)));
    }
    xml.push_str("</ofd:Signatures>");
    xml
}

// ---- layout ----

/// Where the signatures of a package live.
pub(crate) struct Layout {
    /// Package path of the signature list.
    pub list_path: String,
    /// Its directory. New signatures go into `Sign_n` below it.
    pub sign_dir: String,
    /// Whether `sign_dir` holds signatures only. It does not when it is the
    /// package root or the document's own directory.
    pub dedicated_dir: bool,
    /// The list as the package holds it.
    pub list: SignatureList,
}

impl Layout {
    pub(crate) fn of(package: &PackageHandle) -> Result<Self, SignError> {
        let document_entry = package
            .document_entry
            .as_deref()
            .ok_or_else(|| invalid("the package declares no document"))?;
        let document_dir = parent_dir(document_entry);
        let list_path = package
            .signatures_entry
            .clone()
            .unwrap_or_else(|| join(&document_dir, "Signs/Signatures.xml"));
        let sign_dir = parent_dir(&list_path);
        let list = read_signature_list(&package.entries, &list_path)?;
        Ok(Self {
            dedicated_dir: !sign_dir.is_empty() && sign_dir != document_dir,
            list_path,
            sign_dir,
            list,
        })
    }
}

/// The smallest `n` for which no entry named by `names` is under
/// `{sign_dir}/Sign_{n}/`.
pub(crate) fn smallest_free_sign_index<'a>(
    names: impl IntoIterator<Item = &'a str>,
    sign_dir: &str,
) -> usize {
    let prefix = join(sign_dir, "Sign_");
    let used: BTreeSet<usize> = names
        .into_iter()
        .filter_map(|name| {
            let rest = name.strip_prefix(&prefix)?;
            let digits: usize = rest.chars().take_while(char::is_ascii_digit).count();
            let number = rest[..digits].parse().ok()?;
            (digits > 0 && rest[digits..].starts_with('/')).then_some(number)
        })
        .collect();
    (0..).find(|n| !used.contains(n)).unwrap_or(0)
}

/// What a package with no signature yet receives when the first one lands
/// (spec §10.2 step 2).
#[derive(Debug)]
pub(crate) struct Scaffolds {
    /// The entries to replace or add.
    pub patches: Vec<Patch>,
    /// The annotation entry the document comes to declare.
    pub annotations_entry: Option<String>,
}

/// What [`scaffolds`] found for a package.
pub(crate) enum Scaffold {
    Ready(Scaffolds),
    /// A patch to Document.xml has no anchor. Nothing protects that file (a
    /// package with no signature is the only one patched), so the document
    /// can be given a generated package first; `sign` of bytes reports the
    /// error as it always did.
    NoAnchor(SignError),
}

/// What a package that has no signature yet needs and could no longer
/// receive once it has one, as patches - the package itself is not touched:
///
/// - the annotation entry, declared and carried, when the document has none.
///   A list the package carries names its page annotation files wherever
///   they are, so annotations written in the old layout (a file in the page
///   directory, no list) do not disappear behind an empty one;
/// - the ID reserve in `MaxUnitID`;
/// - the signature entry in OFD.xml, when DocBody does not declare one.
///
/// Page annotation files themselves are never among the patches.
pub(crate) fn scaffolds(
    package: &PackageHandle,
    document: &OfdDocument,
    layout: &Layout,
) -> Result<Scaffold, SignError> {
    let document_entry = package
        .document_entry
        .as_deref()
        .ok_or_else(|| invalid("the package declares no document"))?;
    let document_dir = parent_dir(document_entry);
    let manifest_path = package
        .annotations_entry
        .clone()
        .unwrap_or_else(|| join(&document_dir, "Annots/Annotations.xml"));
    let mut patches = Vec::new();

    let mut patched = entry_text(&package.entries, document_entry)?.to_owned();
    if package.annotations_entry.is_none() {
        let loc = relative_to(&manifest_path, &document_dir);
        patched = match with_annotations_loc(&patched, &loc) {
            Some(patched) => patched,
            None => {
                return Ok(Scaffold::NoAnchor(encode(
                    document_entry,
                    "no place to declare the annotation entry file",
                )))
            }
        };
    }
    let reserve = object_id_seed(document).saturating_add(crate::sign::SIGNATURE_ID_RESERVE);
    patched = match with_max_unit_id(&patched, reserve) {
        Some(patched) => patched,
        None => {
            return Ok(Scaffold::NoAnchor(encode(
                document_entry,
                "MaxUnitID is missing or is not an integer",
            )))
        }
    };
    patches.push(Patch {
        name: document_entry.to_owned(),
        kind: EntryKind::of(document_entry),
        bytes: patched.into_bytes(),
        consumed: false,
    });

    // The entry file, when the package does not carry it: declared or not,
    // the document needs one before anything protects Document.xml.
    if entry_bytes(&package.entries, &manifest_path).is_none() {
        let manifest_dir = parent_dir(&manifest_path);
        let pages: Vec<(rofd_dom::PageId, String)> = package
            .annotation_files
            .iter()
            .map(|(id, path)| (id.clone(), relative_to(path, &manifest_dir)))
            .collect();
        patches.push(Patch {
            name: manifest_path.clone(),
            kind: EntryKind::of(&manifest_path),
            bytes: serialize_annotations_entry_with(&pages).into_bytes(),
            consumed: true,
        });
    }

    if !package.signatures_declared {
        let ofd_xml = entry_text(&package.entries, "OFD.xml")?;
        let patched = with_signatures_loc(ofd_xml, &layout.list_path)
            .ok_or_else(|| encode("OFD.xml", "no place to declare the signature list"))?;
        patches.push(Patch {
            name: "OFD.xml".to_owned(),
            kind: EntryKind::of("OFD.xml"),
            bytes: patched.into_bytes(),
            consumed: false,
        });
    }

    Ok(Scaffold::Ready(Scaffolds {
        patches,
        // The entry the document declares once the patch has landed:
        // what it declared, or the declaration just made.
        annotations_entry: Some(manifest_path),
    }))
}

// ---- references ----

/// What a new signature does not protect (spec I4, GB/T 48666-2026 8 c):
/// the signatures and the annotation layer.
pub(crate) struct Unprotected {
    pub list_path: String,
    /// `sign_dir`, when it holds signatures only.
    pub signature_dir: Option<String>,
    /// The directories holding a signature, when `sign_dir` is shared with
    /// the document's own files and cannot be left out as a whole.
    pub signature_dirs: Vec<String>,
    /// The annotation entry file and the page annotation files.
    pub annotations: Vec<String>,
}

impl Unprotected {
    fn contains(&self, name: &str) -> bool {
        name == self.list_path
            || self
                .signature_dir
                .as_deref()
                .is_some_and(|dir| under(name, dir))
            || self.signature_dirs.iter().any(|dir| under(name, dir))
            || self.annotations.iter().any(|path| path == name)
    }

    /// What a new signature in the package directory `dir` does not protect.
    pub(crate) fn of(package: &PackageHandle, layout: &Layout, dir: &str) -> Self {
        // A directory shared with the document's files cannot be left out as
        // a whole: only the directories that hold a signature are.
        let signature_dirs = if layout.dedicated_dir {
            Vec::new()
        } else {
            layout
                .list
                .records
                .iter()
                .map(|record| parent_dir(&resolve_loc(&record.base_loc, &layout.sign_dir)))
                .chain(std::iter::once(dir.to_owned()))
                .filter(|dir| !dir.is_empty())
                .collect()
        };
        Self {
            signature_dir: layout.dedicated_dir.then(|| layout.sign_dir.clone()),
            signature_dirs,
            list_path: layout.list_path.clone(),
            annotations: annotation_layer(package),
        }
    }
}

/// The annotation layer of `package` as its layout locates it: the entry
/// file the document declares - or the one it would declare, next to its
/// document - and the page annotation files parse found. A signature leaves
/// exactly these out, and [`crate::in_place::Seen`] compares everything
/// else, so the two must agree. A file that merely looks like an annotation
/// and is not located is in neither list: a signature protects it.
pub(crate) fn annotation_layer(package: &PackageHandle) -> Vec<String> {
    let document_dir = parent_dir(package.document_entry.as_deref().unwrap_or_default());
    let mut annotations = vec![package
        .annotations_entry
        .clone()
        .unwrap_or_else(|| join(&document_dir, "Annots/Annotations.xml"))];
    annotations.extend(
        package
            .annotation_files
            .iter()
            .map(|(_, path)| path.clone()),
    );
    annotations
}

/// The reference list of a new signature: every entry but the unprotected
/// ones, by path. The entries are borrowed - the digest is taken where the
/// bytes lie.
///
/// Directory entries (names that end in a slash) hold no file, so they are
/// not listed: strict readers take a `FileRef` for a file and reject one
/// that names a directory.
pub(crate) fn references(entries: &[(&str, &[u8])], unprotected: &Unprotected) -> Vec<Reference> {
    let is_file = |name: &str| !name.ends_with('/') && !unprotected.contains(name);
    let mut references: Vec<Reference> = entries
        .iter()
        .filter(|(name, _)| is_file(name))
        .map(|(name, bytes)| Reference {
            file_ref: format!("/{name}"),
            check_value_base64: check_value(bytes),
        })
        .collect();
    references.sort_by(|a, b| a.file_ref.cmp(&b.file_ref));
    references
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle(names: &[&str]) -> PackageHandle {
        PackageHandle {
            entries: names
                .iter()
                .map(|name| PkgEntry {
                    name: (*name).to_owned(),
                    kind: EntryKind::of(name),
                    bytes: Arc::new(name.as_bytes().to_vec()),
                })
                .collect(),
            ..PackageHandle::default()
        }
    }

    #[test]
    fn paths_join_and_test_containment() {
        assert_eq!(parent_dir("Doc_0/Signs/Signatures.xml"), "Doc_0/Signs");
        assert_eq!(parent_dir("OFD.xml"), "");
        assert_eq!(
            join("Doc_0", "Signs/Signatures.xml"),
            "Doc_0/Signs/Signatures.xml"
        );
        assert_eq!(join("", "Sign_0"), "Sign_0");
        assert_eq!(
            relative_to("Doc_0/Annots/Annotations.xml", "Doc_0"),
            "Annots/Annotations.xml"
        );
        assert_eq!(relative_to("Annotations.xml", ""), "Annotations.xml");
        assert_eq!(
            resolve_loc("/Doc_0/Document.xml", "Doc_0"),
            "Doc_0/Document.xml"
        );
        assert_eq!(
            resolve_loc("Sign_0/Signature.xml", "Doc_0/Signs"),
            "Doc_0/Signs/Sign_0/Signature.xml"
        );
        assert!(under("Doc_0/Signs/Sign_0/Seal.esl", "Doc_0/Signs"));
        assert!(under("Doc_0/Signs/", "Doc_0/Signs"), "a directory entry");
        assert!(!under("Doc_0/Signs_extra/x", "Doc_0/Signs"));
    }

    #[test]
    fn setting_an_entry_keeps_the_position_and_add_appends() {
        let mut package = handle(&["a", "b"]);
        set_entry(
            &mut package,
            Patch {
                name: "a".to_owned(),
                kind: EntryKind::Other,
                bytes: b"9".to_vec(),
                consumed: false,
            },
        );
        set_entry(
            &mut package,
            Patch {
                name: "c".to_owned(),
                kind: EntryKind::Annotation,
                bytes: b"3".to_vec(),
                consumed: true,
            },
        );
        let shape: Vec<(&str, &[u8])> = package
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.bytes.as_slice()))
            .collect();
        assert_eq!(
            shape,
            vec![("a", &b"9"[..]), ("b", &b"b"[..]), ("c", &b"3"[..])]
        );
        assert_eq!(package.consumed.iter().collect::<Vec<_>>(), vec!["c"]);
    }

    #[test]
    fn the_smallest_free_index_skips_used_and_unrelated_names() {
        let names = [
            "Doc_0/Signs/Sign_0/Signature.xml",
            "Doc_0/Signs/Sign_2/Signature.xml",
            "Doc_0/Signs/Sign_x/Signature.xml",
            "Doc_0/Signs/Signatures.xml",
            "Doc_0/Signs/Sign_1_rofd/x",
        ];
        assert_eq!(
            smallest_free_sign_index(names.iter().copied(), "Doc_0/Signs"),
            1
        );
        assert_eq!(smallest_free_sign_index([], ""), 0);
        // At the package root; a number with leading zeros still counts.
        assert_eq!(
            smallest_free_sign_index(["Sign_0/Signature.xml", "Sign_03/x"], ""),
            1
        );
        assert_eq!(smallest_free_sign_index(["Sign_007/x", "Sign_6/x"], ""), 0);
    }

    #[test]
    fn the_list_round_trips_records_as_written() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_0/Signature.xml\"/>\
<ofd:Signature ID=\"s2\" BaseLoc=\"Sign_1/Signature.xml\"/>\
<ofd:MaxSignId>s1</ofd:MaxSignId></ofd:Signatures>";
        let list = parse_signature_list(xml).expect("read");
        assert_eq!(list.records.len(), 2);
        assert_eq!(list.records[1].id, "s2");
        assert_eq!(list.records[1].kind, None);
        assert_eq!(list.max_sign_id.as_deref(), Some("s1"));
        assert!(
            parse_signature_list("<ofd:Signatures").is_none(),
            "unreadable"
        );

        let added = ListRecord {
            id: "3".to_owned(),
            kind: Some("Seal".to_owned()),
            base_loc: "/Doc_0/Signs/Sign_2/Signature.xml".to_owned(),
        };
        let written = signature_list_xml(&list, &added, "4");
        assert_eq!(
            written,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:MaxSignId>4</ofd:MaxSignId>\
<ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_0/Signature.xml\"/>\
<ofd:Signature ID=\"s2\" BaseLoc=\"Sign_1/Signature.xml\"/>\
<ofd:Signature ID=\"3\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_2/Signature.xml\"/>\
</ofd:Signatures>"
        );
        // And it reads back the same way.
        let again = parse_signature_list(&written).expect("read");
        assert_eq!(again.max_sign_id.as_deref(), Some("4"));
        assert_eq!(again.records.len(), 3);
    }

    #[test]
    fn references_leave_out_what_they_should_by_path() {
        let entries: Vec<(&str, &[u8])> = [
            "OFD.xml",
            "Doc_0/Document.xml",
            "Doc_0/Annots/Annotations.xml",
            "Doc_0/Annots/P_7.xml",
            "Doc_0/Signs/Signatures.xml",
            "Doc_0/Signs/Sign_0/Signature.xml",
        ]
        .into_iter()
        .map(|name| (name, name.as_bytes()))
        .collect();
        let unprotected = Unprotected {
            list_path: "Doc_0/Signs/Signatures.xml".to_owned(),
            signature_dir: Some("Doc_0/Signs".to_owned()),
            signature_dirs: Vec::new(),
            annotations: vec![
                "Doc_0/Annots/Annotations.xml".to_owned(),
                "Doc_0/Annots/P_7.xml".to_owned(),
            ],
        };
        let refs = references(&entries, &unprotected);
        let paths: Vec<&str> = refs.iter().map(|r| r.file_ref.as_str()).collect();
        assert_eq!(paths, vec!["/Doc_0/Document.xml", "/OFD.xml"]);
        assert_eq!(
            refs[1].check_value_base64,
            check_value(b"OFD.xml"),
            "the digest is of the entry as it is in the package"
        );

        // A shared directory: only the signatures' own directories are left out.
        let shared = Unprotected {
            list_path: "Doc_0/Signatures.xml".to_owned(),
            signature_dir: None,
            signature_dirs: vec!["Doc_0/Sign_0".to_owned(), "Doc_0/Sign_1".to_owned()],
            annotations: unprotected.annotations.clone(),
        };
        let refs = references(&entries, &shared);
        let paths: Vec<&str> = refs.iter().map(|r| r.file_ref.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "/Doc_0/Document.xml",
                "/Doc_0/Signs/Sign_0/Signature.xml",
                "/Doc_0/Signs/Signatures.xml",
                "/OFD.xml"
            ]
        );
    }
}
