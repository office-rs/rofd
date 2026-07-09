//! Smoke test: build a body Scene for the fixture page without panicking.
//!
//! Reuses the io crate's fixture builder (`build_minimal_ofd`) to get a parsed
//! Page with a TextObject and a PathObject, then builds a FontStore (default
//! font = TestFont.ttf, since the fixture's Font.xml declares no FontFile) and
//! calls `build_body_scene`. The assertion is coarse: the call returns without
//! panicking and `scene.encoding()` exists (vello 0.8 introspection).

use std::sync::Arc;

use rofd_render::{
    build_annotation_scene, build_body_scene, hit_test, FontStore, PageSceneCache, RenderEngine,
    Viewport,
};

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn body_scene_builds_for_fixture_page() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];

    // Build the body scene - must not panic. The fixture page has a TextObject
    // ("Hello", font F1, black fill, no CTM) and a PathObject (red stroke,
    // fill none, line_width 1, no CTM).
    let scene = build_body_scene(page, &report.document.resources, &fonts);

    // Coarse structure: the scene encoding exists (non-trivial content was
    // encoded for the path + text objects). We do not assert on vello internals
    // beyond this - the gate is "builds without panic".
    let _ = scene.encoding();
}

#[test]
fn annotation_scene_builds_for_fixture() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let fonts = FontStore::from_resources(&report.document.resources, font_bytes);
    let page = &report.document.pages[0];
    let anns = report.document.annotations.for_page(&page.id);
    let _scene = build_annotation_scene(anns, &report.document.resources, &fonts);
    // No panic; overlay built.
}

#[test]
fn composite_builds_paper_on_desk_scene() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let engine = RenderEngine::new(font_bytes);
    let mut cache = PageSceneCache::new();
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };
    let scene = engine.composite(&report.document, &vp, &mut cache);
    let _ = scene.encoding(); // built without panic
}

/// End-to-end: parse fixture -> composite -> hit_test -> invalidate annotation
/// cache -> re-composite. Exercises the full render pipeline with no panic.
///
/// This is the Phase 2 integration gate: the io fixture builder produces a
/// parsed document (text + path body objects + annotations), and the render
/// crate composites a scene, answers a hit-test, then rebuilds a page's
/// annotation scene after invalidation (simulating an annotation edit).
#[test]
fn end_to_end_parse_composite_hit_test() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).expect("fixture parses");
    let font_bytes = Arc::new(include_bytes!("fixtures/fonts/TestFont.ttf").to_vec());
    let engine = RenderEngine::new(font_bytes);
    let mut cache = PageSceneCache::new();
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };

    // First composite: builds + caches both body and annotation scenes.
    let scene = engine.composite(&report.document, &vp, &mut cache);
    let _ = scene.encoding();

    // Hit-test somewhere on page 0 (annotation entries exist in the fixture).
    // The result is not asserted on a specific target - the gate is that the
    // full geometry path runs without panicking for a point on the page.
    let _hit = hit_test(&report.document, &vp, (400.0, 50.0));

    // Simulate an annotation edit: invalidate page 0's annotation scene, then
    // re-composite. The body scene is reused from cache; the annotation scene
    // is rebuilt. Must not panic and must produce a scene.
    cache.invalidate(&report.document.pages[0].id);
    let scene2 = engine.composite(&report.document, &vp, &mut cache);
    let _ = scene2.encoding();
}
