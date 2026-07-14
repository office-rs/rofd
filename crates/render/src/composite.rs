//! Render engine: composite a paper-on-desk scene from a document + viewport.
//!
//! [`RenderEngine::composite`] builds the top-level [`imaging::record::Scene`] the
//! host paints: a gray "desk" background filling the viewport, white pages centered
//! horizontally and stacked vertically with a gap, and each page's body + annotation
//! content drawn directly with the page-origin + zoom transform baked into every
//! draw call.
//!
//! # Why imaging (not vello::Scene)
//!
//! Masonry (the widget toolkit xilem builds on) authors widget paint via the
//! `imaging::Painter` command-stream API, not `vello::Scene` directly. imaging is a
//! backend-agnostic IR layered above vello: the native xilem canvas consumes an
//! `imaging::record::Scene` via `Painter::replay`, and the web-view converts it to a
//! `vello::Scene` via `imaging_vello::VelloSceneSink`. Producing an imaging Scene
//! here lets both hosts share one render path and insulates rofd-render from vello
//! API churn (the imaging_vello backend absorbs it).
//!
//! # Transforms
//!
//! imaging has no "replay a pre-built sub-scene with a transform" primitive (unlike
//! vello's `Scene::append(child, Some(affine))`), so the body/annotation builders do
//! not produce cached sub-scenes. Instead they draw into the shared painter with
//! `compose_transform(page_origin, zoom, ctm)` applied per draw call - equivalent to
//! the old "page-local sub-scene + append with page transform" but with the transform
//! folded into each command.

use std::sync::Arc;

use imaging::kurbo::Rect;
use imaging::peniko::Color;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::OfdDocument;

use crate::annotation_scene::draw_annotations;
use crate::body_scene::draw_body;
use crate::text::FontStore;
use crate::viewport::Viewport;

/// Renders an [`OfdDocument`] into a paper-on-desk [`imaging::record::Scene`] for a [`Viewport`].
///
/// Stores only the default fallback font bytes (`Arc`-shared, no copy per
/// composite). A per-document [`FontStore`] (document fonts + the default) is
/// built once per `composite` call - cheap, because the font bytes are
/// `Arc`-shared.
pub struct RenderEngine {
    pub default_font_bytes: Arc<Vec<u8>>,
}

impl RenderEngine {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self { default_font_bytes }
    }

    /// Composite paper-on-desk: gray viewport background, white centered pages,
    /// body + annotation per page drawn with the page-origin + zoom transform.
    ///
    /// Pages are stacked vertically with `page_gap` between them and centered
    /// horizontally within the viewport. `scroll` offsets the stack (positive y
    /// scrolls pages downward) and `zoom` scales each page's physical box.
    ///
    /// `fonts` is a caller-cached [`FontStore`] (document fonts + default). It is
    /// passed in rather than built per call so a large default CJK font is not
    /// re-registered every frame.
    pub fn composite(&self, doc: &OfdDocument, vp: &Viewport, fonts: &FontStore) -> Scene {
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);

        // Gray "desk" background filling the viewport.
        let gray = Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF);
        let bg = Rect::new(0.0, 0.0, vp.size.0, vp.size.1);
        painter.fill_rect(bg, gray);

        // Stack pages vertically, centered horizontally, offset by scroll.
        let mut y = vp.page_gap - vp.scroll.1;
        for page in &doc.pages {
            let page_w = page.physical_box.w * vp.zoom;
            let page_h = page.physical_box.h * vp.zoom;
            let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
            let page_origin = (page_x + vp.scroll.0, y);

            // Cull off-screen pages: skip pages fully above the viewport
            // (scrolled past) and stop once a page starts below it. This avoids
            // shaping/drawing pages the user can't see - critical for
            // multi-page docs (shaping dominates render time). The page-origin
            // math matches hit_test.rs exactly.
            if page_origin.1 + page_h < 0.0 {
                y += page_h + vp.page_gap;
                continue;
            }
            if page_origin.1 > vp.size.1 {
                break;
            }

            // White page background.
            let white = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);
            let page_rect = Rect::new(
                page_origin.0,
                page_origin.1,
                page_origin.0 + page_w,
                page_origin.1 + page_h,
            );
            painter.fill_rect(page_rect, white);

            // Body + annotation, drawn directly with page_origin + zoom baked
            // into each draw call (no cached sub-scenes - see module docs).
            draw_body(
                &mut painter,
                page,
                &doc.resources,
                fonts,
                page_origin,
                vp.zoom,
            );
            let anns = doc.annotations.for_page(&page.id);
            draw_annotations(
                &mut painter,
                anns,
                &doc.resources,
                fonts,
                page_origin,
                vp.zoom,
            );

            y += page_h + vp.page_gap;
        }

        scene
    }
}
