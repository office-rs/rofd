use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, DrawParamId, FontId, ImageId, LayerType, PageId,
    PageObject,
};

#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn parse_minimal_ofd_builds_one_page_with_text_and_path() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert_eq!(report.document.pages.len(), 1);
    let page = &report.document.pages[0];
    assert_eq!(page.id, rofd_dom::PageId::new("1"));
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
    // Two annotation entries: Annotations.xml (entry) + Page_0/Annotation.xml (per-page).
    assert_eq!(report.package.annotation_entries().count(), 2);
}

#[test]
fn parse_collects_font_resource() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert!(report
        .document
        .resources
        .fonts
        .contains_key(&FontId::new("F1")));
}

#[test]
fn parse_collects_annotation_into_model() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let anns = report
        .document
        .annotations
        .for_page(&rofd_dom::PageId::new("1"));
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
    assert_eq!(
        path.stroke,
        Some(Color::Rgb(255, 0, 0)),
        "StrokeColor should be captured"
    );
}

#[test]
fn parse_populates_doc_meta_from_doc_info() {
    // The fixture's OFD.xml has <ofd:DocInfo> with DocID/Title/Author.
    // Before the fix these were silently dropped (meta stayed default/None).
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let meta = &report.document.meta;
    assert_eq!(
        meta.title.as_deref(),
        Some("fixture"),
        "Title should be populated from DocInfo"
    );
    assert_eq!(
        meta.author.as_deref(),
        Some("tester"),
        "Author should be populated from DocInfo"
    );
    assert_eq!(
        meta.doc_id.as_deref(),
        Some("doc-001"),
        "DocID should be populated from DocInfo"
    );
}

#[test]
fn parse_stores_textcode_text() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let page = &report.document.pages[0];
    let body = page
        .layers
        .iter()
        .find(|l| l.layer_type == LayerType::Body)
        .unwrap();
    let rofd_dom::PageObject::Text(t) = &body.objects[0] else {
        panic!("expected text")
    };
    assert_eq!(t.codes[0].text, "Hello");
}

#[test]
fn parse_loads_font_data_from_fontfile() {
    let bytes = fixtures::build_minimal_ofd_with_font();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    assert!(report
        .document
        .resources
        .font_data
        .contains_key(&rofd_dom::FontId::new("F1")));
}

#[test]
fn parse_resolves_drawparam_and_multimedia_from_document_res() {
    let bytes = fixtures::build_minimal_ofd_with_drawparam();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    // DrawParam 5 parsed from DocumentRes.xml with `Value` colors + LineWidth.
    let dp5 = report
        .document
        .resources
        .draw_params
        .get(&DrawParamId::new("5"))
        .expect("DrawParam 5 parsed");
    assert_eq!(dp5.stroke, Some(Color::Rgb(255, 0, 0)));
    assert_eq!(dp5.fill, Some(Color::Rgb(0, 0, 0)));
    assert_eq!(dp5.line_width, Some(2.0));
    // Image 9 loaded from Doc_0/Res/img.png (MediaFile relative to BaseLoc="Res").
    assert!(
        report
            .document
            .resources
            .images
            .contains_key(&ImageId::new("9")),
        "image bytes loaded"
    );
    // The PathObject carries DrawParam="5" and no inline color (resolved at render).
    let page = &report.document.pages[0];
    let path = page
        .layers
        .iter()
        .flat_map(|l| l.objects.iter())
        .find_map(|o| match o {
            PageObject::Path(p) => Some(p),
            _ => None,
        })
        .expect("path object");
    assert_eq!(path.draw_param, Some(DrawParamId::new("5")));
    assert!(
        path.stroke.is_none(),
        "no inline StrokeColor -> None until render resolves"
    );
}

#[test]
fn parse_document_extracts_max_unit_id_and_annotations_loc() {
    // Real-shaped Document.xml fragment: MaxUnitID lives inside CommonData;
    // Annotations is a document-level element carrying the annotation entry path.
    let doc_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
  <ofd:MaxUnitID>1500</ofd:MaxUnitID></ofd:CommonData>
  <ofd:Pages><ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages>
  <ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>
</ofd:Document>"#;
    let header = rofd_io::parse::document::parse_document(doc_xml).unwrap();
    assert_eq!(header.max_unit_id, 1500);
    assert_eq!(
        header.annotations_loc.as_deref(),
        Some("Annots/Annotations.xml")
    );
}

#[test]
fn parse_real_page_annot_underline_and_rectangle() {
    // Real-sample Doc_0/Annots/Page_0/Annotation.xml fragment (trimmed).
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="1488" Creator="flw" LastModDate="2026-07-13 22:43:57" Subtype="Underline">
    <ofd:Appearance Boundary="36.1215 69.6702 38.3357 4.4059">
      <ofd:PathObject ID="1490" Boundary="0 0 38.3357 4.4059" LineWidth="0.5">
        <ofd:StrokeColor Value="0 239 89"/>
        <ofd:AbbreviatedData>M 0.25 4.4059 L 38.0857 4.4059 </ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
  <ofd:Annot Type="Path" ID="1498" Creator="flw" LastModDate="2026-07-13 22:44:09" Subtype="Rectangle">
    <ofd:Appearance Boundary="66.4772 19.7253 75.682 18.349">
      <ofd:PathObject ID="1500" Boundary="0 0 75.682 18.349" LineWidth="0.3528">
        <ofd:StrokeColor Value="255 0 0"/>
        <ofd:AbbreviatedData>M 0.1764 0.1764 L 75.5056 0.1764 L 75.5056 18.1726 L 0.1764 18.1726 </ofd:AbbreviatedData>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = rofd_io::parse::annotation::parse_page_annot(xml, &PageId::new("1")).unwrap();
    assert_eq!(anns.len(), 2);
    // Underline
    assert_eq!(anns[0].id.0, "1488");
    assert!(matches!(anns[0].kind, AnnotationKind::Underline));
    assert_eq!(anns[0].creator, "flw");
    assert_eq!(anns[0].modified, 1_783_982_637_000);
    match &anns[0].payload {
        AnnotationPayload::Markup { color, .. } => assert_eq!(color, &Color::Rgb(0, 239, 89)),
        other => panic!("expected Markup, got {other:?}"),
    }
    // Rectangle
    assert!(matches!(
        anns[1].kind,
        AnnotationKind::Shape(rofd_dom::ShapeKind::Rect)
    ));
    match &anns[1].payload {
        AnnotationPayload::Shape { stroke, .. } => {
            assert_eq!(stroke, &Color::Rgb(255, 0, 0));
        }
        other => panic!("expected Shape, got {other:?}"),
    }
}
