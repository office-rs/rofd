//! Structural PDF assertions for `pdf_from_page_scenes` (print spec §7):
//! PDF header, per-page `/MediaBox` count, byte determinism, and font
//! embedding — byte-shape checks, never pixel comparisons.

use std::sync::Arc;

use imaging::record::Scene;
use rofd_export::{pdf_from_page_scenes, ExportError, PdfPage};

/// Font fixture shared with the render crate's tests (crates/export/tests
/// → up two levels reaches crates/).
const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

fn blank_page(w: f32, h: f32) -> PdfPage {
    let mut scene = Scene::new();
    {
        let mut p = imaging::Painter::new(&mut scene);
        p.fill_rect(
            imaging::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64),
            imaging::peniko::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
        );
    }
    PdfPage {
        width_px: w,
        height_px: h,
        scene,
    }
}

fn mediabox_count(b: &[u8]) -> usize {
    b.windows(9).filter(|w| w == b"/MediaBox").count()
}

#[test]
fn empty_pages_is_error() {
    assert!(matches!(
        pdf_from_page_scenes(&[]),
        Err(ExportError::EmptyPages)
    ));
}

#[test]
fn two_pages_yield_two_mediaboxes_and_pdf_header() {
    let out = pdf_from_page_scenes(&[blank_page(200.0, 100.0), blank_page(200.0, 100.0)]).unwrap();
    assert!(out.starts_with(b"%PDF-"));
    assert_eq!(mediabox_count(&out), 2);
}

#[test]
fn encode_is_deterministic() {
    // Same input twice must encode byte-identically: the document carries
    // no dates (print spec §2.7).
    let pages = [blank_page(200.0, 100.0), blank_page(120.0, 240.0)];
    let a = pdf_from_page_scenes(&pages).unwrap();
    let b = pdf_from_page_scenes(&pages).unwrap();
    assert_eq!(a, b, "same input must encode byte-identically");
}

#[test]
fn glyph_run_embeds_fontfile_and_tounicode() {
    let mut scene = Scene::new();
    {
        // Map 'D' through the font's own cmap so the glyph id is real and
        // the ToUnicode reverse cmap can explain it.
        let font_ref = swash::FontRef::from_index(FONT, 0).expect("TestFont must be readable");
        let gid = swash::Charmap::from_font(&font_ref).map('D');
        assert_ne!(gid, 0, "TestFont must map 'D' to a real glyph");

        let font = imaging::peniko::FontData {
            data: imaging::peniko::Blob::new(Arc::new(FONT.to_vec())),
            index: 0,
        };
        let mut p = imaging::Painter::new(&mut scene);
        p.fill_rect(
            imaging::kurbo::Rect::new(0.0, 0.0, 200.0, 100.0),
            imaging::peniko::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
        );
        p.glyphs(
            &font,
            imaging::peniko::Color::from_rgba8(0x00, 0x00, 0x00, 0xFF),
        )
        .font_size(32.0)
        .draw(
            &imaging::peniko::Style::Fill(imaging::peniko::Fill::NonZero),
            [imaging::record::Glyph {
                x: 0.0,
                y: 32.0,
                id: u32::from(gid),
            }],
        );
    }
    let out = pdf_from_page_scenes(&[PdfPage {
        width_px: 200.0,
        height_px: 100.0,
        scene,
    }])
    .unwrap();
    // The glyph run reached krilla as text: an embedded font program
    // (FontFile2 for TrueType) and a ToUnicode cmap must exist. A dropped
    // run would produce neither (every drop path also warns first).
    assert!(
        out.windows(9).any(|w| w == b"/FontFile"),
        "embedded font program missing"
    );
    assert!(
        out.windows(10).any(|w| w == b"/ToUnicode"),
        "ToUnicode cmap missing"
    );
}
