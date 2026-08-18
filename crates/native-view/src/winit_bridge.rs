use rofd_component::{Key, Modifiers, MouseButton, ScrollDirection, ViewEvent};

/// Transient winit translation state. NOT a field on EditorApp (keeps EditorApp
/// framework-agnostic). The Host owns both and passes &mut EditorApp into handle_window_event.
pub struct WinitEventBridge {
    pub modifiers: Modifiers,
    cursor_phys_x: f64,
    cursor_phys_y: f64,
    pub scale_factor: f64,
    /// Logical-pixel origin of the canvas widget within the window. Pushed by
    /// the host each frame (the canvas sits below the toolbar, so its origin is
    /// not (0, 0)). Until set, coord-bearing pointer events are dropped.
    canvas_origin: Option<(f64, f64)>,
}

impl WinitEventBridge {
    pub fn new() -> Self {
        Self {
            modifiers: Modifiers::default(),
            cursor_phys_x: 0.0,
            cursor_phys_y: 0.0,
            scale_factor: 1.0,
            canvas_origin: None,
        }
    }

    pub fn set_scale_factor(&mut self, sf: f64) {
        self.scale_factor = sf;
    }

    pub fn set_cursor(&mut self, x: f64, y: f64) {
        self.cursor_phys_x = x;
        self.cursor_phys_y = y;
    }

    /// Inform the bridge of the canvas widget's logical-pixel origin within the
    /// window. The host's canvas/render callback should call this every frame.
    /// Without it, pointer events are silently dropped.
    pub fn set_canvas_origin(&mut self, logical_x: f64, logical_y: f64) {
        self.canvas_origin = Some((logical_x, logical_y));
    }

    /// Last cursor position in canvas-local logical pixels, or `None` if the
    /// canvas origin has not been set yet.
    fn canvas_local_cursor(&self) -> Option<(f64, f64)> {
        let (ox, oy) = self.canvas_origin?;
        let lx = self.cursor_phys_x / self.scale_factor;
        let ly = self.cursor_phys_y / self.scale_factor;
        Some((lx - ox, ly - oy))
    }

    /// Translate a winit WindowEvent into a rofd ViewEvent (or None if not
    /// relevant). CursorMoved updates the stored cursor position; pointer
    /// events are emitted with canvas-local coordinates (and dropped until
    /// [`set_canvas_origin`](Self::set_canvas_origin) has been called).
    pub fn translate(&mut self, event: &winit::event::WindowEvent) -> Option<ViewEvent> {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_phys_x = position.x;
                self.cursor_phys_y = position.y;
                self.canvas_local_cursor()
                    .map(|(x, y)| ViewEvent::PointerMove { x, y })
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let btn = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return None,
                };
                let (x, y) = self.canvas_local_cursor()?;
                match state {
                    winit::event::ElementState::Pressed => Some(ViewEvent::PointerDown {
                        button: btn,
                        x,
                        y,
                        modifiers: self.modifiers,
                        // 真实多击计数由后续任务的 bridge 接上；先传 1。
                        click_count: 1,
                    }),
                    winit::event::ElementState::Released => {
                        Some(ViewEvent::PointerUp { button: btn, x, y })
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(lx, ly) => {
                        (*lx as f64 * 20.0, *ly as f64 * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
                };
                if self.modifiers.control {
                    let factor = if dy > 0.0 { 1.1 } else { 0.9 };
                    // ZoomAt anchors on the cursor so the content under it
                    // stays put. Falls back to plain Zoom (center-agnostic)
                    // when the canvas origin is unknown (cursor outside canvas
                    // or origin not yet pushed by the host).
                    if let Some((cx, cy)) = self.canvas_local_cursor() {
                        Some(ViewEvent::ZoomAt {
                            factor,
                            center: (cx, cy),
                        })
                    } else {
                        Some(ViewEvent::Zoom { factor })
                    }
                } else {
                    Some(ViewEvent::Scroll { dx, dy: -dy })
                }
            }
            WindowEvent::Resized(physical_size) => Some(ViewEvent::Resize {
                width: physical_size.width as f64 / self.scale_factor,
                height: physical_size.height as f64 / self.scale_factor,
            }),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != winit::event::ElementState::Pressed {
                    return None;
                }
                // PageUp/PageDown scroll by one page height (ScrollPage), not a
                // generic KeyDown: the component's handle_key doesn't act on
                // these keys, so routing them as ScrollPage gives them an effect
                // (viewport scroll by page_h + page_gap).
                if let Some(direction) = page_scroll_direction(&event.physical_key) {
                    return Some(ViewEvent::ScrollPage { direction });
                }
                let key = winit_key_to_rofd(&event.physical_key, &event.text);
                Some(ViewEvent::KeyDown {
                    key,
                    modifiers: self.modifiers,
                })
            }
            WindowEvent::Focused(focused) => {
                if *focused {
                    Some(ViewEvent::FocusGained)
                } else {
                    Some(ViewEvent::FocusLost)
                }
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
fn winit_key_to_rofd(
    key: &winit::keyboard::PhysicalKey,
    text: &Option<winit::keyboard::SmolStr>,
) -> Key {
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

/// If `key` is PageUp/PageDown, return the corresponding scroll direction
/// (used to emit `ViewEvent::ScrollPage` instead of a no-op `KeyDown`).
/// Returns `None` for all other keys.
fn page_scroll_direction(key: &winit::keyboard::PhysicalKey) -> Option<ScrollDirection> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    if let PhysicalKey::Code(code) = key {
        match code {
            KeyCode::PageUp => Some(ScrollDirection::Up),
            KeyCode::PageDown => Some(ScrollDirection::Down),
            _ => None,
        }
    } else {
        None
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
        bridge.set_canvas_origin(0.0, 0.0);
        let (x, y) = bridge.canvas_local_cursor().expect("origin set");
        assert_eq!(x, 50.0);
        assert_eq!(y, 100.0);
    }

    #[test]
    fn canvas_local_default_scale_1() {
        let mut bridge = WinitEventBridge::new();
        bridge.set_cursor(10.0, 20.0);
        bridge.set_canvas_origin(0.0, 0.0);
        let (x, y) = bridge.canvas_local_cursor().expect("origin set");
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn canvas_local_subtracts_origin() {
        let mut bridge = WinitEventBridge::new();
        bridge.set_cursor(100.0, 200.0);
        bridge.set_canvas_origin(10.0, 20.0);
        let (x, y) = bridge.canvas_local_cursor().expect("origin set");
        assert_eq!(x, 90.0);
        assert_eq!(y, 180.0);
    }

    #[test]
    fn canvas_local_none_until_origin_set() {
        let mut bridge = WinitEventBridge::new();
        bridge.set_cursor(100.0, 200.0);
        assert!(
            bridge.canvas_local_cursor().is_none(),
            "dropped before canvas origin set"
        );
        bridge.set_canvas_origin(0.0, 0.0);
        assert!(bridge.canvas_local_cursor().is_some());
    }

    #[test]
    fn update_modifiers_maps_correctly() {
        let mut bridge = WinitEventBridge::new();
        let state = winit::keyboard::ModifiersState::CONTROL;
        bridge.update_modifiers(&state);
        assert!(bridge.modifiers.control);
        assert!(!bridge.modifiers.shift);
    }

    #[test]
    fn page_scroll_direction_maps_page_up_down() {
        use winit::keyboard::{KeyCode, PhysicalKey};
        assert_eq!(
            page_scroll_direction(&PhysicalKey::Code(KeyCode::PageUp)),
            Some(ScrollDirection::Up)
        );
        assert_eq!(
            page_scroll_direction(&PhysicalKey::Code(KeyCode::PageDown)),
            Some(ScrollDirection::Down)
        );
        // Non-paging keys return None (fall through to KeyDown).
        assert_eq!(
            page_scroll_direction(&PhysicalKey::Code(KeyCode::ArrowUp)),
            None
        );
        assert_eq!(
            page_scroll_direction(&PhysicalKey::Code(KeyCode::Enter)),
            None
        );
    }

    #[test]
    fn ctrl_wheel_emits_zoomat_at_cursor() {
        use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};
        let mut bridge = WinitEventBridge::new();
        // Ctrl held + cursor at logical (30, 40) with canvas origin (0,0).
        bridge.update_modifiers(&winit::keyboard::ModifiersState::CONTROL);
        bridge.set_cursor(30.0, 40.0);
        bridge.set_canvas_origin(0.0, 0.0);

        let ev = WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.0), // scroll up
            phase: TouchPhase::Moved,
        };
        match bridge.translate(&ev) {
            Some(ViewEvent::ZoomAt { factor, center }) => {
                assert!(factor > 1.0, "scroll up zooms in");
                assert_eq!(center, (30.0, 40.0));
            }
            other => panic!("expected ZoomAt, got {:?}", other),
        }
    }

    #[test]
    fn wheel_without_ctrl_emits_scroll() {
        use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};
        let mut bridge = WinitEventBridge::new();
        // No modifiers -> plain Scroll.
        bridge.set_cursor(30.0, 40.0);
        bridge.set_canvas_origin(0.0, 0.0);

        let ev = WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: TouchPhase::Moved,
        };
        match bridge.translate(&ev) {
            Some(ViewEvent::Scroll { .. }) => {}
            other => panic!("expected Scroll without Ctrl, got {:?}", other),
        }
    }

    #[test]
    fn ctrl_wheel_without_canvas_origin_falls_back_to_zoom() {
        use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};
        let mut bridge = WinitEventBridge::new();
        bridge.update_modifiers(&winit::keyboard::ModifiersState::CONTROL);
        // Deliberately do NOT set_canvas_origin -> cursor local unknown.
        bridge.set_cursor(30.0, 40.0);

        let ev = WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, -1.0), // scroll down
            phase: TouchPhase::Moved,
        };
        match bridge.translate(&ev) {
            Some(ViewEvent::Zoom { factor }) => {
                assert!(factor < 1.0, "scroll down zooms out");
            }
            other => panic!("expected Zoom fallback, got {:?}", other),
        }
    }
}
