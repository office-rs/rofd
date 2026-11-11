//! Write-fidelity tests: `write_ofd` output must re-parse into the same
//! model (spec 2026-11-11 §8). The harness compares pages / resources /
//! annotations / meta but NOT `max_unit_id` (writing raises it), and pins
//! byte determinism (same model, same bytes twice).

use std::io::Read;
use std::sync::Arc;

use rofd_dom::{
    Color, CompositeObject, Ctm, DocMeta, DrawParam, DrawParamId, FontId, FontRef, ImageId,
    ImageObject, Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject, PathCommand,
    PathData, PathObject, Rect, TextCode, TextObject,
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
    read_entry(bytes, "Doc_0/Pages/Page_0/Content.xml")
}

/// Extract one zip entry's text content.
fn read_entry(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip archive");
    let mut s = String::new();
    zip.by_name(name)
        .unwrap_or_else(|e| panic!("entry {name}: {e}"))
        .read_to_string(&mut s)
        .expect("read entry");
    s
}

fn has_entry(bytes: &[u8], name: &str) -> bool {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip archive");
    let found = zip.file_names().any(|n| n == name);
    found
}

/// Minimal PNG-magic payload (sniffing only reads the magic bytes).
const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
/// Minimal JPEG-magic payload.
const JPG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
/// Minimal sfnt 1.0 (TrueType) payload.
const TTF_MAGIC: &[u8] = &[0x00, 0x01, 0x00, 0x00, 9, 9];

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

#[test]
fn resources_round_trip() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    doc.resources
        .images
        .insert(ImageId::new("img1"), Arc::new(PNG_MAGIC.to_vec()));
    doc.resources.fonts.insert(
        FontId::new("4"),
        FontRef {
            id: FontId::new("4"),
            family_name: Some("SimSun".into()),
        },
    );
    doc.resources
        .font_data
        .insert(FontId::new("4"), Arc::new(TTF_MAGIC.to_vec()));
    doc.resources.draw_params.insert(
        DrawParamId::new("dp1"),
        DrawParam {
            line_width: Some(0.5),
            stroke: Some(Color::Rgb(255, 0, 0)),
            fill: Some(Color::Rgb(0, 0, 255)),
        },
    );

    let bytes = write_ofd(&doc).expect("write_ofd");
    let res_xml = read_entry(&bytes, "Doc_0/Res/DocumentRes.xml");
    assert!(res_xml.contains("Format=\"png\""), "{res_xml}");
    assert!(
        res_xml.contains("<ofd:MediaFile>Img_img1.png</ofd:MediaFile>"),
        "{res_xml}"
    );
    assert!(res_xml.contains("FontName=\"SimSun\""), "{res_xml}");
    assert!(
        res_xml.contains("<ofd:FontFile>Font_4.ttf</ofd:FontFile>"),
        "{res_xml}"
    );
    assert!(
        res_xml.contains("<ofd:DrawParam ID=\"dp1\" LineWidth=\"0.5\">"),
        "{res_xml}"
    );
    assert!(has_entry(&bytes, "Doc_0/Res/Img_img1.png"));
    assert!(has_entry(&bytes, "Doc_0/Res/Font_4.ttf"));
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        doc_xml.contains("<ofd:DocumentRes BaseLoc=\"Res/DocumentRes.xml\"/>"),
        "{doc_xml}"
    );

    let rt = assert_round_trip(&doc);
    assert_eq!(
        rt.resources
            .images
            .get(&ImageId::new("img1"))
            .map(|b| b.as_slice()),
        Some(PNG_MAGIC),
        "image bytes survive round trip"
    );
    assert!(
        rt.resources.font_data.contains_key(&FontId::new("4")),
        "font bytes survive round trip"
    );
}

#[test]
fn jpeg_sniffed_as_jpg() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    doc.resources
        .images
        .insert(ImageId::new("img9"), Arc::new(JPG_MAGIC.to_vec()));
    let bytes = write_ofd(&doc).expect("write_ofd");
    let res_xml = read_entry(&bytes, "Doc_0/Res/DocumentRes.xml");
    assert!(res_xml.contains("Format=\"jpg\""), "{res_xml}");
    assert!(res_xml.contains("Img_img9.jpg"), "{res_xml}");
    assert_round_trip(&doc);
}

#[test]
fn empty_resources_emit_nothing() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    let bytes = write_ofd(&doc).expect("write_ofd");
    assert!(!has_entry(&bytes, "Doc_0/Res/DocumentRes.xml"));
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(!doc_xml.contains("PublicRes"), "{doc_xml}");
    assert!(!doc_xml.contains("DocumentRes"), "{doc_xml}");
    assert_round_trip(&doc);
}

#[test]
fn image_object_round_trips() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        layer_type: LayerType::Body,
        objects: vec![PageObject::Image(ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 20.0,
            },
            ctm: None,
            image: ImageId::new("img1"),
        })],
    }]));
    doc.resources
        .images
        .insert(ImageId::new("img1"), Arc::new(PNG_MAGIC.to_vec()));

    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(
        xml.contains("<ofd:ImageObject ID=\"i1\" Boundary=\"10 10 50 20\" ResourceID=\"img1\"/>"),
        "{xml}"
    );
    assert_round_trip(&doc);
}

#[test]
fn doc_meta_round_trips_via_ofd_xml() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    doc.meta = DocMeta {
        doc_id: Some("DOC-1".into()),
        title: Some("验收报告".into()),
        author: Some("张三".into()),
        creation_date: Some("2026-11-11".into()),
        mod_date: Some("2026-11-11T10:00:00".into()),
    };
    let bytes = write_ofd(&doc).expect("write_ofd");
    let ofd = read_entry(&bytes, "OFD.xml");
    assert!(ofd.contains("<ofd:DocInfo>"), "{ofd}");
    assert!(ofd.contains("<ofd:Title>验收报告</ofd:Title>"), "{ofd}");
    assert!(
        ofd.contains("<ofd:LastModDate>2026-11-11T10:00:00</ofd:LastModDate>"),
        "{ofd}"
    );
    assert_round_trip(&doc);
}

#[test]
fn empty_meta_omits_docinfo() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    let bytes = write_ofd(&doc).expect("write_ofd");
    let ofd = read_entry(&bytes, "OFD.xml");
    assert!(!ofd.contains("DocInfo"), "{ofd}");
    assert_round_trip(&doc);
}

/// Independently-built identical models must write identical bytes: HashMap
/// RandomState differs per instance, so any unsorted resource iteration
/// shows up here, not in a same-instance double write.
fn resource_heavy_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    for i in 1..=3 {
        let img = format!("img{i}");
        let f = format!("f{i}");
        let dp = format!("dp{i}");
        doc.resources
            .images
            .insert(ImageId::new(img), Arc::new(PNG_MAGIC.to_vec()));
        doc.resources.fonts.insert(
            FontId::new(&f),
            FontRef {
                id: FontId::new(&f),
                family_name: Some(format!("Fam{i}")),
            },
        );
        doc.resources
            .font_data
            .insert(FontId::new(&f), Arc::new(TTF_MAGIC.to_vec()));
        doc.resources.draw_params.insert(
            DrawParamId::new(dp),
            DrawParam {
                line_width: Some(0.1 * i as f64),
                stroke: None,
                fill: None,
            },
        );
    }
    doc
}

#[test]
fn same_model_writes_identical_bytes() {
    let a = write_ofd(&resource_heavy_doc()).expect("write first");
    let b = write_ofd(&resource_heavy_doc()).expect("write second");
    assert_eq!(
        a, b,
        "same model (independent instances) must write identical bytes"
    );
}
