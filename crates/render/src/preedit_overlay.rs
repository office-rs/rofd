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
/// `offset` is the composition's origin CHAR offset in the content: the
/// composition paints at that offset's visual line and column (the same
/// wrapped layout the caret uses), so it commits where it appears.
pub fn paint_preedit_overlay(
    scene: &mut Scene,
    doc: &rofd_dom::OfdDocument,
    viewport: &crate::Viewport,
    fonts: &crate::FontStore,
    text: &str,
    annotation: &AnnotationId,
    offset: usize,
) {
    let Some(ann) = doc.annotations.find(annotation) else {
        return;
    };
    let (rect, font_id, size, dom_color, content, rich) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            font,
            size,
            color,
            content,
            runs,
            ..
        } => (
            rect,
            font,
            *size,
            *color,
            content.as_str(),
            !runs.is_empty(),
        ),
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
    let (x_in_line, line_top) = if rich {
        // Rich boxes keep the legacy single-line caret path: position by the
        // flat prefix width on the first line (spec 2026-11-09 §1.2.2).
        let prefix: String = content
            .chars()
            .take(offset.min(content.chars().count()))
            .collect();
        (fonts.line_width(font_id, &prefix, size), 0.0)
    } else {
        let wrapped = fonts.shape_wrapped(font_id, content, size, rect.w.max(size));
        let (line_idx, local) = wrapped.line_at_offset(offset.min(content.chars().count()));
        let Some(line) = wrapped.lines.get(line_idx) else {
            return;
        };
        let x = if local < line.glyph_end - line.glyph_start {
            wrapped.glyphs[line.glyph_start + local].x as f64
        } else {
            line.width
        };
        (x, line.top)
    };
    let (font_data, glyphs) = shape_positioned(text, font_id, size, fonts);
    let Some(font_data) = font_data else {
        return;
    };
    let brush = to_peniko(dom_color);
    // Shaped composition glyphs are layout-relative: offset by the TextBox
    // origin + the target line's top + the column pen.
    let affine = base * Affine::translate((rect.x + x_in_line, rect.y + line_top));
    let mut painter = Painter::new(scene);
    painter.with_clip(ClipRef::fill(clip), |p| {
        draw_glyph_run(p, &font_data, &glyphs, affine, brush, size);
    });
}
