//! WasmEditor - wasm-bindgen surface for the rofd web editor.
//!
//! Wraps [`EditorComponent`] + [`WebGpuRenderTarget`] and exposes simple
//! methods (`handle_keydown`, `handle_pointerdown`, `render`, `load_ofd`,
//! `save_ofd`, ...) that JS calls from DOM event listeners.
//!
//! # Module gating
//! The `WasmEditor` struct + its `#[wasm_bindgen]` impl are gated behind
//! `cfg(target_arch = "wasm32")` because they use `wasm_bindgen`, `web_sys`,
//! and [`WebGpuRenderTarget`] (which itself is wasm32-only).
//!
//! [`parse_key`] and its tests are **not** cfg-gated - they are pure Rust and
//! run on native (`cargo test -p rofd-web-view`), giving TDD coverage without
//! needing a browser.

use rofd_component::Key;

// ─── parse_key (native + wasm) ───────────────────────────────────────────────

/// Convert a JS `KeyboardEvent.key` string into a rofd [`Key`].
///
/// Named keys (`"Enter"`, `"ArrowLeft"`, ...) map directly; single-character
/// strings map to [`Key::Char`]; everything else is [`Key::Unidentified`].
///
/// This function is pure Rust (no wasm types) so it runs under `cargo test`
/// on native. The WasmEditor methods call it to translate JS key strings.
pub fn parse_key(s: &str) -> Key {
    match s {
        "Enter" => Key::Enter,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Tab" => Key::Tab,
        "Escape" => Key::Escape,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        // Single-character keys (letters, digits, punctuation) -> Char.
        _ if s.len() == 1 => Key::Char(s.chars().next().unwrap()),
        _ => Key::Unidentified,
    }
}

// ─── WasmEditor (wasm32 only) ────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::sync::Arc;

    use rofd_component::{
        EditorComponent, EditorConfig, Modifiers, MouseButton, ViewEvent,
    };
    use rofd_io::{parse_ofd, write_ofd};
    use wasm_bindgen::prelude::*;

    use crate::webgpu_render_target::WebGpuRenderTarget;
    use crate::wasm_editor::parse_key;

    /// wasm-bindgen editor surface for the web.
    ///
    /// Owns an [`EditorComponent`] (model + render) and a [`WebGpuRenderTarget`]
    /// (canvas -> WebGPU -> vello). JS creates this via [`WasmEditor::new`],
    /// then calls the `handle_*` / `render` / `load_ofd` / `save_ofd` methods
    /// from DOM event listeners.
    #[wasm_bindgen]
    pub struct WasmEditor {
        component: EditorComponent,
        render_target: WebGpuRenderTarget,
    }

    #[wasm_bindgen]
    impl WasmEditor {
        /// Create a new editor bound to a canvas element.
        ///
        /// Async because [`WebGpuRenderTarget::new`] requests a wgpu adapter +
        /// device, which is async on wasm32 (browser WebGPU Promises).
        ///
        // `#[allow(deprecated)]`: wasm-bindgen warns that async constructors
        // produce invalid TS code, but the plan specifies this pattern and the
        // TS SDK (Task 4) calls `WasmEditor.new(canvas, fontBytes)` which works
        // with the constructor export.
        #[wasm_bindgen(constructor)]
        #[allow(deprecated)]
        pub async fn new(
            canvas: web_sys::HtmlCanvasElement,
            font_bytes: Vec<u8>,
        ) -> Result<WasmEditor, JsValue> {
            let config = EditorConfig::new(Arc::new(font_bytes));
            let component = EditorComponent::new(config);
            let width = canvas.client_width() as u32;
            let height = canvas.client_height() as u32;
            let render_target = WebGpuRenderTarget::new(canvas, width, height)
                .await
                .map_err(|e| JsValue::from_str(&e))?;
            Ok(Self {
                component,
                render_target,
            })
        }

        /// Handle a keydown event. Returns `true` if the editor needs a repaint.
        ///
        /// `key` is `KeyboardEvent.key` (e.g. `"Enter"`, `"a"`, `"ArrowLeft"`).
        /// `ctrl`/`shift` reflect the modifier state at event time.
        #[wasm_bindgen(js_name = handle_keydown)]
        pub fn handle_keydown(&mut self, key: &str, ctrl: bool, shift: bool) -> bool {
            let key = parse_key(key);
            let modifiers = Modifiers {
                control: ctrl,
                shift,
                ..Default::default()
            };
            self.dispatch(ViewEvent::KeyDown { key, modifiers })
        }

        /// Handle a pointerdown event. Returns `true` if repaint is needed.
        ///
        /// `button` follows the JS `MouseEvent.button` convention:
        /// 0 = left, 1 = middle, 2 = right.
        #[wasm_bindgen(js_name = handle_pointerdown)]
        pub fn handle_pointerdown(&mut self, x: f64, y: f64, button: u32) -> bool {
            let btn = match button {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                _ => return false,
            };
            self.dispatch(ViewEvent::PointerDown {
                button: btn,
                x,
                y,
                modifiers: Modifiers::default(),
            })
        }

        /// Handle a pointerup event. Returns `true` if repaint is needed.
        #[wasm_bindgen(js_name = handle_pointerup)]
        pub fn handle_pointerup(&mut self, x: f64, y: f64, button: u32) -> bool {
            let btn = match button {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                _ => return false,
            };
            self.dispatch(ViewEvent::PointerUp { button: btn, x, y })
        }

        /// Handle a pointermove event. Returns `true` if repaint is needed.
        #[wasm_bindgen(js_name = handle_pointermove)]
        pub fn handle_pointermove(&mut self, x: f64, y: f64) -> bool {
            self.dispatch(ViewEvent::PointerMove { x, y })
        }

        /// Handle a scroll/wheel event. Returns `true` if repaint is needed.
        ///
        /// `dx`/`dy` are in CSS pixels (from `WheelEvent.deltaX/deltaY`).
        #[wasm_bindgen(js_name = handle_scroll)]
        pub fn handle_scroll(&mut self, dx: f64, dy: f64) -> bool {
            self.dispatch(ViewEvent::Scroll { dx, dy })
        }

        /// Handle a canvas resize. Reconfigures the WebGPU surface and updates
        /// the editor viewport. Returns `true` if repaint is needed.
        #[wasm_bindgen(js_name = handle_resize)]
        pub fn handle_resize(&mut self, width: f64, height: f64) -> bool {
            self.render_target.resize(width as u32, height as u32);
            self.dispatch(ViewEvent::Resize { width, height })
        }

        /// Render the current editor state to the canvas via WebGPU + vello.
        ///
        /// JS should call this from a `requestAnimationFrame` loop whenever the
        /// editor needs repainting (any `handle_*` method returned `true`).
        pub fn render(&mut self) {
            self.component.render(&mut self.render_target);
        }

        /// Load an OFD document from raw `.ofd` package bytes.
        ///
        /// Parses the zip package via [`rofd_io::parse_ofd`] and loads the
        /// resulting [`rofd_dom::OfdDocument`] into the editor component.
        /// Replaces any previously loaded document.
        ///
        /// Note: `EditorComponent` does not have a `load_ofd` method (it has
        /// `load_document`). We call `parse_ofd` + `load_document` directly,
        /// matching the native-view's `EditorApp::load_ofd` pattern.
        #[wasm_bindgen(js_name = load_ofd)]
        pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            let report = parse_ofd(bytes).map_err(|e| JsValue::from_str(&format!("{e}")))?;
            self.component.load_document(report.document);
            Ok(())
        }

        /// Serialize the current document to OFD package bytes.
        ///
        /// Returns a `Uint8Array` (from `Vec<u8>`) that JS can wrap in a
        /// `Blob` for download.
        #[wasm_bindgen(js_name = save_ofd)]
        pub fn save_ofd(&self) -> Result<Vec<u8>, JsValue> {
            write_ofd(self.component.document()).map_err(|e| JsValue::from_str(&format!("{e}")))
        }

        /// Whether there are undoable operations in the history.
        #[wasm_bindgen(js_name = can_undo)]
        pub fn can_undo(&self) -> bool {
            self.component.can_undo()
        }

        /// Whether there are redoable operations in the history.
        #[wasm_bindgen(js_name = can_redo)]
        pub fn can_redo(&self) -> bool {
            self.component.can_redo()
        }

        /// Set the annotation clock (author + timestamp) for subsequent edits.
        #[wasm_bindgen(js_name = set_clock)]
        pub fn set_clock(&mut self, author: String, ts: i64) {
            self.component.set_clock(author, ts);
        }

        /// Dispatch a [`ViewEvent`] to the component and return whether a
        /// repaint is needed.
        fn dispatch(&mut self, event: ViewEvent) -> bool {
            self.component.handle_event(&event).needs_repaint
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_impl::WasmEditor;

// ─── parse_key tests (native) ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_single_char() {
        assert_eq!(parse_key("a"), Key::Char('a'));
        assert_eq!(parse_key("Z"), Key::Char('Z'));
    }

    #[test]
    fn parse_key_named() {
        assert_eq!(parse_key("Enter"), Key::Enter);
        assert_eq!(parse_key("Backspace"), Key::Backspace);
        assert_eq!(parse_key("ArrowLeft"), Key::ArrowLeft);
        assert_eq!(parse_key("Escape"), Key::Escape);
    }

    #[test]
    fn parse_key_unknown() {
        assert_eq!(parse_key("F1"), Key::Unidentified);
        assert_eq!(parse_key(""), Key::Unidentified);
    }
}
