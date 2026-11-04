// Ported from rword@4e0b25b6 (crates/gpui-view/src/events.rs) — rofd
// divergences: PointerDown carries a u8 click_count, PointerUp drops the
// modifiers, wheel deltas land in ViewEvent::Scroll {dx, dy}, and ctrl+wheel
// emits an anchored multiplicative ViewEvent::ZoomAt under rofd's px-per-mm
// bounds instead of rword's absolute Zoom.

//! gpui event structs -> rofd `ViewEvent` mapping.
//!
//! Pure functions over gpui's plain event structs — no window state, so the
//! whole table is unit-testable headlessly (`tests/event_mapping.rs`).
//!
//! Direction/units facts (gpui-line spike report 2026-10-28, from
//! gpui-pre-windows `events.rs`): the Windows wheel reports `Lines(notches *
//! OS wheel_scroll_lines)` with POSITIVE y = wheel UP, while the kernel
//! wants +y = scroll DOWN — so y is negated here, and the OS line
//! multiplier (default 3) is divided back out so one notch == one
//! `SCROLL_LINE_PX` step like the xilem line. gpui pixel deltas are logical
//! `Pixels` (the platform scales internally), so unlike masonry's raw
//! device-pixel path there is no scale division.
//!
//! The `_at` variants take viewport-local coordinates: the mouse listeners
//! translate window -> viewport before calling (the kernel speaks viewport
//! px), keeping this module free of element-bounds state.

use gpui::{
    KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollDelta, ScrollEvent,
};
use rofd_component::{
    Key, Modifiers as KernelModifiers, MouseButton as KernelMouseButton, ViewEvent, FIT_MIN_ZOOM,
    MAX_ZOOM, MIN_ZOOM,
};

/// Logical px per wheel notch — matches the xilem line's feel.
pub const SCROLL_LINE_PX: f32 = 20.0;
/// Windows default `wheel_scroll_lines`: gpui multiplies notches by the OS
/// setting; divide it out. Users with a non-default setting get a
/// proportional feel shift (same class of deviation as the OS setting
/// itself) — revisit if it bothers anyone.
pub const GPUI_LINES_PER_NOTCH: f32 = 3.0;
/// Ctrl+wheel zoom factors — same values as the xilem line's
/// ZOOM_IN_STEP/ZOOM_OUT_STEP (crates/xilem-view/src/ofd_widget.rs), so
/// both native adapters feel identical.
const ZOOM_IN_STEP: f64 = 1.1;
const ZOOM_OUT_STEP: f64 = 0.9;

pub fn modifiers_from(m: &Modifiers) -> KernelModifiers {
    KernelModifiers {
        shift: m.shift,
        control: m.control,
        alt: m.alt,
        // gpui names the windows/super/command key `platform`.
        meta: m.platform,
    }
}

/// Named-key table (gpui `Keystroke.key` is a lowercase string) ported
/// from the xilem line's `masonry_events.rs`; single characters become
/// `Key::Char`. Unknown names (F-keys, media keys, …) map to
/// `Key::Unidentified` — the kernel decides what to do with those.
pub fn map_key(key: &str) -> Key {
    match key.to_lowercase().as_str() {
        "enter" | "return" => Key::Enter,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "arrowleft" | "left" => Key::ArrowLeft,
        "arrowright" | "right" => Key::ArrowRight,
        "arrowup" | "up" => Key::ArrowUp,
        "arrowdown" | "down" => Key::ArrowDown,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        _ => {
            // Single printable character; anything longer (F1, space names,
            // ime-aware key_char variants) or empty degrades to
            // Unidentified for v1.
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c),
                _ => Key::Unidentified,
            }
        }
    }
}

/// Auto-repeat (`is_held`) is filtered — the kernel must not see repeats
/// (mirrors the xilem line's `!repeat` guard). Modifiers are NOT
/// pre-filtered: the kernel owns the ctrl/alt guards.
///
/// BARE printable chars are dropped here ON PURPOSE: gpui delivers
/// printable input on TWO channels — raw keys AND
/// WM_CHAR -> InputHandler::replace_text_in_range — and the kernel inserts
/// on both, doubling every typed character. The input-handler channel owns
/// plain text (IME compositions and plain chars alike); the key channel
/// carries named keys and modifier shortcuts only.
pub fn key_down(event: &KeyDownEvent) -> Option<ViewEvent> {
    if event.is_held {
        return None;
    }
    let m = &event.keystroke.modifiers;
    let bare_printable = !m.control
        && !m.alt
        && !m.platform
        && matches!(map_key(&event.keystroke.key), Key::Char(_));
    if bare_printable {
        return None;
    }
    Some(ViewEvent::KeyDown {
        key: map_key(&event.keystroke.key),
        modifiers: modifiers_from(m),
    })
}

pub fn mouse_down_at(event: &MouseDownEvent, x: f32, y: f32) -> ViewEvent {
    ViewEvent::PointerDown {
        button: button_from(event.button),
        x: x as f64,
        y: y as f64,
        modifiers: modifiers_from(&event.modifiers),
        // The kernel field is u8 (winit/DOM `detail`); clamp, never
        // truncate.
        click_count: event.click_count.min(u8::MAX as usize) as u8,
    }
}

/// rofd's `PointerUp` carries no modifiers.
pub fn mouse_up_at(event: &MouseUpEvent, x: f32, y: f32) -> ViewEvent {
    ViewEvent::PointerUp {
        button: button_from(event.button),
        x: x as f64,
        y: y as f64,
    }
}

pub fn mouse_move_at(_event: &MouseMoveEvent, x: f32, y: f32) -> ViewEvent {
    ViewEvent::PointerMove {
        x: x as f64,
        y: y as f64,
    }
}

fn button_from(button: MouseButton) -> KernelMouseButton {
    match button {
        MouseButton::Left => KernelMouseButton::Left,
        MouseButton::Right => KernelMouseButton::Right,
        MouseButton::Middle => KernelMouseButton::Middle,
        // Navigation and future buttons have no kernel semantics.
        _ => KernelMouseButton::Left,
    }
}

/// What a scroll event resolves to: a calibrated content scroll, an
/// anchored zoom change (ctrl + wheel is adapter-internal, never a content
/// scroll), or nothing (zero delta / zoom that cannot move).
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollOutcome {
    Scroll(ViewEvent),
    Zoom(ViewEvent),
    None,
}

/// Map a gpui scroll event at the given current zoom (px/mm) and pointer
/// position (viewport px). Wheel UP zooms IN (standard editor convention),
/// anchored at the pointer: the kernel's `ZoomAt` keeps the `center` point
/// on the same document position.
pub fn scroll(event: &ScrollEvent, current_zoom: f64, pointer: (f32, f32)) -> ScrollOutcome {
    if event.modifiers.control {
        let raw_y = raw_y(&event.delta);
        if raw_y == 0.0 {
            return ScrollOutcome::None;
        }
        let factor = if raw_y > 0.0 {
            ZOOM_IN_STEP
        } else {
            ZOOM_OUT_STEP
        };
        if !zoom_can_move(current_zoom, current_zoom * factor) {
            return ScrollOutcome::None;
        }
        return ScrollOutcome::Zoom(ViewEvent::ZoomAt {
            factor,
            center: (pointer.0 as f64, pointer.1 as f64),
        });
    }
    let (dx, dy) = match &event.delta {
        ScrollDelta::Lines(l) => (
            (l.x * SCROLL_LINE_PX / GPUI_LINES_PER_NOTCH) as f64,
            (-l.y * SCROLL_LINE_PX / GPUI_LINES_PER_NOTCH) as f64,
        ),
        ScrollDelta::Pixels(p) => (f32::from(p.x) as f64, -(f32::from(p.y) as f64)),
    };
    if dx == 0.0 && dy == 0.0 {
        return ScrollOutcome::None;
    }
    ScrollOutcome::Scroll(ViewEvent::Scroll { dx, dy })
}

/// The kernel's own no-move check, mirrored adapter-side: `ZoomAt` clamps
/// to `[zoom_floor(z), MAX_ZOOM]` and value-dedupes, but a boundary event
/// still exits fit mode and fires `on_zoom_mode_change` — so zooms that
/// cannot move the value are suppressed HERE and never reach the kernel.
/// `zoom_floor` (component zoom_floor fn) drops to `FIT_MIN_ZOOM` when a
/// fit mode landed the zoom below `MIN_ZOOM` (no-yank contract), which is
/// why out-zooming below MIN_ZOOM is NOT suppressed.
fn zoom_can_move(current_zoom: f64, candidate: f64) -> bool {
    let floor = if current_zoom < MIN_ZOOM {
        FIT_MIN_ZOOM
    } else {
        MIN_ZOOM
    };
    let clamped = candidate.clamp(floor, MAX_ZOOM);
    (clamped - current_zoom).abs() > f64::EPSILON
}

fn raw_y(delta: &ScrollDelta) -> f32 {
    match delta {
        ScrollDelta::Lines(l) => l.y,
        ScrollDelta::Pixels(p) => f32::from(p.y),
    }
}
