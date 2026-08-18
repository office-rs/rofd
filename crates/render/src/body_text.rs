//! Body-text geometry shared by drawing, hit-testing, and selection rects
//! (spec P3 §5.3: single source of truth - never a second implementation).
//!
//! v1 limitation: TextObjects with a scaling/rotating CTM are not selectable
//! (only translation is accounted for); Freehand-style shaped text is drawn
//! by glyph ids but positioned by the same deltas used here.

use rofd_dom::{ObjectId, OfdDocument, Page, PageId, Rect, TextCode, TextObject};

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

/// All body-text segments on a page in reading order (layer order, object
/// order, code order): `(object, code_index, char_count)`.
fn page_text_segments(page: &Page) -> Vec<(ObjectId, usize, usize)> {
    let mut segs = Vec::new();
    for layer in &page.layers {
        for obj in &layer.objects {
            if let rofd_dom::PageObject::Text(t) = obj {
                for (ci, code) in t.codes.iter().enumerate() {
                    segs.push((t.id.clone(), ci, code_char_count(code)));
                }
            }
        }
    }
    segs
}

fn find_object<'a>(page: &'a Page, object: &ObjectId) -> Option<&'a TextObject> {
    page.layers
        .iter()
        .flat_map(|l| l.objects.iter())
        .find_map(|o| match o {
            rofd_dom::PageObject::Text(t) if &t.id == object => Some(t),
            _ => None,
        })
}

fn hit_to_segment(page: &Page, h: &TextHit) -> Option<usize> {
    page_text_segments(page)
        .iter()
        .position(|(o, c, _)| o == &h.object && *c == h.code_index)
}

/// Character ranges covering the span from `a` to `b` (either order) within
/// the page's reading order. Endpoint segments are partial (clamped to their
/// char count); segments between are full. Zero-width ranges are dropped.
pub fn body_text_ranges_between(page: &Page, a: &TextHit, b: &TextHit) -> Vec<BodyTextRange> {
    let segs = page_text_segments(page);
    let (Some(ia), Some(ib)) = (hit_to_segment(page, a), hit_to_segment(page, b)) else {
        return Vec::new();
    };
    // Normalize direction: lo is the earlier segment. Within the SAME
    // segment the offsets must also be ordered (the brief's draft missed
    // this - a reversed drag in one code would produce start > end and be
    // silently dropped by the zero-width filter below).
    let a_first = ia < ib || (ia == ib && a.char_offset <= b.char_offset);
    let (lo, hi, lo_off, hi_off) = if a_first {
        (ia, ib, a.char_offset, b.char_offset)
    } else {
        (ib, ia, b.char_offset, a.char_offset)
    };
    let span = hi - lo;
    segs[lo..=hi]
        .iter()
        .enumerate()
        .map(|(k, (o, c, n))| {
            let start = if k == 0 { lo_off.min(*n) } else { 0 };
            let end = if k == span { hi_off.min(*n) } else { *n };
            BodyTextRange {
                object: o.clone(),
                code_index: *c,
                start,
                end,
            }
        })
        .filter(|r| r.end > r.start)
        .collect()
}

/// Character class for double-click word segmentation (CJK: 连续同类字符为
/// 一段). 0 = alphabetic, 1 = numeric, 2 = CJK ideograph, 3 = other.
fn char_class(c: char) -> u8 {
    if c.is_alphabetic() && !is_cjk(c) {
        0
    } else if c.is_numeric() {
        1
    } else if is_cjk(c) {
        2
    } else {
        3
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{3000}'..='\u{303F}'
        | '\u{FF00}'..='\u{FFEF}')
}

/// Double-click word: the maximal run of one char class around the hit
/// character, inside the hit's TextCode.
pub fn word_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange> {
    let Some(t) = find_object(page, &hit.object) else {
        return Vec::new();
    };
    let Some(code) = t.codes.get(hit.code_index) else {
        return Vec::new();
    };
    let chars: Vec<char> = code.text.chars().collect();
    // Codes with no chars to classify (glyph-only, empty text) fall back to
    // the whole code; glyph-id codes WITH text still segment by chars
    // (char offsets == glyph indices when lengths agree). A fully empty code
    // yields nothing (zero-width ranges are dropped).
    if chars.is_empty() {
        let n = code_char_count(code);
        return if n > 0 {
            vec![BodyTextRange {
                object: hit.object.clone(),
                code_index: hit.code_index,
                start: 0,
                end: n,
            }]
        } else {
            Vec::new()
        };
    }
    let n = chars.len();
    // The clicked character: at the boundary, take the char before the caret
    // when past the end, else the char at the offset.
    let idx = hit.char_offset.min(n - 1);
    let class = char_class(chars[idx]);
    let mut start = idx;
    let mut end = idx + 1;
    while start > 0 && char_class(chars[start - 1]) == class {
        start -= 1;
    }
    while end < n && char_class(chars[end]) == class {
        end += 1;
    }
    vec![BodyTextRange {
        object: hit.object.clone(),
        code_index: hit.code_index,
        start,
        end,
    }]
}

/// Triple-click paragraph: every TextCode of the hit's TextObject, full
/// content (spec: 同一 TextObject 的全部内容).
pub fn paragraph_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange> {
    let Some(t) = find_object(page, &hit.object) else {
        return Vec::new();
    };
    t.codes
        .iter()
        .enumerate()
        .filter(|(_, code)| code_char_count(code) > 0)
        .map(|(ci, code)| BodyTextRange {
            object: hit.object.clone(),
            code_index: ci,
            start: 0,
            end: code_char_count(code),
        })
        .collect()
}

/// One covering rect per selected code line, viewport space. Uses the same
/// shared cells + line band as `hit_test_body_text`.
pub fn text_selection_rects(
    doc: &OfdDocument,
    vp: &Viewport,
    sel: &BodyTextSelection,
) -> Vec<Rect> {
    let Some(page_idx) = doc.pages.iter().position(|p| p.id == sel.page) else {
        return Vec::new();
    };
    let Some((ox, oy)) = crate::composite::page_origin(doc, vp, page_idx) else {
        return Vec::new();
    };
    let page = &doc.pages[page_idx];
    let mut out = Vec::new();
    for range in &sel.ranges {
        if range.start >= range.end {
            continue;
        }
        let Some(t) = find_object(page, &range.object) else {
            continue;
        };
        let Some(code) = t.codes.get(range.code_index) else {
            continue;
        };
        let n = code_char_count(code);
        let cells = code_char_cells(t, code, n);
        let (Some(first), Some(last)) = (cells.get(range.start), cells.get(range.end - 1)) else {
            continue;
        };
        let x0 = ox + (t.boundary.x + first.x) * vp.zoom;
        let x1 = ox + (t.boundary.x + last.x + last.advance) * vp.zoom;
        let y0 = oy + (t.boundary.y + last.y - t.size) * vp.zoom;
        let y1 = oy + (t.boundary.y + last.y + t.size * 0.25) * vp.zoom;
        out.push(Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        });
    }
    out
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
    fn ranges_between_same_code_clamps_and_orders() {
        let obj = text_obj(vec![code4()]);
        let page = &doc_with(obj).0.pages[0];
        let hit = |o: usize| TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: 0,
            char_offset: o,
        };
        // 反向拖（b 在 a 前）-> 归一化为 [1,3)。
        let r = body_text_ranges_between(page, &hit(3), &hit(1));
        assert_eq!(
            r,
            vec![BodyTextRange {
                object: ObjectId::new("t1"),
                code_index: 0,
                start: 1,
                end: 3
            }]
        );
        // 同 offset -> 空（零宽选区不产出 range）。
        assert!(body_text_ranges_between(page, &hit(2), &hit(2)).is_empty());
        // 越界 clamp 到 n。
        let r = body_text_ranges_between(page, &hit(9), &hit(0));
        assert_eq!(r[0].end, 4);
    }

    #[test]
    fn ranges_between_two_codes_partial_ends_full_middle() {
        let obj = text_obj(vec![
            TextCode {
                glyph_ids: vec![1, 2, 3],
                deltas: vec![(10.0, 0.0), (10.0, 0.0)],
                text: "ABC".into(),
                x: 0.0,
                y: 10.0,
            },
            TextCode {
                glyph_ids: vec![4, 5, 6],
                deltas: vec![(10.0, 0.0), (10.0, 0.0)],
                text: "DEF".into(),
                x: 0.0,
                y: 30.0,
            },
            TextCode {
                glyph_ids: vec![7, 8],
                deltas: vec![(10.0, 0.0)],
                text: "GH".into(),
                x: 0.0,
                y: 50.0,
            },
        ]);
        let page = &doc_with(obj).0.pages[0];
        let hit = |ci: usize, o: usize| TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: ci,
            char_offset: o,
        };
        let rs = body_text_ranges_between(page, &hit(0, 2), &hit(2, 1));
        // code0 [2,3) + code1 全部 + code2 [0,1)。
        assert_eq!(rs.len(), 3);
        assert_eq!((rs[0].code_index, rs[0].start, rs[0].end), (0, 2, 3));
        assert_eq!((rs[1].code_index, rs[1].start, rs[1].end), (1, 0, 3));
        assert_eq!((rs[2].code_index, rs[2].start, rs[2].end), (2, 0, 1));
    }

    #[test]
    fn word_range_at_same_char_class_run() {
        // "AB12中" (glyph 计数 5)：点在 'B'(offset 1) -> 词 = [0,2)（连续字母段）。
        let code = TextCode {
            glyph_ids: vec![1, 2, 3, 4, 5],
            deltas: vec![(10.0, 0.0); 4],
            text: "AB12中".into(),
            x: 0.0,
            y: 10.0,
        };
        let obj = text_obj(vec![code]);
        let page = &doc_with(obj).0.pages[0];
        let hit_at = |o: usize| TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: 0,
            char_offset: o,
        };
        let w = word_range_at(page, &hit_at(1));
        assert_eq!(
            w,
            vec![BodyTextRange {
                object: ObjectId::new("t1"),
                code_index: 0,
                start: 0,
                end: 2
            }]
        );
        // 点在 '1'(offset 2) -> [2,4)（数字段）。
        assert_eq!(word_range_at(page, &hit_at(2))[0].start, 2);
        assert_eq!(word_range_at(page, &hit_at(2))[0].end, 4);
        // CJK 每字一类段：点在 '中'(offset 4) -> [4,5)。
        assert_eq!(
            (
                word_range_at(page, &hit_at(4))[0].start,
                word_range_at(page, &hit_at(4))[0].end
            ),
            (4, 5)
        );
        // 空白类：命中在空格上 -> 只选空格本身。
    }

    #[test]
    fn paragraph_range_covers_whole_object() {
        let obj = text_obj(vec![
            TextCode {
                glyph_ids: vec![1],
                deltas: vec![],
                text: "A".into(),
                x: 0.0,
                y: 10.0,
            },
            TextCode {
                glyph_ids: vec![2],
                deltas: vec![],
                text: "B".into(),
                x: 0.0,
                y: 30.0,
            },
        ]);
        let page = &doc_with(obj).0.pages[0];
        let hit = TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: 1,
            char_offset: 0,
        };
        let rs = paragraph_range_at(page, &hit);
        assert_eq!(rs.len(), 2, "三击 = 同一 TextObject 全部 codes");
        assert_eq!((rs[0].code_index, rs[0].start, rs[0].end), (0, 0, 1));
        assert_eq!((rs[1].code_index, rs[1].start, rs[1].end), (1, 0, 1));
    }

    #[test]
    fn selection_rects_one_per_code_line() {
        // boundary (10,20)；code0 y=10 行带 y ∈ [20, 32.5]；选 [1,3) -> x ∈ [20, 40]。
        let obj = text_obj(vec![code4()]);
        let (doc, vp) = doc_with(obj);
        let sel = BodyTextSelection {
            page: PageId::new("P0"),
            ranges: vec![BodyTextRange {
                object: ObjectId::new("t1"),
                code_index: 0,
                start: 1,
                end: 3,
            }],
        };
        let rects = text_selection_rects(&doc, &vp, &sel);
        assert_eq!(rects.len(), 1);
        let r = &rects[0];
        assert_eq!(r.x, 10.0 + 10.0, "start cell x");
        assert_eq!(r.x + r.w, 10.0 + 20.0 + 10.0, "last cell x + advance");
        assert_eq!(r.y, 20.0, "band top");
        assert_eq!(r.y + r.h, 20.0 + 12.5, "band bottom (size + 0.25*size)");
        // 未知页 -> 空。
        let mut bad = sel.clone();
        bad.page = PageId::new("P9");
        assert!(text_selection_rects(&doc, &vp, &bad).is_empty());
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
