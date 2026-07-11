use rofd_component::{EditorComponent, EditorConfig, Key, Modifiers, RenderTarget, ViewEvent};
use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
use std::sync::{Arc, Mutex};
use rofd_render::Scene;

struct MockRenderTarget {
    drawn: usize,
}
impl RenderTarget for MockRenderTarget {
    fn draw_scene(&mut self, _: &Scene) { self.drawn += 1; }
    fn size(&self) -> (f64, f64) { (800.0, 600.0) }
}

#[test]
fn end_to_end_create_select_edit_undo_render() {
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    c.set_clock("tester".into(), 1_700_000_000_000);
    // Create a note annotation via the component pass-through (Task 8 Step 3 adds it).
    let id = c.create_annotation(
        AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0 },
            color: Color::Rgb(0, 0, 0), content: "Hello".into(), icon: NoteIcon::Note,
        },
    );
    // create_annotation pass-through handles cache invalidation + on_change.

    // Undo the create via handle_event.
    let outcome = c.handle_event(&ViewEvent::KeyDown {
        key: Key::Char('z'),
        modifiers: Modifiers { control: true, ..Default::default() },
    });
    assert!(outcome.needs_repaint);
    assert!(c.document().annotations.find(&id).is_none(), "undo removed the annotation");

    // Redo.
    c.handle_event(&ViewEvent::KeyDown {
        key: Key::Char('y'),
        modifiers: Modifiers { control: true, ..Default::default() },
    });
    assert!(c.document().annotations.find(&id).is_some(), "redo restored it");

    // Render (no panic).
    let mut rt = MockRenderTarget { drawn: 0 };
    c.render(&mut rt);
    assert_eq!(rt.drawn, 1);
}

#[test]
fn on_change_fires_on_undo() {
    let fired = Arc::new(Mutex::new(false));
    let f = fired.clone();
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    c.set_clock("t".into(), 1);
    c.on_change(move |_| { *f.lock().unwrap() = true; });
    c.create_annotation(
        AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            color: Color::Rgb(0, 0, 0), content: "".into(), icon: NoteIcon::Note,
        },
    );
    *fired.lock().unwrap() = false;
    c.handle_event(&ViewEvent::KeyDown {
        key: Key::Char('z'),
        modifiers: Modifiers { control: true, ..Default::default() },
    });
    assert!(*fired.lock().unwrap(), "on_change fired on undo");
}
