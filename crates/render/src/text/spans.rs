//! Conversion from a TextBox's stored style spans to parley layout runs.
//!
//! A flat box ([`rofd_dom::AnnotationPayload::TextBox::spans`] empty) is one
//! run covering the whole content. A span box is one run per span; the spans'
//! texts concatenate to `content` (the editor maintains that invariant), so
//! char counts translate directly into byte ranges.

use std::ops::Range;

use rofd_dom::{Color, FontId, TextSpan};

/// One style run over byte ranges of the annotation content, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutRun {
    pub byte_range: Range<usize>,
    pub font: FontId,
    pub size: f64,
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

/// Build the layout runs for a TextBox.
///
/// `flat_font`/`flat_size`/`flat_color` apply when `spans` is empty. When
/// `spans` is non-empty their concatenated texts must equal `content`; the
/// ranges are derived from char counts, not from the span texts themselves.
pub fn layout_runs(
    content: &str,
    spans: &[TextSpan],
    flat_font: &FontId,
    flat_size: f64,
    flat_color: Color,
) -> Vec<LayoutRun> {
    if content.is_empty() {
        // Keep one empty run so the shaper's default font size comes from the
        // box, not parley's hard-coded fallback (empty-box caret/resize).
        return vec![LayoutRun {
            byte_range: 0..0,
            font: flat_font.clone(),
            size: flat_size,
            color: Some(flat_color),
            bold: false,
            italic: false,
        }];
    }
    if spans.is_empty() {
        return vec![LayoutRun {
            byte_range: 0..content.len(),
            font: flat_font.clone(),
            size: flat_size,
            color: Some(flat_color),
            bold: false,
            italic: false,
        }];
    }

    let make = |index: usize, byte_range: Range<usize>| {
        let span = &spans[index];
        LayoutRun {
            byte_range,
            font: span.font.clone(),
            size: span.size,
            // Effective color: `None` inherits the flat color. Concrete colors
            // let the shaper split style runs at color-only boundaries, so
            // per-span colors survive layout (glyph runs merge by style).
            color: Some(span.color.unwrap_or(flat_color)),
            bold: span.bold,
            italic: span.italic,
        }
    };
    let counts: Vec<usize> = spans.iter().map(|s| s.text.chars().count()).collect();
    let mut out: Vec<LayoutRun> = Vec::with_capacity(spans.len());
    let mut index = 0usize;
    let mut consumed = 0usize;
    let mut start = 0usize;
    while index < spans.len() && counts[index] == 0 {
        index += 1;
    }
    for (byte_idx, _) in content.char_indices() {
        if index < spans.len() && consumed == counts[index] {
            out.push(make(index, start..byte_idx));
            start = byte_idx;
            index += 1;
            consumed = 0;
            while index < spans.len() && counts[index] == 0 {
                index += 1;
            }
        }
        if index < spans.len() {
            consumed += 1;
        }
    }
    if index < spans.len() {
        out.push(make(index, start..content.len()));
    } else if let Some(last) = out.last_mut() {
        // Defensive: an under-filled span list still covers the content.
        last.byte_range.end = content.len();
    }
    if out.is_empty() {
        return vec![LayoutRun {
            byte_range: 0..content.len(),
            font: flat_font.clone(),
            size: flat_size,
            color: Some(flat_color),
            bold: false,
            italic: false,
        }];
    }
    out
}

/// Byte index of the `n`-th char (content length when past the end).
fn char_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

/// Insert `preedit` into the flow at char `offset`: returns the composed text
/// and the layout runs over it. The composition run inherits the style of the
/// char before `offset` (the char the insertion would take its style from).
pub fn layout_runs_with_preedit(
    content: &str,
    runs: &[LayoutRun],
    offset: usize,
    preedit: &str,
) -> (String, Vec<LayoutRun>) {
    if preedit.is_empty() {
        return (content.to_string(), runs.to_vec());
    }
    let off = offset.min(content.chars().count());
    let byte = char_byte(content, off);
    let probe = char_byte(content, off.saturating_sub(1));
    let Some(style) = runs
        .iter()
        .find(|r| r.byte_range.start <= probe && probe < r.byte_range.end)
        .or_else(|| runs.first())
    else {
        return (content.to_string(), runs.to_vec());
    };
    let make = |range: std::ops::Range<usize>, from: &LayoutRun| LayoutRun {
        byte_range: range,
        font: from.font.clone(),
        size: from.size,
        color: from.color,
        bold: from.bold,
        italic: from.italic,
    };
    let delta = preedit.len();
    let preedit_run = make(byte..byte + delta, style);
    let mut out: Vec<LayoutRun> = Vec::with_capacity(runs.len() + 1);
    let mut inserted = false;
    for run in runs {
        let r = run.byte_range.clone();
        if r.end <= byte {
            out.push(run.clone());
        } else if r.start >= byte {
            if !inserted {
                out.push(preedit_run.clone());
                inserted = true;
            }
            out.push(make(r.start + delta..r.end + delta, run));
        } else {
            if r.start < byte {
                out.push(make(r.start..byte, run));
            }
            if !inserted {
                out.push(preedit_run.clone());
                inserted = true;
            }
            if byte < r.end {
                out.push(make(byte + delta..r.end + delta, run));
            }
        }
    }
    if !inserted {
        out.push(preedit_run);
    }

    // The composition inherits the preceding style, so adjacent runs can end up
    // identical (the flat box becomes one run again). Merging keeps the glyph
    // groups identical to the committed text.
    let same = |a: &LayoutRun, b: &LayoutRun| {
        a.font == b.font
            && (a.size - b.size).abs() < 1e-9
            && a.color == b.color
            && a.bold == b.bold
            && a.italic == b.italic
    };
    let mut merged: Vec<LayoutRun> = Vec::with_capacity(out.len());
    for run in out {
        match merged.last_mut() {
            Some(prev) if same(prev, &run) => prev.byte_range.end = run.byte_range.end,
            _ => merged.push(run),
        }
    }

    let mut text = String::with_capacity(content.len() + preedit.len());
    text.push_str(&content[..byte]);
    text.push_str(preedit);
    text.push_str(&content[byte..]);
    (text, merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, font: &str, size: f64) -> TextSpan {
        TextSpan {
            text: text.into(),
            font: FontId::new(font),
            size,
            color: None,
            bold: false,
            italic: false,
        }
    }

    #[test]
    fn preedit_inherits_the_preceding_span_and_merges() {
        let spans = [
            TextSpan {
                text: "ab".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Some(Color::Rgb(255, 0, 0)),
                bold: false,
                italic: false,
            },
            TextSpan {
                text: "cd".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Some(Color::Rgb(0, 0, 255)),
                bold: false,
                italic: false,
            },
        ];
        let runs = layout_runs(
            "abcd",
            &spans,
            &FontId::new("F1"),
            12.0,
            Color::Rgb(0, 0, 0),
        );
        let (text, composed) = layout_runs_with_preedit("abcd", &runs, 3, "X");
        assert_eq!(text, "abcXd");
        assert_eq!(composed.len(), 2, "preedit merges into the preceding span");
        assert_eq!(composed[0].byte_range, 0..2);
        assert_eq!(composed[1].byte_range, 2..5);
        assert_eq!(composed[1].color, Some(Color::Rgb(0, 0, 255)));
    }

    #[test]
    fn preedit_in_a_flat_box_stays_one_run() {
        let runs = layout_runs("abcd", &[], &FontId::new("F1"), 12.0, Color::Rgb(0, 0, 0));
        let (text, composed) = layout_runs_with_preedit("abcd", &runs, 1, "XY");
        assert_eq!(text, "aXYbcd");
        assert_eq!(composed.len(), 1);
        assert_eq!(composed[0].byte_range, 0..6);
    }

    #[test]
    fn layout_runs_cover_content_with_exact_byte_ranges() {
        let spans = [span("ab", "F1", 12.0), span("汉字", "F2", 8.0)];
        let runs = layout_runs(
            "ab汉字",
            &spans,
            &FontId::new("F1"),
            12.0,
            Color::Rgb(0, 0, 0),
        );
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].byte_range, 0..2);
        assert_eq!(runs[1].byte_range, 2..8, "CJK chars are 3 bytes each");
        assert_eq!(runs[1].font.0, "F2");
        assert!((runs[1].size - 8.0).abs() < 1e-9);
    }

    #[test]
    fn flat_box_yields_one_run_over_the_whole_content() {
        let runs = layout_runs("abc", &[], &FontId::new("F1"), 12.0, Color::Rgb(1, 2, 3));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].byte_range, 0..3);
        assert_eq!(runs[0].color, Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn empty_content_keeps_one_flat_run_for_metrics() {
        let runs = layout_runs("", &[], &FontId::new("F1"), 4.2333, Color::Rgb(1, 2, 3));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].byte_range, 0..0);
        assert!((runs[0].size - 4.2333).abs() < 1e-9);
    }
}
