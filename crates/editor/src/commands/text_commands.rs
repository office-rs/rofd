use rofd_dom::{AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, Rect};

use crate::editor::Editor;
use crate::payload_util::{set_color, set_width};

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

    /// Insert text into a text annotation (TextBox/Note/Watermark) at char offset.
    pub fn insert_text(&mut self, id: &AnnotationId, offset: usize, chars: &str) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        if let Some(content) = text_content_mut(&mut after.payload) {
            let off = offset.min(content.chars().count());
            let mut new = content.chars().take(off).collect::<String>();
            new.push_str(chars);
            new.extend(content.chars().skip(off));
            *content = new;
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Delete `len` chars from a text annotation at char offset.
    pub fn delete_text(&mut self, id: &AnnotationId, offset: usize, len: usize) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
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
        if let Some(content) = text_content_mut(&mut after.payload) {
            *content = text.into();
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
        AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect,
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
}
