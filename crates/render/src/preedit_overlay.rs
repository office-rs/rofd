//! Detached preedit overlay for span-bearing TextBoxes.
//!
//! Flat boxes (`spans` empty) preview the composition INLINE instead:
//! [`draw_annotations`](crate::draw_annotations) inserts the composition into
//! the text flow (`PreeditPreview`), so the following text moves aside
//! exactly as it will after commit. Painting a detached composition on top of
//! the content painted the composition over the text it precedes - the
//! overlap this module no longer causes for flat boxes.
//!
//! Span boxes (`spans` non-empty) keep the composition detached; it is anchored
//! at the caret's line top and drawn with the caret char's style, so a mixed
//! size/color box previews the composition where the caret actually is. The
//! text enters the dom only on commit - no ghost-document reflow.

use imaging::kurbo::{Affine, Rect as KurboRect};
use imaging::record::Scene;
use imaging::{ClipRef, Painter};
use rofd_dom::{AnnotationId, AnnotationPayload, Color, FontId};

use crate::annotation_scene::{draw_glyph_run, shape_positioned};
use crate::color::to_peniko;

/// Style of the char an insertion at `offset` inherits: the preceding char
/// (or the first char at offset 0), falling back to the flat fields.
fn inherited_style(
    font: &FontId,
    size: f64,
    color: Color,
    spans: &[rofd_dom::TextSpan],
    offset: usize,
) -> (FontId, f64, Color) {
    if spans.is_empty() {
        return (font.clone(), size, color);
    }
    let probe = offset.saturating_sub(1);
    let mut cursor = 0usize;
    for span in spans {
        cursor += span.text.chars().count();
        if probe < cursor {
            return (span.font.clone(), span.size, span.color.unwrap_or(color));
        }
    }
    spans
        .last()
        .map(|s| (s.font.clone(), s.size, s.color.unwrap_or(color)))
        .unwrap_or_else(|| (font.clone(), size, color))
}

/// Paint the preedit overlay when the composition targets a span-box TextBox
/// (`spans` non-empty). Flat boxes are handled inline by `draw_annotations`;
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
    let (rect, flat_font, flat_size, flat_color, spans) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            font,
            size,
            color,
            content: _,
            spans,
            ..
        } => (rect, font, *size, *color, spans.as_slice()),
        _ => return,
    };
    // Flat boxes preview inline (draw_annotations); nothing to paint here.
    if spans.is_empty() {
        return;
    }
    let Some(caret) =
        crate::caret_rect::caret_rect_on_line(doc, viewport, fonts, annotation, offset, None)
    else {
        return;
    };
    // Locate the owning page to resolve the clip rect (page-local mm).
    let Some(page_idx) = doc.pages.iter().position(|p| ann.page == p.id) else {
        return;
    };
    let Some(origin) = crate::page_origin(doc, viewport, page_idx) else {
        return;
    };
    // Viewport-space fill-clip rect of the TextBox.
    let clip = KurboRect::new(
        origin.0 + rect.x * viewport.zoom,
        origin.1 + rect.y * viewport.zoom,
        origin.0 + (rect.x + rect.w) * viewport.zoom,
        origin.1 + (rect.y + rect.h) * viewport.zoom,
    );
    let (font_id, size, color) = inherited_style(flat_font, flat_size, flat_color, spans, offset);
    let (font_data, glyphs) = shape_positioned(text, &font_id, size, fonts);
    let Some(font_data) = font_data else {
        return;
    };
    // The caret rect is already viewport space: draw the composition glyphs at
    // the caret's line top, scaled by zoom (glyph y is line-relative).
    let affine = Affine::translate((caret.x, caret.y)) * Affine::scale(viewport.zoom);
    let brush = to_peniko(color);
    let mut painter = Painter::new(scene);
    painter.with_clip(ClipRef::fill(clip), |p| {
        draw_glyph_run(p, &font_data, &glyphs, affine, brush, size);
    });
}
