//! Body-text geometry shared by drawing, hit-testing, and selection rects
//! (spec P3 §5.3: single source of truth - never a second implementation).
//!
//! v1 limitation: TextObjects with a scaling/rotating CTM are not selectable
//! (only translation is accounted for); Freehand-style shaped text is drawn
//! by glyph ids but positioned by the same deltas used here.

use rofd_dom::{ObjectId, OfdDocument, PageId, TextCode, TextObject};

use crate::viewport::Viewport;

/// A selected character range within one TextCode: chars `[start, end)` of
/// `code_index` inside `object`. Offsets are char (not byte) offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyTextRange {
    pub object: ObjectId,
    pub code_index: usize,
    pub start: usize,
    pub end: usize,
}

/// The text-selection UI state (spec §5.1 plan A: pure UI state - never in
/// dom, editor history, or saved output). One page only (v1: no cross-page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyTextSelection {
    pub page: PageId,
    pub ranges: Vec<BodyTextRange>,
}

/// Where a pointer hit landed in the body text (char offsets; `char_offset`
/// may equal the code's char count = past-the-end boundary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHit {
    pub page: PageId,
    pub object: ObjectId,
    pub code_index: usize,
    pub char_offset: usize,
}

/// Pen position + advance of one character in a TextCode (object-local:
/// relative to the TextObject's Boundary origin, CTM not applied).
pub(crate) struct CharCell {
    pub x: f64,
    pub y: f64,
    pub advance: f64,
}

/// Character cells for a TextCode: the pen starts at `(code.x, code.y)`;
/// each character sits at the pen and advances by its document delta
/// (GB/T 33190 DeltaX semantics - the same math `draw_text` uses to place
/// glyphs, extracted so hit/selection/draw cannot drift apart).
///
/// `n` is the cell count to produce: the glyph count for glyph-id codes,
/// the char count for shaped text (the drawer passes its own glyph count).
/// The last character's advance (no following delta) falls back to the last
/// non-zero delta, then to the font `size`.
pub(crate) fn code_char_cells(t: &TextObject, code: &TextCode, n: usize) -> Vec<CharCell> {
    let fallback = code
        .deltas
        .last()
        .map(|d| d.0 as f64)
        .filter(|a| *a > 0.0)
        .unwrap_or(t.size);
    let mut cells = Vec::with_capacity(n);
    let mut pen_x = code.x;
    let mut pen_y = code.y;
    for i in 0..n {
        let (dx, dy) = code.deltas.get(i).copied().unwrap_or((0.0, 0.0));
        let advance = if i < code.deltas.len() {
            dx as f64
        } else {
            fallback
        };
        cells.push(CharCell {
            x: pen_x,
            y: pen_y,
            advance,
        });
        pen_x += dx as f64;
        pen_y += dy as f64;
    }
    cells
}

/// Character count of a code as hit-testing sees it: glyph count when the
/// code carries glyph ids, else the text's char count.
pub(crate) fn code_char_count(code: &TextCode) -> usize {
    if code.glyph_ids.is_empty() {
        code.text.chars().count()
    } else {
        code.glyph_ids.len()
    }
}

/// Hit-test a viewport-space point against body text (all pages, document
/// order). Within a code's line band the nearest character boundary wins
/// (前/后半宽). Among bands, the vertically nearest wins. CTM rotation is
/// ignored (v1 - see module docs).
pub fn hit_test_body_text(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> Option<TextHit> {
    use rofd_dom::PageObject;
    let origins = crate::composite::page_origins(doc, vp);
    let mut best: Option<(f64, TextHit)> = None;
    for (page, &(ox, oy)) in doc.pages.iter().zip(origins.iter()) {
        for layer in &page.layers {
            for obj in &layer.objects {
                let PageObject::Text(t) = obj else { continue };
                let base_x = ox + t.boundary.x * vp.zoom;
                let base_y = oy + t.boundary.y * vp.zoom;
                for (ci, code) in t.codes.iter().enumerate() {
                    let n = code_char_count(code);
                    if n == 0 {
                        continue;
                    }
                    let cells = code_char_cells(t, code, n);
                    let last = &cells[n - 1];
                    // Line band: ascent (one em) above the pen, a quarter em
                    // descender below (same band `text_selection_rects` uses).
                    let band_top = base_y + (last.y - t.size) * vp.zoom;
                    let band_bot = base_y + (last.y + t.size * 0.25) * vp.zoom;
                    if point.1 < band_top || point.1 > band_bot {
                        continue;
                    }
                    let x_local = (point.0 - base_x) / vp.zoom;
                    // x extent: half a char of slack left, 1.5 advances right.
                    let x0 = cells[0].x - cells[0].advance / 2.0;
                    let x1 = last.x + last.advance * 1.5;
                    if x_local < x0 || x_local > x1 {
                        continue;
                    }
                    let dist = (point.1 - (band_top + band_bot) / 2.0).abs();
                    if best.as_ref().is_some_and(|(d, _)| dist >= *d) {
                        continue;
                    }
                    let mut offset = n;
                    for (i, c) in cells.iter().enumerate() {
                        if x_local < c.x + c.advance / 2.0 {
                            offset = i;
                            break;
                        }
                    }
                    best = Some((
                        dist,
                        TextHit {
                            page: page.id.clone(),
                            object: t.id.clone(),
                            code_index: ci,
                            char_offset: offset,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, h)| h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Viewport;
    use rofd_dom::{
        Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject, Rect, TextCode,
        TextObject,
    };

    fn text_obj(codes: Vec<TextCode>) -> TextObject {
        TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect {
                x: 10.0,
                y: 20.0,
                w: 100.0,
                h: 20.0,
            },
            ctm: None,
            font: rofd_dom::FontId::new("F1"),
            size: 10.0,
            fill: None,
            codes,
            draw_param: None,
        }
    }

    fn code4() -> TextCode {
        // 4 个字形，每个推进 10mm：笔位 x = 0,10,20,30（对象局部，code.x=0）。
        TextCode {
            glyph_ids: vec![1, 2, 3, 4],
            deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)],
            text: "ABCD".into(),
            x: 0.0,
            y: 10.0,
        }
    }

    fn doc_with(obj: TextObject) -> (OfdDocument, Viewport) {
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![Layer {
                layer_type: LayerType::Body,
                objects: vec![PageObject::Text(obj)],
            }],
            template: None,
        });
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        (doc, vp)
    }

    #[test]
    fn char_cells_pen_positions_and_fallback_advance() {
        let t = text_obj(vec![code4()]);
        let cells = code_char_cells(&t, &t.codes[0], 4);
        assert_eq!(cells.len(), 4);
        assert_eq!((cells[0].x, cells[0].y), (0.0, 10.0));
        assert_eq!(cells[1].x, 10.0);
        // 前 3 个 advance 来自 deltas；最后一个无 delta -> 回退最后一个非零 delta (10)。
        assert_eq!(cells[2].advance, 10.0);
        assert_eq!(cells[3].x, 30.0);
        assert_eq!(cells[3].advance, 10.0, "fallback = last non-zero delta");
    }

    #[test]
    fn char_cells_empty_deltas_fallback_to_size() {
        let t = text_obj(vec![TextCode {
            glyph_ids: vec![1, 2],
            deltas: vec![],
            text: "AB".into(),
            x: 0.0,
            y: 0.0,
        }]);
        let cells = code_char_cells(&t, &t.codes[0], 2);
        // 无任何 delta -> advance 回退字号。
        assert_eq!(cells[0].advance, 10.0);
        assert_eq!(cells[1].x, 0.0, "zero deltas -> pen never advances");
    }

    // 对象 boundary (10,20)，code.y=10 -> 行带 viewport y ∈ [20+(10-10), 20+(10+2.5)] = [20, 32.5]。
    // 字符 i 的格子的 viewport x 起点 = 10 + 10i，中线边界在 x = 10 + 10i + 5。
    #[test]
    fn hit_mid_first_char_is_offset_zero() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        let h = hit_test_body_text(&doc, &vp, (12.0, 25.0)).expect("hit");
        assert_eq!(h.char_offset, 0);
        assert_eq!(h.code_index, 0);
        assert_eq!(h.object, ObjectId::new("t1"));
    }

    #[test]
    fn hit_second_half_of_char_advances_offset() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        // x=16 落在字符 0 格子 (10..20) 的后半 -> offset 1。
        let h = hit_test_body_text(&doc, &vp, (16.0, 25.0)).expect("hit");
        assert_eq!(h.char_offset, 1);
        // x=36 在字符 2 (30..40) 后半 -> offset 3。
        assert_eq!(
            hit_test_body_text(&doc, &vp, (36.0, 25.0))
                .unwrap()
                .char_offset,
            3
        );
        // x=50（最后一格 30..40 右缘的 1.5 格余量内）-> offset = n。
        assert_eq!(
            hit_test_body_text(&doc, &vp, (50.0, 25.0))
                .unwrap()
                .char_offset,
            4
        );
    }

    #[test]
    fn hit_misses_blank_area_and_wrong_band() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        assert!(
            hit_test_body_text(&doc, &vp, (100.0, 100.0)).is_none(),
            "blank desk"
        );
        assert!(
            hit_test_body_text(&doc, &vp, (15.0, 60.0)).is_none(),
            "below the line band"
        );
        // x 远超行尾（最后一个 advance 的 1.5 倍余量之外）不算命中。
        assert!(
            hit_test_body_text(&doc, &vp, (200.0, 25.0)).is_none(),
            "far right of line"
        );
    }

    #[test]
    fn hit_picks_nearest_line_among_codes() {
        let obj = text_obj(vec![
            TextCode {
                glyph_ids: vec![1, 2],
                deltas: vec![(10.0, 0.0)],
                text: "AB".into(),
                x: 0.0,
                y: 10.0,
            },
            TextCode {
                glyph_ids: vec![3, 4],
                deltas: vec![(10.0, 0.0)],
                text: "CD".into(),
                x: 0.0,
                y: 30.0,
            },
        ]);
        let (doc, vp) = doc_with(obj);
        // 靠近第二行 (viewport y 中心 50) -> code_index 1。
        assert_eq!(
            hit_test_body_text(&doc, &vp, (15.0, 51.0))
                .unwrap()
                .code_index,
            1
        );
        assert_eq!(
            hit_test_body_text(&doc, &vp, (15.0, 25.0))
                .unwrap()
                .code_index,
            0
        );
    }
}
