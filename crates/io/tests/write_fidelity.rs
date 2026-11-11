//! Write-fidelity tests: `write_ofd` output must re-parse into the same
//! model (spec 2026-11-11 §8). The harness compares pages / resources /
//! annotations / meta but NOT `max_unit_id` (writing raises it), and pins
//! byte determinism (same model, same bytes twice).

use std::io::Read;

use rofd_dom::{
    Color, Ctm, FontId, Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject, Rect,
    TextCode, TextObject,
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
