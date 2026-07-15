//! WasmEditor - wasm-bindgen surface for the rofd web editor.
//!
//! Mirrors reditor's `WasmEditor`: created via the `create_wasm_editor` factory
//! (WebGPU init + warmup), then the SDK registers fonts (`register_font`),
//! wires JS callbacks (`set_on_*`), and feeds DOM events (`handle_*`) from
//! `requestAnimationFrame` + event listeners.
//!
//! # Module gating
//! The `WasmEditor` struct + its `#[wasm_bindgen]` impl are gated behind
//! `cfg(target_arch = "wasm32")` because they use `wasm_bindgen`, `web_sys`,
//! and [`WebGpuRenderTarget`](crate::WebGpuRenderTarget) (wasm32-only).
//!
//! [`parse_key`] and its tests are NOT cfg-gated - pure Rust, run on native.

use rofd_component::{Key, Tool};
use rofd_dom::{AnnotationKind, ShapeKind};

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
        _ if s.chars().count() == 1 => Key::Char(s.chars().next().unwrap()),
        _ => Key::Unidentified,
    }
}

/// Map a JS-friendly tool-kind string to a [`Tool`]. Unknown strings fall
/// back to [`Tool::Select`] (safe default). Mirrors the native-app's seven
/// toolbar buttons: select / highlight / underline / strikeout / squiggly
/// / freehand / rect.
///
/// Pure Rust (no wasm types) so it runs under `cargo test` on native, like
/// [`parse_key`]. The WasmEditor's `setTool` method calls this.
pub fn parse_tool_kind(kind: &str) -> Tool {
    match kind {
        "select" => Tool::Select,
        "highlight" => Tool::Create(AnnotationKind::Highlight),
        "underline" => Tool::Create(AnnotationKind::Underline),
        "strikeout" => Tool::Create(AnnotationKind::Strikeout),
        "squiggly" => Tool::Create(AnnotationKind::Squiggly),
        "freehand" => Tool::Create(AnnotationKind::Freehand),
        "rect" => Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)),
        _ => Tool::Select,
    }
}

// ─── WasmEditor (wasm32 only) ────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::cell::RefCell;
    use std::rc::Rc;

    use rofd_component::{
        ContextTarget, EditorComponent, EditorConfig, Modifiers, MouseButton, ViewEvent,
    };
    use rofd_dom::{AnnotationId, AnnotationSelection, OfdDocument};
    use rofd_editor::TextCursor;
    use rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle};
    use wasm_bindgen::prelude::*;

    use crate::wasm_editor::{parse_key, parse_tool_kind};
    use crate::webgpu_render_target::WebGpuRenderTarget;

    /// JS callback slots. Each is an `Rc<RefCell<Option<Function>>>` so the
    /// Rust bridge closures (registered in [`WasmEditor::setup_bridge_callbacks`])
    /// can read the current slot, and JS can swap the callback via `set_on_*`
    /// at any time. Single-threaded (wasm32).
    #[derive(Default)]
    pub(crate) struct JsCallbacks {
        pub on_change: Rc<RefCell<Option<js_sys::Function>>>,
        pub on_selection_change: Rc<RefCell<Option<js_sys::Function>>>,
        pub on_cursor_change: Rc<RefCell<Option<js_sys::Function>>>,
        pub on_save_request: Rc<RefCell<Option<js_sys::Function>>>,
        pub on_context_menu: Rc<RefCell<Option<js_sys::Function>>>,
    }

    /// wasm-bindgen editor surface for the web.
    ///
    /// Owns an [`EditorComponent`] (model + render) and a [`WebGpuRenderTarget`]
    /// (canvas -> WebGPU -> vello). The SDK creates this via
    /// [`create_wasm_editor`](crate::create_wasm_editor), then registers fonts
    /// and JS callbacks, and feeds DOM events from listeners.
    #[wasm_bindgen]
    pub struct WasmEditor {
        component: EditorComponent,
        render_target: WebGpuRenderTarget,
        callbacks: JsCallbacks,
        package: Option<PackageHandle>,
    }

    #[wasm_bindgen]
    impl WasmEditor {
        /// Render one frame. Called by the JS SDK on each requestAnimationFrame.
        #[wasm_bindgen(js_name = renderFrame)]
        pub fn render_frame(&mut self) -> Result<(), JsValue> {
            self.component.render(&mut self.render_target);
            Ok(())
        }

        /// Register font data (raw bytes) with the editor. Call after
        /// `create_wasm_editor` to load fonts (e.g. NotoSansCJK) - the web can't
        /// access system fonts, so registered fonts are the only font source.
        /// Can be called multiple times. Returns `true` if the bytes parsed.
        #[wasm_bindgen(js_name = registerFont)]
        pub fn register_font(&mut self, bytes: &[u8]) -> bool {
            self.component.register_font_data(bytes.to_vec())
        }

        /// Handle viewport resize (device pixels). Reconfigures the WebGPU
        /// surface and updates the editor viewport.
        #[wasm_bindgen(js_name = handleResize)]
        pub fn handle_resize(&mut self, width: f64, height: f64) -> Result<(), JsValue> {
            self.render_target.resize(width as u32, height as u32);
            self.component
                .handle_event(&ViewEvent::Resize { width, height });
            Ok(())
        }

        // ─── JS Callback Registration ───────────────────────────────────────

        #[wasm_bindgen(js_name = setOnChange)]
        pub fn set_on_change(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_change.borrow_mut() = callback;
        }

        #[wasm_bindgen(js_name = setOnSelectionChange)]
        pub fn set_on_selection_change(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_selection_change.borrow_mut() = callback;
        }

        #[wasm_bindgen(js_name = setOnCursorChange)]
        pub fn set_on_cursor_change(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_cursor_change.borrow_mut() = callback;
        }

        #[wasm_bindgen(js_name = setOnSaveRequest)]
        pub fn set_on_save_request(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_save_request.borrow_mut() = callback;
        }

        /// Register the right-click context menu callback. JS receives
        /// `(x, y, annotationId)` where `annotationId` is `null` when the
        /// right-click hit a page body or the desk background (no annotation).
        #[wasm_bindgen(js_name = setOnContextMenu)]
        pub fn set_on_context_menu(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_context_menu.borrow_mut() = callback;
        }

        // ─── Event Handlers ─────────────────────────────────────────────────

        #[wasm_bindgen(js_name = handleKeyDown)]
        pub fn handle_key_down(
            &mut self,
            key: &str,
            shift: bool,
            ctrl: bool,
            alt: bool,
            meta: bool,
        ) -> Result<(), JsValue> {
            let modifiers = Modifiers {
                shift,
                control: ctrl,
                alt,
                meta,
            };
            self.component.handle_event(&ViewEvent::KeyDown {
                key: parse_key(key),
                modifiers,
            });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleMouseDown)]
        pub fn handle_mouse_down(
            &mut self,
            button: u32,
            x: f64,
            y: f64,
            shift: bool,
            ctrl: bool,
            alt: bool,
            meta: bool,
        ) -> Result<(), JsValue> {
            let modifiers = Modifiers {
                shift,
                control: ctrl,
                alt,
                meta,
            };
            self.component.handle_event(&ViewEvent::PointerDown {
                button: parse_mouse_button(button),
                x,
                y,
                modifiers,
            });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleMouseUp)]
        pub fn handle_mouse_up(
            &mut self,
            button: u32,
            x: f64,
            y: f64,
            _shift: bool,
            _ctrl: bool,
            _alt: bool,
            _meta: bool,
        ) -> Result<(), JsValue> {
            // rofd's PointerUp carries no modifiers (only Down does).
            self.component.handle_event(&ViewEvent::PointerUp {
                button: parse_mouse_button(button),
                x,
                y,
            });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleMouseMove)]
        pub fn handle_mouse_move(&mut self, x: f64, y: f64) -> Result<(), JsValue> {
            self.component
                .handle_event(&ViewEvent::PointerMove { x, y });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleMouseScroll)]
        pub fn handle_mouse_scroll(&mut self, dx: f64, dy: f64) -> Result<(), JsValue> {
            // Web `wheel` events use the OPPOSITE sign convention to winit:
            // `deltaY > 0` means the user scrolled DOWN, whereas winit's
            // `LineDelta` y > 0 means UP. `ViewEvent::Scroll`'s semantic is
            // "dy > 0 = scroll toward the bottom of the document" (see
            // `composite.rs`: `y = page_gap - scroll.1`). The native winit
            // bridge negates winit's dy to match; on the web we must NOT
            // negate, or the scroll direction is inverted. Mirrors reditor's
            // `handle_mouse_scroll` (no negation). dx is the same sign on both
            // platforms (positive = scroll right), so it passes through.
            self.component.handle_event(&ViewEvent::Scroll { dx, dy });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleZoom)]
        pub fn handle_zoom(&mut self, factor: f64) -> Result<(), JsValue> {
            self.component.handle_event(&ViewEvent::Zoom { factor });
            Ok(())
        }

        #[wasm_bindgen(js_name = handleFocusGained)]
        pub fn handle_focus_gained(&mut self) -> Result<(), JsValue> {
            self.component.handle_event(&ViewEvent::FocusGained);
            Ok(())
        }

        #[wasm_bindgen(js_name = handleFocusLost)]
        pub fn handle_focus_lost(&mut self) -> Result<(), JsValue> {
            self.component.handle_event(&ViewEvent::FocusLost);
            Ok(())
        }

        // ─── Document I/O ───────────────────────────────────────────────────

        /// Load an OFD document from raw `.ofd` package bytes. Retains the
        /// parsed `PackageHandle` so subsequent `saveOfd` calls can perform a
        /// surgical save (preserving unmodelled body bytes byte-for-byte).
        #[wasm_bindgen(js_name = loadOfd)]
        pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            let report =
                parse_ofd(bytes).map_err(|e| JsValue::from_str(&format!("parse failed: {e}")))?;
            self.package = Some(report.package);
            self.component.load_document(report.document);
            Ok(())
        }

        /// Serialize the current document to OFD package bytes. Surgical save
        /// (preserves unmodelled body byte-for-byte) when a package was loaded;
        /// full write otherwise.
        #[wasm_bindgen(js_name = saveOfd)]
        pub fn save_ofd(&self) -> Result<Vec<u8>, JsValue> {
            match &self.package {
                Some(pkg) => save_ofd(self.component.document(), pkg),
                None => write_ofd(self.component.document()),
            }
            .map_err(|e| JsValue::from_str(&format!("save failed: {e}")))
        }

        /// Whether there are undoable operations in the history.
        #[wasm_bindgen(js_name = canUndo)]
        pub fn can_undo(&self) -> bool {
            self.component.can_undo()
        }

        /// Whether there are redoable operations in the history.
        #[wasm_bindgen(js_name = canRedo)]
        pub fn can_redo(&self) -> bool {
            self.component.can_redo()
        }

        /// Set the annotation clock (author + timestamp) for subsequent edits.
        #[wasm_bindgen(js_name = setClock)]
        pub fn set_clock(&mut self, author: String, ts: i64) {
            self.component.set_clock(author, ts);
        }

        /// Set the active editing tool. `kind` is a JS-friendly string:
        /// `"select"` | `"highlight"` | `"underline"` | `"strikeout"` |
        /// `"squiggly"` | `"freehand"` | `"rect"`. Unknown strings fall back
        /// to `Select` (safe default). Mirrors the native-app's toolbar buttons.
        #[wasm_bindgen(js_name = setTool)]
        pub fn set_tool(&mut self, kind: &str) {
            let tool = parse_tool_kind(kind);
            self.component.set_tool(tool);
        }

        /// Delete the annotation with the given id string. Used by the
        /// right-click context menu's "Delete" action. Returns `false` if no
        /// annotation with that id exists (the editor's `delete_annotation` is
        /// itself a no-op in that case, but this gives JS a success signal).
        #[wasm_bindgen(js_name = deleteAnnotation)]
        pub fn delete_annotation(&mut self, id: &str) -> bool {
            let id = AnnotationId::new(id);
            let exists = self.component.document().annotations.find(&id).is_some();
            if !exists {
                return false;
            }
            self.component.delete_annotation(&id);
            true
        }

        /// Delete all currently-selected annotations. Returns the count
        /// deleted. Mirrors the Delete-key path (handled in `handle_event`)
        /// but exposed for programmatic use (e.g. a toolbar "Delete" button).
        #[wasm_bindgen(js_name = deleteSelected)]
        pub fn delete_selected(&mut self) -> u32 {
            let ids: Vec<AnnotationId> = match self.component.selection() {
                AnnotationSelection::None => vec![],
                AnnotationSelection::Single(id) => vec![id.clone()],
                AnnotationSelection::Multi(ids) => ids.clone(),
            };
            let count = ids.len() as u32;
            for id in &ids {
                self.component.delete_annotation(id);
            }
            count
        }
    }

    impl WasmEditor {
        /// Internal constructor. Called by `create_wasm_editor` after WebGPU
        /// init + warmup. Wires the component's Rust callbacks to the JS
        /// callback slots.
        pub(crate) fn new_internal(
            width: u32,
            height: u32,
            render_target: WebGpuRenderTarget,
        ) -> Result<Self, JsValue> {
            let config = EditorConfig::new(std::sync::Arc::new(vec![]));
            let mut component = EditorComponent::new(config);
            // Seed the viewport to the canvas size so the first frame isn't
            // zero-sized (the SDK also calls handleResize after layout).
            component.handle_event(&ViewEvent::Resize {
                width: width as f64,
                height: height as f64,
            });

            let mut editor = Self {
                component,
                render_target,
                callbacks: JsCallbacks::default(),
                package: None,
            };
            editor.setup_bridge_callbacks();
            Ok(editor)
        }

        /// Wire the component's Rust callbacks to the JS callback slots. JS
        /// sets the actual functions via `set_on_*`; the Rust closures read the
        /// slots and invoke the JS function (if set) when the component fires.
        /// Callbacks are signal-only (no payload) - the SDK's render loop
        /// re-renders every frame, so callbacks just notify the app of state
        /// changes (e.g. to update a "modified" indicator).
        fn setup_bridge_callbacks(&mut self) {
            let on_change_js = self.callbacks.on_change.clone();
            self.component
                .on_change(Box::new(move |_doc: &OfdDocument| {
                    call_js0(&on_change_js);
                }));

            let on_selection_change_js = self.callbacks.on_selection_change.clone();
            self.component
                .on_selection_change(Box::new(move |_sel: &AnnotationSelection| {
                    call_js0(&on_selection_change_js);
                }));

            let on_cursor_change_js = self.callbacks.on_cursor_change.clone();
            self.component
                .on_cursor_change(Box::new(move |_cur: Option<&TextCursor>| {
                    call_js0(&on_cursor_change_js);
                }));

            let on_save_request_js = self.callbacks.on_save_request.clone();
            self.component.on_save_request(Box::new(move || {
                call_js0(&on_save_request_js);
            }));

            let on_context_menu_js = self.callbacks.on_context_menu.clone();
            self.component.on_context_menu(Box::new(
                move |point: (f64, f64), target: ContextTarget| {
                    let id_str: Option<String> = match &target {
                        ContextTarget::Annotation(id) => Some(id.0.clone()),
                        ContextTarget::Page | ContextTarget::Empty => None,
                    };
                    call_js3(&on_context_menu_js, point.0, point.1, id_str);
                },
            ));
        }
    }

    /// Invoke a JS callback slot with no arguments (no-op if the slot is empty).
    fn call_js0(slot: &Rc<RefCell<Option<js_sys::Function>>>) {
        if let Some(ref js_fn) = *slot.borrow() {
            let _ = js_fn.call0(&JsValue::null());
        }
    }

    /// Invoke a JS callback slot with three arguments: two f64s (the
    /// right-click point) and an `Option<String>` (the annotation id, or null
    /// for Page/Empty context targets). No-op if the slot is empty.
    fn call_js3(slot: &Rc<RefCell<Option<js_sys::Function>>>, x: f64, y: f64, id: Option<String>) {
        if let Some(ref js_fn) = *slot.borrow() {
            let id_val = match id {
                Some(s) => JsValue::from_str(&s),
                None => JsValue::NULL,
            };
            let _ = js_fn.call3(
                &JsValue::null(),
                &JsValue::from_f64(x),
                &JsValue::from_f64(y),
                &id_val,
            );
        }
    }

    fn parse_mouse_button(button: u32) -> MouseButton {
        match button {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => MouseButton::Left,
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

    #[test]
    fn parse_tool_kind_select() {
        assert_eq!(parse_tool_kind("select"), Tool::Select);
    }

    #[test]
    fn parse_tool_kind_markup_variants() {
        assert_eq!(
            parse_tool_kind("highlight"),
            Tool::Create(AnnotationKind::Highlight)
        );
        assert_eq!(
            parse_tool_kind("underline"),
            Tool::Create(AnnotationKind::Underline)
        );
        assert_eq!(
            parse_tool_kind("strikeout"),
            Tool::Create(AnnotationKind::Strikeout)
        );
        assert_eq!(
            parse_tool_kind("squiggly"),
            Tool::Create(AnnotationKind::Squiggly)
        );
    }

    #[test]
    fn parse_tool_kind_freehand_and_rect() {
        assert_eq!(
            parse_tool_kind("freehand"),
            Tool::Create(AnnotationKind::Freehand)
        );
        assert_eq!(
            parse_tool_kind("rect"),
            Tool::Create(AnnotationKind::Shape(ShapeKind::Rect))
        );
    }

    #[test]
    fn parse_tool_kind_unknown_falls_back_to_select() {
        assert_eq!(parse_tool_kind("unknown"), Tool::Select);
        assert_eq!(parse_tool_kind(""), Tool::Select);
        assert_eq!(parse_tool_kind("SELECT"), Tool::Select); // case-sensitive
    }
}
