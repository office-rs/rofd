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
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::AnnotationSelection;
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
