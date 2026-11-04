// Ported from rword@4e0b25b6 (crates/gpui-view/tests/event_mapping.rs) —
// rofd targets: PointerDown (u8 click_count) / PointerUp (no modifiers) /
// Scroll {dx,dy} / ctrl+wheel emits anchored multiplicative ZoomAt with
// px-per-mm bounds instead of rword's absolute Zoom.

//! gpui event structs -> rofd `ViewEvent` mapping.
//!
//! Direction facts (gpui-line spike report / gpui-pre-windows events.rs):
//! - Windows wheel: `Lines(notches * OS wheel_scroll_lines)`, POSITIVE y =
//!   wheel UP. The kernel wants +y = scroll DOWN => negate.
//! - gpui lines carry the OS multiplier (default 3 lines/notch); divide it
//!   back out so one notch == SCROLL_LINE_PX (20px, xilem-line feel).
//! - gpui pixel deltas are logical Pixels => no scale division (platform
//!   scales internally; differs from masonry's raw device-px path).
//! - Zoom bounds are rofd's px-per-mm absolutes (re-exported by
//!   rofd-component): the kernel clamps and value-dedupes ZoomAt itself,
//!   but a boundary ZoomAt still exits fit mode — the adapter-side no-op
//!   suppression under test is load-bearing.

use gpui::{
    px, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Point,
    ScrollDelta, ScrollEvent, ScrollSource, TouchPhase,
};
use rofd_component::{Key, ViewEvent, MAX_ZOOM, MIN_ZOOM};
use rofd_gpui_view::events::{self, ScrollOutcome, GPUI_LINES_PER_NOTCH, SCROLL_LINE_PX};

fn scroll_event(delta: ScrollDelta, control: bool) -> ScrollEvent {
    ScrollEvent {
        position: Point::new(px(10.), px(10.)),
        delta,
        modifiers: Modifiers {
            control,
            ..Default::default()
        },
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }
}

fn keystroke(key: &str, control: bool) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control,
                ..Default::default()
            },
            key: key.to_string(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    }
}

#[test]
fn modifiers_from_maps_platform_to_meta() {
    let m = events::modifiers_from(&Modifiers {
        control: true,
        alt: true,
        shift: true,
        platform: true,
        function: true,
    });
    assert!(m.control && m.alt && m.shift && m.meta);
    // gpui's `function` has no kernel counterpart — dropped, not invented.
    let bare = events::modifiers_from(&Modifiers::default());
    assert_eq!(bare, rofd_component::Modifiers::default());
}

#[test]
fn map_key_covers_rofd_key_set() {
    let table = [
        ("enter", Key::Enter),
        ("return", Key::Enter),
        ("backspace", Key::Backspace),
        ("delete", Key::Delete),
        ("del", Key::Delete),
        ("tab", Key::Tab),
        ("escape", Key::Escape),
        ("esc", Key::Escape),
        ("arrowleft", Key::ArrowLeft),
        ("left", Key::ArrowLeft),
        ("arrowright", Key::ArrowRight),
        ("right", Key::ArrowRight),
        ("arrowup", Key::ArrowUp),
        ("up", Key::ArrowUp),
        ("arrowdown", Key::ArrowDown),
        ("down", Key::ArrowDown),
        ("home", Key::Home),
        ("end", Key::End),
        ("pageup", Key::PageUp),
        ("pagedown", Key::PageDown),
        ("a", Key::Char('a')),
    ];
    for (name, expected) in table {
        assert_eq!(events::map_key(name), expected, "map_key({name})");
    }
    // Longer names (F-keys, media keys) and the empty string degrade to
    // Unidentified — the kernel decides what to do with those.
    assert_eq!(events::map_key("f1"), Key::Unidentified);
    assert_eq!(events::map_key("ab"), Key::Unidentified);
    assert_eq!(events::map_key(""), Key::Unidentified);
}

#[test]
fn key_down_filters_repeats_and_bare_printables() {
    assert_eq!(
        events::key_down(&keystroke("enter", false)),
        Some(ViewEvent::KeyDown {
            key: Key::Enter,
            modifiers: Default::default(),
        })
    );
    // Modifier shortcuts pass through with their modifiers.
    assert_eq!(
        events::key_down(&keystroke("a", true)),
        Some(ViewEvent::KeyDown {
            key: Key::Char('a'),
            modifiers: rofd_component::Modifiers {
                control: true,
                ..Default::default()
            },
        })
    );
    // Auto-repeat (is_held) must not re-fire (mirrors the xilem line's
    // !repeat filter).
    let mut held = keystroke("a", true);
    held.is_held = true;
    assert_eq!(events::key_down(&held), None);
    // BARE printable chars ride the input-handler channel only (gpui
    // delivers printable input on BOTH raw keys and
    // InputHandler::replace_text_in_range): forwarding them here too
    // would double every typed character.
    assert_eq!(events::key_down(&keystroke("a", false)), None);
    assert_eq!(events::key_down(&keystroke("B", false)), None);
}

#[test]
fn mouse_down_at_maps_button_click_count_and_coords() {
    let event = MouseDownEvent {
        button: MouseButton::Right,
        position: Point::new(px(1.0), px(2.0)),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        click_count: 2,
        first_mouse: false,
    };
    assert_eq!(
        events::mouse_down_at(&event, 12.5, 40.25),
        ViewEvent::PointerDown {
            button: rofd_component::MouseButton::Right,
            x: 12.5,
            y: 40.25,
            modifiers: rofd_component::Modifiers {
                control: true,
                ..Default::default()
            },
            click_count: 2,
        }
    );
    // The kernel field is u8 — clamp instead of truncating.
    let mut big = event;
    big.click_count = 300;
    assert_eq!(
        events::mouse_down_at(&big, 0.0, 0.0),
        ViewEvent::PointerDown {
            button: rofd_component::MouseButton::Right,
            x: 0.0,
            y: 0.0,
            modifiers: rofd_component::Modifiers {
                control: true,
                ..Default::default()
            },
            click_count: u8::MAX,
        }
    );
}

#[test]
fn mouse_up_at_drops_modifiers() {
    // rofd's PointerUp carries no modifiers.
    let event = MouseUpEvent {
        button: MouseButton::Middle,
        position: Point::new(px(1.0), px(2.0)),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        click_count: 1,
    };
    assert_eq!(
        events::mouse_up_at(&event, 7.5, 9.25),
        ViewEvent::PointerUp {
            button: rofd_component::MouseButton::Middle,
            x: 7.5,
            y: 9.25,
        }
    );
}

#[test]
fn wheel_lines_calibrate_to_scroll_line_px() {
    // Windows wheel DOWN arrives as Lines(0, -3) (up is positive); the
    // kernel wants +y = down, so one notch down == +SCROLL_LINE_PX.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, -3.0)), false),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Scroll(ViewEvent::Scroll { dx, dy }) => {
            assert!((dy - SCROLL_LINE_PX as f64).abs() < 1e-6, "dy={dy}");
            assert!(dx.abs() < 1e-6);
        }
        other => panic!("expected scroll, got {other:?}"),
    }
    // Wheel UP (Lines positive y) negates.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, 3.0)), false),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Scroll(ViewEvent::Scroll { dy, .. }) => {
            assert!((dy + SCROLL_LINE_PX as f64).abs() < 1e-6, "dy={dy}");
        }
        other => panic!("expected scroll, got {other:?}"),
    }
    // The OS multiplier (default 3 lines/notch) is divided out: a full
    // 3-line tick is exactly one 20px notch.
    let out = events::scroll(
        &scroll_event(
            ScrollDelta::Lines(Point::new(0.0, -GPUI_LINES_PER_NOTCH)),
            false,
        ),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Scroll(ViewEvent::Scroll { dy, .. }) => {
            assert!((dy - 20.0).abs() < 1e-6);
        }
        other => panic!("expected scroll, got {other:?}"),
    }
    // Horizontal (Shift+wheel on Windows): Lines(x), +x = right, kept.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(1.5, 0.0)), false),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Scroll(ViewEvent::Scroll { dx, dy }) => {
            assert!((dx - 10.0).abs() < 1e-6, "dx={dx}");
            assert!(dy.abs() < 1e-6);
        }
        other => panic!("expected scroll, got {other:?}"),
    }
}

#[test]
fn pixel_deltas_negate_and_stay_logical() {
    // gpui pixel deltas are logical Pixels — no scale division (unlike
    // masonry's raw device-pixel path). Down = -y in gpui => +dy kernel.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Pixels(Point::new(px(0.), px(-15.))), false),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Scroll(ViewEvent::Scroll { dx, dy }) => {
            assert!((dy - 15.0).abs() < 1e-6);
            assert!(dx.abs() < 1e-6);
        }
        other => panic!("expected scroll, got {other:?}"),
    }
}

#[test]
fn ctrl_wheel_emits_anchored_zoom_at() {
    // Wheel UP zooms IN. rofd's ZoomAt is multiplicative and anchored at
    // the pointer's viewport position.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, 3.0)), true),
        1.0,
        (12.5, 40.25),
    );
    match out {
        ScrollOutcome::Zoom(ViewEvent::ZoomAt { factor, center }) => {
            assert!((factor - 1.1).abs() < 1e-9, "factor={factor}");
            assert_eq!(center, (12.5, 40.25));
        }
        other => panic!("expected zoom, got {other:?}"),
    }
    // Wheel DOWN zooms out by the xilem-line out-step (0.9, parity with
    // crates/xilem-view ZOOM_OUT_STEP).
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, -3.0)), true),
        1.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Zoom(ViewEvent::ZoomAt { factor, .. }) => {
            assert!((factor - 0.9).abs() < 1e-9, "factor={factor}");
        }
        other => panic!("expected zoom, got {other:?}"),
    }
    // Zero ctrl delta is a no-op.
    assert!(matches!(
        events::scroll(
            &scroll_event(ScrollDelta::Lines(Point::new(0.0, 0.0)), true),
            1.0,
            (0.0, 0.0),
        ),
        ScrollOutcome::None
    ));
}

#[test]
fn ctrl_wheel_at_bounds_is_noop() {
    // Already at max, zooming in: suppressed. The kernel value-dedupes
    // ZoomAt, but a boundary event would still exit fit mode and fire
    // on_zoom_mode_change — the adapter must not forward it.
    assert!(matches!(
        events::scroll(
            &scroll_event(ScrollDelta::Lines(Point::new(0.0, 3.0)), true),
            MAX_ZOOM,
            (0.0, 0.0),
        ),
        ScrollOutcome::None
    ));
    // Already at min, zooming out: suppressed.
    assert!(matches!(
        events::scroll(
            &scroll_event(ScrollDelta::Lines(Point::new(0.0, -3.0)), true),
            MIN_ZOOM,
            (0.0, 0.0),
        ),
        ScrollOutcome::None
    ));
}

#[test]
fn ctrl_wheel_partial_step_emits_raw_factor() {
    // Near max: the candidate overshoots the ceiling but the zoom can
    // still move (the kernel clamps the value) — emit the raw step and
    // let the kernel own the final clamp.
    let out = events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, 3.0)), true),
        11.0,
        (0.0, 0.0),
    );
    match out {
        ScrollOutcome::Zoom(ViewEvent::ZoomAt { factor, .. }) => {
            assert!((factor - 1.1).abs() < 1e-9, "factor={factor}");
        }
        other => panic!("expected zoom, got {other:?}"),
    }
}

#[test]
fn ctrl_wheel_below_min_moves_freely() {
    // A fit mode may land the zoom below MIN_ZOOM; the kernel's floor
    // then drops to FIT_MIN_ZOOM (no-yank contract) so BOTH directions
    // still move — the adapter must not suppress them (and must never
    // redirect an out-zoom back up to MIN_ZOOM).
    let below_min = MIN_ZOOM * 0.5;
    assert!(below_min < MIN_ZOOM);
    match events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, -3.0)), true),
        below_min,
        (0.0, 0.0),
    ) {
        ScrollOutcome::Zoom(ViewEvent::ZoomAt { factor, .. }) => {
            assert!((factor - 0.9).abs() < 1e-9, "factor={factor}");
        }
        other => panic!("expected zoom out, got {other:?}"),
    }
    match events::scroll(
        &scroll_event(ScrollDelta::Lines(Point::new(0.0, 3.0)), true),
        below_min,
        (0.0, 0.0),
    ) {
        ScrollOutcome::Zoom(ViewEvent::ZoomAt { factor, .. }) => {
            assert!((factor - 1.1).abs() < 1e-9, "factor={factor}");
        }
        other => panic!("expected zoom in, got {other:?}"),
    }
}
