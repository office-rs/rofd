//! Write-fidelity tests: `write_ofd` output must re-parse into the same
//! model (spec 2026-11-11 §8). The harness compares pages / resources /
//! annotations / meta but NOT `max_unit_id` (writing raises it), and pins
//! byte determinism (same model, same bytes twice).

use std::io::Read;

use rofd_dom::{
    Color, CompositeObject, Ctm, DrawParamId, FontId, Layer, LayerType, ObjectId, OfdDocument,
    Page, PageId, PageObject, PathCommand, PathData, PathObject, Rect, TextCode, TextObject,
};
use rofd_io::{parse_ofd, write_ofd};

/// Full-write round trip: write -> parse -> model equality + determinism.
/// Returns the re-parsed document for extra assertions by the caller.
fn assert_round_trip(doc: &OfdDocument) -> OfdDocument {
    let bytes = write_ofd(doc).expect("write_ofd");
    let bytes_again = write_ofd(doc).expect("write_ofd again");
    assert_eq!(bytes, bytes_again, "same model must write identical bytes");
    let report = parse_ofd(&bytes).expect("re-parse written bytes");
    assert_eq!(report.document.pages, doc.pages, "pages must round trip");
    assert_eq!(
        report.document.resources, doc.resources,
        "resources must round trip"
    );
    assert_eq!(
        report.document.annotations, doc.annotations,
        "annotations must round trip"
    );
    assert_eq!(report.document.meta, doc.meta, "meta must round trip");
    assert!(
        report.document.max_unit_id >= doc.max_unit_id,
        "max_unit_id may only grow"
    );
    report.document
}

/// Extract `Doc_0/Pages/Page_0/Content.xml` from written package bytes.
fn content_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip archive");
    let mut s = String::new();
    zip.by_name("Doc_0/Pages/Page_0/Content.xml")
        .expect("Content.xml entry")
        .read_to_string(&mut s)
        .expect("read entry");
    s
}

fn page_with(layers: Vec<Layer>) -> Page {
    Page {
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 210.0,
            h: 297.0,
        },
        layers,
        template: None,
    }
}

#[test]
fn text_object_round_trips() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![PageObject::Text(TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect {
                x: 31.75,
                y: 26.31,
                w: 17.58,
                h: 3.68,
            },
            ctm: Some(Ctm {
                a: 0.0176,
                b: 0.0,
                c: 0.0,
                d: 0.0176,
                e: 0.0,
                f: 0.0,
            }),
            font: FontId::new("4"),
            size: 3.68,
            fill: Some(Color::Rgb(0, 0, 0)),
            codes: vec![TextCode {
                glyph_ids: vec![20744, 1246],
                deltas: vec![(2.5, 0.0), (2.5, 0.0)],
                text: "高亮".into(),
                x: 0.0,
                y: 1.79,
            }],
            draw_param: None,
        })],
    }]));

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(xml.contains("<ofd:CGTransform"), "{xml}");
    assert!(xml.contains("<ofd:Glyphs>20744 1246</ofd:Glyphs>"), "{xml}");
    assert!(xml.contains("DeltaX=\"2.5 2.5\""), "{xml}");

    let rt = assert_round_trip(&doc);
    assert_eq!(rt.pages.len(), 1);
}

#[test]
fn layer_without_objects_emits_self_closing() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![],
    }]));

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(xml.contains("<ofd:Layer Type=\"Body\"/>"), "{xml}");
    assert_round_trip(&doc);
}

#[test]
fn path_object_round_trips() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![
            PageObject::Path(PathObject {
                id: ObjectId::new("p1"),
                boundary: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                ctm: None,
                fill: Some(Color::Rgb(255, 0, 0)),
                stroke: Some(Color::Rgb(0, 0, 255)),
                line_width: 0.5,
                data: PathData {
                    commands: vec![
                        PathCommand::M(0.0, 0.0),
                        PathCommand::Q(5.0, 5.0, 10.0, 0.0),
                        PathCommand::C(1.0, 2.0, 3.0, 4.0, 5.0, 6.0),
                    ],
                },
                draw_param: None,
                stroke_enabled: Some(false),
                fill_enabled: Some(true),
            }),
            PageObject::Path(PathObject {
                id: ObjectId::new("p2"),
                boundary: Rect {
                    x: 0.0,
                    y: 8.0,
                    w: 100.0,
                    h: 1.0,
                },
                ctm: None,
                fill: None,
                stroke: None,
                line_width: 0.48,
                data: PathData {
                    commands: vec![PathCommand::M(0.24, 8.0), PathCommand::L(90.0, 8.0)],
                },
                draw_param: Some(DrawParamId::new("dp1")),
                stroke_enabled: None,
                fill_enabled: None,
            }),
        ],
    }]));

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    // Explicit Stroke/Fill booleans are written; absent ones (p2) are not.
    assert!(xml.contains("Stroke=\"false\""), "{xml}");
    assert!(
        xml.contains("<ofd:PathObject ID=\"p2\" Boundary=\"0 8 100 1\" LineWidth=\"0.48\" DrawParam=\"dp1\">"),
        "{xml}"
    );
    assert!(xml.contains("<ofd:FillColor Value=\"255 0 0\"/>"), "{xml}");
    assert!(
        xml.contains("<ofd:StrokeColor Value=\"0 0 255\"/>"),
        "{xml}"
    );
    assert!(xml.contains("Q 5 5 10 0"), "{xml}");
    assert_round_trip(&doc);
}

#[test]
fn path_z_round_trips_as_z() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![PageObject::Path(PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            ctm: None,
            fill: None,
            stroke: None,
            line_width: 0.35,
            data: PathData {
                commands: vec![
                    PathCommand::M(0.0, 0.0),
                    PathCommand::L(10.0, 0.0),
                    PathCommand::Z,
                ],
            },
            draw_param: None,
            stroke_enabled: None,
            fill_enabled: None,
        })],
    }]));

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    // Fidelity: Z emits as a close operator, not an L back to the start
    // (annotation-side path_to_abbrev deliberately expands; body must not).
    assert!(xml.contains("M 0 0 L 10 0 Z"), "{xml}");
    assert_round_trip(&doc);
}

#[test]
fn composite_round_trips() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![PageObject::Composite(CompositeObject {
            id: ObjectId::new("c1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 20.0,
            },
            ctm: None,
            unit: "Page_2/Content.xml".into(),
        })],
    }]));
    assert_round_trip(&doc);
}

#[test]
fn template_round_trips() {
    let mut doc = OfdDocument::default();
    let mut page = page_with(vec![]);
    page.template = Some("Pages/Tpl.xml".into());
    doc.pages.push(page);

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(
        xml.contains("<ofd:Template TemplateLoc=\"Pages/Tpl.xml\"/>"),
        "{xml}"
    );
    assert_round_trip(&doc);
}
