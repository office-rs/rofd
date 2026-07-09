use rofd_dom::{AnnotationKind, AnnotationPayload, Color, FontId, LayerType, PageObject};

#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn parse_minimal_ofd_builds_one_page_with_text_and_path() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert_eq!(report.document.pages.len(), 1);
    let page = &report.document.pages[0];
    assert_eq!(page.id, rofd_dom::PageId::new("P0"));
    assert_eq!(page.physical_box.w, 210.0);
    let body = page
        .layers
        .iter()
        .find(|l| l.layer_type == LayerType::Body)
        .expect("body layer exists");
    assert_eq!(body.objects.len(), 2, "text + path");
    assert!(matches!(body.objects[0], PageObject::Text(_)));
    assert!(matches!(body.objects[1], PageObject::Path(_)));
}

#[test]
fn parse_records_annotation_entry_in_package() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert_eq!(report.package.annotation_entries().count(), 1);
}

#[test]
fn parse_collects_font_resource() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert!(report.document.resources.fonts.contains_key(&FontId::new("F1")));
}

#[test]
fn parse_collects_annotation_into_model() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let anns = report.document.annotations.for_page(&rofd_dom::PageId::new("P0"));
    assert_eq!(anns.len(), 1);
    assert!(matches!(anns[0].kind, AnnotationKind::Highlight));
    assert!(matches!(anns[0].payload, AnnotationPayload::Markup { .. }));
}

#[test]
fn parse_path_object_captures_stroke_color() {
    // The fixture's PathObject has <ofd:StrokeColor Color="255 0 0"/>. Before
    // the fix this color was parsed then discarded (stroke stayed None).
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let page = &report.document.pages[0];
    let body = page
        .layers
        .iter()
        .find(|l| l.layer_type == LayerType::Body)
        .expect("body layer exists");
    let path = body
        .objects
        .iter()
        .find_map(|o| match o {
            PageObject::Path(p) => Some(p),
            _ => None,
        })
        .expect("path object exists");
    assert_eq!(path.stroke, Some(Color::Rgb(255, 0, 0)), "StrokeColor should be captured");
}

#[test]
fn parse_populates_doc_meta_from_doc_info() {
    // The fixture's OFD.xml has <ofd:DocInfo> with DocID/Title/Author.
    // Before the fix these were silently dropped (meta stayed default/None).
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let meta = &report.document.meta;
    assert_eq!(meta.title.as_deref(), Some("fixture"), "Title should be populated from DocInfo");
    assert_eq!(meta.author.as_deref(), Some("tester"), "Author should be populated from DocInfo");
    assert_eq!(meta.doc_id.as_deref(), Some("doc-001"), "DocID should be populated from DocInfo");
}

#[test]
fn parse_stores_textcode_text() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let page = &report.document.pages[0];
    let body = page.layers.iter().find(|l| l.layer_type == LayerType::Body).unwrap();
    let rofd_dom::PageObject::Text(t) = &body.objects[0] else { panic!("expected text") };
    assert_eq!(t.codes[0].text, "Hello");
}
