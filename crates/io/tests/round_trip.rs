#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn write_ofd_round_trips_through_parse() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    // Full write from model (no package).
    let written = rofd_io::write_ofd(&report.document).unwrap();
    let reparsed = rofd_io::parse_ofd(&written).unwrap();
    assert_eq!(reparsed.document.pages.len(), 1);
    assert_eq!(reparsed.document.pages[0].id, rofd_dom::PageId::new("P0"));
    // Real structure that survives the round-trip: physical box and body layer.
    assert_eq!(reparsed.document.pages[0].physical_box.w, 210.0, "physical_box.w preserved");
    assert!(
        reparsed.document.pages[0].layers.iter().any(|l| l.layer_type == rofd_dom::LayerType::Body),
        "body layer survives write_ofd round-trip"
    );
}

#[test]
fn write_ofd_round_trips_annotation() {
    // The fixture's document has a Highlight annotation. write_ofd emits it via
    // the full-write path (Doc_0/Pages/Page_{i}/Annotation.xml); reparse must
    // recover it. This proves annotation round-trip via write_ofd (not just
    // save_ofd surgical path).
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let written = rofd_io::write_ofd(&report.document).unwrap();
    let reparsed = rofd_io::parse_ofd(&written).unwrap();
    let anns = reparsed.document.annotations.for_page(&rofd_dom::PageId::new("P0"));
    assert!(
        anns.iter().any(|a| matches!(a.kind, rofd_dom::AnnotationKind::Highlight)),
        "Highlight annotation survives write_ofd round-trip"
    );
}

#[test]
fn load_annotate_save_preserves_body_and_keeps_annotation() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    // Mutate annotations (simulate editor): add a reply note to page 0.
    let mut doc = report.document.clone();
    use rofd_dom::*;
    doc.annotations.by_page.entry(PageId::new("P0")).or_default().push(Annotation {
        id: AnnotationId::from_int(100),
        kind: AnnotationKind::Note,
        page: PageId::new("P0"),
        creator: "李四".into(),
        created: 1_700_000_001_000,
        modified: 1_700_000_001_000,
        reply_to: None,
        payload: AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
            color: Color::Rgb(255, 200, 0),
            content: "reply".into(),
            icon: NoteIcon::Note,
        },
    });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let reparsed = rofd_io::parse_ofd(&saved).unwrap();
    // Body preserved (one page, unchanged objects).
    assert_eq!(reparsed.document.pages.len(), 1);
    // Annotation round-trips.
    let anns = reparsed.document.annotations.for_page(&PageId::new("P0"));
    assert!(anns.iter().any(|a| matches!(a.kind, AnnotationKind::Note)), "added note survived");
}
