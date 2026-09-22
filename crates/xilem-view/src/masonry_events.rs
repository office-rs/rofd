//! Pure translation from masonry event types (ui-events / keyboard-types)
//! to rofd `ViewEvent`s.
//!
//! This module holds no widget state: every function is a pure mapping,
//! unit-testable without a window or widget tree. It is the masonry
//! successor to the retired winit-bridge translation table.
//!
//! Sign conventions (load-bearing, verified against sources):
//! - ui-events-winit passes winit wheel signs through verbatim
//!   (`MouseScrollDelta::LineDelta(x, y) -> ScrollDelta::LineDelta(x, y)`),
//!   so positive `y` still means "scrolled up" (towards the user).
//! - The component expects web-style deltas: positive `dy` scrolls DOWN,
//!   positive `dx` scrolls RIGHT (`ViewEvent::Scroll`).
//! - Therefore `y` is negated; `x` passes through. Logical pixels.

use rofd_component::{Key, Modifiers, MouseButton, ViewEvent};
use xilem::masonry::core::keyboard::{Key as MasonryKey, NamedKey};
use xilem::masonry::core::{Modifiers as MasonryModifiers, PointerButton, ScrollDelta};

/// Logical pixels scrolled per wheel "line". Matches the legacy
/// winit-bridge constant (20 px) and typical desktop scroll distance.
pub const SCROLL_LINE_PX: f64 = 20.0;

/// Convert masonry (keyboard-types) modifiers to component modifiers.
pub fn rofd_modifiers(m: &MasonryModifiers) -> Modifiers {
    Modifiers {
        shift: m.shift(),
        control: m.ctrl(),
        alt: m.alt(),
        meta: m.meta(),
    }
}

/// Map a keyboard-types logical (named) key to the component `Key`.
///
/// Returns `None` for everything not in the table (dead keys, raw
/// numerics, F-keys); `key_down_events` then emits nothing.
pub fn named_key(key: &NamedKey) -> Option<Key> {
    use NamedKey as N;
    Some(match key {
        N::Enter => Key::Enter,
        N::Backspace => Key::Backspace,
        N::Delete => Key::Delete,
        N::Tab => Key::Tab,
        N::Escape => Key::Escape,
        N::ArrowLeft => Key::ArrowLeft,
        N::ArrowRight => Key::ArrowRight,
        N::ArrowUp => Key::ArrowUp,
        N::ArrowDown => Key::ArrowDown,
        N::Home => Key::Home,
        N::End => Key::End,
        N::PageUp => Key::PageUp,
        N::PageDown => Key::PageDown,
        _ => return None,
    })
}

/// Translate a key-down into the `ViewEvent`s to dispatch.
///
/// - Named keys map through [`named_key`]; unmapped ones produce nothing.
/// - `Character` produces one KeyDown for the string's first char (masonry
///   strings are single graphemes in this revision). The component applies
///   its own ctrl/alt/meta guard, so no filtering happens here.
pub fn key_down_events(key: &MasonryKey, modifiers: &MasonryModifiers) -> Vec<ViewEvent> {
    let mods = rofd_modifiers(modifiers);
    match key {
        MasonryKey::Named(named) => named_key(named).map(|k| ViewEvent::KeyDown {
            key: k,
            modifiers: mods,
        }),
        MasonryKey::Character(s) => s
            .chars()
            .map(|c| ViewEvent::KeyDown {
                key: Key::Char(c),
                modifiers: mods,
            })
            .next(),
    }
    .into_iter()
    .collect()
}

/// Map a masonry pointer button to the component mouse button.
///
/// `None` (touch contact) and unmapped back/forward buttons produce no
/// event — matching the legacy bridge behaviour.
pub fn mouse_button(button: Option<&PointerButton>) -> Option<MouseButton> {
    match button {
        Some(PointerButton::Primary) => Some(MouseButton::Left),
        Some(PointerButton::Secondary) => Some(MouseButton::Right),
        Some(PointerButton::Auxiliary) => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Wheel deltas as `(dx, dy)` in logical pixels, web convention (positive
/// `y` scrolls down).
///
/// `scale_factor` converts physical-px `PixelDelta`s to logical; `page_px`
/// is the page-scroll policy (the visible height) for `PageDelta`.
pub fn scroll_deltas(delta: &ScrollDelta, scale_factor: f64, page_px: f64) -> (f64, f64) {
    match *delta {
        // Line/Page fields are f32 (ui-events 0.3); the component viewport
        // is f64 logical px.
        ScrollDelta::LineDelta(x, y) => (
            f64::from(x) * SCROLL_LINE_PX,
            f64::from(-y) * SCROLL_LINE_PX,
        ),
        ScrollDelta::PageDelta(x, y) => (f64::from(x) * page_px, f64::from(-y) * page_px),
        // PixelDelta arrives in physical px (trackpads); component viewport
        // is logical. The legacy bridge skipped this division — bug fixed.
        ScrollDelta::PixelDelta(p) => (p.x / scale_factor, -p.y / scale_factor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xilem::masonry::core::keyboard::Key as MK;
    use xilem::masonry::dpi::PhysicalPosition;

    fn mods(ctrl: bool) -> MasonryModifiers {
        let mut m = MasonryModifiers::empty();
        if ctrl {
            m |= MasonryModifiers::CONTROL;
        }
        m
    }

    #[test]
    fn named_key_table() {
        use NamedKey as N;
        assert_eq!(named_key(&N::Enter), Some(Key::Enter));
        assert_eq!(named_key(&N::Backspace), Some(Key::Backspace));
        assert_eq!(named_key(&N::ArrowLeft), Some(Key::ArrowLeft));
        assert_eq!(named_key(&N::PageUp), Some(Key::PageUp));
        assert_eq!(named_key(&N::Home), Some(Key::Home));
        assert!(named_key(&N::F1).is_none());
    }

    #[test]
    fn character_key_maps_to_char() {
        let evs = key_down_events(&MK::Character("a".into()), &mods(false));
        assert_eq!(evs.len(), 1);
        assert!(matches!(
            evs[0],
            ViewEvent::KeyDown {
                key: Key::Char('a'),
                ..
            }
        ));
    }

    #[test]
    fn character_key_passes_ctrl_through() {
        // The component owns the ctrl guard ("skip if control/alt/meta
        // held"); the translator must not pre-filter.
        let evs = key_down_events(&MK::Character("s".into()), &mods(true));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ViewEvent::KeyDown { modifiers, .. } if modifiers.control));
    }

    #[test]
    fn unmapped_named_key_maps_to_nothing() {
        assert!(key_down_events(&MK::Named(NamedKey::F1), &mods(false)).is_empty());
        assert!(key_down_events(&MK::Character("".into()), &mods(false)).is_empty());
    }

    #[test]
    fn modifiers_map_fields() {
        let mut m = MasonryModifiers::empty();
        m |= MasonryModifiers::SHIFT | MasonryModifiers::ALT;
        let r = rofd_modifiers(&m);
        assert!(r.shift && r.alt && !r.control && !r.meta);
    }

    #[test]
    fn line_delta_sign_and_scale() {
        // winit: positive y = scroll up → component dy must be negative
        // (web convention: positive scrolls down).
        let (dx, dy) = scroll_deltas(&ScrollDelta::LineDelta(1.0, -3.0), 1.0, 800.0);
        assert_eq!(dx, 20.0);
        assert_eq!(dy, 60.0);
    }

    #[test]
    fn pixel_delta_converts_to_logical() {
        let (dx, dy) = scroll_deltas(
            &ScrollDelta::PixelDelta(PhysicalPosition::new(100.0, -50.0)),
            2.0,
            800.0,
        );
        assert_eq!(dx, 50.0);
        assert_eq!(dy, 25.0);
    }

    #[test]
    fn page_delta_uses_visible_height() {
        let (dx, dy) = scroll_deltas(&ScrollDelta::PageDelta(1.0, 1.0), 1.0, 500.0);
        assert_eq!(dx, 500.0);
        assert_eq!(dy, -500.0);
    }

    #[test]
    fn mouse_button_mapping() {
        assert_eq!(
            mouse_button(Some(&PointerButton::Primary)),
            Some(MouseButton::Left)
        );
        assert_eq!(
            mouse_button(Some(&PointerButton::Secondary)),
            Some(MouseButton::Right)
        );
        assert_eq!(
            mouse_button(Some(&PointerButton::Auxiliary)),
            Some(MouseButton::Middle)
        );
        assert!(mouse_button(Some(&PointerButton::X1)).is_none());
        assert!(mouse_button(None).is_none());
    }
}
