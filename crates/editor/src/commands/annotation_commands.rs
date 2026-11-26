use rofd_dom::{Annotation, AnnotationId, AnnotationKind, AnnotationPayload, PageId, Rect};

use crate::editor::Editor;
use crate::payload_util::{move_payload, resize_payload};
use crate::selection::AnnotationSelection;
use crate::steps::annotation_steps::{
    DeleteAnnotationStep, InsertAnnotationStep, ReplaceAnnotationStep,
};
use crate::steps::transaction::Transaction;

impl Editor {
    /// Create an annotation. Returns the new id. Stamps created/modified from current_ts + author.
    pub fn create_annotation(
        &mut self,
        kind: AnnotationKind,
        page: PageId,
        payload: AnnotationPayload,
    ) -> AnnotationId {
        // A signed document can no longer change Document.xml, so its new
        // ids stay under the reserve written at signing time instead of
        // pushing MaxUnitID up (spec 2026-11-23 M14).
        let signed = self.document.signatures.declared;
        let n = if signed {
            self.document.highest_known_object_id() + 1
        } else {
            self.document.max_unit_id + 1
        };
        if !signed {
            self.document.max_unit_id = n;
        }
        let id = AnnotationId::from_int(n);
        let ann = Annotation {
            id: id.clone(),
            kind,
            page,
            creator: self.author.clone(),
            created: self.current_ts,
            modified: self.current_ts,
            reply_to: None,
            payload,
        };
        let txn = Transaction {
            steps: vec![Box::new(InsertAnnotationStep { annotation: ann })],
            selection_before: self.selection.clone(),
            selection_after: AnnotationSelection::Single(id.clone()),
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: None,
        };
        self.execute_transaction(txn);
        id
    }

    /// Delete an annotation by id.
    pub fn delete_annotation(&mut self, id: &AnnotationId) {
        let ann = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let sel_after = if self.selection.contains(id) {
            AnnotationSelection::None
        } else {
            self.selection.clone()
        };
        let cur_after = if self
            .text_cursor
            .as_ref()
            .is_some_and(|c| &c.annotation == id)
        {
            None
        } else {
            self.text_cursor.clone()
        };
        let txn = Transaction {
            steps: vec![Box::new(DeleteAnnotationStep { annotation: ann })],
            selection_before: self.selection.clone(),
            selection_after: sel_after,
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: cur_after,
        };
        self.execute_transaction(txn);
    }

    /// Delete all selected annotations.
    pub fn delete_selected(&mut self) {
        let ids: Vec<AnnotationId> = match &self.selection {
            AnnotationSelection::None => vec![],
            AnnotationSelection::Single(id) => vec![id.clone()],
            AnnotationSelection::Multi(ids) => ids.clone(),
        };
        for id in &ids {
            self.delete_annotation(id);
        }
    }

    /// Move an annotation by (dx, dy).
    pub fn move_annotation(&mut self, id: &AnnotationId, dx: f64, dy: f64) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        move_payload(&mut after.payload, dx, dy);
        after.modified = self.current_ts;
        let txn = self.replace_txn(id.clone(), before, after);
        self.execute_transaction(txn);
    }

    /// Resize an annotation to new_rect (rect-based payloads; no-op for Markup/Freehand).
    pub fn resize_annotation(&mut self, id: &AnnotationId, new_rect: Rect) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        resize_payload(&mut after.payload, new_rect);
        after.modified = self.current_ts;
        let txn = self.replace_txn(id.clone(), before, after);
        self.execute_transaction(txn);
    }

    /// Move a single vertex of a point-based Shape annotation (Line/Arrow/
    /// Polygon/PolyLine). `index` is the vertex position in `payload.points`;
    /// `new_point` is page-local. Rect/Ellipse and non-Shape payloads are a
    /// no-op (no history entry). Spec §4.2: vertex drag.
    pub fn move_annotation_vertex(
        &mut self,
        id: &AnnotationId,
        index: usize,
        new_point: (f64, f64),
    ) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a,
            None => return,
        };
        let mut after = before.clone();
        if !crate::payload_util::move_vertex_payload(&mut after.payload, index, new_point) {
            return;
        }
        after.modified = self.current_ts;
        let txn = self.replace_txn(id.clone(), before, after);
        self.execute_transaction(txn);
    }

    /// Helper: build a ReplaceAnnotationStep Transaction preserving selection/cursor.
    pub(crate) fn replace_txn(
        &self,
        id: AnnotationId,
        before: Annotation,
        after: Annotation,
    ) -> Transaction {
        Transaction {
            steps: vec![Box::new(ReplaceAnnotationStep { id, before, after })],
            selection_before: self.selection.clone(),
            selection_after: self.selection.clone(),
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: self.text_cursor.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn editor_with_max_id(n: u64) -> Editor {
        let mut e = Editor::new();
        e.document.max_unit_id = n;
        e.set_clock("tester".into(), 1_700_000_000_000);
        e
    }

    #[test]
    fn create_annotation_allocates_id_from_max_unit_id_plus_one() {
        let mut e = editor_with_max_id(1500);
        let id = e.create_annotation(
            AnnotationKind::Note,
            PageId::new("1"),
            AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "hi".into(),
                icon: NoteIcon::Note,
            },
        );
        assert_eq!(id.0, "1501", "new id = max_unit_id + 1");
        assert_eq!(e.document().max_unit_id, 1501, "max_unit_id 自增");
    }

    #[test]
    fn create_annotation_ids_monotonic_unique() {
        let mut e = editor_with_max_id(100);
        let a = e.create_annotation(
            AnnotationKind::Note,
            PageId::new("1"),
            AnnotationPayload::Note {
                rect: Rect::default(),
                color: Color::Rgb(0, 0, 0),
                content: "a".into(),
                icon: NoteIcon::Note,
            },
        );
        let b = e.create_annotation(
            AnnotationKind::Note,
            PageId::new("1"),
            AnnotationPayload::Note {
                rect: Rect::default(),
                color: Color::Rgb(0, 0, 0),
                content: "b".into(),
                icon: NoteIcon::Note,
            },
        );
        assert_ne!(a, b, "ids 唯一");
        assert_eq!(a.0, "101");
        assert_eq!(b.0, "102");
    }

    #[test]
    fn create_annotation_on_signed_document_allocates_above_known_ids_and_keeps_max_unit_id() {
        // 65_545 is the reserve written into Document.xml at signing time;
        // Document.xml can no longer change, so new ids must fit under it.
        let mut e = editor_with_max_id(65_545);
        e.document.signatures.declared = true;
        e.document.annotations.insert(rofd_dom::Annotation {
            id: rofd_dom::AnnotationId::from_int(9),
            kind: AnnotationKind::Note,
            page: PageId::new("1"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect::default(),
                color: Color::Rgb(0, 0, 0),
                content: "earlier".into(),
                icon: NoteIcon::Note,
            },
        });
        let note = |content: &str| AnnotationPayload::Note {
            rect: Rect::default(),
            color: Color::Rgb(0, 0, 0),
            content: content.to_owned(),
            icon: NoteIcon::Note,
        };
        let first = e.create_annotation(AnnotationKind::Note, PageId::new("1"), note("first"));
        assert_eq!(
            first.0, "10",
            "a signed document allocates above the highest known id"
        );
        assert_eq!(
            e.document().max_unit_id,
            65_545,
            "the reserve ceiling is left alone"
        );
        let second = e.create_annotation(AnnotationKind::Note, PageId::new("1"), note("second"));
        assert_eq!(second.0, "11", "ids stay unique and consecutive");
        assert_eq!(e.document().max_unit_id, 65_545);
    }
}
