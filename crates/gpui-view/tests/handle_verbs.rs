//! Host verb surface (Task 9): `GpuiOfdHandle` forwards each verb to the
//! kernel component inside `entity.update` and notifies. The wrapper is a
//! uniform `update + notify` shell, so these tests cover representative
//! verbs per cluster — fit-mode roundtrip + anchored zoom, undo/redo
//! history flags, selection queries, the paste guard, document swaps, and
//! delete-annotation existence — not every method.

use std::sync::Arc;

use gpui::{AppContext, TestAppContext};
use rofd_component::{OfdConfig, ZoomMode};
use rofd_dom::{
    AnnotationId, AnnotationKind, AnnotationPayload, Color, Layer, NoteIcon, OfdDocument, Page,
    PageId, Rect,
};
use rofd_gpui_view::{GpuiOfdHandle, OfdGpuiView};

/// One empty 200x200 mm page, P0.
fn page_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        raw_children: Vec::new(),
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    doc
}

/// Entity + handle built headless (no window — the verbs under test never
/// paint). `configure` seeds kernel state before the handle takes over.
fn handle_with<R>(
    cx: &mut TestAppContext,
    configure: impl FnOnce(&mut OfdGpuiView) -> R,
) -> GpuiOfdHandle {
    cx.update(|cx| {
        let entity = cx.new(|cx| {
            let mut view = OfdGpuiView::new(OfdConfig::new(Arc::new(vec![])).with_zoom(1.0), cx);
            configure(&mut view);
            view
        });
        GpuiOfdHandle::from(entity)
    })
}

/// Seed one Note annotation on P0 — the cheapest deterministic history
/// entry (one Transaction on the undo stack).
fn add_note(view: &mut OfdGpuiView) -> AnnotationId {
    view.component.set_clock("t".into(), 1);
    view.component.load_document(page_doc());
    view.component.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            },
            color: Color::Rgb(255, 216, 0),
            content: "n".into(),
            icon: NoteIcon::Note,
        },
    )
}

fn page_count(cx: &mut TestAppContext, handle: &GpuiOfdHandle) -> usize {
    handle
        .entity()
        .update(cx, |view, _| view.component.document().pages.len())
}

#[gpui::test]
fn zoom_mode_roundtrip_and_manual_zoom_exits_fit(cx: &mut TestAppContext) {
    let handle = handle_with(cx, |view| {
        view.component.load_document(page_doc());
        view.component.set_viewport_size(400.0, 300.0);
    });
    cx.update(|cx| handle.set_zoom_mode(ZoomMode::FitWidth, cx));
    assert_eq!(cx.update(|cx| handle.zoom_mode(cx)), ZoomMode::FitWidth);
    // Anchored zoom: the anchor center comes from the component viewport.
    let z0 = cx.update(|cx| handle.zoom(cx));
    cx.update(|cx| handle.zoom_at(1.1, cx));
    assert_eq!(cx.update(|cx| handle.zoom_mode(cx)), ZoomMode::Custom);
    let z1 = cx.update(|cx| handle.zoom(cx));
    assert!(
        (z1 - z0 * 1.1).abs() < 1e-9,
        "zoom scales by the factor: {z0} -> {z1}"
    );
}

#[gpui::test]
fn undo_redo_flip_history_flags(cx: &mut TestAppContext) {
    let handle = handle_with(cx, add_note);
    assert!(cx.update(|cx| handle.can_undo(cx)));
    assert!(!cx.update(|cx| handle.can_redo(cx)));
    cx.update(|cx| handle.undo(cx));
    assert!(!cx.update(|cx| handle.can_undo(cx)));
    assert!(cx.update(|cx| handle.can_redo(cx)));
    cx.update(|cx| handle.redo(cx));
    assert!(cx.update(|cx| handle.can_undo(cx)));
}

#[gpui::test]
fn selection_queries_on_empty_selection(cx: &mut TestAppContext) {
    let handle = handle_with(cx, |view| {
        view.component.load_document(page_doc());
    });
    assert_eq!(cx.update(|cx| handle.selected_text(cx)), None);
    assert!(!cx.update(|cx| handle.has_text_selection(cx)));
}

#[gpui::test]
fn paste_text_without_cursor_is_false(cx: &mut TestAppContext) {
    let handle = handle_with(cx, |view| {
        view.component.load_document(page_doc());
    });
    // No text cursor is set, so the paste is refused.
    assert!(!cx.update(|cx| handle.paste_text("hello", cx)));
    // Empty text is always refused.
    assert!(!cx.update(|cx| handle.paste_text("", cx)));
}

#[gpui::test]
fn load_and_new_document_swap_state(cx: &mut TestAppContext) {
    let handle = handle_with(cx, add_note);
    assert!(cx.update(|cx| handle.can_undo(cx)));

    let mut two = page_doc();
    two.pages.push(Page {
        raw_children: Vec::new(),
        id: PageId::new("P1"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    cx.update(|cx| handle.load_document(two, cx));
    assert_eq!(page_count(cx, &handle), 2);
    assert!(
        !cx.update(|cx| handle.can_undo(cx)),
        "a document swap clears the history"
    );

    cx.update(|cx| handle.new_document(cx));
    assert_eq!(page_count(cx, &handle), 0);
}

#[gpui::test]
fn delete_annotation_reports_existence(cx: &mut TestAppContext) {
    let mut created = None;
    let handle = handle_with(cx, |view| {
        created = Some(add_note(view));
    });
    let id = created.expect("fixture annotation id");
    assert!(cx.update(|cx| handle.delete_annotation(id.clone(), cx)));
    assert!(
        !cx.update(|cx| handle.delete_annotation(id, cx)),
        "the annotation is gone after the first delete"
    );
    assert!(!cx.update(|cx| handle.delete_annotation(AnnotationId::new("nope"), cx)));
}
