//! Preserving-save tests (spec 2026-11-23 §7): the editor save rewrites only
//! the annotation entries and keeps every other entry byte-identical, so a
//! signed document can gain annotations without invalidating its signatures.

use std::io::Write;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, OfdDocument, PageId, Rect,
    ShapeKind, TextLayoutHints,
};
use rofd_io::{DocumentXmlChange, Preserved};

// ---- package builders shared by every test in this file ----

/// Build a zip from `(entry name, text content)` pairs, in order.
fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        w.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        w.write_all(content.as_bytes()).expect("write entry");
    }
    w.finish().expect("finish zip").into_inner()
}

/// Every `(name, bytes)` entry of a package, in archive order.
fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    rofd_io::zip_util::read_all_entries(bytes).expect("read entries")
}

/// OFD.xml with an optional `<Signatures>` child on DocBody.
fn ofd_xml(signatures: Option<&str>) -> String {
    let signatures = signatures
        .map(|loc| format!("<ofd:Signatures>{loc}</ofd:Signatures>"))
        .unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody>\
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>{signatures}\
</ofd:DocBody></ofd:OFD>"
    )
}

/// Document.xml with one page (ID 7) and an optional annotation entry loc.
fn document_xml(max_unit_id: u64, annotations_loc: Option<&str>) -> String {
    let annotations = annotations_loc
        .map(|loc| format!("<ofd:Annotations>{loc}</ofd:Annotations>"))
        .unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:CommonData><ofd:MaxUnitID>{max_unit_id}</ofd:MaxUnitID>\
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>\
</ofd:CommonData>\
<ofd:Pages><ofd:Page ID=\"7\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages>\
{annotations}</ofd:Document>"
    )
}

const PAGE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>\
<ofd:Content/></ofd:Page>";

/// Annotation entry file listing one page file under a non-canonical name.
const MANIFEST_P7: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Page PageID=\"7\"><ofd:FileLoc>P_7.xml</ofd:FileLoc></ofd:Page>\
</ofd:Annotations>";

const EMPTY_PAGE_ANNOT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\"></ofd:PageAnnot>";

#[test]
fn package_records_annotation_layout_and_signature_declaration() {
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(Some("/Doc_0/Signs/Signatures.xml"))),
        (
            "Doc_0/Document.xml",
            &document_xml(9, Some("Annots/Annotations.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
        ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
    ]);
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    let pkg = report.document.package.as_ref().expect("package");
    assert_eq!(
        pkg.document_entry.as_deref(),
        Some("Doc_0/Document.xml"),
        "document entry is the DocRoot"
    );
    assert_eq!(
        pkg.annotations_entry.as_deref(),
        Some("Doc_0/Annots/Annotations.xml"),
        "annotation entry loc resolves against the document directory"
    );
    assert_eq!(
        pkg.annotation_files,
        vec![(PageId::new("7"), "Doc_0/Annots/P_7.xml".to_string())],
        "annotation files keep their original names, resolved"
    );
    assert!(
        pkg.signatures_declared,
        "DocBody declares a Signatures entry"
    );
    // Every recorded path addresses a real entry of the package.
    let names: Vec<String> = zip_entries(&bytes)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let recorded = std::iter::once(pkg.annotations_entry.clone())
        .chain(pkg.annotation_files.iter().map(|(_, p)| Some(p.clone())));
    for path in recorded.flatten() {
        assert!(
            names.contains(&path),
            "recorded path {path} is a package entry"
        );
    }
}

#[test]
fn package_without_signatures_is_not_declared() {
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        ("Doc_0/Document.xml", &document_xml(3, None)),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    let pkg = report.document.package.as_ref().expect("package");
    assert!(!pkg.signatures_declared);
    assert_eq!(pkg.annotations_entry, None);
    assert!(pkg.annotation_files.is_empty());
    assert_eq!(pkg.document_entry.as_deref(), Some("Doc_0/Document.xml"));
}

#[test]
fn signatures_declaration_survives_default_namespace_and_relative_loc() {
    // Vendor writers may use the default namespace and a relative loc.
    let ofd = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<OFD xmlns=\"http://www.ofdspec.org/2016\"><DocBody>\
<DocRoot>Doc_0/Document.xml</DocRoot>\
<Signatures>Doc_0/Signs/Signatures.xml</Signatures>\
</DocBody></OFD>";
    let bytes = zip_of(&[
        ("OFD.xml", ofd),
        ("Doc_0/Document.xml", &document_xml(3, None)),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    assert!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .signatures_declared
    );
}

// ---- the preserving writer ----

/// Document.xml with two pages (IDs 7 and 8).
fn document_xml_two_pages(max_unit_id: u64, annotations_loc: Option<&str>) -> String {
    document_xml(max_unit_id, annotations_loc).replace(
        "</ofd:Pages>",
        "<ofd:Page ID=\"8\" BaseLoc=\"Pages/Page_1/Content.xml\"/></ofd:Pages>",
    )
}

const SIGNATURES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_0/Signature.xml\"/>\
</ofd:Signatures>";

/// A package that declares signatures, carrying an opaque `Sign_0` directory.
/// With `annotations_loc` it also has an annotation entry file and one page
/// annotation file under a non-canonical name.
fn signed_package(max_unit_id: u64, annotations_loc: bool) -> Vec<u8> {
    let mut files: Vec<(&str, String)> = vec![
        ("OFD.xml", ofd_xml(Some("/Doc_0/Signs/Signatures.xml"))),
        (
            "Doc_0/Document.xml",
            document_xml(
                max_unit_id,
                annotations_loc.then_some("Annots/Annotations.xml"),
            ),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE.to_owned()),
        ("Doc_0/Signs/Signatures.xml", SIGNATURES_XML.to_owned()),
        (
            "Doc_0/Signs/Sign_0/Signature.xml",
            "<ofd:Signature/>".to_owned(),
        ),
        ("Doc_0/Signs/Sign_0/SignedValue.dat", "opaque".to_owned()),
        ("Doc_0/Signs/Sign_0/Seal.esl", "opaque".to_owned()),
    ];
    if annotations_loc {
        files.push(("Doc_0/Annots/Annotations.xml", MANIFEST_P7.to_owned()));
        files.push(("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT.to_owned()));
    }
    zip_of_owned(&files)
}

/// The same layout without the signature declaration.
fn unsigned_package() -> Vec<u8> {
    zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        (
            "Doc_0/Document.xml",
            &document_xml(9, Some("Annots/Annotations.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
        ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
    ])
}

fn zip_of_owned(files: &[(&str, String)]) -> Vec<u8> {
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(n, c)| (*n, c.as_str())).collect();
    zip_of(&borrowed)
}

/// Like [`zip_of`], with an empty directory entry `dir/` written first.
fn zip_of_with_directory(dir: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.add_directory::<_, ()>(dir, zip::write::SimpleFileOptions::default())
        .expect("add directory");
    for (name, content) in files {
        w.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        w.write_all(content.as_bytes()).expect("write entry");
    }
    w.finish().expect("finish zip").into_inner()
}

/// Insert one rectangle annotation on `page`, the way the editor does: an
/// unsigned document numbers from MaxUnitID, a signed one from the highest
/// id actually in use (Document.xml no longer changes).
fn add_rect(doc: &mut OfdDocument, page: &str) {
    let used = doc
        .annotations
        .by_page
        .values()
        .flatten()
        .filter_map(|a| a.id.0.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let next = if doc.signatures.declared {
        doc.highest_known_object_id() + 1
    } else {
        used.max(doc.max_unit_id) + 1
    };
    insert_rect(doc, page, next);
}

/// Insert one rectangle annotation with an explicit ID.
fn insert_rect(doc: &mut OfdDocument, page: &str, id: u64) {
    doc.annotations.insert(Annotation {
        id: AnnotationId::from_int(id),
        kind: AnnotationKind::Shape(ShapeKind::Rect),
        page: PageId::new(page),
        creator: "tester".into(),
        created: 1_700_000_000_000,
        modified: 1_700_000_000_000,
        reply_to: None,
        payload: AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 30.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        },
    });
}

/// Parse, edit the model, save through the editor entry point.
fn save_after(
    bytes: &[u8],
    edit: impl FnOnce(&mut OfdDocument),
) -> (Preserved, Vec<(String, Vec<u8>)>) {
    let report = rofd_io::parse_ofd(bytes).expect("parse_ofd");
    let mut doc = report.document;
    edit(&mut doc);
    let preserved = rofd_io::save_ofd(&doc, &TextLayoutHints::new()).expect("save_ofd");
    let entries = zip_entries(&preserved.bytes);
    (preserved, entries)
}

fn entry<'a>(entries: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &entries
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("entry {name} present"))
        .1
}

fn entry_text(entries: &[(String, Vec<u8>)], name: &str) -> String {
    String::from_utf8(entry(entries, name).to_vec()).expect("entry is UTF-8")
}

/// Names of the package's annotation entries, by the parser's classification.
fn annotation_entry_names(bytes: &[u8]) -> Vec<String> {
    let report = rofd_io::parse_ofd(bytes).expect("parse_ofd");
    report
        .document
        .package
        .as_ref()
        .expect("package")
        .entries
        .iter()
        .filter(|e| e.kind == rofd_io::EntryKind::Annotation)
        .map(|e| e.name.clone())
        .collect()
}

/// Non-annotation entries of `before` whose bytes differ in `after`.
fn changed_non_annotation_entries(before: &[u8], after: &[(String, Vec<u8>)]) -> Vec<String> {
    let annotations = annotation_entry_names(before);
    zip_entries(before)
        .into_iter()
        .filter(|(name, _)| !annotations.contains(name))
        .filter(|(name, bytes)| {
            after
                .iter()
                .find(|(n, _)| n == name)
                .is_none_or(|(_, after_bytes)| after_bytes != bytes)
        })
        .map(|(name, _)| name)
        .collect()
}

/// The text with MaxUnitID's value masked (whatever the namespace prefix),
/// so two versions that differ only in that number compare equal.
fn mask_max_unit_id(xml: &str) -> String {
    let Some(tag) = xml.find("MaxUnitID") else {
        return xml.to_owned();
    };
    let a = tag + xml[tag..].find('>').expect("MaxUnitID start tag ends") + 1;
    let b = a + xml[a..].find('<').expect("MaxUnitID end tag starts");
    format!("{}N{}", &xml[..a], &xml[b..])
}

fn entry_owned_text(entries: &[(String, Vec<u8>)], name: &str) -> String {
    String::from_utf8(
        entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("entry {name} present"))
            .1
            .clone(),
    )
    .expect("entry is UTF-8")
}

#[test]
fn unsigned_package_without_annotations_saves_byte_identical_entries() {
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        ("Doc_0/Document.xml", &document_xml(9, None)),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let (preserved, entries) = save_after(&bytes, |_| {});
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    assert_eq!(entries, zip_entries(&bytes));
}

#[test]
fn unsigned_no_change_save_touches_only_annotation_entries_and_max_unit_id() {
    let (first, _) = save_after(&unsigned_package(), |doc| add_rect(doc, "7"));
    let (preserved, entries) = save_after(&first.bytes, |_| {});
    assert_eq!(
        changed_non_annotation_entries(&first.bytes, &entries),
        vec!["Doc_0/Document.xml".to_string()]
    );
    let before = entry_text(&zip_entries(&first.bytes), "Doc_0/Document.xml");
    let after = entry_text(&entries, "Doc_0/Document.xml");
    assert_eq!(mask_max_unit_id(&after), mask_max_unit_id(&before));
    assert_eq!(preserved.document_xml, DocumentXmlChange::MaxUnitId);
}

#[test]
fn signed_no_change_save_keeps_every_entry() {
    let (first, _) = save_after(&signed_package(9, true), |doc| add_rect(doc, "7"));
    let (preserved, entries) = save_after(&first.bytes, |_| {});
    assert!(
        changed_non_annotation_entries(&first.bytes, &entries).is_empty(),
        "Document.xml and the signature entries stay byte-identical"
    );
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    let (_, again) = save_after(&preserved.bytes, |_| {});
    assert_eq!(again, entries, "a second no-change save is fully stable");
}

#[test]
fn adding_one_annotation_changes_only_annotation_entries() {
    let bytes = unsigned_package();
    let (preserved, entries) = save_after(&bytes, |doc| add_rect(doc, "7"));
    assert_eq!(
        changed_non_annotation_entries(&bytes, &entries),
        vec!["Doc_0/Document.xml".to_string()]
    );
    let before = entry_text(&zip_entries(&bytes), "Doc_0/Document.xml");
    let after = entry_text(&entries, "Doc_0/Document.xml");
    assert_eq!(mask_max_unit_id(&after), mask_max_unit_id(&before));
    assert_eq!(preserved.document_xml, DocumentXmlChange::MaxUnitId);
    assert!(
        entry_text(&entries, "Doc_0/Annots/P_7.xml").contains("ID=\"10\""),
        "the annotation lands in the page's original file"
    );
}

#[test]
fn first_annotation_inserts_annotations_element_in_schema_order() {
    let doc = document_xml(9, None).replace(
        "</ofd:Document>",
        "<ofd:Extensions>Doc_0/Extensions.xml</ofd:Extensions></ofd:Document>",
    );
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        ("Doc_0/Document.xml", &doc),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let (preserved, entries) = save_after(&bytes, |doc| add_rect(doc, "7"));
    let after = entry_text(&entries, "Doc_0/Document.xml");
    assert_eq!(after.matches("<ofd:Annotations>").count(), 1, "{after}");
    assert!(
        after.find("<ofd:Annotations>").unwrap() < after.find("<ofd:Extensions>").unwrap(),
        "Annotations precedes Extensions: {after}"
    );
    assert!(
        entries
            .iter()
            .any(|(name, _)| name == "Doc_0/Annots/Annotations.xml"),
        "the annotation entry file is created"
    );
    assert_eq!(
        preserved.document_xml,
        DocumentXmlChange::AnnotationsLocInserted
    );
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("saved package re-parses");
    assert_eq!(reparsed.document.annotations.by_page.len(), 1);
}

#[test]
fn max_unit_id_never_decreases() {
    let (first, first_entries) = save_after(&unsigned_package(), |doc| add_rect(doc, "7"));
    let (_, second_entries) = save_after(&first.bytes, |_| {});
    let first_max = rofd_io::max_unit_id_of(&entry_text(&first_entries, "Doc_0/Document.xml"))
        .expect("MaxUnitID");
    let second_max = rofd_io::max_unit_id_of(&entry_text(&second_entries, "Doc_0/Document.xml"))
        .expect("MaxUnitID");
    assert!(second_max >= first_max, "{second_max} < {first_max}");
}

#[test]
fn missing_max_unit_id_falls_back_to_full_write() {
    let doc = document_xml(9, Some("Annots/Annotations.xml"))
        .replace("<ofd:MaxUnitID>9</ofd:MaxUnitID>", "");
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        ("Doc_0/Document.xml", &doc),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
        ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
    ]);
    let (preserved, _) = save_after(&bytes, |doc| add_rect(doc, "7"));
    assert!(preserved.full_write_fallback);
    assert_eq!(preserved.document_xml, DocumentXmlChange::Generated);
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("fallback output re-parses");
    assert_eq!(reparsed.document.annotations.by_page.len(), 1);
}

#[test]
fn saved_package_reparses_to_same_annotations() {
    let report = rofd_io::parse_ofd(&unsigned_package()).expect("parse_ofd");
    let mut doc = report.document;
    add_rect(&mut doc, "7");
    let expected = doc.annotations.clone();
    let preserved = rofd_io::save_ofd(&doc, &TextLayoutHints::new()).expect("save_ofd");
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("saved package re-parses");
    assert_eq!(reparsed.document.annotations, expected);
}

#[test]
fn original_file_loc_names_are_kept_and_new_files_do_not_collide() {
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        (
            "Doc_0/Document.xml",
            &document_xml_two_pages(9, Some("Annots/Annotations.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Pages/Page_1/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
        ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
        (
            "Doc_0/Annots/Page_1/Annotation.xml",
            "not listed in the entry file",
        ),
    ]);
    let (preserved, entries) = save_after(&bytes, |doc| {
        add_rect(doc, "7");
        add_rect(doc, "8");
    });
    assert!(
        entry_text(&entries, "Doc_0/Annots/P_7.xml").contains("ID=\"10\""),
        "page 7 keeps its original file"
    );
    assert!(
        entry_text(&entries, "Doc_0/Annots/Page_1/Annotation.xml_rofd").contains("ID=\"11\""),
        "page 8 gets a file that does not collide with the existing entry"
    );
    assert_eq!(
        entry(&entries, "Doc_0/Annots/Page_1/Annotation.xml"),
        b"not listed in the entry file",
        "the unrelated entry is left alone"
    );
    let manifest = entry_text(&entries, "Doc_0/Annots/Annotations.xml");
    assert!(
        manifest.contains("<ofd:FileLoc>P_7.xml</ofd:FileLoc>"),
        "{manifest}"
    );
    assert!(
        manifest.contains("<ofd:FileLoc>Page_1/Annotation.xml_rofd</ofd:FileLoc>"),
        "{manifest}"
    );
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("saved package re-parses");
    assert_eq!(reparsed.document.annotations.by_page.len(), 2);
}

#[test]
fn directory_entries_survive_preserving_save() {
    let bytes = zip_of_with_directory(
        "Doc_0",
        &[
            ("OFD.xml", &ofd_xml(None)),
            (
                "Doc_0/Document.xml",
                &document_xml(9, Some("Annots/Annotations.xml")),
            ),
            ("Doc_0/Pages/Page_0/Content.xml", PAGE),
            ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
            ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
        ],
    );
    let mut before: Vec<String> = zip_entries(&bytes).into_iter().map(|(n, _)| n).collect();
    before.sort();
    let (_, entries) = save_after(&bytes, |doc| add_rect(doc, "7"));
    let mut after: Vec<String> = entries.into_iter().map(|(n, _)| n).collect();
    after.sort();
    assert_eq!(after, before);
}

#[test]
fn reserve_exhausted_is_reported() {
    // The reserve is used up: the highest id in use (9) reaches the
    // MaxUnitID ceiling of the signed package.
    let (full, _) = save_after(&signed_package(9, true), |doc| insert_rect(doc, "7", 9));
    let report = rofd_io::parse_ofd(&full.bytes).expect("parse_ofd");
    assert_eq!(report.document.highest_known_object_id(), 9);
    assert_eq!(report.document.max_unit_id, 9);

    let (preserved, entries) = save_after(&full.bytes, |doc| add_rect(doc, "7"));
    assert!(preserved.reserve_exhausted);
    assert!(!preserved.blocked_by_signature);
    let before = entry_text(&zip_entries(&full.bytes), "Doc_0/Document.xml");
    assert_eq!(entry_text(&entries, "Doc_0/Document.xml"), before);
}

#[test]
fn signed_without_annotation_entry_is_blocked() {
    let bytes = signed_package(9, false);
    let (preserved, entries) = save_after(&bytes, |doc| add_rect(doc, "7"));
    assert!(preserved.blocked_by_signature);
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    assert_eq!(
        entries,
        zip_entries(&bytes),
        "the package comes back as it was"
    );

    let (preserved, _) = save_after(&bytes, |_| {});
    assert!(
        !preserved.blocked_by_signature,
        "nothing to write, nothing blocked"
    );
}

#[test]
fn no_package_generates_full_write() {
    let report = rofd_io::parse_ofd(&unsigned_package()).expect("parse_ofd");
    let mut bare = report.document.clone();
    bare.package = None;
    let preserved = rofd_io::save_ofd(&bare, &TextLayoutHints::new()).expect("save_ofd");
    assert_eq!(preserved.document_xml, DocumentXmlChange::Generated);
    assert!(!preserved.full_write_fallback);
    rofd_io::parse_ofd(&preserved.bytes).expect("generated package re-parses");
}

#[test]
fn declared_entry_file_missing_is_written_on_the_declared_path() {
    // Document.xml names `Annots/List.xml`, but the package does not carry it.
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        (
            "Doc_0/Document.xml",
            &document_xml(9, Some("Annots/List.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let (preserved, entries) = save_after(&bytes, |doc| add_rect(doc, "7"));
    assert!(
        entries
            .iter()
            .any(|(name, _)| name == "Doc_0/Annots/List.xml"),
        "the entry file is created where Document.xml says"
    );
    assert_eq!(preserved.document_xml, DocumentXmlChange::MaxUnitId);
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("saved package re-parses");
    assert_eq!(reparsed.document.annotations.by_page.len(), 1);
}

/// Real-sample check against the local OFD `test/ru-yuan-ji-lu.ofd`
/// (gitignored; run with `-- --ignored`): a no-change save leaves every
/// non-annotation entry byte-identical, except that an unsigned package's
/// Document.xml may move its MaxUnitID, and the model re-parses equal.
#[test]
#[ignore = "requires the local OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn real_sample_no_change_save_keeps_body_entries() {
    let bytes = std::fs::read("../../test/ru-yuan-ji-lu.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    assert!(
        !report.document.annotations.by_page.is_empty(),
        "sample has annotations"
    );
    let document_entry = report
        .document
        .package
        .as_ref()
        .expect("package")
        .document_entry
        .clone()
        .expect("document entry recorded");
    let preserved = rofd_io::save_ofd(&report.document, &TextLayoutHints::new()).expect("save_ofd");
    assert!(!preserved.full_write_fallback);
    assert!(!preserved.blocked_by_signature);
    let after = zip_entries(&preserved.bytes);
    let changed = changed_non_annotation_entries(&bytes, &after);
    let unexpected: Vec<&String> = changed.iter().filter(|n| *n != &document_entry).collect();
    assert!(
        unexpected.is_empty(),
        "body entries changed: {unexpected:?}"
    );
    if changed.contains(&document_entry) {
        let before_doc = entry_owned_text(&zip_entries(&bytes), &document_entry);
        let after_doc = entry_owned_text(&after, &document_entry);
        assert_eq!(
            mask_max_unit_id(&after_doc),
            mask_max_unit_id(&before_doc),
            "Document.xml differs only in MaxUnitID"
        );
    }
    let reparsed = rofd_io::parse_ofd(&preserved.bytes).expect("saved package re-parses");
    assert_eq!(reparsed.document.pages, report.document.pages);
    assert_eq!(reparsed.document.annotations, report.document.annotations);
}

#[test]
fn reserve_is_not_reported_below_the_ceiling() {
    // MaxUnitID 500 is the reserve written at signing time; an annotation
    // added afterwards takes a low id inside it, so nothing is exhausted.
    let (preserved, _) = save_after(&signed_package(500, true), |doc| add_rect(doc, "7"));
    assert!(!preserved.reserve_exhausted);
}

// ---- annotation files outside the entry file's directory ----
// Found by the final review of the signing line: a page annotation file that
// does not lie below the directory of the annotation entry file was listed
// by its package path, which a reader resolves against the entry file's own
// directory - so after any save nothing could find the annotations.

#[test]
fn old_layout_package_keeps_its_annotations_reachable() {
    // No annotation entry and no entry file: the page annotation file sits
    // in the page directory, a layout the parser accepts with a warning.
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        ("Doc_0/Document.xml", &document_xml(9, None)),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Pages/Page_0/Annotation.xml", EMPTY_PAGE_ANNOT),
    ]);
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    assert_eq!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .annotation_files,
        vec![(
            PageId::new("7"),
            "Doc_0/Pages/Page_0/Annotation.xml".to_string()
        )],
        "the page file is recorded where it is"
    );
    let mut doc = report.document;
    add_rect(&mut doc, "7");
    let preserved = rofd_io::save_ofd(&doc, &TextLayoutHints::new()).expect("save");
    assert!(!preserved.full_write_fallback, "nothing needs a full write");
    let entries = zip_entries(&preserved.bytes);
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        names.contains(&"Doc_0/Pages/Page_0/Annotation.xml"),
        "the page file stays where it was: {names:?}"
    );
    let manifest = entry_text(&entries, "Doc_0/Annots/Annotations.xml");
    assert!(
        manifest.contains("<ofd:FileLoc>../Pages/Page_0/Annotation.xml</ofd:FileLoc>"),
        "the entry file names the page file relative to its own directory: {manifest}"
    );
    assert!(
        entry_text(&entries, "Doc_0/Document.xml")
            .contains("<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>"),
        "Document.xml declares the entry file"
    );
    let reopened = rofd_io::parse_ofd(&preserved.bytes).expect("the saved package re-parses");
    assert_eq!(
        reopened.document.annotations, doc.annotations,
        "the annotation is reachable after the save"
    );
}

#[test]
fn a_page_file_named_through_a_parent_directory_stays_reachable() {
    // A writer that put the entry file into Annots/ and the page files next
    // to the pages names them with `..` (GB/T 33190 table 2 allows it).
    let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Page PageID=\"7\"><ofd:FileLoc>../Pages/Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>\
</ofd:Annotations>";
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(None)),
        (
            "Doc_0/Document.xml",
            &document_xml(9, Some("Annots/Annotations.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", manifest),
        ("Doc_0/Pages/Page_0/Annotation.xml", EMPTY_PAGE_ANNOT),
    ]);
    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd");
    assert_eq!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .annotation_files,
        vec![(
            PageId::new("7"),
            "Doc_0/Pages/Page_0/Annotation.xml".to_string()
        )],
        "a loc with `..` resolves"
    );
    let mut doc = report.document;
    add_rect(&mut doc, "7");
    let preserved = rofd_io::save_ofd(&doc, &TextLayoutHints::new()).expect("save");
    let entries = zip_entries(&preserved.bytes);
    assert!(
        entry_text(&entries, "Doc_0/Annots/Annotations.xml")
            .contains("<ofd:FileLoc>../Pages/Page_0/Annotation.xml</ofd:FileLoc>"),
        "the loc is written the way it was read"
    );
    let reopened = rofd_io::parse_ofd(&preserved.bytes).expect("the saved package re-parses");
    assert_eq!(reopened.document.annotations, doc.annotations);
}

#[test]
fn a_new_annotation_on_a_signed_document_takes_no_page_id() {
    // The page is the document's highest ID and carries no object. A new
    // annotation on a signed document is numbered above the highest ID in
    // use, and a page's ID is in use too (final review).
    let bytes = zip_of(&[
        ("OFD.xml", &ofd_xml(Some("/Doc_0/Signs/Signatures.xml"))),
        (
            "Doc_0/Document.xml",
            &document_xml(65_545, Some("Annots/Annotations.xml")),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
        ("Doc_0/Annots/Annotations.xml", MANIFEST_P7),
        ("Doc_0/Annots/P_7.xml", EMPTY_PAGE_ANNOT),
        ("Doc_0/Signs/Signatures.xml", SIGNATURES_XML),
    ]);
    let (preserved, _) = save_after(&bytes, |doc| {
        for _ in 0..8 {
            add_rect(doc, "7");
        }
    });
    let reopened = rofd_io::parse_ofd(&preserved.bytes).expect("re-parses");
    let ids: Vec<u64> = reopened
        .document
        .annotations
        .by_page
        .values()
        .flatten()
        .filter_map(|annotation| annotation.id.0.parse().ok())
        .collect();
    assert_eq!(ids, (8..=15).collect::<Vec<u64>>(), "above the page's ID 7");
    assert!(!preserved.reserve_exhausted);
}
