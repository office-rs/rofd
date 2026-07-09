use rofd_dom::OfdDocument;

use crate::cursor::TextCursor;
use crate::selection::AnnotationSelection;
use crate::steps::history::History;
use crate::steps::transaction::Transaction;

/// Annotation editor. Owns the document; mutates only `.annotations` via commands.
/// No callbacks - the host/component layer queries state after commands.
pub struct Editor {
    pub(crate) document: OfdDocument,
    pub(crate) selection: AnnotationSelection,
    pub(crate) text_cursor: Option<TextCursor>,
    pub(crate) history: History,
    pub(crate) author: String,
    pub(crate) current_ts: i64,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            document: OfdDocument::default(),
            selection: AnnotationSelection::None,
            text_cursor: None,
            history: History::new(100),
            author: String::new(),
            current_ts: 0,
        }
    }

    pub fn load_document(&mut self, doc: OfdDocument) {
        self.document = doc;
        self.selection = AnnotationSelection::None;
        self.text_cursor = None;
        self.history = History::new(100);
    }

    /// Caller-supplied author + timestamp. The library never reads a system clock.
    pub fn set_clock(&mut self, author: String, ts: i64) {
        self.author = author;
        self.current_ts = ts;
    }

    pub fn document(&self) -> &OfdDocument { &self.document }
    pub fn selection(&self) -> &AnnotationSelection { &self.selection }
    pub fn text_cursor(&self) -> Option<&TextCursor> { self.text_cursor.as_ref() }
    pub fn can_undo(&self) -> bool { self.history.can_undo() }
    pub fn can_redo(&self) -> bool { self.history.can_redo() }

    /// Apply a Transaction: run its steps forward, set selection/cursor to the
    /// "after" state, and push it onto the history. Used by the command methods
    /// (create/delete/move/...) starting in Task 5; the unit tests here exercise
    /// it directly.
    pub(crate) fn execute_transaction(&mut self, txn: Transaction) {
        for step in &txn.steps {
            step.apply(&mut self.document.annotations);
        }
        self.selection = txn.selection_after.clone();
        self.text_cursor = txn.text_cursor_after.clone();
        self.history.push(txn);
    }

    pub fn undo(&mut self) -> bool {
        let txn = self.history.undo();
        if let Some(txn) = txn {
            for step in txn.steps.iter().rev() {
                step.revert(&mut self.document.annotations);
            }
            self.selection = txn.selection_before.clone();
            self.text_cursor = txn.text_cursor_before.clone();
            true
        } else { false }
    }

    pub fn redo(&mut self) -> bool {
        let txn = self.history.redo();
        if let Some(txn) = txn {
            for step in &txn.steps {
                step.apply(&mut self.document.annotations);
            }
            self.selection = txn.selection_after.clone();
            self.text_cursor = txn.text_cursor_after.clone();
            true
        } else { false }
    }
}

impl Default for Editor {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::annotation_steps::InsertAnnotationStep;
    use rofd_dom::{Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn note_ann(id: &str) -> Annotation {
        Annotation {
            id: AnnotationId(uuid::Uuid::parse_str(id).unwrap()),
            kind: AnnotationKind::Note, page: PageId::new("P0"),
            creator: "t".into(), created: 0, modified: 0, reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, color: Color::Rgb(0,0,0),
                content: "x".into(), icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn execute_undo_redo_via_transaction() {
        let mut e = Editor::new();
        let ann = note_ann("00000000-0000-0000-0000-000000000021");
        let txn = Transaction {
            steps: vec![Box::new(InsertAnnotationStep { annotation: ann.clone() })],
            selection_before: AnnotationSelection::None,
            selection_after: AnnotationSelection::Single(ann.id.clone()),
            text_cursor_before: None, text_cursor_after: None,
        };
        e.execute_transaction(txn);
        assert!(e.document().annotations.find(&ann.id).is_some());
        assert!(e.undo());
        assert!(e.document().annotations.find(&ann.id).is_none());
        assert!(e.redo());
        assert!(e.document().annotations.find(&ann.id).is_some());
    }

    #[test]
    fn history_capacity_evicts_oldest() {
        let mut e = Editor::new();
        for i in 0..105u32 {
            let ann = note_ann(&format!("00000000-0000-0000-0000-0000{:08x}", i));
            let txn = Transaction {
                steps: vec![Box::new(InsertAnnotationStep { annotation: ann })],
                selection_before: AnnotationSelection::None,
                selection_after: AnnotationSelection::None,
                text_cursor_before: None, text_cursor_after: None,
            };
            e.execute_transaction(txn);
        }
        assert!(e.can_undo());
        for _ in 0..100 { assert!(e.undo()); }
        assert!(!e.can_undo());
    }
}
