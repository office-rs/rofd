use rofd_dom::{AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, NoteIcon, Rect};

use crate::editor::Editor;
use crate::payload_util::{set_color, set_width};

/// Collapse a rich TextBox to its flat fallback style. Runs are the
/// parse-time appearance snapshot of a foreign file; any text edit makes
/// them stale (fragments no longer map to the content), so the first
/// mutation folds the first run's style into the flat fields and clears
/// the runs - inside the SAME transaction, so one undo restores the rich
/// appearance wholesale. A no-op for runs-empty boxes and other payloads.
fn collapse_runs(payload: &mut AnnotationPayload) {
    if let AnnotationPayload::TextBox {
        font,
        size,
        color,
        runs,
        ..
    } = payload
    {
        if let Some(first) = runs.first() {
            let run_font = first.font.clone();
            let run_size = first.size;
            let run_color = first.color;
            *font = run_font;
            *size = run_size;
            if let Some(c) = run_color {
                *color = c;
            }
            runs.clear();
        }
    }
}

/// Normalize line endings to `\n` (`\r\n` and lone `\r`).
///
/// Annotation content's model invariant is LF-only (spec 2026-11-09 §1.2.1):
/// parley treats `\r\n` as TWO newline clusters, which would render a blank
/// line between them.
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

impl Editor {
    /// Set the primary color.
    pub fn set_annotation_color(&mut self, id: &AnnotationId, color: Color) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        set_color(&mut after.payload, color);
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Set the stroke width (Freehand/Shape).
    pub fn set_annotation_width(&mut self, id: &AnnotationId, width: f64) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        set_width(&mut after.payload, width);
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Set the font size (millimetres) of a TextBox in ONE transaction.
    /// `measure` follows the [`Self::insert_text_measured`] contract and is
    /// applied inside the same transaction (grow-only callers pass `None`
    /// when the box does not need to grow).
    pub fn set_annotation_font_size_measured<F>(
        &mut self,
        id: &AnnotationId,
        size_mm: f64,
        measure: F,
    ) where
        F: FnOnce(&AnnotationPayload) -> Option<f64>,
    {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        let AnnotationPayload::TextBox { size, .. } = &mut after.payload else {
            return;
        };
        *size = size_mm;
        if let Some(height) = measure(&after.payload) {
            if let AnnotationPayload::TextBox { rect, .. } = &mut after.payload {
                rect.h = height;
            }
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Set the font family of a TextBox (`font = FontId::new(family)`) in ONE
    /// transaction. Collapses rich runs like any other text edit.
    pub fn set_annotation_font_family_measured<F>(
        &mut self,
        id: &AnnotationId,
        family: &str,
        measure: F,
    ) where
        F: FnOnce(&AnnotationPayload) -> Option<f64>,
    {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        let AnnotationPayload::TextBox { font, .. } = &mut after.payload else {
            return;
        };
        *font = FontId::new(family);
        if let Some(height) = measure(&after.payload) {
            if let AnnotationPayload::TextBox { rect, .. } = &mut after.payload {
                rect.h = height;
            }
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Insert text into a text annotation (TextBox/Note/Watermark) at char offset.
    pub fn insert_text(&mut self, id: &AnnotationId, offset: usize, chars: &str) {
        self.insert_text_measured(id, offset, chars, |_| None);
    }

    /// Insert text and let `measure` suggest a new annotation height. The
    /// closure sees the post-edit payload (after `collapse_runs`), so the
    /// component can shape the exact text with its font store; a `Some(h)`
    /// is applied in the SAME transaction (one undo restores text + height).
    /// `measure` must return `Some` only when growth is needed.
    pub fn insert_text_measured<F>(
        &mut self,
        id: &AnnotationId,
        offset: usize,
        chars: &str,
        measure: F,
    ) where
        F: FnOnce(&AnnotationPayload) -> Option<f64>,
    {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        let chars = normalize_newlines(chars);
        if let Some(content) = text_content_mut(&mut after.payload) {
            let off = offset.min(content.chars().count());
            let mut new = content.chars().take(off).collect::<String>();
            new.push_str(&chars);
            new.extend(content.chars().skip(off));
            *content = new;
        }
        if let Some(height) = measure(&after.payload) {
            if let AnnotationPayload::TextBox { rect, .. } = &mut after.payload {
                rect.h = height;
            }
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Replace the char range `[start, end)` with `chars` in ONE transaction.
    /// Typing / Backspace / Delete / Enter / paste / IME commit over an active
    /// text selection all route here, so the edit is a single undo step.
    /// `measure` follows the same contract as [`Self::insert_text_measured`].
    pub fn replace_text_measured<F>(
        &mut self,
        id: &AnnotationId,
        start: usize,
        end: usize,
        chars: &str,
        measure: F,
    ) where
        F: FnOnce(&AnnotationPayload) -> Option<f64>,
    {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        let chars = normalize_newlines(chars);
        if let Some(content) = text_content_mut(&mut after.payload) {
            let total = content.chars().count();
            let s = start.min(total);
            let e = end.min(total).max(s);
            let mut new: String = content.chars().take(s).collect();
            new.push_str(&chars);
            new.extend(content.chars().skip(e));
            *content = new;
        }
        if let Some(height) = measure(&after.payload) {
            if let AnnotationPayload::TextBox { rect, .. } = &mut after.payload {
                rect.h = height;
            }
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Delete `len` chars from a text annotation at char offset.
    pub fn delete_text(&mut self, id: &AnnotationId, offset: usize, len: usize) {
        self.delete_text_measured(id, offset, len, |_| None);
    }

    /// Delete text and let `measure` suggest a new annotation height (same
    /// contract as [`Self::insert_text_measured`]).
    pub fn delete_text_measured<F>(
        &mut self,
        id: &AnnotationId,
        offset: usize,
        len: usize,
        measure: F,
    ) where
        F: FnOnce(&AnnotationPayload) -> Option<f64>,
    {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        if let Some(content) = text_content_mut(&mut after.payload) {
            let total = content.chars().count();
            let start = offset.min(total);
            let end = (offset + len).min(total);
            let kept: String = content
                .chars()
                .enumerate()
                .filter(|(i, _)| *i < start || *i >= end)
                .map(|(_, c)| c)
                .collect();
            *content = kept;
        }
        if let Some(height) = measure(&after.payload) {
            if let AnnotationPayload::TextBox { rect, .. } = &mut after.payload {
                rect.h = height;
            }
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Replace the whole text content.
    pub fn set_annotation_text(&mut self, id: &AnnotationId, text: &str) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        collapse_runs(&mut after.payload);
        if let Some(content) = text_content_mut(&mut after.payload) {
            *content = normalize_newlines(text);
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Reply to an annotation (creates a Note with reply_to set).
    pub fn reply_to(&mut self, parent: &AnnotationId, content: &str) -> AnnotationId {
        // Find the parent's page so the reply lives on the same page.
        let page = self
            .document
            .annotations
            .find(parent)
            .map(|a| a.page.clone())
            .unwrap_or_default();
        self.create_annotation(
            AnnotationKind::Note,
            page,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 20.0,
                },
                color: Color::Rgb(255, 200, 0),
                content: content.into(),
                icon: NoteIcon::Comment,
            },
        );
        // create_annotation doesn't set reply_to; patch it with a Replace step.
        // (Slight inefficiency: create + replace = 2 transactions. Acceptable for v1.)
        let new_id = self.last_created_id();
        if let Some(new_id) = new_id {
            // Re-fetch the created annotation to snapshot before/after.
            if let Some(before) = self.document.annotations.find(&new_id).cloned() {
                let mut after = before.clone();
                after.reply_to = Some(parent.clone());
                after.modified = self.current_ts;
                self.execute_transaction(self.replace_txn(new_id.clone(), before, after));
            }
            return new_id;
        }
        // create_annotation always sets selection to Single(id), so last_created_id
        // is always Some here. This is unreachable in practice.
        unreachable!("create_annotation should have set selection to the new id")
    }

    /// The id of the most recently created annotation (create_annotation sets
    /// selection to Single(id), so this reads from selection).
    fn last_created_id(&self) -> Option<AnnotationId> {
        match &self.selection {
            crate::selection::AnnotationSelection::Single(id) => Some(id.clone()),
            _ => None,
        }
    }
}

/// Mutable text content for text-bearing payloads (TextBox/Note/Watermark). None for others.
fn text_content_mut(p: &mut AnnotationPayload) -> Option<&mut String> {
    match p {
        AnnotationPayload::Note { content, .. } => Some(content),
        AnnotationPayload::TextBox { content, .. } => Some(content),
        AnnotationPayload::Watermark { content, .. } => Some(content),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::Editor;
    use rofd_dom::{
        AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, NoteIcon, PageId, Rect,
        TextRun,
    };

    fn note_editor(content: &str) -> (Editor, AnnotationId) {
        let mut e = Editor::new();
        e.set_clock("t".into(), 1);
        let id = e.create_annotation(
            AnnotationKind::Note,
            PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: content.into(),
                icon: NoteIcon::Note,
            },
        );
        (e, id)
    }

    fn rich_textbox_editor() -> (Editor, AnnotationId) {
        let mut e = Editor::new();
        e.set_clock("t".into(), 1);
        let id = e.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 46.0,
                    h: 10.0,
                },
                content: "发的古格".into(),
                font: FontId::new("118"),
                size: 4.2333,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: vec![
                    TextRun {
                        text: "发的古".into(),
                        font: FontId::new("118"),
                        size: 4.2333,
                        color: None,
                        bold: false,
                        italic: false,
                        x: 1.0,
                        baseline: 6.0,
                    },
                    TextRun {
                        text: "格".into(),
                        font: FontId::new("157"),
                        size: 4.2333,
                        color: Some(Color::Rgb(75, 172, 198)),
                        bold: true,
                        italic: false,
                        x: 20.0,
                        baseline: 6.0,
                    },
                ],
            },
        );
        (e, id)
    }

    fn textbox_payload<'a>(e: &'a Editor, id: &AnnotationId) -> &'a AnnotationPayload {
        &e.document().annotations.find(id).unwrap().payload
    }

    fn content_of(e: &Editor, id: &AnnotationId) -> String {
        let a = e.document().annotations.find(id).unwrap();
        match &a.payload {
            AnnotationPayload::Note { content, .. } => content.clone(),
            _ => panic!("not a text annotation"),
        }
    }

    #[test]
    fn insert_text_at_offset() {
        let (mut e, id) = note_editor("Hello");
        e.insert_text(&id, 2, "XX");
        assert_eq!(content_of(&e, &id), "HeXXllo");
    }

    #[test]
    fn text_edit_collapses_rich_runs_to_flat_style() {
        // The first text edit makes the parse-time runs stale; the SAME
        // transaction collapses the box to the flat fallback style
        // (runs[0]) so caret/hit geometry and rendering stay consistent.
        let (mut e, id) = rich_textbox_editor();
        e.insert_text(&id, 4, "!");
        match textbox_payload(&e, &id) {
            AnnotationPayload::TextBox {
                content,
                font,
                size,
                runs,
                ..
            } => {
                assert_eq!(content, "发的古格!");
                assert!(runs.is_empty(), "runs collapsed on first edit");
                assert_eq!(font.0, "118", "flat style <- runs[0]");
                assert!((size - 4.2333).abs() < 1e-9);
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn text_edit_collapse_undo_restores_runs() {
        // One undo restores the rich appearance wholesale (the collapse
        // lives inside the edit's transaction, not a separate step).
        let (mut e, id) = rich_textbox_editor();
        let before = textbox_payload(&e, &id).clone();
        e.delete_text(&id, 0, 1);
        assert!(matches!(
            textbox_payload(&e, &id),
            AnnotationPayload::TextBox { runs, .. } if runs.is_empty()
        ));
        assert!(e.undo());
        assert_eq!(textbox_payload(&e, &id), &before, "undo restores runs");
    }

    #[test]
    fn flat_textbox_edit_keeps_runs_empty() {
        let mut e = Editor::new();
        e.set_clock("t".into(), 1);
        let id = e.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 30.0,
                    h: 8.0,
                },
                content: "hi".into(),
                font: FontId::new("F1"),
                size: 3.5,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: vec![],
            },
        );
        e.insert_text(&id, 1, "o");
        assert!(matches!(
            textbox_payload(&e, &id),
            AnnotationPayload::TextBox { content, runs, .. }
                if content == "hoi" && runs.is_empty()
        ));
    }

    #[test]
    fn delete_text_range() {
        let (mut e, id) = note_editor("Hello");
        e.delete_text(&id, 1, 2);
        assert_eq!(content_of(&e, &id), "Hlo");
    }

    #[test]
    fn set_text_replaces() {
        let (mut e, id) = note_editor("Hello");
        e.set_annotation_text(&id, "World");
        assert_eq!(content_of(&e, &id), "World");
    }

    #[test]
    fn text_edit_undo_restores() {
        let (mut e, id) = note_editor("Hello");
        e.insert_text(&id, 5, "!");
        assert_eq!(content_of(&e, &id), "Hello!");
        e.undo();
        assert_eq!(content_of(&e, &id), "Hello");
    }

    #[test]
    fn reply_to_creates_note_with_parent() {
        let (mut e, parent) = note_editor("parent");
        let child = e.reply_to(&parent, "reply");
        let c = e.document().annotations.find(&child).unwrap();
        assert_eq!(c.reply_to, Some(parent.clone()));
    }

    fn flat_textbox_editor() -> (Editor, AnnotationId) {
        flat_textbox_editor_with("hi", 10.0)
    }

    fn flat_textbox_editor_with(content: &str, h: f64) -> (Editor, AnnotationId) {
        let mut e = Editor::new();
        e.set_clock("t".into(), 1);
        let id = e.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 30.0,
                    h,
                },
                content: content.into(),
                font: FontId::new("F1"),
                size: 3.5,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: vec![],
            },
        );
        (e, id)
    }

    fn textbox_rect(e: &Editor, id: &AnnotationId) -> Rect {
        match textbox_payload(e, id) {
            AnnotationPayload::TextBox { rect, .. } => *rect,
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    fn textbox_content(e: &Editor, id: &AnnotationId) -> String {
        match textbox_payload(e, id) {
            AnnotationPayload::TextBox { content, .. } => content.clone(),
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn insert_text_measured_grows_height_in_same_transaction() {
        let (mut e, id) = flat_textbox_editor(); // rect h = 10.0, content "hi"
        e.insert_text_measured(&id, 0, "x", |payload| {
            assert!(matches!(payload, AnnotationPayload::TextBox { .. }));
            Some(77.0)
        });
        assert_eq!(textbox_rect(&e, &id).h, 77.0);
        assert_eq!(textbox_content(&e, &id), "xhi");
        assert!(e.undo());
        assert_eq!(textbox_rect(&e, &id).h, 10.0, "undo restores height too");
        assert_eq!(textbox_content(&e, &id), "hi");
    }

    #[test]
    fn insert_text_measured_none_keeps_height() {
        let (mut e, id) = flat_textbox_editor();
        e.insert_text_measured(&id, 0, "x", |_| None);
        assert_eq!(textbox_rect(&e, &id).h, 10.0);
    }

    #[test]
    fn delete_text_measured_applies_growth() {
        let (mut e, id) = flat_textbox_editor_with("hello", 10.0);
        e.delete_text_measured(&id, 0, 1, |_| Some(33.0));
        assert_eq!(textbox_rect(&e, &id).h, 33.0);
        assert_eq!(textbox_content(&e, &id), "ello");
    }

    #[test]
    fn insert_text_normalizes_crlf() {
        let (mut e, id) = flat_textbox_editor(); // content "hi"
        e.insert_text(&id, 0, "a\r\nb\rc");
        assert_eq!(textbox_content(&e, &id), "a\nb\nchi");
    }

    #[test]
    fn set_annotation_text_normalizes_crlf() {
        let (mut e, id) = flat_textbox_editor();
        e.set_annotation_text(&id, "a\r\nb");
        assert_eq!(textbox_content(&e, &id), "a\nb");
    }

    #[test]
    fn replace_text_range_is_one_transaction() {
        let (mut e, id) = flat_textbox_editor_with("hello world", 10.0);
        // Replace "lo wo" (chars 3..8) with "XY".
        e.replace_text_measured(&id, 3, 8, "XY", |_| Some(42.0));
        assert_eq!(textbox_content(&e, &id), "helXYrld");
        assert_eq!(
            textbox_rect(&e, &id).h,
            42.0,
            "measure applied in-transaction"
        );
        assert_eq!(e.history_len(), 2, "create + one replace transaction");
        assert!(e.undo());
        assert_eq!(textbox_content(&e, &id), "hello world");
        assert_eq!(textbox_rect(&e, &id).h, 10.0, "undo restores text + height");
        assert_eq!(e.history_len(), 1);
    }

    #[test]
    fn replace_text_range_clamps_and_handles_empty_insert() {
        let (mut e, id) = flat_textbox_editor_with("abc", 10.0);
        // Out-of-range end clamps to the content end; empty text = delete.
        e.replace_text_measured(&id, 1, 99, "", |_| None);
        assert_eq!(textbox_content(&e, &id), "a");
        // Reversed range collapses at `start` (defensive).
        e.replace_text_measured(&id, 1, 0, "Z", |_| None);
        assert_eq!(textbox_content(&e, &id), "aZ");
    }

    #[test]
    fn rich_textbox_first_edit_collapses_and_grows_atomically() {
        let (mut e, id) = rich_textbox_editor(); // rect h = 10.0, runs non-empty
        e.insert_text_measured(&id, 4, "x", |payload| {
            let AnnotationPayload::TextBox {
                runs, rect, font, ..
            } = payload
            else {
                panic!("expected TextBox");
            };
            assert!(runs.is_empty(), "measure sees the post-collapse payload");
            assert_eq!(font.0, "118", "collapsed to runs[0]");
            Some(rect.h + 5.0)
        });
        match textbox_payload(&e, &id) {
            AnnotationPayload::TextBox { rect, runs, .. } => {
                assert_eq!(rect.h, 15.0);
                assert!(runs.is_empty());
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
        assert!(e.undo());
        assert!(
            matches!(
                textbox_payload(&e, &id),
                AnnotationPayload::TextBox { runs, rect, .. } if !runs.is_empty() && rect.h == 10.0
            ),
            "one undo restores runs AND height"
        );
    }

    #[test]
    fn set_font_size_is_one_transaction_and_restores_on_undo() {
        let (mut e, id) = flat_textbox_editor_with("hello", 20.0);
        let before = textbox_payload(&e, &id).clone();
        let history = e.history_len();
        e.set_annotation_font_size_measured(&id, 20.0, |_| Some(80.0));
        assert_eq!(e.history_len(), history + 1, "one transaction");
        let AnnotationPayload::TextBox { rect, size, .. } = textbox_payload(&e, &id) else {
            panic!("expected TextBox");
        };
        assert_eq!(*size, 20.0);
        assert_eq!(rect.h, 80.0, "measure result is applied in the same txn");
        assert!(e.undo());
        assert_eq!(textbox_payload(&e, &id), &before);
    }

    #[test]
    fn set_font_size_never_shrinks_the_box() {
        let (mut e, id) = flat_textbox_editor_with("hello", 40.0);
        e.set_annotation_font_size_measured(&id, 6.0, |_| None);
        let AnnotationPayload::TextBox { rect, size, .. } = textbox_payload(&e, &id) else {
            panic!("expected TextBox");
        };
        assert_eq!(*size, 6.0);
        assert_eq!(rect.h, 40.0, "grow-only");
    }

    #[test]
    fn set_font_family_writes_the_font_id_and_collapses_runs() {
        let (mut e, id) = rich_textbox_editor();
        e.set_annotation_font_family_measured(&id, "宋体", |_| Some(24.0));
        let AnnotationPayload::TextBox {
            font, runs, rect, ..
        } = textbox_payload(&e, &id)
        else {
            panic!("expected TextBox");
        };
        assert_eq!(font.0, "宋体");
        assert!(
            runs.is_empty(),
            "rich runs collapse on the first style edit"
        );
        assert_eq!(rect.h, 24.0);
        assert!(e.undo());
        assert!(
            matches!(
                textbox_payload(&e, &id),
                AnnotationPayload::TextBox { runs, font, .. }
                    if !runs.is_empty() && font.0 == "118"
            ),
            "undo restores the rich appearance"
        );
    }

    #[test]
    fn unknown_family_still_updates_the_payload() {
        let (mut e, id) = flat_textbox_editor_with("hi", 20.0);
        e.set_annotation_font_family_measured(&id, "No Such Family", |_| None);
        let AnnotationPayload::TextBox { font, .. } = textbox_payload(&e, &id) else {
            panic!("expected TextBox");
        };
        assert_eq!(font.0, "No Such Family");
    }
}
