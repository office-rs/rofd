//! Smoke test: composite a paper-on-desk Scene for the fixture page without
//! panicking.
//!
//! Reuses the io crate's fixture builder (`build_minimal_ofd`) to get a parsed
//! Page with a TextObject and a PathObject, then drives the render pipeline
//! (composite -> hit_test, plus isolated body / annotation draws). Assertions
//! are coarse: calls return without panicking. imaging::record::Scene has no
//! introspection equivalent to vello's `encoding()`, so we only assert
//! non-panic.

use std::sync::Arc;

use imaging::kurbo::Rect as KurboRect;
use imaging::record::{Command, Draw, Scene};
use imaging::Painter;
use rofd_dom::{AnnotationSelection, PageObject};
use rofd_render::{
    draw_annotations, draw_body, hit_test, FontStore, RenderEngine, Viewport, PX_PER_MM,
};

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

/// Draw a page's body into a fresh scene (non-panic gate).
fn body_scene_for_fixture_page() -> Scene {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    painter.fill_rect(KurboRect::new(0.0, 0.0, 800.0, 600.0), peniko::Color::BLACK);
    // The fixture page has a TextObject ("Hello", font F1, black fill, no CTM)
    // and a PathObject (red stroke, fill none, line_width 1, no CTM).
    draw_body(
        &mut painter,
        page,
        &report.document.resources,
        &fonts,
        (0.0, 0.0),
        1.0,
    );
    scene
}

#[test]
fn body_scene_draws_for_fixture_page() {
    let _ = body_scene_for_fixture_page();
}

#[test]
fn annotation_scene_draws_for_fixture() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];
    let anns = report.document.annotations.for_page(&page.id);
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    draw_annotations(
        &mut painter,
        anns,
        &report.document.resources,
        &fonts,
        (0.0, 0.0),
        1.0,
    );
    let _ = scene;
}

#[test]
fn composite_builds_paper_on_desk_scene() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let engine = RenderEngine::new(Arc::new(vec![]));
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };
    let _scene = engine.composite(
        &report.document,
        &vp,
        &fonts,
        &AnnotationSelection::None,
        None,
        None,
    ); // built without panic
}

/// End-to-end: parse fixture -> composite -> hit_test -> re-composite.
/// Exercises the full render pipeline with no panic.
#[test]
fn end_to_end_parse_composite_hit_test() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let engine = RenderEngine::new(Arc::new(vec![]));
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };

    let _scene = engine.composite(
        &report.document,
        &vp,
        &fonts,
        &AnnotationSelection::None,
        None,
        None,
    );

    // Hit-test somewhere on page 0 (annotation entries exist in the fixture).
    // The result is not asserted on a specific target - the gate is that the
    // full geometry path runs without panicking for a point on the page.
    let _hit = hit_test(
        &report.document,
        &vp,
        &AnnotationSelection::None,
        (400.0, 50.0),
    );

    // Re-composite (simulates a repaint after a state change). Must not panic.
    let _scene2 = engine.composite(
        &report.document,
        &vp,
        &fonts,
        &AnnotationSelection::None,
        None,
        None,
    );
}

/// Parses the real `test/ru-yuan-ji-lu.ofd` (if present locally) and composites
/// a scene, asserting 3 pages with non-zero physical boxes, resolved DrawParams
/// + images, and that the body draws without panic. Ignored by default: the
///   fixture file is gitignored (not in CI). Run with `--ignored real_ofd`.
#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn real_ofd_parses_and_composites() {
    let bytes = match std::fs::read("../../test/ru-yuan-ji-lu.ofd") {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skipping: real OFD not readable: {e}");
            return;
        }
    };
    let report = rofd_io::parse_ofd(&bytes).expect("real OFD parses");
    assert_eq!(report.document.pages.len(), 3, "3 pages");
    for (i, p) in report.document.pages.iter().enumerate() {
        assert!(
            p.physical_box.w > 200.0,
            "page {i} physical_box.w = {}",
            p.physical_box.w
        );
        assert!(
            p.physical_box.h > 290.0,
            "page {i} physical_box.h = {}",
            p.physical_box.h
        );
    }
    assert!(
        !report.document.resources.draw_params.is_empty(),
        "DrawParams parsed"
    );
    assert_eq!(report.document.resources.images.len(), 2, "2 images loaded");

    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let engine = RenderEngine::new(Arc::new(vec![]));
    let vp = Viewport {
        zoom: PX_PER_MM,
        size: (1000.0, 1400.0),
        page_gap: 20.0,
        ..Default::default()
    };
    let _scene = engine.composite(
        &report.document,
        &vp,
        &fonts,
        &AnnotationSelection::None,
        None,
        None,
    ); // composites without panic

    // Draw page 0's body in isolation (non-panic).
    let mut body_scene = Scene::new();
    let mut painter = Painter::new(&mut body_scene);
    draw_body(
        &mut painter,
        &report.document.pages[0],
        &report.document.resources,
        &fonts,
        (0.0, 0.0),
        PX_PER_MM,
    );
    let _ = body_scene;
}

/// Parses the real `test/sample.ofd` (if present locally) and verifies body
/// text renders by the document glyph IDs (the glyph-by-ID path), not parley
/// shape (which returns .notdef on the cmap-less subset font and vanishes the
/// text). Ignored: the fixture is gitignored. Run with `--ignored sample_ofd`.
#[test]
#[ignore = "requires the real OFD at ../../test/sample.ofd"]
fn sample_ofd_body_text_renders_by_glyph_ids() {
    let bytes = std::fs::read("../../test/sample.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("sample parses");
    let page0 = &report.document.pages[0];
    let first_text = page0
        .layers
        .iter()
        .flat_map(|l| {
            l.objects.iter().filter_map(|o| match o {
                PageObject::Text(t) => Some(t),
                _ => None,
            })
        })
        .next()
        .expect("page 0 has a body TextObject");
    let expected_ids = first_text.codes[0].glyph_ids.clone();
    assert!(!expected_ids.is_empty(), "TextCode has glyph_ids");

    // Empty default font: body text must resolve the document font
    // (font_4_4.ttf, in resources) and draw by glyph_ids.
    let fonts = FontStore::from_resources(&report.document.resources, Arc::new(vec![]));
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    draw_body(
        &mut painter,
        page0,
        &report.document.resources,
        &fonts,
        (0.0, 0.0),
        PX_PER_MM,
    );

    // A glyph run whose IDs equal the first TextCode's glyph_ids must exist
    // (the glyph-by-ID path). If shape were used instead, the GlyphRun would
    // carry .notdef (id 0) because font_4_4.ttf has no cmap.
    let found = scene.commands().iter().any(|cmd| {
        if let Command::Draw(id) = cmd {
            if let Draw::GlyphRun(gr) = scene.draw_op(*id) {
                let ids: Vec<u32> = gr.glyphs.iter().map(|g| g.id).collect();
                return ids == expected_ids;
            }
        }
        false
    });
    assert!(
        found,
        "body text must draw by glyph_ids {:?} (glyph-by-ID path, not shape)",
        expected_ids
    );
}

/// Parses the real `test/sample-content.ofd` (if present locally) and verifies
/// the WPS table lines render: the file's 10 border PathObjects (5 horizontal +
/// 5 vertical) carry NO StrokeColor/FillColor/DrawParam and rely on the
/// GB/T 33190 表35 defaults (Stroke 缺省 true, StrokeColor 缺省黑色). The old
/// renderer skipped them entirely. Ignored: run with `--ignored
/// sample_content_table`.
#[test]
#[ignore = "requires the real OFD at ../../test/sample-content.ofd"]
fn sample_content_table_lines_stroke_by_default() {
    let bytes = std::fs::read("../../test/sample-content.ofd").expect("sample present");
    let report = rofd_io::parse_ofd(&bytes).expect("sample parses");
    let page0 = &report.document.pages[0];

    // Sanity: the model carries the WPS table borders with no colors resolved.
    let borders: Vec<&rofd_dom::PathObject> = page0
        .layers
        .iter()
        .flat_map(|l| l.objects.iter())
        .filter_map(|o| match o {
            PageObject::Path(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(borders.len(), 10, "10 table-border PathObjects");
    assert!(
        borders
            .iter()
            .all(|p| p.stroke.is_none() && p.fill.is_none()),
        "borders carry no inline colors (WPS relies on spec defaults)"
    );

    let fonts = FontStore::from_resources(&report.document.resources, Arc::new(vec![]));
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    draw_body(
        &mut painter,
        page0,
        &report.document.resources,
        &fonts,
        (0.0, 0.0),
        PX_PER_MM,
    );

    // All 10 borders must stroke black, landing in the table region
    // x [29.6, 180.4]mm, y [96.8, 119.8]mm (Boundary values, zoom-scaled).
    use imaging::kurbo::Shape as _;
    let mut strokes = 0;
    for cmd in scene.commands() {
        if let Command::Draw(id) = cmd {
            if let Draw::Stroke {
                transform,
                brush,
                shape,
                ..
            } = scene.draw_op(*id)
            {
                let bb = shape.to_path(1e-3).bounding_box();
                let p0 = *transform * imaging::kurbo::Point::new(bb.x0, bb.y0);
                let p1 = *transform * imaging::kurbo::Point::new(bb.x1, bb.y1);
                let r = KurboRect::from_points(p0, p1);
                assert!(
                    r.x0 > 29.0 * PX_PER_MM && r.x1 < 181.0 * PX_PER_MM,
                    "stroke inside table x-range, got {r:?}"
                );
                assert!(
                    r.y0 > 96.0 * PX_PER_MM && r.y1 < 120.5 * PX_PER_MM,
                    "stroke inside table y-range, got {r:?}"
                );
                if let peniko::Brush::Solid(c) = brush {
                    let rgba = c.to_rgba8();
                    assert_eq!(
                        (rgba.r, rgba.g, rgba.b),
                        (0, 0, 0),
                        "default stroke color is black"
                    );
                }
                strokes += 1;
            }
        }
    }
    assert_eq!(strokes, 10, "all 10 table borders stroked, got {strokes}");
}
