use rofd_editor::Editor;
use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

#[test]
fn create_undo_redo_restores_state() {
    let mut e = Editor::new();
    e.set_clock("tester".into(), 1_700_000_000_000);
    let id = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            color: Color::Rgb(0, 0, 0), content: "hi".into(), icon: NoteIcon::Note,
        },
    );
    assert!(e.document().annotations.find(&id).is_some());
    assert!(e.can_undo());
    assert!(e.undo());
    assert!(e.document().annotations.find(&id).is_none());
    assert!(e.can_redo());
    assert!(e.redo());
    assert!(e.document().annotations.find(&id).is_some());
}
