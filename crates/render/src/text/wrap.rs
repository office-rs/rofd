//! Wrapped annotation-text layout: parley line breaking at a maximum width,
//! with per-line char/glyph ranges and geometry for rendering, caret math and
//! OFD appearance serialization (spec 2026-11-09 §1.1, §4.1).

use std::rc::Rc;

use peniko::FontData;
use rofd_dom::TextLineHint;

use super::shape::{RawLine, ShapedGlyph};

/// One visual line of shaped annotation text.
#[derive(Debug, Clone)]
pub struct WrappedLine {
    /// First char offset of the line in the content.
    pub char_start: usize,
    /// Number of chars on the line (line-break characters excluded).
    pub char_len: usize,
    /// First glyph index in [`WrappedText::glyphs`].
    pub glyph_start: usize,
    /// One past the last glyph index.
    pub glyph_end: usize,
    /// Line advance (pen position after the last glyph, trailing space
    /// included).
    pub width: f64,
    /// Line box top relative to the text origin (first line is 0).
    pub top: f64,
    /// Baseline relative to the text origin.
    pub baseline: f64,
    /// Line box height.
    pub height: f64,
    /// Line text: trailing spaces kept, line-break characters removed.
    pub text: String,
}

/// A flat annotation text shaped and wrapped at a maximum width.
#[derive(Debug, Clone)]
pub struct WrappedText {
    /// Draw handle (the first font group's font, mirroring `FontStore::shape`).
    pub font: Option<FontData>,
    /// Glyphs with line-relative x and layout-relative y.
    pub glyphs: Rc<Vec<ShapedGlyph>>,
    /// Visual lines in reading order.
    pub lines: Rc<Vec<WrappedLine>>,
    /// Total chars in the content (line-break characters included).
    pub total_chars: usize,
}

impl WrappedText {
    pub(crate) fn from_layout(
        text: &str,
        groups: Vec<(FontData, Vec<ShapedGlyph>)>,
        raw_lines: Vec<RawLine>,
    ) -> Self {
        let total_chars = text.chars().count();
        let len = text.len();
        let font = groups.first().map(|(font, _)| font.clone());
        let glyphs: Vec<ShapedGlyph> = groups.into_iter().flat_map(|(_, gs)| gs).collect();
        let glyph_count = glyphs.len();

        let mut lines = Vec::with_capacity(raw_lines.len());
        let mut glyph_cursor = 0usize;
        for raw in raw_lines {
            let start = raw.byte_range.start.min(len);
            let end = raw.byte_range.end.min(len);
            let raw_text = text.get(start..end).unwrap_or("");
            let line_text = raw_text.trim_end_matches(['\n', '\r']).to_string();
            let char_start = text[..start].chars().count();
            let char_len = line_text.chars().count();
            let glyph_start = glyph_cursor;
            let available = glyph_count.saturating_sub(glyph_cursor);
            let glyph_end = glyph_cursor + raw.glyph_count.min(available);
            glyph_cursor = glyph_end;
            lines.push(WrappedLine {
                char_start,
                char_len,
                glyph_start,
                glyph_end,
                width: raw.width,
                top: raw.top,
                baseline: raw.baseline,
                height: raw.height,
                text: line_text,
            });
        }

        Self {
            font,
            glyphs: Rc::new(glyphs),
            lines: Rc::new(lines),
            total_chars,
        }
    }

    /// Height required by the wrapped lines: last line top + height.
    /// Empty content needs no height (0.0).
    pub fn height(&self) -> f64 {
        if self.total_chars == 0 {
            return 0.0;
        }
        self.lines
            .last()
            .map(|line| line.top + line.height)
            .unwrap_or(0.0)
    }

    /// Map a char offset to `(line index, line-local column)`.
    ///
    /// Rules (spec 2026-11-09 §1.3):
    /// - a soft-wrap boundary belongs to the NEXT line's start;
    /// - an explicit newline character's position belongs to the previous
    ///   line's end;
    /// - the end of the content belongs to the last line's end.
    pub fn line_at_offset(&self, offset: usize) -> (usize, usize) {
        if self.lines.is_empty() {
            return (0, 0);
        }
        let last = self.lines.len() - 1;
        let offset = offset.min(self.total_chars);
        for (i, line) in self.lines.iter().enumerate() {
            let start = line.char_start;
            let end = start + line.char_len;
            if offset < start {
                let prev = i.saturating_sub(1);
                return (prev, self.lines[prev].char_len);
            }
            if offset < end {
                return (i, offset - start);
            }
            if offset == end {
                match self.lines.get(i + 1) {
                    // Explicit break gap (offset on a newline char): line end.
                    Some(next) if offset < next.char_start => return (i, line.char_len),
                    // Soft break / next line's start.
                    Some(_) => return (i + 1, 0),
                    None => return (i, line.char_len),
                }
            }
        }
        (last, self.lines[last].char_len)
    }

    /// Convert to the serializer-facing per-line hints (spec §1.5, §4.5).
    pub fn to_layout_hints(&self) -> Vec<TextLineHint> {
        self.lines
            .iter()
            .map(|line| {
                let mut advances = Vec::with_capacity(line.glyph_end - line.glyph_start);
                for i in line.glyph_start..line.glyph_end {
                    let x = self.glyphs[i].x as f64;
                    let next_x = if i + 1 < line.glyph_end {
                        self.glyphs[i + 1].x as f64
                    } else {
                        line.width
                    };
                    advances.push((next_x - x).max(0.0));
                }
                TextLineHint {
                    text: line.text.clone(),
                    top: line.top,
                    baseline: line.baseline,
                    height: line.height,
                    width: line.width,
                    advances,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rofd_dom::{FontId, Resources};

    use crate::text::FontStore;

    fn store() -> FontStore {
        let bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(bytes.to_vec()))
    }

    #[test]
    fn wraps_latin_at_spaces() {
        let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
        assert_eq!(w.lines.len(), 2, "two visual lines");
        assert_eq!(w.lines[0].text, "abc def "); // trailing space stays on line 1
        assert_eq!(w.lines[1].text, "ghi jkl");
        assert_eq!(w.lines[0].char_start, 0);
        assert_eq!(w.lines[1].char_start, 8);
    }

    #[test]
    fn wraps_cjk_per_char() {
        let w = store().shape_wrapped(&FontId::new("F1"), "你好世界你好世界", 12.0, 30.0);
        if w.glyphs.len() < 8 {
            // CJK advances come from a system fallback font; a machine
            // without one cannot exercise per-char wrapping.
            eprintln!("skipping: no CJK fallback font");
            return;
        }
        assert_eq!(w.lines.len(), 4, "8 chars / 2 per line");
        assert!(w.lines.iter().all(|l| l.char_len == 2));
        assert_eq!(w.total_chars, 8);
    }

    #[test]
    fn breaks_long_words() {
        // TestFont @12 is ~7.2mm/char; 45mm forces emergency breaks inside the word.
        let w = store().shape_wrapped(&FontId::new("F1"), "abcdefghijklmnop", 12.0, 45.0);
        assert_eq!(w.lines.len(), 3);
        let joined: String = w.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(joined, "abcdefghijklmnop");
    }

    #[test]
    fn explicit_newline_is_a_line() {
        let w = store().shape_wrapped(&FontId::new("F1"), "ab cd\nef gh", 12.0, 1000.0);
        assert_eq!(w.lines.len(), 2);
        assert_eq!(w.lines[0].text, "ab cd");
        assert_eq!(w.lines[1].text, "ef gh");
        assert_eq!(w.total_chars, 11);
    }

    #[test]
    fn empty_content_has_one_empty_line() {
        let w = store().shape_wrapped(&FontId::new("F1"), "", 12.0, 40.0);
        assert_eq!(w.lines.len(), 1);
        assert_eq!(w.lines[0].text, "");
        assert_eq!(w.total_chars, 0);
        assert_eq!(w.height(), 0.0, "empty content needs no height");
    }

    #[test]
    fn line_at_offset_boundaries() {
        let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
        assert_eq!(w.line_at_offset(0), (0, 0));
        assert_eq!(
            w.line_at_offset(7),
            (0, 7),
            "trailing space belongs to line 1"
        );
        assert_eq!(w.line_at_offset(8), (1, 0), "soft break -> next line start");

        let w = store().shape_wrapped(&FontId::new("F1"), "ab cd\nef gh", 12.0, 1000.0);
        assert_eq!(
            w.line_at_offset(5),
            (0, 5),
            "offset on the newline -> line 1 end"
        );
        assert_eq!(w.line_at_offset(6), (1, 0));
        assert_eq!(
            w.line_at_offset(11),
            (1, 5),
            "end of content -> last line end"
        );
    }

    #[test]
    fn to_layout_hints_geometry() {
        let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
        let h = w.to_layout_hints();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].top, 0.0, "first line top is the text origin");
        assert!((h[0].advances.iter().sum::<f64>() - h[0].width).abs() < 1e-6);
        assert!(
            (h[1].top - h[0].height).abs() < 1e-6,
            "lines stack by height"
        );
        assert!(h[0].baseline > 0.0);
    }

    #[test]
    fn wrapped_layout_is_cached() {
        let s = store();
        let a = s.shape_wrapped(&FontId::new("F1"), "cache me", 12.0, 40.0);
        let b = s.shape_wrapped(&FontId::new("F1"), "cache me", 12.0, 40.0);
        assert!(std::rc::Rc::ptr_eq(&a, &b), "same key -> same Rc");
    }
}
