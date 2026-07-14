use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
use rofd_editor::Editor;

#[test]
fn create_undo_redo_restores_state() {
    let mut e = Editor::new();
    e.set_clock("tester".into(), 1_700_000_000_000);
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
            content: "hi".into(),
            icon: NoteIcon::Note,
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

#[test]
fn move_then_undo_restores_position() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1_700_000_000_000);
    let id = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 5.0,
                h: 5.0,
            },
            color: Color::Rgb(0, 0, 0),
            content: "".into(),
            icon: NoteIcon::Note,
        },
    );
    e.move_annotation(&id, 3.0, 4.0);
    {
        let a = e.document().annotations.find(&id).unwrap();
        assert!(matches!(
            &a.payload,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 13.0,
                    y: 14.0,
                    ..
                },
                ..
            }
        ));
    }
    e.undo();
    {
        let a = e.document().annotations.find(&id).unwrap();
        assert!(matches!(
            &a.payload,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    ..
                },
                ..
            }
        ));
    }
}

#[test]
fn delete_selected_removes_all_selected() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1);
    let a = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
            color: Color::Rgb(0, 0, 0),
            content: "".into(),
            icon: NoteIcon::Note,
        },
    );
    let b = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
            color: Color::Rgb(0, 0, 0),
            content: "".into(),
            icon: NoteIcon::Note,
        },
    );
    e.set_selection(rofd_editor::AnnotationSelection::Multi(vec![
        a.clone(),
        b.clone(),
    ]));
    e.delete_selected();
    assert!(e.document().annotations.find(&a).is_none());
    assert!(e.document().annotations.find(&b).is_none());
}

#[test]
fn selection_restored_on_undo() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1);
    let id = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
            color: Color::Rgb(0, 0, 0),
            content: "".into(),
            icon: NoteIcon::Note,
        },
    );
    // create sets selection to Single(id); undo restores selection_before (None).
    assert_eq!(
        e.selection(),
        &rofd_editor::AnnotationSelection::Single(id.clone())
    );
    e.undo();
    assert_eq!(e.selection(), &rofd_editor::AnnotationSelection::None);
}
