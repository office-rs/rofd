#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn surgical_save_preserves_body_byte_identical() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();

    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();

    // Body Content.xml + OFD.xml + resources must be byte-identical
    // (surgical save invariant §4.3). Document.xml is NOT in this set -
    // its <MaxUnitID> is byte-patched, so it is only identical when the
    // model's max_unit_id matches the original (the no-mutation case here).
    for name in [
        "OFD.xml",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Res/Font.xml",
    ] {
        assert_eq!(
            by_name(&orig_entries, name),
            by_name(&saved_entries, name),
            "{name} changed"
        );
    }
    // Document.xml is byte-patched; with no mutation it stays identical.
    assert_eq!(
        by_name(&orig_entries, "Doc_0/Document.xml"),
        by_name(&saved_entries, "Doc_0/Document.xml"),
        "Document.xml unchanged when max_unit_id not mutated"
    );
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

/// Task 9: surgical save expands the dirty set -
/// (a) entry file `Annotations.xml` is re-serialized,
/// (b) per-page `Page_N/Annotation.xml` is re-serialized,
/// (c) `Document.xml` `<MaxUnitID>` is byte-patched,
/// (d) body `Content.xml` entries are byte-identical (invariant §4.3).
#[test]
fn surgical_save_rewrites_annotation_entry_and_per_page_and_max_unit_id() {
    use rofd_dom::*;
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let mut doc = report.document.clone();
    // Add a new annotation (triggers max_unit_id increment).
    let new_id = doc.max_unit_id + 1;
    doc.max_unit_id = new_id;
    doc.annotations
        .by_page
        .entry(PageId::new("1"))
        .or_default()
        .push(Annotation {
            id: AnnotationId::from_int(new_id),
            kind: AnnotationKind::Note,
            page: PageId::new("1"),
            creator: "t".into(),
            created: 1_783_656_237_000,
            modified: 1_783_656_237_000,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 5.0,
                    h: 5.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "new".into(),
                icon: NoteIcon::Note,
            },
        });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    // (d) body Content.xml byte-identical (invariant §4.3).
    for name in orig_entries
        .iter()
        .filter(|(n, _)| n.ends_with("Content.xml"))
        .map(|(n, _)| n.as_str())
    {
        let o = orig_entries.iter().find(|(n, _)| n == name).unwrap();
        let s = saved_entries.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(o.1, s.1, "body {name} byte-identical");
    }
    // (c) Document.xml MaxUnitID updated.
    let doc_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Document.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(
        doc_xml.contains(&format!("<ofd:MaxUnitID>{new_id}</ofd:MaxUnitID>")),
        "MaxUnitID updated to {new_id}: {doc_xml}"
    );
    // (b) per-page annotation file contains the new annotation.
    let ann_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Annots/Page_0/Annotation.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(
        ann_xml.contains("Note") && ann_xml.contains("new"),
        "new annot in per-page file: {ann_xml}"
    );
    // (a) entry file `Annotations.xml` re-serialized (still references Page_0).
    let entry_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Annots/Annotations.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(
        entry_xml.contains("Page_0/Annotation.xml"),
        "entry file references per-page file: {entry_xml}"
    );
}
