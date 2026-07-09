use rofd_dom::{AnnotationKind, AnnotationPayload, FontId, LayerType, PageObject};

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
