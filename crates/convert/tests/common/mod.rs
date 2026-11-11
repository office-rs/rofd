//! Shared fixtures for the rofd-convert integration tests (convert spec
//! phase 1): plain and annotated OFD byte packages built through the real
//! `rofd-io` writer. Shared with the Task 6 host-side tests.
//!
//! `write_ofd`'s v1 full-write emits no resource entries (DocumentRes.xml /
//! image / font files are a generation-path gap, not a bug — see the media
//! budget note in the task brief), so fixtures that must carry resource
//! bytes through `parse_ofd` are repacked with [`repack_with_resources`]
//! using the same `zip` crate the io layer uses.

// Shared across test binaries: each binary exercises a subset of the helpers.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::sync::Arc;

use rofd_component::PrintContent;
use rofd_convert::{ConvertOptions, ConvertOutput, Converter, OutputFormat};
use rofd_dom::{Color, FontId, ImageId, Layer, OfdDocument, Page, PageId, Point, Rect};

/// Convert `bytes` to PDF with default options (all layers on, default
/// budgets), panicking on failure.
pub fn convert_default(bytes: &[u8]) -> ConvertOutput {
    Converter::new()
        .convert(OutputFormat::Pdf, bytes, &ConvertOptions::default())
        .expect("convert succeeds")
}

/// Convert `bytes` to PDF with the given content flags (other options
/// default), returning the encoded bytes.
pub fn convert_with(bytes: &[u8], content: PrintContent) -> Vec<u8> {
    let opts = ConvertOptions {
        content,
        ..ConvertOptions::default()
    };
    Converter::new()
        .convert(OutputFormat::Pdf, bytes, &opts)
        .expect("convert succeeds")
        .bytes
}

/// Minimal 1x1 white PNG (valid IHDR/IDAT/IEND), mirroring the component
/// print-session fixture: stamps decode through `image::load_from_memory`,
/// so fixture bytes must be a real image.
pub const TINY_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 255, 255, 63, 0, 5,
    254, 2, 254, 13, 239, 70, 184, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// The render test fixture font (a real TTF), the same bytes the component
/// print-session tests shape with. Registering it as the process default
/// font makes watermark text shape deterministically with no system-font
/// dependence.
pub const TEST_FONT: &[u8] = include_bytes!("../../../render/tests/fixtures/fonts/TestFont.ttf");

/// One 210x297mm single-layer empty page (mirrors the component test
/// construction).
pub fn page(id: &str) -> Page {
    Page {
        raw_children: Vec::new(),
        id: PageId::new(id),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 210.0,
            h: 297.0,
        },
        layers: vec![Layer::default()],
        template: None,
    }
}

/// Two plain A4 pages, no annotations, no resources.
pub fn two_page_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(page("P0"));
    doc.pages.push(page("P1"));
    doc
}

/// `two_page_doc` serialized through the real `rofd-io` full writer.
pub fn fixture_bytes() -> Vec<u8> {
    rofd_io::write_ofd(&two_page_doc()).expect("two-page fixture writes")
}

/// A two-page package carrying one annotation per content layer (convert
/// spec Review Focus 4): a Highlight and a Stamp on P0, a Watermark on P1.
/// The annotations are authored through the real component command path
/// (`create_annotation` stamps creator/created from `set_clock`), then
/// serialized with `write_ofd`; the stamp's image bytes — which the v1
/// full-write cannot carry — are repacked in as a `DocumentRes.xml`
/// declared resource so every content flag has an observable draw target
/// after `parse_ofd`.
pub fn annotated_fixture() -> Vec<u8> {
    use rofd_dom::{AnnotationKind, AnnotationPayload};

    let mut c =
        rofd_component::OfdComponent::new_native(rofd_component::OfdConfig::new(Arc::new(vec![])));
    c.set_clock("t".into(), 1);
    c.load_document(two_page_doc());
    c.create_annotation(
        AnnotationKind::Highlight,
        PageId::new("P0"),
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
            color: Color::Rgb(255, 255, 0),
        },
    );
    c.create_annotation(
        AnnotationKind::Stamp,
        PageId::new("P0"),
        AnnotationPayload::Stamp {
            rect: Rect {
                x: 20.0,
                y: 20.0,
                w: 60.0,
                h: 60.0,
            },
            image: ImageId::new("IMG-STAMP"),
        },
    );
    c.create_annotation(
        AnnotationKind::Watermark,
        PageId::new("P1"),
        AnnotationPayload::Watermark {
            rect: Rect {
                x: 50.0,
                y: 130.0,
                w: 110.0,
                h: 40.0,
            },
            content: "t".into(),
            opacity: 0.5,
            angle: 0.785,
            font: FontId::new("F-wm"),
            size: 40.0,
            color: Color::Rgb(128, 128, 128),
        },
    );
    let base = rofd_io::write_ofd(c.document()).expect("annotated fixture writes");
    repack_with_resources(
        &base,
        &[
            (
                "Doc_0/Res/DocumentRes.xml".to_string(),
                document_res_xml("IMG-STAMP", "stamp.png").into_bytes(),
            ),
            ("Doc_0/Res/stamp.png".to_string(), TINY_PNG.to_vec()),
        ],
    )
}

/// A minimal `DocumentRes.xml` declaring one image resource (the exact
/// shape `rofd-io`'s `parse_res` pins in its own unit tests: `MediaFile`
/// text is the path relative to the entry's own directory).
pub fn document_res_xml(image_id: &str, file: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\" BaseLoc=\"\">\
         <ofd:MultiMedias>\
         <ofd:MultiMedia Type=\"Image\" Format=\"PNG\" ID=\"{image_id}\">\
         <ofd:MediaFile>{file}</ofd:MediaFile>\
         </ofd:MultiMedia>\
         </ofd:MultiMedias>\
         </ofd:Res>"
    )
}

/// Read `ofd`, append `extra` entries, and re-zip with deflate (the same
/// writer posture as `rofd-io`'s `zip_util::write_zip`). Test-side door for
/// giving `write_ofd`-produced fixtures resource entries the v1 full-write
/// does not emit.
pub fn repack_with_resources(ofd: &[u8], extra: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(ofd)).expect("fixture zip reads");
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(archive.len() + extra.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("fixture zip entry");
        let name = entry.name().to_string();
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).expect("fixture entry bytes");
        drop(entry);
        entries.push((name, bytes));
    }
    entries.extend_from_slice(extra);

    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in &entries {
        zip.start_file(name, opts).expect("fixture entry starts");
        zip.write_all(bytes).expect("fixture entry writes");
    }
    zip.finish().expect("fixture zip finishes").into_inner()
}

/// Count `/MediaBox` occurrences — one per PDF page (structural page
/// count, the same posture as the xilem-app export tests).
pub fn mediabox_count(bytes: &[u8]) -> usize {
    bytes.windows(9).filter(|w| w == b"/MediaBox").count()
}

/// The editor export path (convert spec D18's reference pipeline): parse
/// the same bytes, load them into a real component (empty default font),
/// begin a print session, and assemble the per-page `PdfPage` list through
/// the SAME shared helper (`rofd_convert::pdf_pages`) the convert pipeline
/// and every host export path use — parity is by construction now, so this
/// stand-in only supplies the editor-side session (component-loaded
/// document, config-empty fonts) and the encode step.
///
/// Registry-free by construction — the component builds its own
/// `FontStore` from the document resources and the config's empty default,
/// never consulting the process font registry — so this is also the
/// byte-exact stand-in for "convert output on an empty registry" wherever
/// the calling binary has already anchored the registry.
pub fn editor_path_pdf(bytes: &[u8], content: PrintContent) -> Vec<u8> {
    let doc = rofd_io::parse_ofd(bytes).expect("fixture parses").document;
    let mut component =
        rofd_component::OfdComponent::new_native(rofd_component::OfdConfig::new(Arc::new(vec![])));
    component.load_document(doc);
    let mut session = component.begin_print(content);
    let pages = rofd_convert::pdf_pages(&mut session);
    rofd_export::pdf_from_page_scenes(&pages).expect("editor path encodes")
}

/// A minimal `DocumentRes.xml` declaring one font whose `FontFile` entry is
/// absent from the package (same pinned shape family as
/// [`document_res_xml`]; the `ofd:Font`/`ofd:FontFile` nesting follows what
/// `resource::parse_res` pins in its own unit tests). Parsing it emits
/// `OfdWarning::FontSubstituted` at the io warning site.
pub fn document_res_xml_missing_font(font_id: &str, family: &str, file: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <ofd:Res xmlns:ofd=\"http://www.ofdspec.org/2016\" BaseLoc=\"\">\
         <ofd:Fonts>\
         <ofd:Font ID=\"{font_id}\" FontName=\"{family}\">\
         <ofd:FontFile>{file}</ofd:FontFile>\
         </ofd:Font>\
         </ofd:Fonts>\
         </ofd:Res>"
    )
}

/// `fixture_bytes()` repacked with a font declaration whose file is absent:
/// a package whose `parse_ofd` report carries a REAL (non-empty) warning.
/// The fixture behind the warnings-passthrough test — an empty-vs-empty
/// comparison would prove nothing.
pub fn missing_font_fixture() -> Vec<u8> {
    repack_with_resources(
        &fixture_bytes(),
        &[(
            "Doc_0/Res/DocumentRes.xml".to_string(),
            document_res_xml_missing_font("F-gone", "GoneFont", "gone.ttf").into_bytes(),
        )],
    )
}
