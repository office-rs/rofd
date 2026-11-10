//! Detached preedit overlay for RICH TextBoxes (parsed per-run appearance).
//!
//! Flat boxes (`runs` empty) preview the composition INLINE instead:
//! [`draw_annotations`](crate::draw_annotations) inserts the composition into
//! the text flow (`PreeditPreview`), so the following text moves aside
//! exactly as it will after commit. Painting a detached composition on top of
//! the content painted the composition over the text it precedes - the
//! overlap this module no longer causes for flat boxes.
//!
//! Rich boxes never reflow (spec 2026-11-09 §1.2.2): their flat `content` and
//! the `runs` snapshot can disagree, so the composition is still painted
//! detached, positioned by the flat prefix width on the first line. The text
//! enters the dom only on commit - no ghost-document reflow.

use imaging::kurbo::{Affine, Rect as KurboRect};
use imaging::record::Scene;
use imaging::{ClipRef, Painter};
use rofd_dom::{AnnotationId, AnnotationPayload};

use crate::annotation_scene::{draw_glyph_run, shape_positioned};
use crate::color::to_peniko;

/// Paint the preedit overlay when the composition targets a RICH TextBox
/// (`runs` non-empty). Flat boxes are handled inline by `draw_annotations`;
/// calling this for them draws nothing.
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
    // Flat boxes preview inline (draw_annotations); nothing to paint here.
    if !rich {
        return;
    }
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
    // Rich boxes keep the legacy single-line caret path: position by the flat
    // prefix width on the first line (spec 2026-11-09 §1.2.2).
    let prefix: String = content
        .chars()
        .take(offset.min(content.chars().count()))
        .collect();
    let x_in_line = fonts.line_width(font_id, &prefix, size);
    let (font_data, glyphs) = shape_positioned(text, font_id, size, fonts);
    let Some(font_data) = font_data else {
        return;
    };
    let brush = to_peniko(dom_color);
    // Shaped composition glyphs are layout-relative: offset by the TextBox
    // origin + the column pen on the first line.
    let affine = base * Affine::translate((rect.x + x_in_line, rect.y));
    let mut painter = Painter::new(scene);
    painter.with_clip(ClipRef::fill(clip), |p| {
        draw_glyph_run(p, &font_data, &glyphs, affine, brush, size);
    });
}
