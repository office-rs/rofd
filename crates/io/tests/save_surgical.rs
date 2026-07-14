#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn surgical_save_preserves_body_byte_identical() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();

    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();

    // Body entries must be byte-identical (surgical save invariant §4.3).
    for name in [
        "OFD.xml",
        "Doc_0/Document.xml",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Res/Font.xml",
    ] {
        assert_eq!(
            by_name(&orig_entries, name),
            by_name(&saved_entries, name),
            "{name} changed"
        );
    }
}

fn by_name<'a>(entries: &'a [(String, Vec<u8>)], name: &'a str) -> &'a [u8] {
    entries
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| b.as_slice())
        .unwrap()
}

#[test]
fn surgical_save_rewrites_annotation_entry() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let ann = saved_entries
        .iter()
        .find(|(n, _)| n == "Doc_0/Annots/Page_0/Annotation.xml")
        .map(|(_, b)| b.as_slice())
        .unwrap();
    // Re-serialized in GB/T 33190 §15.2 <PageAnnot><Annot> format.
    assert!(std::str::from_utf8(ann).unwrap().contains("<ofd:PageAnnot"));
    assert!(std::str::from_utf8(ann).unwrap().contains("<ofd:Annot"));
}
