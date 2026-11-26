//! The document carries its package (spec 2026-11-23 §17.11): parse puts it
//! there, the save reads it from there, and the document-level fields can be
//! refreshed from it without the pages being read again.

use std::sync::Arc;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, OfdDocument,
    OfdWarning, PageId, Rect, TextLayoutHints,
};
use rofd_io::zip_util::{read_all_entries, write_zip, write_zip_borrowed};
use rofd_io::{
    annotations_unwritable, generate_package, parse_ofd, refresh_document_level, save_ofd,
    with_max_unit_id, with_signatures_loc, DocumentXmlChange, EntryKind, PackageHandle,
};

const NOWHERE: Rect = Rect {
    x: 1.0,
    y: 1.0,
    w: 2.0,
    h: 2.0,
};

/// A signature list with one record, pointing at a description the package
/// does not carry.
const SIGNATURES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ofd:Signatures xmlns:ofd="http://www.ofdspec.org/2016"><ofd:MaxSignId>1</ofd:MaxSignId><ofd:Signature ID="1" Type="Seal" BaseLoc="/Doc_0/Signs/Sign_0/Signature.xml"/></ofd:Signatures>"#;

/// An unsigned package another tool wrote: one A4 page, an attachment.
fn unsigned_package() -> Vec<u8> {
    let ofd_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#;
    let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData><ofd:Pages><ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#;
    let page = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>"#;
    zip_of(&[
        ("OFD.xml", ofd_xml),
        ("Doc_0/Document.xml", document),
        ("Doc_0/Pages/Page_0/Content.xml", page),
        ("Doc_0/Attachs/note.txt", "kept"),
    ])
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    write_zip(
        &files
            .iter()
            .map(|(name, text)| ((*name).to_owned(), text.as_bytes().to_vec()))
            .collect::<Vec<_>>(),
    )
    .expect("zip")
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    read_all_entries(bytes).expect("entries")
}

/// A package handle back as a file.
fn zipped(package: &PackageHandle) -> Vec<u8> {
    let borrowed: Vec<(&str, &[u8])> = package
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.bytes.as_slice()))
        .collect();
    write_zip_borrowed(&borrowed).expect("zip")
}

/// Replace an entry, keeping its position, or append it.
fn set_entry(package: &mut PackageHandle, name: &str, bytes: Vec<u8>) {
    match package.entries.iter_mut().find(|entry| entry.name == name) {
        Some(entry) => entry.bytes = Arc::new(bytes),
        None => package.entries.push(rofd_dom::PkgEntry {
            name: name.to_owned(),
            kind: EntryKind::of(name),
            bytes: Arc::new(bytes),
        }),
    }
}

fn text_of(package: &PackageHandle, name: &str) -> String {
    String::from_utf8(package.find(name).expect("entry").bytes.to_vec()).expect("UTF-8")
}

/// A Note annotation on `page`.
fn note(page: &PageId) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(9),
        kind: AnnotationKind::Note,
        page: page.clone(),
        creator: "tester".into(),
        created: 0,
        modified: 0,
        reply_to: None,
        payload: AnnotationPayload::Note {
            rect: NOWHERE,
            color: Color::Rgb(0, 0, 0),
            content: "a note".into(),
            icon: NoteIcon::Note,
        },
    }
}

#[test]
fn parse_puts_the_package_into_the_document() {
    let report = parse_ofd(&unsigned_package()).expect("parse");
    let package = report
        .document
        .package
        .as_ref()
        .expect("the document carries its package");
    let names: Vec<&str> = package.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "OFD.xml",
            "Doc_0/Document.xml",
            "Doc_0/Pages/Page_0/Content.xml",
            "Doc_0/Attachs/note.txt",
        ]
    );
    assert_eq!(
        package
            .find("Doc_0/Attachs/note.txt")
            .map(|e| e.kind.clone()),
        Some(EntryKind::Other)
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn the_document_saves_from_its_own_package() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    let saved = save_ofd(&document, &TextLayoutHints::new()).expect("save");
    assert_eq!(
        entries(&saved.bytes),
        entries(&unsigned_package()),
        "an untouched document saves every entry byte-identical"
    );
    // Without a package the full write takes over.
    document.package = None;
    let saved = save_ofd(&document, &TextLayoutHints::new()).expect("save");
    assert_eq!(saved.document_xml, DocumentXmlChange::Generated);
}

#[test]
fn refreshing_the_document_level_rereads_the_package() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    // What a first signature does to a package, done by hand.
    {
        let package = document.package.as_mut().unwrap();
        let document_xml = text_of(package, "Doc_0/Document.xml");
        set_entry(
            package,
            "Doc_0/Document.xml",
            with_max_unit_id(&document_xml, 65_545)
                .expect("MaxUnitID")
                .into_bytes(),
        );
        let ofd_xml = text_of(package, "OFD.xml");
        set_entry(
            package,
            "OFD.xml",
            with_signatures_loc(&ofd_xml, "/Doc_0/Signs/Signatures.xml")
                .expect("DocBody")
                .into_bytes(),
        );
        set_entry(
            package,
            "Doc_0/Signs/Signatures.xml",
            SIGNATURES_XML.as_bytes().to_vec(),
        );
    }
    let warnings = refresh_document_level(&mut document).expect("refresh");
    let reloaded = parse_ofd(&zipped(document.package.as_ref().unwrap()))
        .expect("parse")
        .document;
    assert_eq!(document.max_unit_id, 65_545);
    assert_eq!(document.max_unit_id, reloaded.max_unit_id);
    assert_eq!(document.meta, reloaded.meta);
    assert_eq!(document.raw_doc_children, reloaded.raw_doc_children);
    assert_eq!(document.raw_common_children, reloaded.raw_common_children);
    assert_eq!(
        document.raw_ofd_body_children,
        reloaded.raw_ofd_body_children
    );
    assert_eq!(document.signatures, reloaded.signatures);
    assert!(document.signatures.declared);
    assert_eq!(document.signatures.items.len(), 1);
    // The handle says what a reload's says too: these two flags used to be
    // kept in step by hand wherever a package changed.
    let (handle, reloaded_handle) = (
        document.package.as_ref().unwrap(),
        reloaded.package.as_ref().unwrap(),
    );
    assert_eq!(
        handle.signatures_declared,
        reloaded_handle.signatures_declared
    );
    assert_eq!(handle.signatures_entry, reloaded_handle.signatures_entry);
    assert!(handle.signatures_declared);
    assert_eq!(
        handle.signatures_entry.as_deref(),
        Some("Doc_0/Signs/Signatures.xml")
    );
    assert!(
        !warnings.is_empty(),
        "the record points at no description: a warning"
    );
    // What refresh leaves alone.
    assert_eq!(document.pages.len(), reloaded.pages.len());
    assert_eq!(document.pages[0].id, reloaded.pages[0].id);
}

#[test]
fn refreshing_follows_ofd_xml_for_the_handle_too() {
    let mut document = parse_ofd(&unsigned_package()).unwrap().document;
    {
        let package = document.package.as_mut().unwrap();
        package.signatures_declared = true;
        package.signatures_entry = Some("Doc_0/Signs/Signatures.xml".to_owned());
    }
    refresh_document_level(&mut document).unwrap();
    let package = document.package.as_ref().unwrap();
    assert!(!package.signatures_declared, "OFD.xml declares no list");
    assert_eq!(package.signatures_entry, None);
}

#[test]
fn annotations_unwritable_matches_the_blocked_flag() {
    let signed = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sign/SESV4SignDoc.ofd"
    ))
    .unwrap();
    let unsigned = unsigned_package();
    let mut any = false;
    for (bytes, annotate) in [
        (&unsigned[..], true),
        (&signed[..], true),
        (&signed[..], false),
    ] {
        let mut document = parse_ofd(bytes).unwrap().document;
        if annotate {
            document.annotations.insert(note(&document.pages[0].id));
        }
        let page_ids: Vec<PageId> = document.pages.iter().map(|page| page.id.clone()).collect();
        let unwritable = annotations_unwritable(
            document.package.as_ref().unwrap(),
            &page_ids,
            &document.annotations,
        );
        let blocked = save_ofd(&document, &TextLayoutHints::new())
            .unwrap()
            .blocked_by_signature;
        assert_eq!(unwritable, blocked);
        any |= unwritable;
    }
    assert!(
        any,
        "the signed file without an annotation entry cannot take one"
    );
}

#[test]
fn generate_package_makes_a_handle_for_a_document_without_one() {
    let generated =
        generate_package(&OfdDocument::default(), &TextLayoutHints::new()).expect("generate");
    assert_eq!(
        generated.package.document_entry.as_deref(),
        Some("Doc_0/Document.xml")
    );
    assert!(generated.package.consumed.contains("OFD.xml"));
    assert!(!generated.package.signatures_declared);
    assert!(generated.warnings.is_empty());
    // With a package behind it, what the model does not carry passes
    // through.
    let report = parse_ofd(&unsigned_package()).unwrap();
    let again = generate_package(&report.document, &TextLayoutHints::new()).unwrap();
    assert!(
        again.package.find("Doc_0/Attachs/note.txt").is_some(),
        "the attachment travels along"
    );
}

/// The unsigned package with an element on its page that rofd does not
/// model: parse keeps it verbatim and says so.
fn unsigned_package_with_unknown_element() -> Vec<u8> {
    let mut files = entries(&unsigned_package());
    let page = files
        .iter_mut()
        .find(|(name, _)| name == "Doc_0/Pages/Page_0/Content.xml")
        .expect("the page");
    page.1 = String::from_utf8(page.1.clone())
        .expect("UTF-8")
        .replace(
            "<ofd:Content/>",
            "<ofd:Content><ofd:Unknown/></ofd:Content>",
        )
        .into_bytes();
    write_zip(&files).expect("zip")
}

#[test]
fn generate_package_hands_out_what_parsing_the_new_package_warns_about() {
    let report = parse_ofd(&unsigned_package_with_unknown_element()).unwrap();
    let degraded = |warnings: &[OfdWarning]| {
        warnings
            .iter()
            .any(|warning| matches!(warning, OfdWarning::PreservedRaw { .. }))
    };
    assert!(
        degraded(&report.warnings),
        "the file said so when it was read"
    );
    let generated = generate_package(&report.document, &TextLayoutHints::new()).unwrap();
    assert!(
        degraded(&generated.warnings),
        "parsing the package that was made says it again"
    );
}
