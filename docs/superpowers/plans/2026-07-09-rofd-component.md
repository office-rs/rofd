# rofd Phase 4a (rofd-component) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-component` - the `EditorComponent` facade that wires `rofd-render` + `rofd-editor` into a single integration entry point: event routing (ViewEvent -> hit_test -> editor commands), per-page dirty cache invalidation, callback firing (on_change/on_selection_change/on_cursor_change/on_save_request), and render (composite -> RenderTarget). Pure logic, no platform deps (winit/wgpu are Phase 4b).

**Architecture:** `EditorComponent` owns an `Editor` + `RenderEngine` + `PageSceneCache` + `Viewport` + `Callbacks` + config. `handle_event(&ViewEvent) -> EventOutcome { needs_repaint }` routes: PointerDown -> `hit_test` -> `editor.select`/`set_cursor`; KeyDown -> text editing (`editor.insert_text`/`delete_text`) / undo/redo / delete_selected / save_request; Scroll/Zoom/Resize -> viewport. After any annotation-mutating command, invalidate all pages' cache + fire `on_change`. `render(&mut dyn RenderTarget)` calls `RenderEngine::composite` + `target.draw_scene`. The component is platform-agnostic (no winit/wgpu/xilem); the Phase 4b native-view provides the `RenderTarget` impl + event translation.

**Tech Stack:** Rust 2021. New crate `rofd-component` deps: `rofd-dom`, `rofd-render`, `rofd-editor`, `vello` (for the `Scene` type in `RenderTarget`). No winit/wgpu/xilem (Phase 4b).

## Global Constraints

Copied from spec §6 + Phase 1-3 carryover; every task implicitly includes these.

- **`rofd-component` deps = `rofd-dom` + `rofd-render` + `rofd-editor` + `vello` only.** No winit/wgpu/xilem/imaging (those are Phase 4b native-view). The `RenderTarget` trait takes `&vello::Scene` directly (no imaging IR).
- **EditorComponent is the sole integration entry.** Hosts (Phase 4b) import only `rofd-component` (like `<textarea>`). Reaching past it into editor/render directly is a bug.
- **Component calls only Editor's PUBLIC commands** (create/delete/move/resize/style/text/reply/undo/redo/select/set_cursor/clear_*). `execute_transaction` is `pub(crate)` on Editor - the component must NOT call it.
- **Callbacks are target-gated `Send`** (`Box<dyn Fn>` + `Send` on native, no `Send` on wasm - `#[cfg]`-gated). The component fires `on_change(&OfdDocument)` / `on_selection_change(&AnnotationSelection)` / `on_cursor_change(Option<&TextCursor>)` / `on_save_request()` after the relevant state changes.
- **Construction is NOT target-gated** (deviation from spec §6.1). rofd's `RenderEngine` is target-agnostic (builds `FontStore` from resources + default bytes on both targets). The target difference lives in the Phase 4b `RenderTarget` impl (VelloRenderTarget native vs WebGpuRenderTarget wasm), not the component. Use `new(config)`.
- **Cache invalidation: brute-force all pages** (v1). After any annotation-mutating command, invalidate every page's annotation scene (`cache.invalidate(&page.id)` for all pages). Optimize to affected-pages-only later.
- **`Viewport` is device pixels** (`scroll: (f64,f64)`, `zoom: f64`, `size: (f64,f64)`, `page_gap: f64`).
- **Commits:** conventional commits, NO Co-Authored-By attribution line.
- **TDD:** red -> green -> commit. Gate: `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green.

---

## File Structure

```
rofd/
├── Cargo.toml                      # add crates/component to members + rofd-component workspace dep
├── crates/
│   └── component/                  # NEW crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # re-exports
│           ├── editor_component.rs # EditorComponent struct + new + load/new + render + handle_event + helpers
│           ├── event.rs            # ViewEvent + Key + Modifiers + MouseButton + EventOutcome
│           ├── render_target.rs    # RenderTarget trait
│           ├── callbacks.rs        # Callbacks struct + target-gated Send callback types
│           └── config.rs           # EditorConfig
└── (tests inline + crates/component/tests/integration.rs)
```

---

## Task 1: `rofd-component` scaffold

**Files:**
- Modify: `Cargo.toml` (workspace members + rofd-component dep)
- Create: `crates/component/Cargo.toml`, `crates/component/src/lib.rs`

**Interfaces:**
- Produces: empty `rofd-component` crate that compiles against rofd-dom + rofd-render + rofd-editor + vello.

- [ ] **Step 1: Add workspace member + dep**

Root `Cargo.toml` - add `crates/component` to members and `rofd-component` to `[workspace.dependencies]`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io", "crates/render", "crates/editor", "crates/component"]

[workspace.dependencies]
# ... existing ...
rofd-component = { path = "crates/component" }
```

- [ ] **Step 2: Create `crates/component/Cargo.toml`**

```toml
[package]
name = "rofd-component"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-dom = { workspace = true }
rofd-render = { workspace = true }
rofd-editor = { workspace = true }
vello = { workspace = true }
```

- [ ] **Step 3: Create `crates/component/src/lib.rs`**

```rust
//! rofd-component - EditorComponent facade. The sole integration entry point.
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check -p rofd-component`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/component/Cargo.toml crates/component/src/lib.rs Cargo.lock
git commit -m "chore: scaffold rofd-component crate"
```

---

## Task 2: ViewEvent + Key + Modifiers + MouseButton + EventOutcome

**Files:**
- Create: `crates/component/src/event.rs`
- Modify: `crates/component/src/lib.rs`
- Test: inline

**Interfaces:**
- Produces: `ViewEvent` enum (PointerDown/Move/Up, KeyDown/Up, Scroll, Zoom, Resize, FocusGained/Lost), `Key` enum, `Modifiers` struct, `MouseButton` enum, `EventOutcome { needs_repaint: bool }`.

- [ ] **Step 1: Write the failing tests**

`crates/component/src/event.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton { Left, Right, Middle }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers { pub shift: bool, pub control: bool, pub alt: bool, pub meta: bool }

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Enter, Backspace, Delete, Tab, Escape,
    ArrowLeft, ArrowRight, ArrowUp, ArrowDown,
    Home, End, PageUp, PageDown,
    Unidentified,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventOutcome { pub needs_repaint: bool }

#[derive(Debug, Clone, PartialEq)]
pub enum ViewEvent {
    PointerDown { button: MouseButton, x: f64, y: f64, modifiers: Modifiers },
    PointerMove { x: f64, y: f64 },
    PointerUp { button: MouseButton, x: f64, y: f64 },
    KeyDown { key: Key, modifiers: Modifiers },
    KeyUp { key: Key, modifiers: Modifiers },
    Scroll { dx: f64, dy: f64 },
    Zoom { factor: f64 },
    Resize { width: f64, height: f64 },
    FocusGained,
    FocusLost,
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
        let o = EventOutcome { needs_repaint: true };
        assert!(o.needs_repaint);
    }

    #[test]
    fn view_event_pointer_down_constructs() {
        let e = ViewEvent::PointerDown { button: MouseButton::Left, x: 10.0, y: 20.0, modifiers: Modifiers::default() };
        assert!(matches!(e, ViewEvent::PointerDown { button: MouseButton::Left, x: 10.0, y: 20.0, .. }));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-component`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

`crates/component/src/lib.rs`:
```rust
//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod event;

pub use event::*;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-component`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/component/src
git commit -m "feat(component): ViewEvent + Key + Modifiers + MouseButton + EventOutcome"
```

---

## Task 3: RenderTarget trait

**Files:**
- Create: `crates/component/src/render_target.rs`
- Modify: `crates/component/src/lib.rs`
- Test: inline

**Interfaces:**
- Produces: `RenderTarget` trait (`draw_scene(&mut self, &vello::Scene)` + `size(&self) -> (f64, f64)`). Phase 4b's `VelloRenderTarget` (native) + `WebGpuRenderTarget` (wasm) impl this.

- [ ] **Step 1: Write the trait + a mock test**

`crates/component/src/render_target.rs`:
```rust
use vello::Scene;

/// Abstract render surface. The host (Phase 4b) implements this to blit a
/// `vello::Scene` to the GPU (native: wgpu surface; wasm: WebGPU canvas).
pub trait RenderTarget {
    fn draw_scene(&mut self, scene: &Scene);
    fn size(&self) -> (f64, f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRenderTarget { drawn: usize, w: f64, h: f64 }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _scene: &Scene) { self.drawn += 1; }
        fn size(&self) -> (f64, f64) { (self.w, self.h) }
    }

    #[test]
    fn mock_render_target_records_draws() {
        let mut rt = MockRenderTarget { drawn: 0, w: 800.0, h: 600.0 };
        let scene = Scene::new();
        rt.draw_scene(&scene);
        assert_eq!(rt.drawn, 1);
        assert_eq!(rt.size(), (800.0, 600.0));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-component render_target`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/component/src/lib.rs`:
```rust
pub mod render_target;
pub use render_target::RenderTarget;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-component`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/render_target.rs crates/component/src/lib.rs
git commit -m "feat(component): RenderTarget trait (draw_scene + size)"
```

---

## Task 4: Callbacks (target-gated Send)

**Files:**
- Create: `crates/component/src/callbacks.rs`
- Modify: `crates/component/src/lib.rs`
- Test: inline

**Interfaces:**
- Consumes: `rofd_dom::OfdDocument`, `rofd_editor::{AnnotationSelection, TextCursor}`.
- Produces: `Callbacks` struct (4 callbacks: on_change, on_selection_change, on_cursor_change, on_save_request). Target-gated `Send` (`+ Send` on native, not on wasm).

- [ ] **Step 1: Write the failing test**

`crates/component/src/callbacks.rs`:
```rust
use rofd_dom::OfdDocument;
use rofd_editor::{AnnotationSelection, TextCursor};

#[cfg(not(target_arch = "wasm32"))]
type SendBound = dyn Fn(&OfdDocument) + Send;
#[cfg(target_arch = "wasm32")]
type SendBound = dyn Fn(&OfdDocument);

// The 4 callback types. on_change passes &OfdDocument; on_selection_change passes
// &AnnotationSelection; on_cursor_change passes Option<&TextCursor>; on_save_request passes ().
#[derive(Default)]
pub struct Callbacks {
    pub on_change: Option<Box<dyn Fn(&OfdDocument)>>,
    pub on_selection_change: Option<Box<dyn Fn(&AnnotationSelection)>>,
    pub on_cursor_change: Option<Box<dyn Fn(Option<&TextCursor>)>>,
    pub on_save_request: Option<Box<dyn Fn()>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn on_change_fires() {
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = fired.clone();
        let mut cbs = Callbacks::default();
        cbs.on_change = Some(Box::new(move |_doc| { *fired_clone.lock().unwrap() = true; }));
        let doc = OfdDocument::default();
        (cbs.on_change.as_ref().unwrap())(&doc);
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn on_save_request_fires() {
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = fired.clone();
        let mut cbs = Callbacks::default();
        cbs.on_save_request = Some(Box::new(move || { *fired_clone.lock().unwrap() = true; }));
        (cbs.on_save_request.as_ref().unwrap())();
        assert!(*fired.lock().unwrap());
    }
}
```

> **Note for the implementer:** the `SendBound` type alias is defined but NOT used in the `Callbacks` struct (the struct uses `Box<dyn Fn(...)>` without `Send` for simplicity). The `Send` bound is needed when the host (Phase 4b native) requires `Send` callbacks. For v1, the component's callbacks are `Box<dyn Fn>` (not `Send`); Phase 4b can wrap them in a `Send` adapter or the component can be made `Send` by adding `+ Send` behind a cfg. If clippy or the Phase 4b host needs `Send`, add `+ Send` to the callback types behind `#[cfg(not(target_arch = "wasm32"))]`. Keep it simple for now - the `SendBound` alias documents the intent. **If `SendBound` is unused and clippy complains, remove it** (it's a documentation placeholder; the actual `+ Send` gating happens when Phase 4b needs it).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-component callbacks`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/component/src/lib.rs`:
```rust
pub mod callbacks;
pub use callbacks::Callbacks;
```

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p rofd-component`
Expected: PASS.
Run: `cargo clippy -p rofd-component -- -D warnings`
Expected: PASS (if `SendBound` is unused, remove it per the note).

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/callbacks.rs crates/component/src/lib.rs
git commit -m "feat(component): Callbacks (on_change/selection/cursor/save_request)"
```

---

## Task 5: EditorConfig + EditorComponent struct + new + load/new + render

**Files:**
- Create: `crates/component/src/config.rs`, `crates/component/src/editor_component.rs`
- Modify: `crates/component/src/lib.rs`
- Test: inline

**Interfaces:**
- Consumes: `rofd_editor::Editor`, `rofd_render::{RenderEngine, PageSceneCache, Viewport}`, `rofd_dom::OfdDocument`, `RenderTarget` (Task 3), `Callbacks` (Task 4).
- Produces: `EditorConfig { default_font_bytes: Arc<Vec<u8>>, page_gap: f64 }`; `EditorComponent` struct + `new(config)` + `load_document(doc)` + `new_document()` + `document()`/`selection()`/`text_cursor()`/`can_undo()`/`can_redo()`/`is_modified()` + `set_clock(author, ts)` + `render(&mut dyn RenderTarget)`.

- [ ] **Step 1: Write config.rs**

`crates/component/src/config.rs`:
```rust
use std::sync::Arc;

#[derive(Clone)]
pub struct EditorConfig {
    pub default_font_bytes: Arc<Vec<u8>>,
    pub page_gap: f64,
}

impl EditorConfig {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self { default_font_bytes, page_gap: 20.0 }
    }
}
```

- [ ] **Step 2: Write the failing test + EditorComponent**

`crates/component/src/editor_component.rs`:
```rust
use std::sync::Arc;

use rofd_dom::OfdDocument;
use rofd_editor::{Editor, AnnotationSelection, TextCursor};
use rofd_render::{RenderEngine, PageSceneCache, Viewport};

use crate::callbacks::Callbacks;
use crate::config::EditorConfig;
use crate::event::EventOutcome;
use crate::render_target::RenderTarget;

pub struct EditorComponent {
    pub(crate) editor: Editor,
    pub(crate) render: RenderEngine,
    pub(crate) cache: PageSceneCache,
    pub(crate) viewport: Viewport,
    pub(crate) callbacks: Callbacks,
    pub(crate) modified: bool,
}

impl EditorComponent {
    pub fn new(config: EditorConfig) -> Self {
        let page_gap = config.page_gap;
        Self {
            editor: Editor::new(),
            render: RenderEngine::new(config.default_font_bytes.clone()),
            cache: PageSceneCache::new(),
            viewport: Viewport { page_gap, ..Default::default() },
            callbacks: Callbacks::default(),
            modified: false,
        }
    }

    pub fn load_document(&mut self, doc: OfdDocument) {
        self.editor.load_document(doc);
        self.cache = PageSceneCache::new();
        self.modified = false;
    }

    pub fn new_document(&mut self) {
        self.editor.load_document(OfdDocument::default());
        self.cache = PageSceneCache::new();
        self.modified = false;
    }

    pub fn document(&self) -> &OfdDocument { self.editor.document() }
    pub fn selection(&self) -> &AnnotationSelection { self.editor.selection() }
    pub fn text_cursor(&self) -> Option<&TextCursor> { self.editor.text_cursor() }
    pub fn can_undo(&self) -> bool { self.editor.can_undo() }
    pub fn can_redo(&self) -> bool { self.editor.can_redo() }
    pub fn is_modified(&self) -> bool { self.modified }
    pub fn set_clock(&mut self, author: String, ts: i64) { self.editor.set_clock(author, ts); }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        let scene = self.render.composite(self.editor.document(), &self.viewport, &mut self.cache);
        target.draw_scene(&scene);
    }

    pub fn handle_event(&mut self, _event: &crate::event::ViewEvent) -> EventOutcome {
        // Task 6+ fills this in.
        EventOutcome { needs_repaint: false }
    }

    // Callback setters
    pub fn on_change(&mut self, cb: impl Fn(&OfdDocument) + 'static) { self.callbacks.on_change = Some(Box::new(cb)); }
    pub fn on_selection_change(&mut self, cb: impl Fn(&AnnotationSelection) + 'static) { self.callbacks.on_selection_change = Some(Box::new(cb)); }
    pub fn on_cursor_change(&mut self, cb: impl Fn(Option<&TextCursor>) + 'static) { self.callbacks.on_cursor_change = Some(Box::new(cb)); }
    pub fn on_save_request(&mut self, cb: impl Fn() + 'static) { self.callbacks.on_save_request = Some(Box::new(cb)); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_target::RenderTarget;
    use vello::Scene;

    struct MockRenderTarget { drawn: usize, w: f64, h: f64 }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _: &Scene) { self.drawn += 1; }
        fn size(&self) -> (f64, f64) { (self.w, self.h) }
    }

    #[test]
    fn new_constructs_with_defaults() {
        let c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        assert!(!c.is_modified());
        assert!(!c.can_undo());
    }

    #[test]
    fn render_draws_to_target() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let mut rt = MockRenderTarget { drawn: 0, w: 800.0, h: 600.0 };
        c.render(&mut rt);
        assert_eq!(rt.drawn, 1);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p rofd-component editor_component`
Expected: FAIL - modules not wired.

- [ ] **Step 4: Wire into lib.rs**

`crates/component/src/lib.rs`:
```rust
//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod config;
pub mod editor_component;
pub mod event;
pub mod render_target;

pub use callbacks::Callbacks;
pub use config::EditorConfig;
pub use editor_component::EditorComponent;
pub use event::*;
pub use render_target::RenderTarget;
```

- [ ] **Step 5: Run tests + clippy**

Run: `cargo test -p rofd-component`
Expected: PASS.
Run: `cargo clippy -p rofd-component -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/component/src
git commit -m "feat(component): EditorComponent struct + new + load/new + render"
```

---

## Task 6: handle_event - pointer/scroll/zoom/resize + cache/callback helpers

**Files:**
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: `rofd_render::hit_test`/`HitTarget`, `rofd_dom::{AnnotationId, AnnotationPayload, PageId}`.
- Produces: `EditorComponent::handle_event` routing for PointerDown/Move/Up, Scroll, Zoom, Resize, Focus. Plus `after_annotation_change()` (invalidate all pages + fire on_change), `fire_selection_change()`, `fire_cursor_change()`, `fire_save_request()`, `text_content_len(&AnnotationId) -> Option<usize>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/component/src/editor_component.rs` test module:
```rust
    use crate::event::{ViewEvent, MouseButton, Modifiers, Key};
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
    use std::sync::{Arc, Mutex};

    fn component_with_note() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        c.editor.create_annotation(
            AnnotationKind::Note, PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 },
                color: Color::Rgb(0, 0, 0), content: "hi".into(), icon: NoteIcon::Note,
            },
        );
        // The create_annotation above bypasses handle_event (direct editor call for test setup).
        // Invalidate cache to stay consistent.
        for p in &c.editor.document().pages.clone() { c.cache.invalidate(&p.id); }
        c.viewport = rofd_render::Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };
        c
    }

    #[test]
    fn scroll_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.scroll, (10.0, 20.0));
    }

    #[test]
    fn zoom_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Zoom { factor: 2.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.zoom, 2.0);
    }

    #[test]
    fn resize_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Resize { width: 1024.0, height: 768.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.size, (1024.0, 768.0));
    }

    #[test]
    fn on_change_fires_after_scroll_callback_set() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = component_with_note();
        c.on_change(move |_| { *f.lock().unwrap() = true; });
        // Scroll doesn't change annotations -> no on_change. But it does need_repaint.
        c.handle_event(&ViewEvent::Scroll { dx: 1.0, dy: 0.0 });
        assert!(!*fired.lock().unwrap(), "scroll does not fire on_change");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-component scroll_updates_viewport`
Expected: FAIL - handle_event returns default (needs_repaint: false).

- [ ] **Step 3: Implement handle_event for pointer/scroll/zoom/resize + helpers**

Replace the stub `handle_event` in `crates/component/src/editor_component.rs` with:
```rust
    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::ViewEvent;
        match event {
            ViewEvent::PointerDown { button: crate::event::MouseButton::Left, x, y, .. } => {
                let target = rofd_render::hit_test(self.editor.document(), &self.viewport, (*x, *y));
                match target {
                    rofd_render::HitTarget::Annotation(id) => {
                        self.editor.select(id.clone());
                        if let Some(len) = self.text_content_len(&id) {
                            self.editor.set_cursor(id.clone(), len);
                        }
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome { needs_repaint: true }
                    }
                    _ => {
                        self.editor.clear_selection();
                        self.editor.clear_cursor();
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome { needs_repaint: true }
                    }
                }
            }
            ViewEvent::Scroll { dx, dy } => {
                self.viewport.scroll.0 += dx; self.viewport.scroll.1 += dy;
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::Zoom { factor } => {
                self.viewport.zoom *= factor;
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::Resize { width, height } => {
                self.viewport.size = (*width, *height);
                EventOutcome { needs_repaint: true }
            }
            _ => EventOutcome { needs_repaint: false },
        }
    }

    fn text_content_len(&self, id: &rofd_dom::AnnotationId) -> Option<usize> {
        let ann = self.editor.document().annotations.find(id)?;
        use rofd_dom::AnnotationPayload;
        match &ann.payload {
            AnnotationPayload::Note { content, .. } | AnnotationPayload::TextBox { content, .. } | AnnotationPayload::Watermark { content, .. } => Some(content.chars().count()),
            _ => None,
        }
    }

    fn after_annotation_change(&mut self) {
        self.modified = true;
        let pages: Vec<rofd_dom::PageId> = self.editor.document().pages.iter().map(|p| p.id.clone()).collect();
        for pid in &pages { self.cache.invalidate(pid); }
        if let Some(cb) = &self.callbacks.on_change { cb(self.editor.document()); }
    }

    fn fire_selection_change(&self) {
        if let Some(cb) = &self.callbacks.on_selection_change { cb(self.editor.selection()); }
    }

    fn fire_cursor_change(&self) {
        if let Some(cb) = &self.callbacks.on_cursor_change { cb(self.editor.text_cursor()); }
    }

    fn fire_save_request(&self) {
        if let Some(cb) = &self.callbacks.on_save_request { cb(); }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-component`
Expected: PASS (scroll/zoom/resize + on_change-not-on-scroll tests green).

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/editor_component.rs
git commit -m "feat(component): handle_event pointer/scroll/zoom/resize + cache/callback helpers"
```

---

## Task 7: handle_event - key routing (text editing + undo/redo + delete + save)

**Files:**
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: `Key`, `Modifiers`, `Editor` commands (insert_text/delete_text/undo/redo/delete_selected/set_cursor/clear_cursor/clear_selection).
- Produces: `handle_key` routing for KeyDown (Ctrl+Z/Y undo/redo, Ctrl+S save, Char/Backspace/Delete text editing, ArrowLeft/Right cursor, Escape clear, Delete/Backspace delete_selected).

- [ ] **Step 1: Write the failing tests**

Append to the test module:
```rust
    #[test]
    fn undo_redo_via_keydown() {
        let mut c = component_with_note();
        // undo the create_annotation (done via direct editor call in setup)
        let outcome = c.handle_event(&ViewEvent::KeyDown { key: Key::Char('z'), modifiers: Modifiers { control: true, ..Default::default() } });
        assert!(outcome.needs_repaint);
        assert!(!c.can_undo(), "undo consumed the create");
        // redo
        let outcome = c.handle_event(&ViewEvent::KeyDown { key: Key::Char('y'), modifiers: Modifiers { control: true, ..Default::default() } });
        assert!(outcome.needs_repaint);
        assert!(c.can_undo(), "redo restored it");
    }

    #[test]
    fn ctrl_s_fires_save_request() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_save_request(move || { *f.lock().unwrap() = true; });
        c.handle_event(&ViewEvent::KeyDown { key: Key::Char('s'), modifiers: Modifiers { control: true, ..Default::default() } });
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn char_key_inserts_text_when_cursor_set() {
        let mut c = component_with_note();
        // Select the note (which sets cursor to end via PointerDown on the annotation).
        // For test setup, directly set cursor.
        let id = c.editor.selection().clone();
        if let rofd_editor::AnnotationSelection::Single(id) = id {
            c.editor.set_cursor(id.clone(), 2); // "hi" has 2 chars; cursor at end
        }
        c.handle_event(&ViewEvent::KeyDown { key: Key::Char('!'), modifiers: Modifiers::default() });
        // The note's content should now be "hi!"
        let sel = c.editor.selection().clone();
        if let rofd_editor::AnnotationSelection::Single(id) = sel {
            let ann = c.editor.document().annotations.find(&id).unwrap();
            assert!(matches!(&ann.payload, rofd_dom::AnnotationPayload::Note { content, .. } if content == "hi!"));
        } else { panic!("expected single selection"); }
    }

    #[test]
    fn escape_clears_selection() {
        let mut c = component_with_note();
        assert!(!matches!(c.editor.selection(), rofd_editor::AnnotationSelection::None));
        c.handle_event(&ViewEvent::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(matches!(c.editor.selection(), rofd_editor::AnnotationSelection::None));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-component undo_redo_via_keydown`
Expected: FAIL - key routing not implemented (handle_event returns false for KeyDown).

- [ ] **Step 3: Implement handle_key**

Add a `handle_key` method to `EditorComponent` and wire it into `handle_event`'s KeyDown arm:
```rust
    // In handle_event, replace the `_ => ...` catch-all's handling of KeyDown by adding
    // a KeyDown arm BEFORE the `_` catch-all:
    //   ViewEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
    // (The existing `_ => EventOutcome { needs_repaint: false }` stays for other events.)

    fn handle_key(&mut self, key: &crate::event::Key, modifiers: &crate::event::Modifiers) -> EventOutcome {
        use crate::event::Key;
        // Ctrl+Z: undo
        if modifiers.control && !modifiers.shift && matches!(key, Key::Char('z') | Key::Char('Z')) {
            if self.editor.undo() { self.after_annotation_change(); return EventOutcome { needs_repaint: true }; }
            return EventOutcome { needs_repaint: false };
        }
        // Ctrl+Y or Ctrl+Shift+Z: redo
        if (modifiers.control && matches!(key, Key::Char('y') | Key::Char('Y')))
            || (modifiers.control && modifiers.shift && matches!(key, Key::Char('z') | Key::Char('Z')))
        {
            if self.editor.redo() { self.after_annotation_change(); return EventOutcome { needs_repaint: true }; }
            return EventOutcome { needs_repaint: false };
        }
        // Ctrl+S: save request
        if modifiers.control && matches!(key, Key::Char('s') | Key::Char('S')) {
            self.fire_save_request();
            return EventOutcome { needs_repaint: false };
        }
        // Delete/Backspace: delete selected (when no text cursor)
        if matches!(key, Key::Delete | Key::Backspace) && self.editor.text_cursor().is_none() {
            if !matches!(self.editor.selection(), rofd_editor::AnnotationSelection::None) {
                self.editor.delete_selected();
                self.after_annotation_change();
                self.fire_selection_change();
                return EventOutcome { needs_repaint: true };
            }
        }
        // Text editing (if text cursor set)
        if let Some(cursor) = self.editor.text_cursor().cloned() {
            match key {
                Key::Char(c) => {
                    let s = c.to_string();
                    let new_off = cursor.offset + s.chars().count();
                    self.editor.insert_text(&cursor.annotation, cursor.offset, &s);
                    self.editor.set_cursor(cursor.annotation.clone(), new_off);
                    self.after_annotation_change();
                    self.fire_cursor_change();
                    return EventOutcome { needs_repaint: true };
                }
                Key::Backspace => {
                    if cursor.offset > 0 {
                        self.editor.delete_text(&cursor.annotation, cursor.offset - 1, 1);
                        self.editor.set_cursor(cursor.annotation.clone(), cursor.offset - 1);
                        self.after_annotation_change();
                        self.fire_cursor_change();
                        return EventOutcome { needs_repaint: true };
                    }
                }
                Key::Delete => {
                    self.editor.delete_text(&cursor.annotation, cursor.offset, 1);
                    self.after_annotation_change();
                    return EventOutcome { needs_repaint: true };
                }
                Key::ArrowLeft => {
                    if cursor.offset > 0 {
                        self.editor.set_cursor(cursor.annotation.clone(), cursor.offset - 1);
                        self.fire_cursor_change();
                        return EventOutcome { needs_repaint: true };
                    }
                }
                Key::ArrowRight => {
                    self.editor.set_cursor(cursor.annotation.clone(), cursor.offset + 1);
                    self.fire_cursor_change();
                    return EventOutcome { needs_repaint: true };
                }
                Key::Escape => {
                    self.editor.clear_cursor();
                    self.editor.clear_selection();
                    self.fire_cursor_change();
                    self.fire_selection_change();
                    return EventOutcome { needs_repaint: true };
                }
                _ => {}
            }
        } else if matches!(key, Key::Escape) {
            self.editor.clear_selection();
            self.fire_selection_change();
            return EventOutcome { needs_repaint: true };
        }
        EventOutcome { needs_repaint: false }
    }
```

And update `handle_event` to route KeyDown:
```rust
            ViewEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
```
(Add this arm before the `_ =>` catch-all in handle_event.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-component`
Expected: PASS (undo/redo, ctrl+s, char insert, escape tests).

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/editor_component.rs
git commit -m "feat(component): handle_event key routing (text/undo/redo/delete/save)"
```

---

## Task 8: lib.rs facade + integration test + workspace gates

**Files:**
- Modify: `crates/component/src/lib.rs` (final re-exports)
- Create: `crates/component/tests/integration.rs`
- Test: workspace gates

**Interfaces:**
- Produces: final `rofd-component` public API: `EditorComponent`, `EditorConfig`, `ViewEvent`, `Key`, `Modifiers`, `MouseButton`, `EventOutcome`, `RenderTarget`, `Callbacks`.

- [ ] **Step 1: Finalize lib.rs re-exports**

`crates/component/src/lib.rs`:
```rust
//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod config;
pub mod editor_component;
pub mod event;
pub mod render_target;

pub use callbacks::Callbacks;
pub use config::EditorConfig;
pub use editor_component::EditorComponent;
pub use event::{EventOutcome, Key, Modifiers, MouseButton, ViewEvent};
pub use render_target::RenderTarget;
```

- [ ] **Step 2: Write the integration test**

`crates/component/tests/integration.rs`:
```rust
use rofd_component::{EditorComponent, EditorConfig, ViewEvent, Key, Modifiers, RenderTarget};
use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
use std::sync::{Arc, Mutex};
use vello::Scene;

struct MockRenderTarget { drawn: usize }
impl RenderTarget for MockRenderTarget {
    fn draw_scene(&mut self, _: &Scene) { self.drawn += 1; }
    fn size(&self) -> (f64, f64) { (800.0, 600.0) }
}

#[test]
fn end_to_end_create_select_edit_undo_render() {
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    c.set_clock("tester".into(), 1_700_000_000_000);
    // Create a note annotation via the component pass-through (Task 8 Step 3 adds it).
    let id = c.create_annotation(
        AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0 },
            color: Color::Rgb(0, 0, 0), content: "Hello".into(), icon: NoteIcon::Note,
        },
    );
    // create_annotation pass-through handles cache invalidation + on_change.

    // Undo the create via handle_event.
    let outcome = c.handle_event(&ViewEvent::KeyDown { key: Key::Char('z'), modifiers: Modifiers { control: true, ..Default::default() } });
    assert!(outcome.needs_repaint);
    assert!(c.document().annotations.find(&id).is_none(), "undo removed the annotation");

    // Redo.
    c.handle_event(&ViewEvent::KeyDown { key: Key::Char('y'), modifiers: Modifiers { control: true, ..Default::default() } });
    assert!(c.document().annotations.find(&id).is_some(), "redo restored it");

    // Render (no panic).
    let mut rt = MockRenderTarget { drawn: 0 };
    c.render(&mut rt);
    assert_eq!(rt.drawn, 1);
}

#[test]
fn on_change_fires_on_undo() {
    let fired = Arc::new(Mutex::new(false));
    let f = fired.clone();
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    c.set_clock("t".into(), 1);
    c.on_change(move |_| { *f.lock().unwrap() = true; });
    c.create_annotation(
        AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            color: Color::Rgb(0, 0, 0), content: "".into(), icon: NoteIcon::Note,
        },
    );
    *fired.lock().unwrap() = false;
    c.handle_event(&ViewEvent::KeyDown { key: Key::Char('z'), modifiers: Modifiers { control: true, ..Default::default() } });
    assert!(*fired.lock().unwrap(), "on_change fired on undo");
}
```

(The test uses `c.create_annotation(...)` - a pass-through added in Step 3 below.)

- [ ] **Step 3: Add command pass-throughs to EditorComponent**

Add to `crates/component/src/editor_component.rs` impl block:
```rust
    pub fn create_annotation(&mut self, kind: AnnotationKind, page: PageId, payload: AnnotationPayload) -> rofd_dom::AnnotationId {
        let id = self.editor.create_annotation(kind, page, payload);
        self.after_annotation_change();
        self.fire_selection_change();
        id
    }

    pub fn delete_annotation(&mut self, id: &rofd_dom::AnnotationId) {
        self.editor.delete_annotation(id);
        self.after_annotation_change();
        self.fire_selection_change();
        self.fire_cursor_change();
    }

    pub fn move_annotation(&mut self, id: &rofd_dom::AnnotationId, dx: f64, dy: f64) {
        self.editor.move_annotation(id, dx, dy);
        self.after_annotation_change();
    }

    pub fn resize_annotation(&mut self, id: &rofd_dom::AnnotationId, new_rect: rofd_dom::Rect) {
        self.editor.resize_annotation(id, new_rect);
        self.after_annotation_change();
    }
```

(These delegate to the editor + fire after_annotation_change. The host calls these for programmatic annotation manipulation; handle_event is for keyboard/mouse. Both paths go through after_annotation_change -> cache invalidate + on_change.)

- [ ] **Step 4: Run workspace gates**

Run: `cargo test --workspace`
Expected: PASS.
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/editor_component.rs crates/component/src/lib.rs crates/component/tests/integration.rs
git commit -m "feat(component): facade re-exports + command pass-throughs + integration test"
```

---

## Phase 4a Done - Definition of Done

- `rofd-component`: `EditorComponent` (owns Editor + RenderEngine + PageSceneCache + Viewport + Callbacks); `handle_event` routes PointerDown (hit_test -> select/cursor), KeyDown (text/undo/redo/delete/save), Scroll/Zoom/Resize (viewport); `after_annotation_change` (invalidate all pages + fire on_change); `render` (composite + draw_scene); command pass-throughs (create/delete/move/resize); callback setters (on_change/on_selection_change/on_cursor_change/on_save_request).
- Tests: event types, RenderTarget mock, callbacks fire, handle_event routing (scroll/zoom/resize, undo/redo, ctrl+s, char insert, escape), integration (create -> undo -> redo -> render, on_change on undo).
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green.
- No platform deps (winit/wgpu/xilem are Phase 4b).

## Deferred to Phase 4b (native-view + apps)

- **`rofd-native-view`:** EditorApp (component + file path + modified) + WinitEventBridge (winit -> ViewEvent, coord conversion) + VelloRenderTarget (wgpu surface + vello::Renderer).
- **`examples/native-app`:** winit ApplicationHandler Host (window, wgpu surface, event loop, file dialogs, redraw scheduling).
- **`rofd_web_view` + `examples/web-app`:** wasm-bindgen + WebGpuRenderTarget + JS event bridge.
- **on_context_menu callback:** right-click context menu (host handles menu UI).
- **Cursor blink:** needs host timer (Phase 4b).
- **IME:** Chinese input (needs host IME plumbing).
- **Click-to-position text cursor:** v1 sets cursor to end on click; click-to-position (via caret_rect) is Phase 4b.
- **Affected-pages optimization:** v1 invalidates all pages; optimize to affected-only later.
- **Send bound on callbacks:** add `+ Send` behind `#[cfg(not(target_arch = "wasm32"))]` when Phase 4b native host requires it.
