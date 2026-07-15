#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Unidentified,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventOutcome {
    pub needs_repaint: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewEvent {
    PointerDown {
        button: MouseButton,
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    PointerMove {
        x: f64,
        y: f64,
    },
    PointerUp {
        button: MouseButton,
        x: f64,
        y: f64,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },
    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
    Scroll {
        dx: f64,
        dy: f64,
    },
    Zoom {
        factor: f64,
    },
    Resize {
        width: f64,
        height: f64,
    },
    FocusGained,
    FocusLost,
    /// Scroll by one page height (Up/Down). The delta is
    /// `page_h * zoom + page_gap` in viewport pixels.
    ScrollPage {
        direction: ScrollDirection,
    },
    /// Zoom by `factor` while keeping the `center` point (viewport px)
    /// anchored to the same document position. Adjusts `scroll` so the
    /// content under the cursor stays put.
    ZoomAt {
        factor: f64,
        center: (f64, f64),
    },
    /// IME composition commit: insert `text` at the text cursor (multi-char).
    /// Falls through as a no-op when no text cursor is set.
    Ime {
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_default_all_false() {
        let m = Modifiers::default();
        assert!(!m.shift && !m.control && !m.alt && !m.meta);
    }

    #[test]
    fn event_outcome_needs_repaint() {
        let o = EventOutcome {
            needs_repaint: true,
        };
        assert!(o.needs_repaint);
    }

    #[test]
    fn view_event_pointer_down_constructs() {
        let e = ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 10.0,
            y: 20.0,
            modifiers: Modifiers::default(),
        };
        assert!(matches!(
            e,
            ViewEvent::PointerDown {
                button: MouseButton::Left,
                x: 10.0,
                y: 20.0,
                ..
            }
        ));
    }
}
