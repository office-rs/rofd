//! Span maintenance for annotation TextBoxes (spec 2026-11-10 rich text).
//!
//! Invariants maintained here:
//! - an empty `spans` list means a flat box (one style for the whole content);
//! - non-empty spans concatenate to `content` char-for-char;
//! - no empty spans and no two adjacent spans with identical style.
//!
//! All functions are pure over the payload so the editor commands can run them
//! inside a single `Transaction`.

use rofd_dom::{AnnotationPayload, Color, FontId, TextSpan};

/// The effective style at a char offset (flat boxes report their flat style).
#[derive(Debug, Clone, PartialEq)]
pub struct SpanStyle {
    pub font: FontId,
    pub size: f64,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Byte index of the `n`-th char (content length when `n` is past the end).
fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

fn same_style(a: &TextSpan, b: &TextSpan) -> bool {
    a.font == b.font
        && (a.size - b.size).abs() < 1e-9
        && a.color == b.color
        && a.bold == b.bold
        && a.italic == b.italic
}

/// Materialize a flat box into a single span covering its content.
pub fn ensure_spans(payload: &mut AnnotationPayload) {
    let AnnotationPayload::TextBox {
        content,
        font,
        size,
        color,
        spans,
        ..
    } = payload
    else {
        return;
    };
    if spans.is_empty() && !content.is_empty() {
        spans.push(TextSpan {
            text: content.clone(),
            font: font.clone(),
            size: *size,
            color: Some(*color),
            bold: false,
            italic: false,
        });
    }
}

/// Merge adjacent spans with identical style and drop empty ones.
fn normalize(spans: &mut Vec<TextSpan>) {
    spans.retain(|s| !s.text.is_empty());
    let mut i = 0;
    while i + 1 < spans.len() {
        if same_style(&spans[i], &spans[i + 1]) {
            let next = spans.remove(i + 1);
            spans[i].text.push_str(&next.text);
        } else {
            i += 1;
        }
    }
}

/// `(span index, local char offset)` where `offset` falls. A boundary belongs
/// to the span ENDING there (the preceding span), except at 0.
fn boundary_span(spans: &[TextSpan], offset: usize) -> (usize, usize) {
    let mut start = 0usize;
    for (i, span) in spans.iter().enumerate() {
        let len = char_count(&span.text);
        if offset <= start + len {
            return (i, offset - start);
        }
        start += len;
    }
    let last = spans.len().saturating_sub(1);
    (last, spans.get(last).map(|s| char_count(&s.text)).unwrap_or(0))
}

/// Insert `text` at `offset`, inheriting the style of the preceding char (or
/// the following char at offset 0). Updates `content` too.
pub fn spans_insert(payload: &mut AnnotationPayload, offset: usize, text: &str) {
    let AnnotationPayload::TextBox {
        content, spans, ..
    } = payload
    else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let total = char_count(content);
    let offset = offset.min(total);
    let byte = byte_at(content, offset);
    content.insert_str(byte, text);
    if spans.is_empty() {
        return;
    }
    let (mut index, mut local) = boundary_span(spans, offset);
    if local == 0 && index > 0 {
        index -= 1;
        local = char_count(&spans[index].text);
    }
    let byte = byte_at(&spans[index].text, local);
    spans[index].text.insert_str(byte, text);
    normalize(spans);
}

/// Delete `len` chars at `offset`. Updates `content` too.
pub fn spans_delete(payload: &mut AnnotationPayload, offset: usize, len: usize) {
    let AnnotationPayload::TextBox {
        content, spans, ..
    } = payload
    else {
        return;
    };
    let total = char_count(content);
    let start = offset.min(total);
    let end = (offset + len).min(total);
    if start >= end {
        return;
    }
    let bytes = byte_at(content, start)..byte_at(content, end);
    content.replace_range(bytes, "");
    if spans.is_empty() {
        return;
    }
    let mut cursor = 0usize;
    for span in spans.iter_mut() {
        let span_len = char_count(&span.text);
        let span_start = cursor;
        let span_end = cursor + span_len;
        cursor = span_end;
        let a = start.max(span_start);
        let b = end.min(span_end);
        if a >= b {
            continue;
        }
        let rel_a = a - span_start;
        let rel_b = b - span_start;
        span.text
            .replace_range(byte_at(&span.text, rel_a)..byte_at(&span.text, rel_b), "");
    }
    normalize(spans);
}

/// Apply `f` to the spans covering `range`.
///
/// - `None` or an empty range is a whole-box operation: the result is written
///   into the flat fields and `spans` is cleared.
/// - Otherwise the range is split out, mutated, normalized, and merged back to
///   the flat fields only when that is lossless (a single non-bold/non-italic
///   span).
pub fn spans_apply(
    payload: &mut AnnotationPayload,
    range: Option<(usize, usize)>,
    f: &mut dyn FnMut(&mut TextSpan),
) {
    let total = match payload {
        AnnotationPayload::TextBox { content, .. } => char_count(content),
        _ => return,
    };
    let bounds = match range {
        Some((a, b)) => {
            let a = a.min(total);
            let b = b.min(total);
            Some((a.min(b), a.max(b)))
        }
        None => None,
    };

    if bounds.is_none_or(|(a, b)| a == b) {
        let AnnotationPayload::TextBox {
            font,
            size,
            color,
            spans,
            ..
        } = payload
        else {
            return;
        };
        let mut probe = TextSpan {
            text: String::new(),
            font: font.clone(),
            size: *size,
            color: Some(*color),
            bold: false,
            italic: false,
        };
        f(&mut probe);
        *font = probe.font;
        *size = probe.size;
        if let Some(c) = probe.color {
            *color = c;
        }
        spans.clear();
        return;
    }

    let (start, end) = bounds.expect("bounds checked above");
    ensure_spans(payload);
    let AnnotationPayload::TextBox {
        font,
        size,
        color,
        spans,
        ..
    } = payload
    else {
        return;
    };
    split_at(spans, start);
    split_at(spans, end);
    let mut cursor = 0usize;
    for span in spans.iter_mut() {
        let span_len = char_count(&span.text);
        let span_start = cursor;
        let span_end = cursor + span_len;
        cursor = span_end;
        if span_end <= start || span_start >= end {
            continue;
        }
        f(span);
    }
    normalize(spans);

    // Merge back to the flat model only when it can express the result.
    if spans.len() == 1 && !spans[0].bold && !spans[0].italic {
        let only = &spans[0];
        *font = only.font.clone();
        *size = only.size;
        if let Some(c) = only.color {
            *color = c;
        }
        spans.clear();
    }
}

/// Split the span containing the strictly-internal char offset `at`.
fn split_at(spans: &mut Vec<TextSpan>, at: usize) {
    let mut start = 0usize;
    for i in 0..spans.len() {
        let len = char_count(&spans[i].text);
        let end = start + len;
        if at > start && at < end {
            let byte = byte_at(&spans[i].text, at - start);
            let right = spans[i].text.split_off(byte);
            let mut tail = spans[i].clone();
            tail.text = right;
            spans.insert(i + 1, tail);
            return;
        }
        start = end;
    }
}

/// The style at `offset` (flat boxes report their flat style). `None` for
/// non-TextBox payloads.
pub fn style_at(payload: &AnnotationPayload, offset: usize) -> Option<SpanStyle> {
    let AnnotationPayload::TextBox {
        content,
        font,
        size,
        color,
        spans,
        ..
    } = payload
    else {
        return None;
    };
    if spans.is_empty() {
        return Some(SpanStyle {
            font: font.clone(),
            size: *size,
            color: *color,
            bold: false,
            italic: false,
        });
    }
    let offset = offset.min(char_count(content));
    let mut cursor = 0usize;
    let last = spans.len() - 1;
    for (i, span) in spans.iter().enumerate() {
        let len = char_count(&span.text);
        let end = cursor + len;
        if offset < end || i == last {
            return Some(SpanStyle {
                font: span.font.clone(),
                size: span.size,
                color: span.color.unwrap_or(*color),
                bold: span.bold,
                italic: span.italic,
            });
        }
        cursor = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::Rgb(255, 0, 0);
    const BLUE: Color = Color::Rgb(0, 0, 255);

    fn span(text: &str, size: f64, color: Color) -> TextSpan {
        TextSpan {
            text: text.into(),
            font: FontId::new("F1"),
            size,
            color: Some(color),
            bold: false,
            italic: false,
        }
    }

    fn textbox(content: &str, spans: Vec<TextSpan>) -> AnnotationPayload {
        AnnotationPayload::TextBox {
            rect: rofd_dom::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 30.0,
            },
            content: content.into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
            border: None,
            spans,
        }
    }

    fn parts(p: &AnnotationPayload) -> (&str, &[TextSpan]) {
        let AnnotationPayload::TextBox {
            content, spans, ..
        } = p
        else {
            panic!("expected TextBox");
        };
        (content, spans)
    }

    fn texts(p: &AnnotationPayload) -> Vec<&str> {
        parts(p).1.iter().map(|s| s.text.as_str()).collect()
    }

    fn flat(p: &AnnotationPayload) -> (f64, Option<Color>) {
        let AnnotationPayload::TextBox { size, color, .. } = p else {
            panic!("expected TextBox");
        };
        (*size, Some(*color))
    }

    #[test]
    fn insert_at_a_span_boundary_inherits_the_preceding_style() {
        let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
        spans_insert(&mut p, 2, "X");
        assert_eq!(parts(&p).0, "abXcd");
        assert_eq!(texts(&p), vec!["abX", "cd"]);
        assert_eq!(parts(&p).1[1].color, Some(BLUE));
    }

    #[test]
    fn insert_in_the_middle_of_one_span_keeps_a_single_span() {
        let mut p = textbox("ab", vec![span("ab", 12.0, RED)]);
        spans_insert(&mut p, 1, "X");
        assert_eq!(texts(&p), vec!["aXb"]);
    }

    #[test]
    fn insert_at_zero_takes_the_following_style() {
        let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
        spans_insert(&mut p, 0, "X");
        assert_eq!(texts(&p), vec!["Xab", "cd"]);
        assert_eq!(parts(&p).1[0].color, Some(RED));
    }

    #[test]
    fn delete_crossing_a_span_boundary_updates_content_and_styles() {
        let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
        spans_delete(&mut p, 1, 2); // removes "bc" -> "ad"
        assert_eq!(parts(&p).0, "ad");
        assert_eq!(texts(&p), vec!["a", "d"]);
        assert_eq!(parts(&p).1[0].color, Some(RED));
        assert_eq!(parts(&p).1[1].color, Some(BLUE));
    }

    #[test]
    fn delete_whole_span_drops_it() {
        let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
        spans_delete(&mut p, 2, 2);
        assert_eq!(texts(&p), vec!["ab"]);
    }

    #[test]
    fn apply_none_writes_the_flat_fields_and_clears_spans() {
        let mut p = textbox("abc", vec![span("abc", 12.0, RED)]);
        spans_apply(&mut p, None, &mut |s| s.size = 20.0);
        assert!(parts(&p).1.is_empty());
        assert_eq!(flat(&p).0, 20.0);
    }

    #[test]
    fn apply_whole_uniform_box_merges_back_to_flat() {
        let mut p = textbox("abc", vec![span("abc", 12.0, RED)]);
        spans_apply(&mut p, Some((0, 3)), &mut |s| s.size = 20.0);
        assert!(parts(&p).1.is_empty(), "uniform result merges to flat");
        assert_eq!(flat(&p).0, 20.0);
    }

    #[test]
    fn apply_whole_mixed_box_keeps_the_other_attribute() {
        let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
        spans_apply(&mut p, Some((0, 4)), &mut |s| s.size = 20.0);
        assert_eq!(texts(&p), vec!["ab", "cd"], "colors keep the box split");
        assert_eq!(parts(&p).1[0].color, Some(RED));
        assert_eq!(parts(&p).1[1].color, Some(BLUE));
        assert!(parts(&p).1.iter().all(|s| (s.size - 20.0).abs() < 1e-9));
    }

    #[test]
    fn apply_partial_range_splits_into_three_spans() {
        let mut p = textbox("hello", vec![span("hello", 12.0, RED)]);
        spans_apply(&mut p, Some((1, 3)), &mut |s| s.size = 30.0);
        assert_eq!(texts(&p), vec!["h", "el", "lo"]);
        assert!((parts(&p).1[1].size - 30.0).abs() < 1e-9);
    }

    #[test]
    fn apply_whole_bold_box_keeps_spans_because_flat_cannot_express_bold() {
        let mut p = textbox("abc", vec![span("abc", 12.0, RED)]);
        if let AnnotationPayload::TextBox { spans, .. } = &mut p {
            spans[0].bold = true;
        }
        spans_apply(&mut p, Some((0, 3)), &mut |s| s.size = 20.0);
        assert_eq!(texts(&p), vec!["abc"]);
        assert!(parts(&p).1[0].bold);
    }

    #[test]
    fn style_at_reports_flat_and_span_styles() {
        let flat_box = textbox("abc", vec![]);
        let s = style_at(&flat_box, 1).expect("flat style");
        assert!((s.size - 12.0).abs() < 1e-9);
        assert_eq!(s.color, Color::Rgb(0, 0, 0));

        let span_box = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 8.0, BLUE)]);
        let s = style_at(&span_box, 3).expect("span style");
        assert_eq!(s.color, BLUE);
        assert!((s.size - 8.0).abs() < 1e-9);
    }
}
