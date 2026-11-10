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
        return Vec::new();
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
            color: span.color,
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
        let runs = layout_runs(
            "abc",
            &[],
            &FontId::new("F1"),
            12.0,
            Color::Rgb(1, 2, 3),
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].byte_range, 0..3);
        assert_eq!(runs[0].color, Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn empty_content_yields_no_runs() {
        let runs = layout_runs("", &[], &FontId::new("F1"), 12.0, Color::Rgb(0, 0, 0));
        assert!(runs.is_empty());
    }
}
