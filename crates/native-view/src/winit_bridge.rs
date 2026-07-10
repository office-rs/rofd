use rofd_component::{Key, Modifiers, MouseButton, ViewEvent};

/// Transient winit translation state. NOT a field on EditorApp (keeps EditorApp
/// framework-agnostic). The Host owns both and passes &mut EditorApp into handle_window_event.
pub struct WinitEventBridge {
    pub modifiers: Modifiers,
    cursor_phys_x: f64,
    cursor_phys_y: f64,
    pub scale_factor: f64,
}

impl WinitEventBridge {
    pub fn new() -> Self {
        Self { modifiers: Modifiers::default(), cursor_phys_x: 0.0, cursor_phys_y: 0.0, scale_factor: 1.0 }
    }

    pub fn set_scale_factor(&mut self, sf: f64) { self.scale_factor = sf; }

    pub fn set_cursor(&mut self, x: f64, y: f64) { self.cursor_phys_x = x; self.cursor_phys_y = y; }

    /// Canvas-local logical px = physical px / scale_factor (full-window canvas, origin = (0,0)).
    fn canvas_local(&self) -> (f64, f64) {
        (self.cursor_phys_x / self.scale_factor, self.cursor_phys_y / self.scale_factor)
    }

    /// Translate a winit WindowEvent into a rofd ViewEvent (or None if not relevant).
    /// Returns Some(ViewEvent) for pointer/keyboard/scroll/resize events.
    pub fn translate(&self, event: &winit::event::WindowEvent) -> Option<ViewEvent> {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                Some(ViewEvent::PointerMove { x: position.x / self.scale_factor, y: position.y / self.scale_factor })
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.canvas_local();
                let btn = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return None,
                };
                match state {
                    winit::event::ElementState::Pressed => Some(ViewEvent::PointerDown { button: btn, x, y, modifiers: self.modifiers }),
                    winit::event::ElementState::Released => Some(ViewEvent::PointerUp { button: btn, x, y }),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(lx, ly) => (*lx as f64 * 20.0, *ly as f64 * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
                };
                if self.modifiers.control {
                    Some(ViewEvent::Zoom { factor: if dy > 0.0 { 1.1 } else { 0.9 } })
                } else {
                    Some(ViewEvent::Scroll { dx, dy: -dy })
                }
            }
            WindowEvent::Resized(physical_size) => {
                Some(ViewEvent::Resize {
                    width: physical_size.width as f64 / self.scale_factor,
                    height: physical_size.height as f64 / self.scale_factor,
                })
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != winit::event::ElementState::Pressed { return None; }
                let key = winit_key_to_rofd(&event.physical_key, &event.text);
                Some(ViewEvent::KeyDown { key, modifiers: self.modifiers })
            }
            WindowEvent::Focused(focused) => {
                if *focused { Some(ViewEvent::FocusGained) } else { Some(ViewEvent::FocusLost) }
            }
            _ => None,
        }
    }

    /// Update modifiers from a winit ModifiersChanged event state.
    ///
    /// Takes the `ModifiersState` obtained from `WindowEvent::ModifiersChanged(m)` via
    /// `m.state()`. In winit 0.30 `ModifiersState` lives in `winit::keyboard` (re-exported
    /// from `winit::event::Modifiers::state()`), not `winit::event`.
    pub fn update_modifiers(&mut self, state: &winit::keyboard::ModifiersState) {
        self.modifiers = Modifiers {
            shift: state.shift_key(),
            control: state.control_key(),
            alt: state.alt_key(),
            meta: state.super_key(),
        };
    }
}

impl Default for WinitEventBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a winit physical key + text to a rofd Key.
///
/// `text` is winit's `Option<SmolStr>` (the `KeyEvent.text` field in 0.30),
/// used as a fallback for character keys not covered by the named KeyCode arms.
fn winit_key_to_rofd(key: &winit::keyboard::PhysicalKey, text: &Option<winit::keyboard::SmolStr>) -> Key {
    use winit::keyboard::PhysicalKey;
    match key {
        PhysicalKey::Code(code) => {
            use winit::keyboard::KeyCode;
            match code {
                KeyCode::Enter => Key::Enter,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::Delete => Key::Delete,
                KeyCode::Tab => Key::Tab,
                KeyCode::Escape => Key::Escape,
                KeyCode::ArrowLeft => Key::ArrowLeft,
                KeyCode::ArrowRight => Key::ArrowRight,
                KeyCode::ArrowUp => Key::ArrowUp,
                KeyCode::ArrowDown => Key::ArrowDown,
                KeyCode::Home => Key::Home,
                KeyCode::End => Key::End,
                KeyCode::PageUp => Key::PageUp,
                KeyCode::PageDown => Key::PageDown,
                _ => {
                    // For character keys, use the text field if available.
                    if let Some(t) = text {
                        if let Some(c) = t.as_str().chars().next() {
                            return Key::Char(c);
                        }
                    }
                    Key::Unidentified
                }
            }
        }
        _ => Key::Unidentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_local_divides_by_scale_factor() {
        let mut bridge = WinitEventBridge::new();
        bridge.set_scale_factor(2.0);
        bridge.set_cursor(100.0, 200.0);
        let (x, y) = bridge.canvas_local();
        assert_eq!(x, 50.0);
        assert_eq!(y, 100.0);
    }

    #[test]
    fn canvas_local_default_scale_1() {
        let mut bridge = WinitEventBridge::new();
        bridge.set_cursor(10.0, 20.0);
        let (x, y) = bridge.canvas_local();
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn update_modifiers_maps_correctly() {
        let mut bridge = WinitEventBridge::new();
        let state = winit::keyboard::ModifiersState::CONTROL;
        bridge.update_modifiers(&state);
        assert!(bridge.modifiers.control);
        assert!(!bridge.modifiers.shift);
    }
}
