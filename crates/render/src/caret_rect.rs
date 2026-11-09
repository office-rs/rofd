//! Caret rect (viewport space) for a text annotation at a char offset.
//!
//! Given a text annotation ([`AnnotationPayload::TextBox`] / [`Note`] /
//! [`Watermark`]) and a char `offset`, [`caret_rect`] computes the caret's
//! rectangle in viewport (device-pixel) space - the rectangle the editor
//! paints as the text cursor.
//!
//! The viewport transform uses [`crate::composite::page_origin`] (the shared
//! page-stacking helper: centering + scroll on both axes + `page_gap` +
//! `zoom`), so the caret aligns with the rendered glyph run. Glyph x positions
//! come from [`FontStore::shape`] (the shaper's natural advances); the caret
//! height is the font `size` scaled by `zoom`.
//!
//! Non-text annotations ([`Markup`] / [`Freehand`] / [`Shape`] / [`Stamp`])
//! return `None` - they have no caret.
//!
//! [`Note`]: AnnotationPayload::Note
//! [`Markup`]: AnnotationPayload::Markup
//! [`Freehand`]: AnnotationPayload::Freehand
//! [`Shape`]: AnnotationPayload::Shape
//! [`Stamp`]: AnnotationPayload::Stamp

use imaging::kurbo::{Rect as KurboRect, Shape};
use imaging::{record::Scene, Painter};
use rofd_dom::{AnnotationId, AnnotationPayload, FontId, OfdDocument, Rect};

use crate::text::{FontStore, WrappedLine, WrappedText};
use crate::viewport::Viewport;

/// Caret rect (viewport space) for a text annotation at char `offset`.
///
/// Returns `None` when the annotation isn't found, isn't a text annotation
/// (`TextBox`/`Note`/`Watermark`), or its page isn't in the document.
///
/// The viewport transform uses [`crate::composite::page_origin`] (the shared
/// page-stacking helper: centering + scroll on both axes + `page_gap` +
/// `zoom`).
///
/// `runs`-empty TextBoxes use the wrapped layout: x is the line-relative
/// glyph pen at `offset`, y is `rect.y + line.top`. Rich TextBoxes and
/// Note/Watermark keep the legacy flat-content geometry (spec 2026-11-09
/// §1.2.2, §1.3).
pub fn caret_rect(
    doc: &OfdDocument,
    vp: &Viewport,
    fonts: &FontStore,
    ann_id: &AnnotationId,
    offset: usize,
) -> Option<Rect> {
    caret_rect_on_line(doc, vp, fonts, ann_id, offset, None)
}

/// Like [`caret_rect`], but with an optional visual-line affinity. A soft
/// wrap boundary offset normally displays at the next line's start (§1.3);
/// vertical caret movement records the line it moved to so the ambiguous
/// offset is shown on that line instead (spec 2026-11-09 §1.7).
pub fn caret_rect_on_line(
    doc: &OfdDocument,
    vp: &Viewport,
    fonts: &FontStore,
    ann_id: &AnnotationId,
    offset: usize,
    line_hint: Option<usize>,
) -> Option<Rect> {
    let ann = doc
        .annotations
        .by_page
        .values()
        .flatten()
        .find(|a| &a.id == ann_id)?;

    // Find the annotation's page index and compute its viewport origin via the
    // shared page_origin helper (same geometry as composite.rs).
    let page_idx = doc.pages.iter().position(|p| p.id == ann.page)?;
    let (origin_x, origin_y) = crate::composite::page_origin(doc, vp, page_idx)?;

    if let AnnotationPayload::TextBox {
        rect,
        content,
        font,
        size,
        runs,
        ..
    } = &ann.payload
    {
        if runs.is_empty() {
            let wrapped = fonts.shape_wrapped(font, content, *size, rect.w.max(*size));
            let (line_idx, local) = line_and_local(&wrapped, offset, line_hint);
            let line = wrapped.lines.get(line_idx)?;
            let x_local = caret_x_on_line(&wrapped, line, local);
            let vx = origin_x + (rect.x + x_local) * vp.zoom;
            let vy = origin_y + (rect.y + line.top) * vp.zoom;
            return Some(Rect {
                x: vx,
                y: vy,
                w: 1.0 * vp.zoom,
                h: size * vp.zoom,
            });
        }
    }

    // Legacy path: rich TextBox, Note, Watermark.
    let (content, font_id, size, rect) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            content,
            font,
            size,
            ..
        } => (content.as_str(), font, *size, *rect),
        AnnotationPayload::Note { rect, content, .. } => {
            // Notes: caret placed inside the popup rect using the note's text.
            (content.as_str(), &rofd_dom::FontId::default(), 12.0, *rect)
        }
        AnnotationPayload::Watermark {
            rect,
            content,
            font,
            size,
            ..
        } => (content.as_str(), font, *size, *rect),
        _ => return None,
    };

    // Shape with the document font (or the default fallback) for glyph advances.
    let (_font, glyphs) = fonts.shape(font_id, content, size);

    // Caret x (page-local) = the shaped glyph x at `offset`. For an offset
    // at or past the end of the run, the caret sits at the shaped line END
    // (the pen position after the last glyph) - NOT at the last glyph's x,
    // which is that glyph's LEFT edge and would draw the blinking bar on
    // top of the final character. Multi-line content ends on its last line.
    // For an empty run, x = 0.
    let last_line = content.rsplit('\n').next().unwrap_or("");
    let caret_x_local = glyphs
        .get(offset)
        .map(|g| g.x as f64)
        .unwrap_or_else(|| fonts.line_width(font_id, last_line, size));

    let vx = origin_x + (rect.x + caret_x_local) * vp.zoom;
    let vy = origin_y + rect.y * vp.zoom;
    Some(Rect {
        x: vx,
        y: vy,
        w: 1.0 * vp.zoom,
        h: size * vp.zoom,
    })
}

/// Caret x within a visual line: the glyph pen at `local`, or the line end.
fn caret_x_on_line(wrapped: &WrappedText, line: &WrappedLine, local: usize) -> f64 {
    if local < line.glyph_end - line.glyph_start {
        wrapped.glyphs[line.glyph_start + local].x as f64
    } else {
        line.width
    }
}

/// Map an offset to `(line, local)` honouring an optional line affinity. The
/// hint is only used when the offset lies within that line's range, so stale
/// affinities from a previous caret position cannot leak.
fn line_and_local(
    wrapped: &WrappedText,
    offset: usize,
    line_hint: Option<usize>,
) -> (usize, usize) {
    if let Some(i) = line_hint {
        if let Some(line) = wrapped.lines.get(i) {
            let end = line.char_start + line.char_len;
            if offset >= line.char_start && offset <= end {
                return (i, offset - line.char_start);
            }
        }
    }
    wrapped.line_at_offset(offset)
}

/// The visual line containing `y_in_rect` (clamped to the first/last line).
fn line_index_at_y(wrapped: &WrappedText, y_in_rect: f64) -> usize {
    if wrapped.lines.is_empty() {
        return 0;
    }
    wrapped
        .lines
        .iter()
        .position(|line| y_in_rect < line.top + line.height)
        .unwrap_or(wrapped.lines.len() - 1)
}

/// Char offset nearest `x_in_rect` within one visual line.
fn nearest_offset_on_line(wrapped: &WrappedText, line: &WrappedLine, x_in_rect: f64) -> usize {
    let count = line.glyph_end - line.glyph_start;
    let mut best = count;
    let mut best_d = (line.width - x_in_rect).abs();
    for i in 0..count {
        let d = (wrapped.glyphs[line.glyph_start + i].x as f64 - x_in_rect).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    line.char_start + best
}

/// The caret char offset nearest `(x_in_rect, y_in_rect)` in a wrapped text
/// box: the click first selects the visual line by y, then the nearest char
/// boundary on that line by x (spec 2026-11-09 §1.3).
pub fn caret_offset_at_point(
    content: &str,
    font_id: &FontId,
    size: f64,
    max_width: f64,
    fonts: &FontStore,
    x_in_rect: f64,
    y_in_rect: f64,
) -> usize {
    let wrapped = fonts.shape_wrapped(font_id, content, size, max_width);
    if wrapped.lines.is_empty() {
        return 0;
    }
    let line_idx = line_index_at_y(&wrapped, y_in_rect);
    nearest_offset_on_line(&wrapped, &wrapped.lines[line_idx], x_in_rect)
}

/// Vertical caret movement direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalMove {
    Up,
    Down,
}

/// Move the caret to the previous/next visual line, keeping `preferred_x`
/// (the column x in mm) across consecutive vertical moves. On the first
/// line `Up` goes to the text start; on the last line `Down` goes to the
/// text end (spec 2026-11-09 §1.7).
///
/// `current_line` is the caller's visual-line affinity from the previous
/// vertical move: it disambiguates a soft-wrap boundary offset (which
/// `line_at_offset` maps to the next line) so consecutive Down presses walk
/// every line. Returns `(new char offset, preferred x, target line)`.
#[allow(clippy::too_many_arguments)] // all parameters describe one caret move
pub fn caret_offset_vertical(
    content: &str,
    font_id: &FontId,
    size: f64,
    max_width: f64,
    fonts: &FontStore,
    offset: usize,
    direction: VerticalMove,
    preferred_x: Option<f64>,
    current_line: Option<usize>,
) -> (usize, f64, usize) {
    let wrapped = fonts.shape_wrapped(font_id, content, size, max_width);
    if wrapped.lines.is_empty() {
        return (0, 0.0, 0);
    }
    let (line_idx, local) = line_and_local(&wrapped, offset, current_line);
    let line = &wrapped.lines[line_idx];
    let current_x = caret_x_on_line(&wrapped, line, local);
    let x = preferred_x.unwrap_or(current_x);
    let last = wrapped.lines.len() - 1;
    match direction {
        VerticalMove::Up if line_idx == 0 => (0, x, 0),
        VerticalMove::Down if line_idx == last => (wrapped.total_chars, x, last),
        VerticalMove::Up => {
            let target = line_idx - 1;
            (
                nearest_offset_on_line(&wrapped, &wrapped.lines[target], x),
                x,
                target,
            )
        }
        VerticalMove::Down => {
            let target = line_idx + 1;
            (
                nearest_offset_on_line(&wrapped, &wrapped.lines[target], x),
                x,
                target,
            )
        }
    }
}

/// The caret char offset whose boundary (a shaped glyph pen, or the line
/// end) is nearest `x_in_rect` (x relative to the annotation rect's
/// origin). Chars map 1:1 to glyphs (the shaper disables ligatures), so
/// glyph index == char offset; boundary `n` (past the last glyph) is the
/// shaped line end. An empty run maps to 0.
pub fn caret_offset_at_x(
    content: &str,
    font_id: &FontId,
    size: f64,
    fonts: &FontStore,
    x_in_rect: f64,
) -> usize {
    let (_, glyphs) = fonts.shape(font_id, content, size);
    let end = fonts.line_width(font_id, content, size);
    let mut best = glyphs.len();
    let mut best_d = (end - x_in_rect).abs();
    for (i, g) in glyphs.iter().enumerate() {
        let d = (g.x as f64 - x_in_rect).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// Paint the text caret: a vertical bar filled black at `rect` (viewport
/// coordinates). Called by the component after body, annotations and
/// scrollbars composite.
pub fn paint_caret(scene: &mut Scene, rect: &rofd_dom::Rect) {
    let rect = KurboRect::new(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let bez = rect.to_path(0.1);
    let mut painter = Painter::new(scene);
    painter.fill(&bez, peniko::Color::from_rgb8(0, 0, 0)).draw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, NoteIcon,
        OfdDocument, Page, PageId, Rect, ShapeKind,
    };

    /// A minimal doc + FontStore for positive tests: one page + the TestFont
    /// registered as the default (so unknown FontIds still shape).
    fn doc_with_font() -> (OfdDocument, FontStore) {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 300.0,
            },
            layers: vec![],
            template: None,
        });
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        (doc, fonts)
    }

    /// Push an annotation onto page P0 and return its id.
    fn push_ann(
        doc: &mut OfdDocument,
        payload: AnnotationPayload,
        kind: AnnotationKind,
    ) -> AnnotationId {
        let id = AnnotationId::from_int(1);
        let ann = Annotation {
            id: id.clone(),
            kind,
            page: PageId::new("P0"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload,
        };
        doc.annotations
            .by_page
            .entry(PageId::new("P0"))
            .or_default()
            .push(ann);
        id
    }

    fn vp() -> Viewport {
        Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (800.0, 600.0),
            page_gap: 20.0,
        }
    }

    #[test]
    fn caret_rect_none_for_non_text_annotation() {
        let doc = OfdDocument::default(); // no annotations
        let vp = vp();
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        let res = caret_rect(&doc, &vp, &fonts, &AnnotationId::default(), 0);
        assert!(res.is_none(), "empty doc (no annotation) -> None");
    }

    #[test]
    fn caret_rect_none_for_markup_annotation() {
        // Markup has no caret - it's a highlight/strikeout, not editable text.
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::Markup {
                quad_points: vec![
                    rofd_dom::Point { x: 0.0, y: 0.0 },
                    rofd_dom::Point { x: 10.0, y: 10.0 },
                ],
                color: Color::Rgb(255, 255, 0),
            },
            AnnotationKind::Highlight,
        );
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(res.is_none(), "Markup annotation -> None");
    }

    #[test]
    fn caret_rect_none_for_shape_annotation() {
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 20.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Rect),
        );
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(res.is_none(), "Shape annotation -> None");
    }

    #[test]
    fn caret_rect_some_for_textbox_at_offset_zero() {
        // TextBox "Hi" at rect origin (10, 20). At offset 0 the caret sits at
        // the annotation rect's x (the first glyph's x is ~0 from the shaper),
        // at the rect's y, with height = size.
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "Hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: Vec::new(),
            },
            AnnotationKind::TextBox,
        );

        // vp: page_gap=20, scroll=(0,0), zoom=1, size=(800,600).
        // page_w = 200, page_x = (800-200)/2 = 300. page_origin = (300, 20).
        // caret x_local at offset 0 = glyphs[0].x (first glyph's shaped x).
        let (_font, glyphs) = fonts.shape(&FontId::new("F1"), "Hi", 12.0);
        let first_x = glyphs[0].x as f64;
        let expected_x = 300.0 + (10.0 + first_x) * 1.0;
        let expected_y = 20.0 + 20.0 * 1.0;

        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        let r = res.expect("TextBox -> Some caret rect");
        assert_eq!(
            r.x, expected_x,
            "caret x = page_origin_x + (rect.x + glyph0.x) * zoom"
        );
        assert_eq!(r.y, expected_y, "caret y = page_origin_y + rect.y * zoom");
        assert_eq!(r.w, 1.0, "caret width = 1px * zoom");
        assert_eq!(r.h, 12.0, "caret height = size * zoom");
    }

    #[test]
    fn caret_rect_at_end_sits_after_the_last_glyph() {
        // Regression: at the end offset (offset == char count - where a
        // click on the box and typing both leave the cursor), the caret
        // used to fall back to the LAST GLYPH'S LEFT EDGE, drawing the
        // blinking bar on top of the final character instead of after the
        // text. The end caret must sit strictly right of the last glyph's
        // pen (by roughly one advance) and exactly at the shaped line end.
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "Hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let at = |off: usize| caret_rect(&doc, &vp(), &fonts, &id, off).unwrap();

        let end = at(2);
        let before_last = at(1);
        assert!(
            end.x > before_last.x,
            "end caret must sit right of the last glyph's pen: {} vs {}",
            end.x,
            before_last.x
        );
        // The step past the pen is the last glyph's advance: within one em.
        let step = end.x - before_last.x;
        assert!(step > 0.0 && step <= 12.0, "advance-sized step, got {step}");
        // Exactly the shaped line end (page_origin 300 + rect.x 10 + width).
        let expected = 300.0 + 10.0 + fonts.line_width(&FontId::new("F1"), "Hi", 12.0);
        assert!(
            (end.x - expected).abs() < 1e-3,
            "end x == page origin + rect.x + line width: {} vs {expected}",
            end.x
        );
    }

    #[test]
    fn caret_rect_respects_scroll_and_zoom() {
        // Scroll both axes + zoom != 1: the caret must track the page origin
        // exactly as composite does (scroll.0 on x, scroll.1 on y).
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "Hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: Vec::new(),
            },
            AnnotationKind::TextBox,
        );

        let vp = Viewport {
            scroll: (50.0, 30.0),
            zoom: 2.0,
            size: (800.0, 600.0),
            page_gap: 20.0,
        };
        // page_w = 200*2 = 400, page_x = (800-400)/2 = 200.
        // page_origin = (200 + 50, 20 - 30) = (250, -10).
        let (_font, glyphs) = fonts.shape(&FontId::new("F1"), "Hi", 12.0);
        let first_x = glyphs[0].x as f64;
        let expected_x = 250.0 + (10.0 + first_x) * 2.0;
        let expected_y = -10.0 + 20.0 * 2.0;

        let res = caret_rect(&doc, &vp, &fonts, &id, 0);
        let r = res.expect("TextBox -> Some caret rect with scroll+zoom");
        assert_eq!(r.x, expected_x, "caret x tracks scroll.0 + zoom");
        assert_eq!(r.y, expected_y, "caret y tracks scroll.1 + zoom");
        assert_eq!(r.w, 2.0, "caret width = 1px * zoom");
        assert_eq!(r.h, 24.0, "caret height = size * zoom");
    }

    #[test]
    fn caret_rect_some_for_note_and_watermark() {
        // Note and Watermark are text-bearing -> Some.
        let (mut doc, fonts) = doc_with_font();

        let note_id = push_ann(
            &mut doc,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    w: 40.0,
                    h: 20.0,
                },
                color: Color::Rgb(255, 200, 0),
                content: "note text".into(),
                icon: NoteIcon::Note,
            },
            AnnotationKind::Note,
        );
        assert!(
            caret_rect(&doc, &vp(), &fonts, &note_id, 0).is_some(),
            "Note -> Some"
        );

        let wm_id = push_ann(
            &mut doc,
            AnnotationPayload::Watermark {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 100.0,
                },
                content: "DRAFT".into(),
                opacity: 0.3,
                angle: 45.0,
                font: FontId::new("F2"),
                size: 48.0,
                color: Color::Rgb(200, 200, 200),
            },
            AnnotationKind::Watermark,
        );
        assert!(
            caret_rect(&doc, &vp(), &fonts, &wm_id, 0).is_some(),
            "Watermark -> Some"
        );
    }

    #[test]
    fn caret_rect_none_when_page_missing() {
        // Annotation references a page that isn't in doc.pages -> None.
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut doc = OfdDocument::default();
        // No pages pushed, but an annotation claims page P0.
        let id = AnnotationId::from_int(1);
        doc.annotations
            .by_page
            .entry(PageId::new("P0"))
            .or_default()
            .push(Annotation {
                id: id.clone(),
                kind: AnnotationKind::TextBox,
                page: PageId::new("P0"),
                creator: "tester".into(),
                created: 0,
                modified: 0,
                reply_to: None,
                payload: AnnotationPayload::TextBox {
                    rect: Rect {
                        x: 10.0,
                        y: 20.0,
                        w: 100.0,
                        h: 30.0,
                    },
                    content: "Hi".into(),
                    font: FontId::new("F1"),
                    size: 12.0,
                    color: Color::Rgb(0, 0, 0),
                    border: None,
                    runs: Vec::new(),
                },
            });
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(
            res.is_none(),
            "annotation on a page not in doc.pages -> None"
        );
    }

    #[test]
    fn caret_rect_second_line_uses_line_top() {
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 45.0,
                    h: 40.0,
                },
                content: "abc def ghi jkl".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let w = fonts.shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
        let line1 = &w.lines[1];
        let r = caret_rect(&doc, &vp(), &fonts, &id, 9).unwrap(); // line 1, local 1
                                                                  // Viewport y = page origin (20) + rect.y (20) + line.top.
        assert!((r.y - (40.0 + line1.top)).abs() < 1e-6, "second line y");
        let expected_x = 300.0 + 10.0 + w.glyphs[line1.glyph_start + 1].x as f64;
        assert!((r.x - expected_x).abs() < 1e-6);
    }

    #[test]
    fn caret_offset_at_point_second_line() {
        let (_, fonts) = doc_with_font();
        let w = fonts.shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
        let off = caret_offset_at_point(
            "abc def ghi jkl",
            &FontId::new("F1"),
            12.0,
            45.0,
            &fonts,
            1.0,
            w.lines[1].top + 1.0,
        );
        assert_eq!(off, 8, "left edge of line 2 -> its first char");
    }

    #[test]
    fn caret_offset_vertical_preserves_column() {
        let (_, fonts) = doc_with_font();
        let text = "abc def ghi jkl";
        let (down, pref, line) = caret_offset_vertical(
            text,
            &FontId::new("F1"),
            12.0,
            45.0,
            &fonts,
            2,
            VerticalMove::Down,
            None,
            None,
        );
        assert!(
            (8..=11).contains(&down),
            "lands on line 2 near column 2, got {down}"
        );
        assert_eq!(line, 1);
        let (up, _, up_line) = caret_offset_vertical(
            text,
            &FontId::new("F1"),
            12.0,
            45.0,
            &fonts,
            down,
            VerticalMove::Up,
            Some(pref),
            Some(line),
        );
        assert_eq!(up, 2, "returns to the original column");
        assert_eq!(up_line, 0);
    }

    #[test]
    fn vertical_move_targets_short_line_and_caret_stays_on_it() {
        let (mut doc, fonts) = doc_with_font();
        let text = "abcdef ghijkl mn";
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 45.0,
                    h: 60.0,
                },
                content: text.into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let w = fonts.shape_wrapped(&FontId::new("F1"), text, 12.0, 45.0);
        assert_eq!(w.lines.len(), 3, "fixture must wrap to three lines");
        let (offset, pref, line) = caret_offset_vertical(
            text,
            &FontId::new("F1"),
            12.0,
            45.0,
            &fonts,
            6,
            VerticalMove::Down,
            None,
            None,
        );
        assert_eq!(line, 1, "Down targets the middle line, not the next one");
        assert_eq!(offset, w.lines[1].char_start + w.lines[1].char_len);
        // The ambiguous soft-boundary offset normally displays on line 2;
        // the affinity keeps it on the line the arrow moved to.
        let r = caret_rect_on_line(&doc, &vp(), &fonts, &id, offset, Some(line)).unwrap();
        assert!(
            (r.y - (40.0 + w.lines[1].top)).abs() < 1e-6,
            "caret stays on the target line"
        );
        let plain = caret_rect(&doc, &vp(), &fonts, &id, offset).unwrap();
        assert!(
            (plain.y - (40.0 + w.lines[2].top)).abs() < 1e-6,
            "without affinity the offset maps to the next line"
        );
        // A second Down continues from the affinity line, not from line 2.
        let (_, _, next_line) = caret_offset_vertical(
            text,
            &FontId::new("F1"),
            12.0,
            45.0,
            &fonts,
            offset,
            VerticalMove::Down,
            Some(pref),
            Some(line),
        );
        assert_eq!(next_line, 2);
    }

    #[test]
    fn vertical_edges_go_to_text_bounds() {
        let (_, fonts) = doc_with_font();
        let text = "abc def ghi jkl";
        assert_eq!(
            caret_offset_vertical(
                text,
                &FontId::new("F1"),
                12.0,
                45.0,
                &fonts,
                2,
                VerticalMove::Up,
                None,
                None
            )
            .0,
            0
        );
        assert_eq!(
            caret_offset_vertical(
                text,
                &FontId::new("F1"),
                12.0,
                45.0,
                &fonts,
                11,
                VerticalMove::Down,
                None,
                None
            )
            .0,
            15
        );
    }

    #[test]
    fn caret_offsets_use_chars_not_bytes_for_cjk() {
        // Deterministic CJK fixture: no system-font fallback dependency.
        let (_, mut fonts) = doc_with_font();
        assert!(fonts.register_font(Arc::new(
            include_bytes!("../tests/fixtures/fonts/TestFontCjk.otf").to_vec()
        )));
        let w = fonts.shape_wrapped(&FontId::new("F1"), "你好世界", 12.0, 30.0);
        assert_eq!(w.lines.len(), 2);
        assert_eq!(w.line_at_offset(2), (1, 0));
        assert_eq!(
            caret_offset_at_point(
                "你好世界",
                &FontId::new("F1"),
                12.0,
                30.0,
                &fonts,
                1.0,
                w.lines[1].top + 1.0
            ),
            2
        );
    }
}
