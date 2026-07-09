//! Smoke test: build a body Scene for the fixture page without panicking.
//!
//! Reuses the io crate's fixture builder (`build_minimal_ofd`) to get a parsed
//! Page with a TextObject and a PathObject, then builds a FontStore (default
//! font = TestFont.ttf, since the fixture's Font.xml declares no FontFile) and
//! calls `build_body_scene`. The assertion is coarse: the call returns without
//! panicking and `scene.encoding()` exists (vello 0.8 introspection).

use std::sync::Arc;

use rofd_render::{build_body_scene, FontStore};

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
