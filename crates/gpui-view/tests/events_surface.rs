// Ported from rword@4e0b25b6 (crates/gpui-view/tests/events_surface.rs) —
// the enum is re-cut one-to-one onto rofd's 14 kernel callbacks and the
// drive cases copy kernel-behavior expectations from
// crates/component (integration tests + ofd_component unit tests).

//! The event surface — kernel callbacks -> `OfdGpuiEvent` queue ->
//! (paint-drain) cx.emit. The routing function is standalone so it is
//! headless-testable without a window. Events must not be lost or deduped
//! between drains (gpui-line review focus #1).

use std::sync::Arc;

use rofd_component::{
    ContextTarget, Key, Modifiers, MouseButton, OfdComponent, OfdConfig, OfdWarning, ViewEvent,
    ZoomMode,
};
use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, OfdDocument, Page, PageId, Rect, ShapeKind,
};
use rofd_gpui_view::events_surface::{install, EventQueue, OfdGpuiEvent};

fn doc_with_page(w: f64, h: f64) -> OfdDocument {
    // Same shape as the kernel's zoom_fit_doc helper (unit-test only — an
    // empty layer list is enough for hit-test/fit math).
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        raw_children: Vec::new(),
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        },
        layers: vec![],
        template: None,
    });
    doc
}

fn component_with_queue() -> (OfdComponent, EventQueue) {
    let mut component = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    let queue = EventQueue::new();
    install(&mut component, &queue);
    (component, queue)
}

#[test]
fn queue_preserves_events_between_drains() {
    let queue = EventQueue::new();
    queue.push(OfdGpuiEvent::ZoomChanged(1.0));
    queue.push(OfdGpuiEvent::ZoomChanged(1.1));
    queue.push(OfdGpuiEvent::Changed);
    let first = queue.drain();
    assert_eq!(first.len(), 3, "order preserved");
    assert!(matches!(first[0], OfdGpuiEvent::ZoomChanged(z) if z == 1.0));
    // No dedup: the same event twice stays twice.
    assert!(matches!(first[1], OfdGpuiEvent::ZoomChanged(z) if z == 1.1));
    assert!(queue.drain().is_empty(), "second drain is empty");
    // Refill after drain works.
    queue.push(OfdGpuiEvent::CopyRequested);
    assert!(matches!(
        queue.drain().as_slice(),
        [OfdGpuiEvent::CopyRequested]
    ));
}

#[test]
fn fit_width_fires_zoom_then_mode() {
    let (mut component, queue) = component_with_queue();
    component.load_document(doc_with_page(210.0, 297.0));
    component.set_viewport_size(780.0, 500.0);
    // Viewport setup announces the first visible page (kernel
    // maybe_fire_page_change on Resize); drain it so the assertion below
    // sees only the set_zoom_mode traffic.
    assert!(matches!(
        queue.drain().as_slice(),
        [OfdGpuiEvent::PageChanged(0)]
    ));
    component.set_zoom_mode(ZoomMode::FitWidth);
    // Kernel fire order (set_zoom_mode -> apply_fit fires on_zoom_change,
    // then the mode change fires at the tail): zoom BEFORE mode. The zoom
    // value matches the kernel's own fit test: z = (780 - 12) / 210.
    let events = queue.drain();
    assert!(
        matches!(
            events.as_slice(),
            [
                OfdGpuiEvent::ZoomChanged(z),
                OfdGpuiEvent::ZoomModeChanged(ZoomMode::FitWidth)
            ] if (z - 768.0 / 210.0).abs() < 1e-9
        ),
        "got {events:?}"
    );
}

#[test]
fn ctrl_s_routes_save_request() {
    // The kernel owns ctrl+s (ofd_component handle_key: KeyDown Char('s')
    // + control -> fire_save_request); no adapter interception anywhere.
    let (mut component, queue) = component_with_queue();
    component.handle_event(&ViewEvent::KeyDown {
        key: Key::Char('s'),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
    });
    assert_eq!(queue.drain(), vec![OfdGpuiEvent::SaveRequested]);
}

#[test]
fn right_click_on_desk_routes_context_menu() {
    // Right-click fires ONLY on_context_menu (no selection change, no
    // cursor change — kernel PointerDown Right arm); with no page under
    // the cursor the target is the desk background.
    let (mut component, queue) = component_with_queue();
    component.handle_event(&ViewEvent::PointerDown {
        button: MouseButton::Right,
        x: 10.0,
        y: 20.0,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    assert_eq!(
        queue.drain(),
        vec![OfdGpuiEvent::ContextMenuRequested(
            (10.0, 20.0),
            ContextTarget::Empty
        )]
    );
}

#[test]
fn annotation_mutation_routes_change_then_selection() {
    let (mut component, queue) = component_with_queue();
    component.load_document(doc_with_page(200.0, 200.0));
    let id = component.create_annotation(
        AnnotationKind::Shape(ShapeKind::Rect),
        PageId::new("P0"),
        AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 30.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![],
        },
    );
    // Kernel fire order (create_annotation): after_annotation_change ->
    // on_change first, then fire_selection_change with Single(new id).
    let events = queue.drain();
    assert!(
        matches!(
            events.as_slice(),
            [OfdGpuiEvent::Changed, OfdGpuiEvent::SelectionChanged(sel)]
                if matches!(sel, rofd_dom::AnnotationSelection::Single(s) if *s == id)
        ),
        "got {events:?}"
    );
}

#[test]
fn warnings_route_to_queue() {
    // load_document itself fires nothing; warnings ride the host-side
    // entry point fire_warnings (the adapter calls it after parse_ofd
    // returns a LoadReport) — kernel test on_warning_fires_with_load_
    // warnings is the behavior source here.
    let (component, queue) = component_with_queue();
    let warning = OfdWarning::MissingFeature {
        feature: "JBIG2".into(),
        entry: "Res.xml".into(),
    };
    component.fire_warnings(std::slice::from_ref(&warning));
    assert_eq!(queue.drain(), vec![OfdGpuiEvent::Warned(vec![warning])]);
}
