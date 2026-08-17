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
        let n = self.document.max_unit_id + 1;
        self.document.max_unit_id = n;
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
}
