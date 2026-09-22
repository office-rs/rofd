//! Headless integration tests driving `OfdWidget` through
//! `masonry_testing`'s `TestHarness` (spec §6.2, seven contracts):
//! click-to-edit, double-click word select, IME preedit/commit, plain
//! wheel, ctrl+wheel ZoomAt accumulation/clamp, drag-select + Ctrl+C,
//! and window-focus caret gating.

use std::sync::{Arc, Mutex};

use masonry_testing::TestHarness;
use rofd_component::OfdConfig;
use rofd_xilem_view::{OfdCommand, OfdWidget, OfdWidgetAction};
use xilem::masonry::core::keyboard::{Key as MasonryKey, KeyState};
use xilem::masonry::core::pointer::PointerButtons;
use xilem::masonry::core::{
    Handled, Ime, KeyboardEvent, Modifiers, PointerButton, PointerButtonEvent, PointerEvent,
    PointerId, PointerInfo, PointerScrollEvent, PointerState, PointerType, ScrollDelta, TextEvent,
    Widget,
};
use xilem::masonry::dpi::{PhysicalPosition, PhysicalSize};
use xilem::masonry::theme::default_property_set;

use rofd_dom::{
    Color, FontId, Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject, Rect,
    TextCode, TextObject,
};

/// Component birth zoom: 96 px/inch on 25.4 mm/inch.
const BASE: f64 = 96.0 / 25.4;

/// Viewport-space origin of the fixture's first page. The render engine
/// centers pages horizontally in the 800px viewport (page is 200mm wide)
/// and offsets the first page's Y by the default `page_gap` (20), so
/// gestures at page-local mm (`px`, `py`) are delivered at
/// `ORIGIN_* + p * BASE`.
const ORIGIN_X: f64 = (800.0 - 200.0 * BASE) / 2.0;
const ORIGIN_Y: f64 = 20.0;

/// A `PointerInfo` for the primary mouse (mirrors the harness's const).
const MOUSE: PointerInfo = PointerInfo {
    pointer_id: Some(PointerId::PRIMARY),
    persistent_device_id: None,
    pointer_type: PointerType::Mouse,
};

fn create_harness() -> TestHarness<OfdWidget> {
    TestHarness::create_with_size(
        default_property_set(),
        OfdWidget::new(OfdConfig::new(Arc::new(vec![]))).prepare(),
        PhysicalSize::new(800, 600),
    )
}

/// Command loading the shared fixture: one page with two body text lines
/// + an empty TextBox below them. Uses only the component's public API.
fn setup_command() -> OfdCommand {
    Arc::new(|c| {
        c.set_clock("t".into(), 1);
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![Layer {
                layer_type: LayerType::Body,
                objects: vec![PageObject::Text(TextObject {
                    id: ObjectId::new("t1"),
                    boundary: Rect {
                        x: 10.0,
                        y: 20.0,
                        w: 100.0,
                        h: 40.0,
                    },
                    ctm: None,
                    font: FontId::new("F1"),
                    size: 10.0,
                    fill: None,
                    codes: vec![
                        TextCode {
                            glyph_ids: vec![1, 2, 3, 4],
                            deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)],
                            text: "ABCD".into(),
                            x: 0.0,
                            y: 10.0,
                        },
                        TextCode {
                            glyph_ids: vec![5, 6],
                            deltas: vec![(10.0, 0.0)],
                            text: "EF".into(),
                            x: 0.0,
                            y: 30.0,
                        },
                    ],
                    draw_param: None,
                })],
            }],
            template: None,
        });
        c.load_document(doc);
        c.create_annotation(
            rofd_dom::AnnotationKind::TextBox,
            PageId::new("P0"),
            rofd_dom::AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 100.0,
                    w: 120.0,
                    h: 40.0,
                },
                content: String::new(),
                font: FontId::new("F1"),
                size: 10.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
            },
        );
    })
}

fn run_setup(harness: &mut TestHarness<OfdWidget>) {
    let command = setup_command();
    harness.edit_root_widget(|mut w| OfdWidget::with_component(&mut w, &command));
}

/// Probe the live component via the host command channel.
fn probe<R: Send + 'static>(
    harness: &mut TestHarness<OfdWidget>,
    f: impl Fn(&mut rofd_component::OfdComponent) -> R + Send + Sync + 'static,
) -> R {
    let out = Arc::new(Mutex::new(None));
    let sink = out.clone();
    let command: OfdCommand = Arc::new(move |c| *sink.lock().unwrap() = Some(f(c)));
    harness.edit_root_widget(|mut w| OfdWidget::with_component(&mut w, &command));
    let result = out.lock().unwrap().take().expect("probe ran");
    result
}

/// Content of the fixture TextBox.
fn textbox_text(harness: &mut TestHarness<OfdWidget>) -> String {
    probe(harness, |c| {
        c.document()
            .annotations
            .for_page(&PageId::new("P0"))
            .iter()
            .find_map(|a| match &a.payload {
                rofd_dom::AnnotationPayload::TextBox { content, .. } => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default()
    })
}

/// Press + release the primary button at (`x`, `y`) with the given click
/// count (harness convenience helpers don't track counts).
fn click(harness: &mut TestHarness<OfdWidget>, x: f64, y: f64, count: u8) {
    let state = || PointerState {
        position: PhysicalPosition::new(x, y),
        count,
        ..PointerState::default()
    };
    harness.process_pointer_event(PointerEvent::Down(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: state(),
    }));
    harness.process_pointer_event(PointerEvent::Up(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: state(),
    }));
}

/// Raw pixel-delta wheel with explicit modifiers.
fn wheel(harness: &mut TestHarness<OfdWidget>, dy: f64, modifiers: Modifiers) -> Handled {
    harness.process_pointer_event(PointerEvent::Scroll(PointerScrollEvent {
        pointer: MOUSE,
        delta: ScrollDelta::PixelDelta(PhysicalPosition::new(0.0, dy)),
        state: PointerState {
            position: PhysicalPosition::new(400.0, 300.0),
            modifiers,
            ..PointerState::default()
        },
    }))
}

/// Drain every submitted action of type T from the harness queue.
fn drain_actions<T: std::fmt::Debug + 'static>(harness: &mut TestHarness<OfdWidget>) -> Vec<T> {
    let mut out = Vec::new();
    while let Some((action, _id)) = harness.pop_action::<T>() {
        out.push(action);
    }
    out
}

fn zoom_changes(actions: &[OfdWidgetAction]) -> Vec<f64> {
    actions
        .iter()
        .filter_map(|a| match a {
            OfdWidgetAction::ZoomChanged(z) => Some(*z),
            _ => None,
        })
        .collect()
}

// 1. Click TextBox → type "hello": content + Changed action.
#[test]
fn click_textbox_type_hello() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // TextBox page rect (0,100,120,40) scaled by BASE.
    click(
        &mut harness,
        ORIGIN_X + 5.0 * BASE,
        ORIGIN_Y + 105.0 * BASE,
        1,
    );
    harness.keyboard_type_chars("hello");

    assert_eq!(textbox_text(&mut harness), "hello");
    let actions = drain_actions::<OfdWidgetAction>(&mut harness);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, OfdWidgetAction::Changed)),
        "Changed action expected, got {actions:?}"
    );
}

// 2. Double click on "ABCD" selects the whole word/code.
#[test]
fn double_click_selects_word() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    click(
        &mut harness,
        ORIGIN_X + 15.0 * BASE,
        ORIGIN_Y + 25.0 * BASE,
        2,
    );

    let selection = probe(&mut harness, |c| c.text_selection().cloned());
    let selection = selection.expect("body text selection on double click");
    assert_eq!(selection.page, PageId::new("P0"));
    assert_eq!(selection.ranges.len(), 1);
    let range = &selection.ranges[0];
    assert_eq!(range.object, ObjectId::new("t1"));
    assert_eq!(range.code_index, 0);
    assert_eq!((range.start, range.end), (0, 4));
}

// 3. IME preedit stays out of the document; IME area h>0; commit enters.
#[test]
fn ime_preedit_area_and_commit() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    assert!(harness.has_ime_session(), "IME session after focus");

    // Anchor a caret first (the component has no caret until clicked).
    click(
        &mut harness,
        ORIGIN_X + 5.0 * BASE,
        ORIGIN_Y + 105.0 * BASE,
        1,
    );

    harness.process_text_event(TextEvent::Ime(Ime::Enabled));
    harness.process_text_event(TextEvent::Ime(Ime::Preedit("ni".into(), None)));

    assert_eq!(textbox_text(&mut harness), "");
    let (_pos, size) = harness.ime_rect();
    assert!(
        size.height > 0.0,
        "IME area must track the caret, got {size:?}"
    );

    // The winit Windows backend always sends an empty preedit (composition
    // clear) right before Commit (WM_IME_COMPOSITION GCS_RESULTSTR path);
    // the component then takes the no-preedit insert path. Without it,
    // spec §2.10's active-preedit rule would commit the preedit text.
    harness.process_text_event(TextEvent::Ime(Ime::Preedit(String::new(), None)));
    harness.process_text_event(TextEvent::Ime(Ime::Commit("你".into())));
    assert_eq!(textbox_text(&mut harness), "你");
}

// 4. Plain wheel consumed; zoom unchanged.
#[test]
fn plain_wheel_consumed_zoom_unchanged() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    let handled = wheel(&mut harness, -120.0, Modifiers::empty());
    assert!(matches!(handled, Handled::Yes), "wheel consumed");

    assert!(
        zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness)).is_empty(),
        "plain wheel must not change zoom"
    );
}

// 5. Ctrl+wheel ZoomAt: accumulates multiplicatively and clamps silent.
#[test]
fn ctrl_wheel_zoom_at_accumulates_and_clamps() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    // Wheel-up physical (dy>0 → flipped dy<0) zooms in ×1.1.
    wheel(&mut harness, 120.0, Modifiers::CONTROL);
    let z1 = zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness))
        .pop()
        .expect("ZoomChanged after first tick");
    assert!((z1 - BASE * 1.1).abs() < 1e-9, "first tick: {z1}");

    // Drive to the clamp: far more in-ticks than needed for MAX.
    for _ in 0..20 {
        wheel(&mut harness, 120.0, Modifiers::CONTROL);
    }
    let last = zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness))
        .pop()
        .expect("zoom ticks");
    assert!(last > 8.0, "accumulated well past baseline: {last}");

    // At the clamp the component stops firing (its guard skips no-change).
    wheel(&mut harness, 120.0, Modifiers::CONTROL);
    assert!(
        zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness)).is_empty(),
        "no ZoomChanged once clamped"
    );
}

// 6. Drag-select both lines → Ctrl+C clipboard equals "ABCD\nEF".
#[test]
fn drag_select_ctrl_c_copies_text() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // Anchor before 'A' (page x 12 → obj-local 2 < half-cell → offset 0)
    // and end after 'F' (page x 26 → obj-local 16 ≥ cell boundary →
    // offset 2 on the second code), line 0 → line 1.
    let anchor = PhysicalPosition::new(ORIGIN_X + 12.0 * BASE, ORIGIN_Y + 25.0 * BASE);
    let end = PhysicalPosition::new(ORIGIN_X + 26.0 * BASE, ORIGIN_Y + 45.0 * BASE);
    let end_state = PointerState {
        position: end,
        buttons: PointerButtons::from(PointerButton::Primary),
        ..PointerState::default()
    };
    harness.process_pointer_event(PointerEvent::Down(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: PointerState {
            position: anchor,
            buttons: PointerButtons::from(PointerButton::Primary),
            ..PointerState::default()
        },
    }));
    harness.process_pointer_event(PointerEvent::Move(xilem::masonry::core::PointerUpdate {
        pointer: MOUSE,
        current: end_state.clone(),
        coalesced: vec![],
        predicted: vec![],
    }));
    harness.process_pointer_event(PointerEvent::Up(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: end_state,
    }));

    assert_eq!(harness.clipboard_contents(), "");

    let handled = harness.process_text_event(TextEvent::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key: MasonryKey::Character("c".into()),
        modifiers: Modifiers::CONTROL,
        ..KeyboardEvent::default()
    }));
    assert!(matches!(handled, Handled::Yes), "Ctrl+C consumed");
    assert_eq!(harness.clipboard_contents(), "ABCD\nEF");
}

// 8. Ctrl + pure horizontal scroll tick (dy=0) is consumed but not zoom.
#[test]
fn ctrl_horizontal_scroll_does_not_zoom() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    let handled = wheel(&mut harness, 0.0, Modifiers::CONTROL);
    assert!(matches!(handled, Handled::Yes), "horizontal tick consumed");

    assert!(
        zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness)).is_empty(),
        "pure horizontal ctrl+scroll must not zoom"
    );
}

// 7. Window focus gates caret visibility.
#[test]
fn window_focus_gates_caret_visibility() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // Place a caret (none exists until the first textbox click).
    click(
        &mut harness,
        ORIGIN_X + 5.0 * BASE,
        ORIGIN_Y + 105.0 * BASE,
        1,
    );

    harness.process_text_event(TextEvent::WindowFocusChange(true));
    let focused = harness.render();

    harness.process_text_event(TextEvent::WindowFocusChange(false));
    let blurred = harness.render();
    assert_ne!(focused, blurred, "caret should hide on window blur");

    harness.process_text_event(TextEvent::WindowFocusChange(true));
    let regained = harness.render();
    assert_eq!(focused, regained, "caret should reappear on refocus");
}
