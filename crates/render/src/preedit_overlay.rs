//! Simplified preedit overlay: paint the composition string at the caret.
//! No ghost-document reflow (intentional rword difference) - rofd has fixed
//! pages with no reflow, and the text enters the dom only on commit. The run
//! is shaped with parley via the render `FontStore` and clipped to the
//! TextBox boundary.

use imaging::kurbo::{Affine, Rect as KurboRect};
use imaging::record::Scene;
use imaging::{ClipRef, Painter};
use rofd_dom::{AnnotationId, AnnotationPayload};

use crate::annotation_scene::{draw_glyph_run, shape_positioned};
use crate::color::to_peniko;

/// Paint the preedit overlay when the composition targets a TextBox.
pub fn paint_preedit_overlay(
    scene: &mut Scene,
    doc: &rofd_dom::OfdDocument,
    viewport: &crate::Viewport,
    fonts: &crate::FontStore,
    text: &str,
    annotation: &AnnotationId,
) {
    let Some(ann) = doc.annotations.find(annotation) else {
        return;
    };
    let (rect, font_id, size, dom_color) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            font,
            size,
            color,
            ..
        } => (rect, font, *size, *color),
        _ => return,
    };
    // Locate the owning page to resolve page origin (TextBox geometry is in
    // page-local millimetres).
    let Some(page_idx) = doc.pages.iter().position(|p| ann.page == p.id) else {
        return;
    };
    let Some(origin) = crate::page_origin(doc, viewport, page_idx) else {
        return;
    };
    let base = Affine::translate(imaging::kurbo::Vec2::new(origin.0, origin.1))
        * Affine::scale(viewport.zoom);
    // Viewport-space fill-clip rect of the TextBox.
    let clip = KurboRect::new(
        origin.0 + rect.x * viewport.zoom,
        origin.1 + rect.y * viewport.zoom,
        origin.0 + (rect.x + rect.w) * viewport.zoom,
        origin.1 + (rect.y + rect.h) * viewport.zoom,
    );
    let (font_data, glyphs) = shape_positioned(text, font_id, size, fonts);
    let Some(font_data) = font_data else {
        return;
    };
    let brush = to_peniko(dom_color);
    // Shaped glyphs are layout-relative: offset by the TextBox origin,
    // matching draw_text_in_rect's affine (the committed-text path).
    let affine = base * Affine::translate((rect.x, rect.y));
    let mut painter = Painter::new(scene);
    painter.with_clip(ClipRef::fill(clip), |p| {
        draw_glyph_run(p, &font_data, &glyphs, affine, brush, size);
    });
}
