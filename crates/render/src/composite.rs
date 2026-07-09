//! Render engine: composite a paper-on-desk scene from a document + viewport.
//!
//! [`RenderEngine::composite`] builds the top-level scene the host paints: a
//! gray "desk" background filling the viewport, white pages centered
//! horizontally and stacked vertically with a gap, and each page's body scene
//! (stable, cached) and annotation scene (rebuilt when dirty) composited onto
//! the page rectangle with the page-origin + zoom transform.
//!
//! # Vello 0.8 scene composition
//!
//! Vello 0.8 has no `push_transform`/`pop` stack. Instead each draw call takes
//! its own `Affine`, and [`vello::Scene::append`] composites a child scene with
//! an additional `Option<Affine>` applied to every transform encoded in the
//! child. `composite` uses `Scene::append(body, Some(transform))` to place each
//! page's body + annotation scenes onto the desk surface. The page transform is
//! `compose_transform(page_origin, zoom, None)` (object CTMs are already baked
//! into the body/annotation scenes by `build_body_scene` /
//! `build_annotation_scene`).

use std::sync::Arc;

use kurbo::Rect;
use peniko::Fill;
use rofd_dom::OfdDocument;
use vello::Scene;

use crate::cache::PageSceneCache;
use crate::text::FontStore;
use crate::viewport::Viewport;

/// Renders an [`OfdDocument`] into a paper-on-desk [`Scene`] for a [`Viewport`].
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
    /// body + annotation per page composited with the page-origin + zoom
    /// transform.
    ///
    /// Pages are stacked vertically with `page_gap` between them and centered
    /// horizontally within the viewport. `scroll` offsets the stack (positive y
    /// scrolls pages downward) and `zoom` scales each page's physical box.
    pub fn composite(
        &self,
        doc: &OfdDocument,
        vp: &Viewport,
        cache: &mut PageSceneCache,
    ) -> Scene {
        let mut scene = Scene::new();

        // Gray "desk" background filling the viewport.
        let gray = peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF);
        let bg = Rect::new(0.0, 0.0, vp.size.0, vp.size.1);
        scene.fill(Fill::NonZero, kurbo::Affine::IDENTITY, gray, None, &bg);

        // Per-doc FontStore: document fonts + default fallback. Arc-shared
        // bytes -> no font copy per composite.
        let doc_fonts = FontStore::from_resources(&doc.resources, self.default_font_bytes.clone());

        // Stack pages vertically, centered horizontally, offset by scroll.
        let mut y = vp.page_gap - vp.scroll.1;
        for page in &doc.pages {
            let page_w = page.physical_box.w * vp.zoom;
            let page_h = page.physical_box.h * vp.zoom;
            let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
            let page_origin = (page_x + vp.scroll.0, y);

            // White page background.
            let white = peniko::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);
            let page_rect = Rect::new(
                page_origin.0,
                page_origin.1,
                page_origin.0 + page_w,
                page_origin.1 + page_h,
            );
            scene.fill(
                Fill::NonZero,
                kurbo::Affine::IDENTITY,
                white,
                None,
                &page_rect,
            );

            // Body (stable, cached) + annotation (rebuilt when dirty),
            // composited with page_origin + zoom. Object CTMs are already
            // applied inside build_body_scene / build_annotation_scene, so the
            // composite transform is compose_transform(_, _, None).
            //
            // The body and annotation scenes are borrowed from the cache in
            // separate scopes so the two &mut cache borrows don't overlap
            // (cache.body / cache.annotation each take &mut self to rebuild on
            // miss).
            let transform = crate::compose_transform(page_origin, vp.zoom, None);
            let anns = doc.annotations.for_page(&page.id);

            {
                let body = cache.body(page, &doc.resources, &doc_fonts);
                scene.append(body, Some(transform));
            }
            {
                let ann = cache.annotation(page, anns, &doc.resources, &doc_fonts);
                scene.append(ann, Some(transform));
            }

            y += page_h + vp.page_gap;
        }

        scene
    }
}
