//! Host-layer integration test for save-as-PDF (`host::export_pdf`).
//!
//! Pure-construction document (no real sample, no `#[ignore]`): A4 pages
//! plus a Highlight annotation, exported through the same host function
//! the toolbar button uses. Assertions are structural bytes on disk —
//! PDF header, one `/MediaBox` per page — never pixel comparisons.

use std::path::PathBuf;
use std::sync::Arc;

use rofd_component::{OfdComponent, OfdConfig, PrintContent};
use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, Layer, OfdDocument, Page,
    PageId, Point, Rect,
};
use rofd_export::{pdf_from_page_scenes, PdfPage};
use xilem_app::host::export_pdf::export_pdf;

/// One A4-portrait page (210x297mm), empty body layer — the component
/// crate's own print-session fixture shape.
fn a4_page(id: &str) -> Page {
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

/// A yellow Highlight on page `P0`.
fn highlight_ann(id: u64) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(id),
        kind: AnnotationKind::Highlight,
        page: PageId::new("P0"),
        creator: "t".into(),
        created: 1,
        modified: 1,
        reply_to: None,
        payload: AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
            color: Color::Rgb(255, 255, 0),
        },
    }
}

/// Component with `n` A4 pages and a Highlight on P0. Empty default font,
/// like the host itself (system fallback — the fixture has no text to
/// shape).
fn component_with_pages(n: usize) -> OfdComponent {
    let mut doc = OfdDocument::default();
    for i in 0..n {
        doc.pages.push(a4_page(&format!("P{i}")));
    }
    doc.annotations.insert(highlight_ann(1));
    let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    c.set_clock("t".into(), 1);
    c.load_document(doc);
    c
}

fn mediabox_count(b: &[u8]) -> usize {
    b.windows(9).filter(|w| w == b"/MediaBox").count()
}

/// Unique temp path (process id); no tempfile dep needed.
fn temp_pdf(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rofd_xilem_export_{name}_{}.pdf",
        std::process::id()
    ))
}

/// The manual pipeline the host function is expected to run: `content`
/// preset, every page, page-local scenes at print zoom.
fn manual_encode(c: &OfdComponent, content: PrintContent) -> Vec<u8> {
    let mut session = c.begin_print(content);
    let mut pages = Vec::with_capacity(session.page_count());
    for i in 0..session.page_count() {
        let (w, h) = session.page_rect(i);
        pages.push(PdfPage {
            width_px: w as f32,
            height_px: h as f32,
            scene: session.page_scene(i),
        });
    }
    pdf_from_page_scenes(&pages).expect("manual encode succeeds")
}

#[test]
fn export_pdf_writes_one_page_pdf() {
    let c = component_with_pages(1);
    let path = temp_pdf("one");
    export_pdf(&c, &path).expect("export succeeds");
    let bytes = std::fs::read(&path).expect("pdf written to disk");
    assert!(bytes.starts_with(b"%PDF-"), "PDF header");
    assert_eq!(mediabox_count(&bytes), 1, "one page -> one /MediaBox");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn highlight_page_and_blank_page_each_get_one_mediabox() {
    // Every document page exports regardless of the all-on default
    // preset: P0 (carries the Highlight) and the blank P1 each contribute
    // exactly one /MediaBox.
    let c = component_with_pages(2);
    let path = temp_pdf("two");
    export_pdf(&c, &path).expect("export succeeds");
    let bytes = std::fs::read(&path).expect("pdf written to disk");
    assert_eq!(mediabox_count(&bytes), 2, "two pages -> two /MediaBox");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn default_export_is_all_layers_preset() {
    let c = component_with_pages(1);
    let path = temp_pdf("preset");
    export_pdf(&c, &path).expect("export succeeds");
    let exported = std::fs::read(&path).expect("pdf written to disk");
    let _ = std::fs::remove_file(&path);

    // The host must run PrintContent::default() over all pages: identical
    // input encodes byte-identically, so the exported file equals the
    // manual all-on-preset pipeline.
    assert!(
        exported == manual_encode(&c, PrintContent::default()),
        "export must equal the default-preset manual pipeline"
    );

    // The equality is only meaningful because the flag changes the bytes:
    // with annotations off, the Highlight quad loses its fill and the
    // encode differs — proving the default export really included it.
    let without_annotations = PrintContent {
        annotations: false,
        ..PrintContent::default()
    };
    assert_ne!(
        exported,
        manual_encode(&c, without_annotations),
        "annotations-off encode must differ (the Highlight was really included by default)"
    );
}
