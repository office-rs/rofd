//! Write-fidelity tests: `write_ofd` output must re-parse into the same
//! model (spec 2026-11-11 §8). The harness compares pages / resources /
//! annotations / meta but NOT `max_unit_id` (writing raises it), and pins
//! byte determinism (same model, same bytes twice).

use std::io::Read;
use std::sync::Arc;

use rofd_dom::{
    Color, CompositeObject, Ctm, DashSpec, DocMeta, DrawParam, DrawParamId, FontId, FontRef,
    ImageId, ImageObject, Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject,
    PathCommand, PathData, PathObject, Rect, TextCode, TextObject,
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
    assert_eq!(
        report.document.raw_doc_children, doc.raw_doc_children,
        "document-root raws must round trip"
    );
    assert_eq!(
        report.document.raw_common_children, doc.raw_common_children,
        "CommonData raws must round trip"
    );
    assert_eq!(
        report.document.raw_ofd_body_children, doc.raw_ofd_body_children,
        "OFD.xml body raws must round trip"
    );
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
        raw_children: Vec::new(),
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Text(TextObject {
            raw_attrs: Vec::new(),
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
            clip: None,
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![
            PageObject::Path(PathObject {
                raw_attrs: Vec::new(),
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
                clip: None,
            }),
            PageObject::Path(PathObject {
                raw_attrs: Vec::new(),
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
                clip: None,
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Path(PathObject {
            raw_attrs: Vec::new(),
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
            clip: None,
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Composite(CompositeObject {
            raw_attrs: Vec::new(),
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
            dash: None,
        },
    );

    let bytes = write_ofd(&doc).expect("write_ofd");
    let public_xml = read_entry(&bytes, "Doc_0/Res/PublicRes.xml");
    let res_xml = read_entry(&bytes, "Doc_0/Res/DocumentRes.xml");
    assert!(res_xml.contains("Format=\"PNG\""), "{res_xml}");
    assert!(
        res_xml.contains("<ofd:MediaFile>Img_img1.png</ofd:MediaFile>"),
        "{res_xml}"
    );
    assert!(public_xml.contains("FontName=\"SimSun\""), "{public_xml}");
    assert!(
        public_xml.contains("<ofd:FontFile>Font_4.ttf</ofd:FontFile>"),
        "{public_xml}"
    );
    assert!(
        public_xml.contains("<ofd:DrawParam ID=\"dp1\" LineWidth=\"0.5\">"),
        "{public_xml}"
    );
    assert!(has_entry(&bytes, "Doc_0/Res/Img_img1.png"));
    assert!(has_entry(&bytes, "Doc_0/Res/Font_4.ttf"));
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        doc_xml.contains("<ofd:PublicRes>Res/PublicRes.xml</ofd:PublicRes>"),
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
    assert!(res_xml.contains("Format=\"JPG\""), "{res_xml}");
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
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Image(ImageObject {
            raw_attrs: Vec::new(),
            id: ObjectId::new("i1"),
            boundary: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 20.0,
            },
            ctm: None,
            image: ImageId::new("img1"),
            clip: None,
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
        raw_info_children: Vec::new(),
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
                dash: None,
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

// ---- Passthrough capture (spec 2026-11-11 D3, parse side) ----

fn build_zip(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        w.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        w.write_all(content.as_bytes()).expect("write entry");
    }
    w.finish().expect("finish zip").into_inner()
}

fn page_package(content_xml: &str, max_unit_id: u64) -> Vec<u8> {
    build_zip(&[
        (
            "OFD.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>",
        ),
        (
            "Doc_0/Document.xml",
            &format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>{max_unit_id}</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>"
            ),
        ),
        ("Doc_0/Pages/Page_0/Content.xml", content_xml),
    ])
}

const PATH_XML: &str = "<ofd:PathObject ID=\"p1\" LineWidth=\"1\"><ofd:AbbreviatedData>M 0 0 L 1 1</ofd:AbbreviatedData></ofd:PathObject>";

#[test]
fn raw_fragment_carries_inherited_namespaces() {
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\" xmlns:ext=\"urn:x\"><ofd:Content><ofd:Layer Type=\"Body\"><ext:Stamp ID=\"9\" ext:seal=\"1\"><ext:Inner>x</ext:Inner></ext:Stamp></ofd:Layer></ofd:Content></ofd:Page>";
    let report = parse_ofd(&page_package(content, 10)).expect("parse");
    let layer = &report.document.pages[0].layers[0];
    assert_eq!(layer.raw_children.len(), 1);
    let frag = &layer.raw_children[0];
    assert_eq!(frag.position, 0);
    assert!(
        frag.xml.contains("xmlns:ext=\"urn:x\""),
        "inherited namespace synthesized into fragment root: {}",
        frag.xml
    );
    assert!(
        frag.xml.contains("<ext:Inner>x</ext:Inner>"),
        "verbatim subtree: {}",
        frag.xml
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, rofd_dom::OfdWarning::PreservedRaw { element, .. } if element == "ext:Stamp")),
        "warnings: {:?}",
        report.warnings
    );
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| matches!(w, rofd_dom::OfdWarning::SkippedObject { .. })),
        "no SkippedObject for preserved elements: {:?}",
        report.warnings
    );
}

#[test]
fn unknown_object_preserved_with_position() {
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Content><ofd:Layer Type=\"Body\">{PATH_XML}<ext:Mark xmlns:ext=\"urn:x\" ID=\"8\"/>{PATH_XML}</ofd:Layer></ofd:Content></ofd:Page>"
    );
    let report = parse_ofd(&page_package(&content, 10)).expect("parse");
    let layer = &report.document.pages[0].layers[0];
    assert_eq!(layer.objects.len(), 2);
    assert_eq!(layer.raw_children.len(), 1);
    assert_eq!(
        layer.raw_children[0].position, 1,
        "interleaved between objects"
    );
}

#[test]
fn unknown_attribute_captured_on_known_object() {
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Content><ofd:Layer Type=\"Body\"><ofd:TextObject ID=\"t1\" Boundary=\"0 0 50 10\" Font=\"4\" Size=\"10\" ReadOrder=\"1\"><ofd:TextCode X=\"0\" Y=\"5\">hi</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>";
    let report = parse_ofd(&page_package(content, 10)).expect("parse");
    let layer = &report.document.pages[0].layers[0];
    let PageObject::Text(t) = &layer.objects[0] else {
        panic!("expected TextObject");
    };
    assert_eq!(
        t.raw_attrs,
        vec![("ReadOrder".to_string(), "1".to_string())],
        "unmodelled attribute captured verbatim"
    );
}

#[test]
fn object_id_seed_scans_raw_fragment_ids() {
    let doc = OfdDocument {
        max_unit_id: 5,
        pages: vec![page_with(vec![Layer {
            raw_attrs: Vec::new(),
            layer_type: LayerType::Body,
            objects: vec![],
            raw_children: vec![rofd_dom::RawFragment {
                position: 0,
                xml: "<ext:Stamp ID=\"9999\"/>".into(),
            }],
        }])],
        ..OfdDocument::default()
    };
    assert_eq!(
        rofd_io::serialize::annotation::object_id_seed(&doc),
        9999,
        "raw fragment IDs must seed the allocator"
    );
}

// ---- Document-level passthrough capture (spec 2026-11-11 §6.2, Task 7) ----

fn custom_package(
    ofd_xml: &str,
    doc_xml: &str,
    content_xml: &str,
    extra: &[(&str, &str)],
) -> Vec<u8> {
    let mut files = vec![
        ("OFD.xml", ofd_xml),
        ("Doc_0/Document.xml", doc_xml),
        ("Doc_0/Pages/Page_0/Content.xml", content_xml),
    ];
    files.extend_from_slice(extra);
    build_zip(&files)
}

const PLAIN_CONTENT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer Type=\"Body\"/></ofd:Content></ofd:Page>";

#[test]
fn document_root_unknown_children_preserved() {
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages><ofd:Bookmarks><ofd:Outline Title=\"ch1\"/></ofd:Bookmarks></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let report = parse_ofd(&custom_package(ofd_xml, doc_xml, PLAIN_CONTENT, &[])).expect("parse");
    let raws = &report.document.raw_doc_children;
    assert_eq!(raws.len(), 1);
    assert_eq!(raws[0].position, 2, "after CommonData(0) and Pages(1)");
    assert!(
        raws[0].xml.contains("<ofd:Outline Title=\"ch1\"/>"),
        "{}",
        raws[0].xml
    );
}

#[test]
fn ofd_xml_body_and_docinfo_raws_preserved() {
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocInfo><ofd:DocID>D1</ofd:DocID><ofd:Subject>s</ofd:Subject></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot><ofd:Version>1.1</ofd:Version></ofd:DocBody></ofd:OFD>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
    let report = parse_ofd(&custom_package(ofd_xml, doc_xml, PLAIN_CONTENT, &[])).expect("parse");
    let body = &report.document.raw_ofd_body_children;
    assert_eq!(body.len(), 1);
    assert!(body[0].xml.contains("1.1</ofd:Version>"), "{}", body[0].xml);
    assert!(
        body[0].xml.contains("xmlns:ofd="),
        "fragment carries the injected namespace: {}",
        body[0].xml
    );
    let info = &report.document.meta.raw_info_children;
    assert_eq!(info.len(), 1);
    assert!(info[0].xml.contains("s</ofd:Subject>"), "{}", info[0].xml);
    assert_eq!(
        report.document.meta.doc_id.as_deref(),
        Some("D1"),
        "modelled DocID still parsed"
    );
}

#[test]
fn consumed_set_records_modeled_entries() {
    let res_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:MultiMedias><ofd:MultiMedia Type=\"Image\" Format=\"png\" ID=\"i1\"><ofd:MediaFile>img.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias></ofd:Res>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/><ofd:Page ID=\"P1\" BaseLoc=\"Pages/Page_1/Content.xml\"/></ofd:Pages></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let bytes = custom_package(
        ofd_xml,
        doc_xml,
        PLAIN_CONTENT,
        &[
            ("Doc_0/Pages/Page_1/Content.xml", PLAIN_CONTENT),
            ("Doc_0/Res/DocumentRes.xml", res_xml),
            ("Doc_0/Res/img.png", "\u{89}PNG-rest"),
            ("Attachments/file.dat", "blob"),
        ],
    );
    let report = parse_ofd(&bytes).expect("parse");
    let consumed = &report.package.consumed;
    for expected in [
        "OFD.xml",
        "Doc_0/Document.xml",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Pages/Page_1/Content.xml",
        "Doc_0/Res/DocumentRes.xml",
        "Doc_0/Res/img.png",
    ] {
        assert!(
            consumed.contains(expected),
            "consumed must contain {expected}: {consumed:?}"
        );
    }
    assert!(
        !consumed.contains("Attachments/file.dat"),
        "unconsumed entries stay out: {consumed:?}"
    );
}

// ---- Passthrough re-emission (spec 2026-11-11 §6.2, Task 8) ----

#[test]
fn layer_raw_children_interleave_in_write() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        raw_attrs: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![
            PageObject::Path(PathObject {
                id: ObjectId::new("p1"),
                boundary: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 1.0,
                },
                ctm: None,
                fill: None,
                stroke: None,
                line_width: 1.0,
                data: PathData {
                    commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(9.0, 0.0)],
                },
                draw_param: None,
                stroke_enabled: None,
                fill_enabled: None,
                raw_attrs: Vec::new(),
                clip: None,
            }),
            PageObject::Path(PathObject {
                id: ObjectId::new("p2"),
                boundary: Rect {
                    x: 0.0,
                    y: 5.0,
                    w: 10.0,
                    h: 1.0,
                },
                ctm: None,
                fill: None,
                stroke: None,
                line_width: 1.0,
                data: PathData {
                    commands: vec![PathCommand::M(0.0, 5.0), PathCommand::L(9.0, 5.0)],
                },
                draw_param: None,
                stroke_enabled: None,
                fill_enabled: None,
                raw_attrs: Vec::new(),
                clip: None,
            }),
        ],
        raw_children: vec![rofd_dom::RawFragment {
            position: 1,
            xml: "<ext:Mark xmlns:ext=\"urn:x\" ID=\"8\"/>".into(),
        }],
    }]));
    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    let mark_at = xml.find("<ext:Mark").expect("raw fragment emitted");
    let p1_at = xml.find("<ofd:PathObject ID=\"p1\"").expect("p1");
    let p2_at = xml.find("<ofd:PathObject ID=\"p2\"").expect("p2");
    assert!(
        p1_at < mark_at && mark_at < p2_at,
        "interleaved order: {xml}"
    );
}

#[test]
fn object_raw_attrs_reemitted() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        raw_attrs: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Text(TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 10.0,
            },
            ctm: None,
            font: FontId::new("4"),
            size: 10.0,
            fill: None,
            codes: vec![TextCode {
                glyph_ids: vec![],
                deltas: vec![(0.0, 0.0)],
                text: "hi".into(),
                x: 0.0,
                y: 5.0,
            }],
            draw_param: None,
            clip: None,
            raw_attrs: vec![("ReadOrder".into(), "1".into())],
        })],
        raw_children: Vec::new(),
    }]));
    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(xml.contains("ReadOrder=\"1\""), "{xml}");
}

#[test]
fn document_and_ofd_xml_raws_reemitted() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    doc.raw_doc_children = vec![rofd_dom::RawFragment {
        position: 2,
        xml: "<ofd:Bookmarks><ofd:Outline Title=\"ch1\"/></ofd:Bookmarks>".into(),
    }];
    doc.raw_common_children = vec![rofd_dom::RawFragment {
        position: 2,
        xml: "<ofd:Security/>".into(),
    }];
    doc.raw_ofd_body_children = vec![rofd_dom::RawFragment {
        position: 1,
        xml: "<ofd:Version>1.1</ofd:Version>".into(),
    }];
    doc.meta.raw_info_children = vec![rofd_dom::RawFragment {
        position: 1,
        xml: "<ofd:Subject>s</ofd:Subject>".into(),
    }];
    doc.meta.title = Some("T".into());

    let bytes = write_ofd(&doc).expect("write_ofd");
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        doc_xml.contains("<ofd:Bookmarks><ofd:Outline Title=\"ch1\"/></ofd:Bookmarks>"),
        "{doc_xml}"
    );
    assert!(
        doc_xml.find("</ofd:Pages>").unwrap() < doc_xml.find("<ofd:Bookmarks>").unwrap(),
        "root raws interleave after Pages: {doc_xml}"
    );
    assert!(
        doc_xml.contains("<ofd:Security/></ofd:CommonData>"),
        "{doc_xml}"
    );
    let ofd = read_entry(&bytes, "OFD.xml");
    assert!(ofd.contains("<ofd:Subject>s</ofd:Subject>"), "{ofd}");
    assert!(
        ofd.find("</ofd:DocInfo>").unwrap() < ofd.find("<ofd:Version>1.1</ofd:Version>").unwrap(),
        "body raws after DocInfo: {ofd}"
    );
    assert!(
        ofd.find("<ofd:DocRoot>").unwrap() < ofd.find("<ofd:Version>1.1</ofd:Version>").unwrap(),
        "body raws after DocRoot: {ofd}"
    );
}

#[test]
fn full_passthrough_stack_round_trips() {
    // Unknown content at every captured level must survive write -> parse.
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\" xmlns:ext=\"urn:x\"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer Type=\"Body\"><ofd:TextObject ID=\"t1\" Boundary=\"0 0 50 10\" Font=\"4\" Size=\"10\" ReadOrder=\"1\"><ofd:TextCode X=\"0\" Y=\"5\">hi</ofd:TextCode></ofd:TextObject><ext:Stamp ID=\"9\" ext:seal=\"1\"/></ofd:Layer></ofd:Content></ofd:Page>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID><ofd:Security/></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages><ofd:Bookmarks><ofd:Outline Title=\"ch1\"/></ofd:Bookmarks></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocInfo><ofd:DocID>D1</ofd:DocID><ofd:Subject>s</ofd:Subject></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot><ofd:Version>1.1</ofd:Version></ofd:DocBody></ofd:OFD>";
    let first = parse_ofd(&custom_package(ofd_xml, doc_xml, content, &[])).expect("parse");
    let bytes = write_ofd(&first.document).expect("write_ofd");
    let second = parse_ofd(&bytes).expect("re-parse");

    // Model-level equality, raws included.
    assert_eq!(
        second.document.pages, first.document.pages,
        "pages incl. layer raws"
    );
    assert_eq!(
        second.document.raw_doc_children,
        first.document.raw_doc_children
    );
    assert_eq!(
        second.document.raw_common_children,
        first.document.raw_common_children
    );
    assert_eq!(
        second.document.raw_ofd_body_children,
        first.document.raw_ofd_body_children
    );
    assert_eq!(
        second.document.meta.raw_info_children,
        first.document.meta.raw_info_children
    );
    // The object's unknown attribute survived.
    let PageObject::Text(t) = &second.document.pages[0].layers[0].objects[0] else {
        panic!("text object");
    };
    assert_eq!(
        t.raw_attrs,
        vec![("ReadOrder".to_string(), "1".to_string())]
    );
}

// ---- Package-layer transport (spec 2026-11-11 D12, Task 9) ----

fn entry_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut f = zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut out = Vec::new();
    f.read_to_end(&mut out).expect("read");
    out
}

#[test]
fn unconsumed_entries_transport_verbatim() {
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let report = parse_ofd(&custom_package(
        ofd_xml,
        doc_xml,
        PLAIN_CONTENT,
        &[
            ("Attachments/file.dat", "blob-bytes"),
            ("Doc_0/Signs/Sign_0/Signature.xml", "<Signature/>"),
        ],
    ))
    .expect("parse");
    let out = rofd_io::write_ofd_with_text_layout(
        &report.document,
        Some(&report.package),
        &rofd_dom::TextLayoutHints::new(),
    )
    .expect("write");
    assert_eq!(entry_bytes(&out, "Attachments/file.dat"), b"blob-bytes");
    assert_eq!(
        entry_bytes(&out, "Doc_0/Signs/Sign_0/Signature.xml"),
        b"<Signature/>"
    );
}

#[test]
fn nonstandard_entry_names_canonicalized_no_duplicates() {
    // Nonstandard Content.xml path (Pages/P0.xml) and a PublicRes.xml at the
    // doc root: full write canonicalizes names, consumed originals are NOT
    // transported, and the result re-parses to the same model.
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer Type=\"Body\"/></ofd:Content></ofd:Page>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/P0.xml\"/></ofd:Pages></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let res_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:MultiMedias><ofd:MultiMedia Type=\"Image\" Format=\"png\" ID=\"img1\"><ofd:MediaFile>img.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias></ofd:Res>";
    let png: Vec<u8> = PNG_MAGIC.to_vec();
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    use std::io::Write;
    for (n, c) in [
        ("OFD.xml", ofd_xml.as_bytes().to_vec()),
        ("Doc_0/Document.xml", doc_xml.as_bytes().to_vec()),
        ("Doc_0/Pages/P0.xml", content.as_bytes().to_vec()),
        ("Doc_0/PublicRes.xml", res_xml.as_bytes().to_vec()),
        ("Doc_0/img.png", png.clone()),
    ] {
        w.start_file::<_, ()>(n, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(&c).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();

    let report = parse_ofd(&bytes).expect("parse");
    assert!(
        report
            .document
            .resources
            .images
            .contains_key(&ImageId::new("img1")),
        "image loaded via nonstandard PublicRes"
    );
    let out = rofd_io::write_ofd_with_text_layout(
        &report.document,
        Some(&report.package),
        &rofd_dom::TextLayoutHints::new(),
    )
    .expect("write");

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
    let names: Vec<&str> = zip.file_names().collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        names.len(),
        sorted.len(),
        "no duplicate entry names: {names:?}"
    );
    assert!(
        names.contains(&"Doc_0/Pages/Page_0/Content.xml"),
        "{names:?}"
    );
    assert!(
        !names.contains(&"Doc_0/Pages/P0.xml"),
        "consumed original not transported: {names:?}"
    );
    assert!(
        !names.contains(&"Doc_0/PublicRes.xml"),
        "consumed original not transported: {names:?}"
    );
    assert!(
        !names.contains(&"Doc_0/img.png"),
        "consumed original not transported: {names:?}"
    );
    assert!(names.contains(&"Doc_0/Res/Img_img1.png"), "{names:?}");

    let second = parse_ofd(&out).expect("re-parse");
    assert_eq!(second.document.pages, report.document.pages);
    assert_eq!(second.document.resources, report.document.resources);
}

#[test]
fn generation_passes_none() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    let a = write_ofd(&doc).expect("write_ofd");
    let b = rofd_io::write_ofd_with_text_layout(&doc, None, &rofd_dom::TextLayoutHints::new())
        .expect("write with None");
    assert_eq!(a, b, "None path == generation path");
}

// ---- Final-review fix pass ----

#[test]
fn page_block_is_transparent_and_objects_parse() {
    // C1: PageBlock is a standard GB/T grouping element; its modeled
    // children must parse into the layer (the old name-based parser did).
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Content><ofd:Layer Type=\"Body\"><ofd:PageBlock>{PATH_XML}<ofd:PageBlock>{PATH_XML}</ofd:PageBlock></ofd:PageBlock></ofd:Layer></ofd:Content></ofd:Page>"
    );
    let report = parse_ofd(&page_package(&content, 10)).expect("parse");
    let layer = &report.document.pages[0].layers[0];
    assert_eq!(layer.objects.len(), 2, "PageBlock children parsed");
    assert!(
        layer.raw_children.is_empty(),
        "wrapper not captured as fragment"
    );
}

#[test]
fn page_root_raws_reemitted() {
    // I1: page-root unknown children must be written back.
    let mut doc = OfdDocument::default();
    let mut page = page_with(vec![]);
    page.raw_children = vec![rofd_dom::RawFragment {
        position: 0,
        xml: "<ofd:ExtraPageThing ID=\"77\"/>".into(),
    }];
    doc.pages.push(page);
    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    assert!(xml.contains("<ofd:ExtraPageThing ID=\"77\"/>"), "{xml}");
}

#[test]
fn interleave_never_drops_out_of_range_raws() {
    // I2: a raw whose position exceeds the writer's known slots (e.g.
    // captured after an <Annotations> the writer no longer emits) must
    // still be written, not silently dropped.
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![]));
    doc.raw_doc_children = vec![rofd_dom::RawFragment {
        position: 3,
        xml: "<ofd:Attachments FileLoc=\"a.dat\"/>".into(),
    }];
    let bytes = write_ofd(&doc).expect("write_ofd");
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        doc_xml.contains("<ofd:Attachments FileLoc=\"a.dat\"/>"),
        "{doc_xml}"
    );
}

#[test]
fn empty_form_unknowns_captured_at_document_and_ofd_level() {
    // I3: self-closing unknown elements must be captured too (the typical
    // form for attachment/outline/Version references).
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages><ofd:Attachments FileLoc=\"a.dat\"/></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot><ofd:Version/></ofd:DocBody></ofd:OFD>";
    let report = parse_ofd(&custom_package(ofd_xml, doc_xml, PLAIN_CONTENT, &[])).expect("parse");
    assert_eq!(
        report.document.raw_doc_children.len(),
        1,
        "empty Attachments captured"
    );
    assert!(report.document.raw_doc_children[0]
        .xml
        .contains("FileLoc=\"a.dat\""));
    assert_eq!(
        report.document.raw_ofd_body_children.len(),
        1,
        "empty Version captured"
    );
}

#[test]
fn prefixed_raw_attrs_carry_namespace_binding() {
    // I4: a prefixed unknown attribute must not dangle - the binding is
    // synthesized onto the object and survives re-parse.
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\" xmlns:ext=\"urn:x\"><ofd:Content><ofd:Layer Type=\"Body\"><ofd:TextObject ID=\"t1\" Boundary=\"0 0 50 10\" Font=\"4\" Size=\"10\" ext:seal=\"1\"><ofd:TextCode X=\"0\" Y=\"5\">hi</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let report = parse_ofd(&custom_package(ofd_xml, doc_xml, content, &[])).expect("parse");
    let bytes = write_ofd(&report.document).expect("write");
    let xml = content_xml(&bytes);
    assert!(xml.contains("ext:seal=\"1\""), "{xml}");
    assert!(
        xml.contains("xmlns:ext=\"urn:x\""),
        "binding present: {xml}"
    );
    // And the second round trip keeps the attribute (the xmlns pseudo-attr
    // re-synthesizes from the written declaration - a stable state).
    let second = parse_ofd(&bytes).expect("re-parse");
    let PageObject::Text(t) = &second.document.pages[0].layers[0].objects[0] else {
        panic!("text");
    };
    assert_eq!(
        t.raw_attrs,
        vec![
            ("ext:seal".to_string(), "1".to_string()),
            ("xmlns:ext".to_string(), "urn:x".to_string()),
        ]
    );
}

#[test]
fn layer_and_composite_raw_attrs_round_trip() {
    // I5: Layer ID and CompositeObject unknown attrs survive save.
    let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Content><ofd:Layer Type=\"Body\" ID=\"2\"><ofd:CompositeObject ID=\"c1\" Boundary=\"0 0 50 20\" ResourceID=\"Page_2/Content.xml\" Alpha=\"0.5\"/></ofd:Layer></ofd:Content></ofd:Page>";
    let doc_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:MaxUnitID>10</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"P0\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let report = parse_ofd(&custom_package(ofd_xml, doc_xml, content, &[])).expect("parse");
    assert_eq!(
        report.document.pages[0].layers[0].raw_attrs,
        vec![("ID".to_string(), "2".to_string())],
        "Layer ID captured"
    );
    let bytes = write_ofd(&report.document).expect("write");
    let xml = content_xml(&bytes);
    assert!(xml.contains("<ofd:Layer Type=\"Body\" ID=\"2\">"), "{xml}");
    assert!(xml.contains("Alpha=\"0.5\""), "{xml}");
}

#[test]
fn template_precedes_content() {
    // I6: GB/T CT_Page orders Template before Content.
    let mut doc = OfdDocument::default();
    let mut page = page_with(vec![]);
    page.template = Some("Pages/Tpl.xml".into());
    doc.pages.push(page);
    let bytes = write_ofd(&doc).expect("write_ofd");
    let xml = content_xml(&bytes);
    let tpl = xml.find("<ofd:Template").expect("template");
    let content_at = xml.find("<ofd:Content>").expect("content");
    assert!(tpl < content_at, "Template before Content: {xml}");
}

#[test]
fn diag_sample_roundtrip_keepall() {
    let bytes = std::fs::read("../../test/sample.ofd").expect("sample");
    let report = parse_ofd(&bytes).expect("parse");
    let out = rofd_io::write_ofd_with_text_layout(
        &report.document,
        Some(&report.package),
        &rofd_dom::TextLayoutHints::new(),
    )
    .expect("write");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
    let names: Vec<&str> = zip.file_names().collect();
    let src = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let src_names: Vec<String> = src.file_names().map(|s| s.to_string()).collect();
    // unconsumed entries transported?
    for n in &src_names {
        if report.package.consumed.contains(n.as_str()) {
            continue;
        }
        assert!(
            names.contains(&n.as_str()),
            "unconsumed {n} must transport; consumed={:?}; names={names:?}",
            report.package.consumed
        );
    }
    // all declared font ids survive?
    let mut zip2 = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
    let res_xml = String::from_utf8(
        zip2.by_name("Doc_0/Res/PublicRes.xml")
            .unwrap()
            .bytes()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    )
    .unwrap();
    for fid in report.document.resources.fonts.keys() {
        assert!(
            res_xml.contains(&format!("ID=\"{}\"", fid.0)),
            "font {} missing from write; model fonts={:?}",
            fid.0,
            report
                .document
                .resources
                .fonts
                .keys()
                .map(|k| k.0.clone())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn res_locs_are_element_text_standard_form() {
    // WPS/strict readers locate resources via the CT_CommonData loc
    // elements' TEXT (`<ofd:PublicRes>Res/PublicRes.xml</ofd:PublicRes>`),
    // not an attribute. GB/T child order: MaxUnitID, PageArea, then locs.
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
            stroke: None,
            fill: None,
            dash: None,
        },
    );

    let bytes = write_ofd(&doc).expect("write_ofd");
    let doc_xml = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        doc_xml.contains("<ofd:PublicRes>Res/PublicRes.xml</ofd:PublicRes>"),
        "PublicRes loc must be element text (WPS form): {doc_xml}"
    );
    assert!(
        doc_xml.contains("<ofd:DocumentRes>Res/DocumentRes.xml</ofd:DocumentRes>"),
        "DocumentRes loc must be element text (WPS form): {doc_xml}"
    );
    assert!(
        !doc_xml.contains("BaseLoc=\"Res/"),
        "no attribute-form resource locs in CommonData: {doc_xml}"
    );
    let max_at = doc_xml.find("<ofd:MaxUnitID>").expect("MaxUnitID");
    let area_at = doc_xml.find("<ofd:PageArea>").expect("PageArea");
    let pres_at = doc_xml.find("<ofd:PublicRes>").expect("PublicRes");
    assert!(
        max_at < area_at && area_at < pres_at,
        "GB/T child order: {doc_xml}"
    );

    // Conventional split: fonts + drawparams in PublicRes, images in
    // DocumentRes (the form real producers emit).
    let public_xml = read_entry(&bytes, "Doc_0/Res/PublicRes.xml");
    assert!(public_xml.contains("FontName=\"SimSun\""), "{public_xml}");
    assert!(public_xml.contains("<ofd:DrawParam "), "{public_xml}");
    assert!(!public_xml.contains("MultiMedia"), "{public_xml}");
    let docres_xml = read_entry(&bytes, "Doc_0/Res/DocumentRes.xml");
    assert!(docres_xml.contains("Img_img1.png"), "{docres_xml}");
    assert!(!docres_xml.contains("FontName"), "{docres_xml}");
    assert!(has_entry(&bytes, "Doc_0/Res/Font_4.ttf"));
    assert!(has_entry(&bytes, "Doc_0/Res/Img_img1.png"));
    assert_round_trip(&doc);
}

#[test]
fn clip_on_objects_and_dash_on_draw_param_round_trip() {
    // GB/T 33190: objects (Text/Path/Image) carry an optional <ofd:Clip>
    // child (its AbbreviatedData clips the object's rendering), and
    // Ct_DrawParam carries DashPattern/DashOffset dash attributes
    // (spec 2026-11-12-ofd-export D14). Both must survive the full write.
    let clip = PathData {
        commands: vec![
            PathCommand::M(10.0, 10.0),
            PathCommand::L(100.0, 10.0),
            PathCommand::L(100.0, 50.0),
            PathCommand::Z,
        ],
    };
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with(vec![Layer {
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![
            PageObject::Text(TextObject {
                raw_attrs: Vec::new(),
                id: ObjectId::new("t1"),
                boundary: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 90.0,
                    h: 40.0,
                },
                ctm: None,
                font: FontId::new("4"),
                size: 4.0,
                fill: Some(Color::Rgb(0, 0, 0)),
                codes: vec![TextCode {
                    glyph_ids: vec![],
                    // Non-zero advances: an empty deltas vec is written with
                    // no DeltaX attribute and re-parsed as all-zero per-char
                    // deltas (7 chars), never as empty.
                    deltas: vec![(2.0, 0.0); 7],
                    text: "clipped".into(),
                    x: 12.0,
                    y: 20.0,
                }],
                draw_param: None,
                clip: Some(clip.clone()),
            }),
            PageObject::Path(PathObject {
                raw_attrs: Vec::new(),
                id: ObjectId::new("p1"),
                boundary: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 10.0,
                },
                ctm: None,
                fill: None,
                stroke: Some(Color::Rgb(0, 0, 255)),
                line_width: 0.35,
                data: PathData {
                    commands: vec![PathCommand::M(0.0, 5.0), PathCommand::L(200.0, 5.0)],
                },
                draw_param: Some(DrawParamId::new("dp1")),
                stroke_enabled: Some(true),
                fill_enabled: None,
                clip: Some(clip.clone()),
            }),
            PageObject::Image(ImageObject {
                raw_attrs: Vec::new(),
                id: ObjectId::new("i1"),
                boundary: Rect {
                    x: 20.0,
                    y: 20.0,
                    w: 50.0,
                    h: 50.0,
                },
                ctm: None,
                image: ImageId::new("img1"),
                clip: Some(clip),
            }),
        ],
    }]));
    doc.resources
        .images
        .insert(ImageId::new("img1"), Arc::new(PNG_MAGIC.to_vec()));
    doc.resources.draw_params.insert(
        DrawParamId::new("dp1"),
        DrawParam {
            line_width: None,
            stroke: None,
            fill: None,
            dash: Some(DashSpec {
                pattern: vec![4.0, 3.0],
                offset: 0.0,
            }),
        },
    );

    // Model round-trip (write -> parse -> equality) plus wire-form pins.
    let back = assert_round_trip(&doc);
    let page = &back.pages[0];
    let objects = &page.layers[0].objects;
    assert_eq!(objects.len(), 3);
    match &objects[0] {
        PageObject::Text(t) => assert!(
            t.clip.is_some(),
            "TextObject clip must survive the round trip"
        ),
        other => panic!("expected TextObject, got {other:?}"),
    }
    match &objects[1] {
        PageObject::Path(p) => {
            assert!(p.clip.is_some(), "PathObject clip must survive");
            assert_eq!(p.draw_param.as_ref().map(|d| d.0.as_str()), Some("dp1"));
        }
        other => panic!("expected PathObject, got {other:?}"),
    }
    match &objects[2] {
        PageObject::Image(i) => assert!(
            i.clip.is_some(),
            "ImageObject clip must survive the round trip"
        ),
        other => panic!("expected ImageObject, got {other:?}"),
    }
    let dp = back
        .resources
        .draw_params
        .get(&DrawParamId::new("dp1"))
        .expect("DrawParam survives");
    assert_eq!(
        dp.dash,
        Some(DashSpec {
            pattern: vec![4.0, 3.0],
            offset: 0.0
        }),
        "dash pattern must survive the round trip"
    );

    // Wire form: the Clip child element and the dash attributes.
    let bytes = write_ofd(&doc).expect("write_ofd");
    let content = content_xml(&bytes);
    assert!(
        content.matches("<ofd:Clip>").count() >= 3,
        "every clipped object emits a Clip child: {content}"
    );
    assert!(
        content.contains("<ofd:AbbreviatedData>M 10 10 L 100 10 L 100 50 Z"),
        "clip payload is abbreviated path data: {content}"
    );
    let public_xml = read_entry(&bytes, "Doc_0/Res/PublicRes.xml");
    assert!(
        public_xml.contains("DashPattern=\"4 3\""),
        "DashPattern attribute on DrawParam: {public_xml}"
    );
    assert!(
        public_xml.contains("DashOffset=\"0\""),
        "DashOffset attribute on DrawParam: {public_xml}"
    );
}
