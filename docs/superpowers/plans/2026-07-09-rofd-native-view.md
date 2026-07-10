# rofd Phase 4b (rofd-native-view + example app) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-native-view` (winit + wgpu + vello adapter) + `examples/native-app` (runnable editor) - the first working OFD view+annotate editor application.

**Architecture:** `VelloRenderTarget` owns the wgpu instance/device/queue/surface + vello `Renderer`, implements `RenderTarget` (draw_scene renders the vello::Scene to the surface via `render_to_texture`). `WinitEventBridge` translates winit `WindowEvent` -> `ViewEvent` (coord conversion: physical px / scale_factor = canvas-local). `EditorApp` wraps `EditorComponent` + `VelloRenderTarget` + file I/O (rofd-io parse/save). `examples/native-app` is a winit 0.30 `ApplicationHandler` that creates the window, drives the event loop, and renders. Full-window canvas (no toolbar/menus in v1).

**Tech Stack:** Rust 2021. New crate `rofd-native-view` deps: `rofd-component`, `rofd-io`, `rofd-dom`, `vello` (0.8), `winit` (0.30), `wgpu` (28, via vello or direct). `examples/native-app` deps: `rofd-native-view`, `rofd-component`, `rofd-io`.

## Global Constraints

- **`rofd-native-view` deps = `rofd-component` + `rofd-io` + `rofd-dom` + `vello` + `winit` + `wgpu`.** The native adapter owns GPU state.
- **VelloRenderTarget uses `vello::Renderer::render_to_texture`** to render the `vello::Scene` to the surface. v1 renders directly to the surface texture (with `STORAGE_BINDING` usage if the wgpu API allows; if not, use an intermediate texture). **Verify the exact wgpu 28 + vello 0.8 API via compile errors** - these are alpha/new versions; adapt call shapes.
- **Full-window canvas:** `canvas_origin = (0, 0)`. Coordinate conversion: `canvas_local = cursor_physical / scale_factor`. No toolbar/menus in v1.
- **WinitEventBridge is NOT a field on EditorApp** (mirrors reditor - keeps EditorApp framework-agnostic; the Host owns both).
- **File I/O on EditorApp** (not EditorComponent): `load_ofd(bytes)` via `rofd_io::parse_ofd`, `save_ofd()` via `rofd_io::save_ofd`. The component stays format-agnostic (`load_document`).
- **Commits:** conventional commits, NO Co-Authored-By attribution line.
- **TDD where testable:** WinitEventBridge coord conversion + event mapping are unit-testable. EditorApp file I/O is testable (parse/save with mock bytes, no GPU). VelloRenderTarget + native-app are compile-check + manual run (GPU surface needs a display).
- **Gate:** `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green, `cargo build -p native-app` compiles.

### Risk: wgpu 28 / vello 0.8 / winit 0.30 API drift

wgpu 28 is very new; winit 0.30 changed the event loop API; vello 0.8 is beta. The exact call shapes in this plan are from the grounding investigation but may drift. **The implementer must verify via compile errors** and adapt call shapes (not behavior). If a wgpu/vello API is fundamentally blocking, report BLOCKED with the error.

---

## File Structure

```
rofd/
├── Cargo.toml                      # add crates/native-view + examples/native-app to members
├── crates/
│   └── native-view/                # NEW crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # re-exports
│           ├── vello_render_target.rs  # VelloRenderTarget (wgpu + vello)
│           ├── winit_bridge.rs     # WinitEventBridge (winit -> ViewEvent)
│           └── editor_app.rs       # EditorApp (component + render target + file I/O)
└── examples/
    └── native-app/                 # NEW example binary
        ├── Cargo.toml
        └── src/
            └── main.rs             # winit ApplicationHandler + main()
```

---

## Task 1: `rofd-native-view` scaffold + deps

**Files:**
- Modify: `Cargo.toml` (workspace members + deps)
- Create: `crates/native-view/Cargo.toml`, `crates/native-view/src/lib.rs`

**Interfaces:**
- Produces: empty `rofd-native-view` crate that compiles against rofd-component + rofd-io + vello + winit + wgpu.

- [ ] **Step 1: Add workspace members + deps**

Root `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io", "crates/render", "crates/editor", "crates/component", "crates/native-view", "examples/native-app"]

[workspace.dependencies]
# ... existing ...
rofd-native-view = { path = "crates/native-view" }
winit = "0.30"
```

- [ ] **Step 2: Create `crates/native-view/Cargo.toml`**

```toml
[package]
name = "rofd-native-view"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-component = { workspace = true }
rofd-io = { workspace = true }
rofd-dom = { workspace = true }
vello = { workspace = true }
winit = { workspace = true }
```

> **Verify:** if `wgpu` types are needed directly (not via `vello::wgpu` re-export), add `wgpu = "28"` to deps. Check if `vello::wgpu` re-export works first (vello 0.8 re-exports wgpu at `lib.rs:143`).

- [ ] **Step 3: Create `crates/native-view/src/lib.rs`**

```rust
//! rofd-native-view - winit + wgpu + vello native adapter for rofd.
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check -p rofd-native-view`
Expected: PASS (deps resolve). If wgpu/winit versions conflict, pin per the Risk note.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/native-view/ Cargo.lock
git commit -m "chore: scaffold rofd-native-view crate"
```

---

## Task 2: VelloRenderTarget (wgpu surface + vello renderer)

**Files:**
- Create: `crates/native-view/src/vello_render_target.rs`
- Modify: `crates/native-view/src/lib.rs`
- Test: compile check only (GPU surface needs a display)

**Interfaces:**
- Consumes: `rofd_component::RenderTarget` trait, `vello::Scene`, `vello::Renderer`, `wgpu` types, `winit::window::Window`.
- Produces: `VelloRenderTarget` struct implementing `RenderTarget` (draw_scene renders vello::Scene to the wgpu surface; size returns surface dimensions; resize reconfigures surface).

- [ ] **Step 1: Write VelloRenderTarget**

`crates/native-view/src/vello_render_target.rs`:
```rust
use rofd_component::RenderTarget;
use vello::{
    AaConfig, RenderParams, Renderer, RendererOptions, Scene,
};
use winit::window::Window;

/// Owns the wgpu GPU state + vello renderer. Implements RenderTarget to
/// draw a vello::Scene to the window's surface.
pub struct VelloRenderTarget {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    width: u32,
    height: u32,
}

impl VelloRenderTarget {
    /// Create from a winit window. Seeds the wgpu instance/adapter/device/queue,
    /// creates a surface from the window, configures it, and creates a vello Renderer.
    pub fn new(window: &Window, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("failed to create surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no suitable adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rofd device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            ..Default::default()
        }))
        .map_err(|e| format!("device request failed: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            ..Default::default()
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, RendererOptions {
            use_cpu: false,
            antialiasing_support: vello::AaSupport::area_only(),
            num_init_threads: std::num::NonZeroUsize::new(1),
            pipeline_cache: None,
        })
        .map_err(|e| format!("vello renderer creation failed: {e}"))?;

        Ok(Self { device, queue, surface, config, renderer, width, height })
    }

    /// Reconfigure the surface on window resize.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 { return; }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn dimensions(&self) -> (u32, u32) { (self.width, self.height) }
}

impl RenderTarget for VelloRenderTarget {
    fn draw_scene(&mut self, scene: &Scene) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => { eprintln!("surface error: {e}"); return; }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let _ = self.renderer.render_to_texture(
            &self.device, &self.queue, scene, &view,
            &RenderParams {
                base_color: vello::peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        );
        frame.present();
    }

    fn size(&self) -> (f64, f64) {
        (self.width as f64, self.height as f64)
    }
}
```

> **API verification notes:**
> - `wgpu::Instance::new(&wgpu::InstanceDescriptor::default())` - verify the exact constructor on wgpu 28 (may take `&` or own the descriptor).
> - `instance.create_surface(window)` - winit 0.30 Window implements `rwh_06::HasWindowHandle`; verify the surface creation signature.
> - `SurfaceConfiguration` fields - verify the exact fields on wgpu 28 (the `..Default::default()` may not work if required fields changed).
> - `RendererOptions` fields - verified from vello 0.8 source.
> - `RenderTarget::size` - the component's RenderTarget trait has `size(&self) -> (f64, f64)` (confirmed from Phase 4a). The struct's own `dimensions() -> (u32, u32)` is separate (avoids the naming conflict).
> - `pollster::block_on` - add `pollster = "1"` to deps if wgpu's async methods need blocking. Or use `futures::executor::block_on`. Verify which is available.
> - If `STORAGE_BINDING` on surface textures is not supported, fall back to rendering to an intermediate `Rgba8Unorm` texture + a blit pass. For v1, try direct first.

- [ ] **Step 2: Add pollster dep + wire lib.rs**

Add to `crates/native-view/Cargo.toml`:
```toml
pollster = "1"
```

`crates/native-view/src/lib.rs`:
```rust
//! rofd-native-view - winit + wgpu + vello native adapter for rofd.

pub mod vello_render_target;

pub use vello_render_target::VelloRenderTarget;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p rofd-native-view`
Expected: PASS (adapt wgpu/vello API per the verification notes). If fundamentally blocked, report the error.

- [ ] **Step 4: Commit**

```bash
git add crates/native-view/
git commit -m "feat(native-view): VelloRenderTarget (wgpu surface + vello renderer)"
```

---

## Task 3: WinitEventBridge (winit -> ViewEvent + coord conversion)

**Files:**
- Create: `crates/native-view/src/winit_bridge.rs`
- Modify: `crates/native-view/src/lib.rs`
- Test: inline (coord conversion is testable; event mapping is testable)

**Interfaces:**
- Consumes: `rofd_component::{ViewEvent, Key, Modifiers, MouseButton, EventOutcome}`.
- Produces: `WinitEventBridge` (holds modifiers, cursor physical px, scale_factor; `handle_window_event` translates winit `WindowEvent` -> `Option<ViewEvent>`; coord conversion: `canvas_local = cursor_phys / scale_factor`).

- [ ] **Step 1: Write the failing tests**

`crates/native-view/src/winit_bridge.rs`:
```rust
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

    /// Update modifiers from a winit ModifiersChanged event.
    pub fn update_modifiers(&mut self, state: &winit::event::ModifiersState) {
        self.modifiers = Modifiers {
            shift: state.shift_key(),
            control: state.control_key(),
            alt: state.alt_key(),
            meta: state.super_key(),
        };
    }
}

fn winit_key_to_rofd(key: &winit::keyboard::PhysicalKey, text: &Option<std::string::String>) -> Key {
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
                        if let Some(c) = t.chars().next() {
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
        let bridge = WinitEventBridge::new();
        bridge.set_cursor(10.0, 20.0);
        let (x, y) = bridge.canvas_local();
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn update_modifiers_maps_correctly() {
        let mut bridge = WinitEventBridge::new();
        let state = winit::event::ModifiersState::CONTROL;
        bridge.update_modifiers(&state);
        assert!(bridge.modifiers.control);
        assert!(!bridge.modifiers.shift);
    }
}
```

> **API verification:** winit 0.30's `WindowEvent`, `KeyboardInput`, `PhysicalKey`, `KeyCode`, `ModifiersState` may have changed from 0.29. Verify via compile errors. The `event.text` field (for character input) may be named differently or absent - check the winit 0.30 API.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-native-view`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

`crates/native-view/src/lib.rs`:
```rust
//! rofd-native-view - winit + wgpu + vello native adapter for rofd.

pub mod vello_render_target;
pub mod winit_bridge;

pub use vello_render_target::VelloRenderTarget;
pub use winit_bridge::WinitEventBridge;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-native-view`
Expected: PASS (3 coord/modifier tests). Adapt winit API per verification notes.

- [ ] **Step 5: Commit**

```bash
git add crates/native-view/src/winit_bridge.rs crates/native-view/src/lib.rs
git commit -m "feat(native-view): WinitEventBridge (winit -> ViewEvent + coord conversion)"
```

---

## Task 4: EditorApp (component + render target + file I/O)

**Files:**
- Create: `crates/native-view/src/editor_app.rs`
- Modify: `crates/native-view/src/lib.rs`
- Test: inline (file I/O testable; render needs GPU)

**Interfaces:**
- Consumes: `rofd_component::{EditorComponent, EditorConfig, ViewEvent, EventOutcome, RenderTarget}`, `rofd_io::{parse_ofd, save_ofd}`, `rofd_dom::OfdDocument`.
- Produces: `EditorApp` struct (EditorComponent + file path + modified flag); `new(config)`, `load_ofd(bytes)`, `save_ofd() -> Result<Vec<u8>>`, `handle_event(&ViewEvent) -> EventOutcome`, `render(&mut dyn RenderTarget)`, `document()`, `is_modified()`.

- [ ] **Step 1: Write the failing test + EditorApp**

`crates/native-view/src/editor_app.rs`:
```rust
use std::path::PathBuf;
use std::sync::Arc;

use rofd_component::{EditorComponent, EditorConfig, EventOutcome, RenderTarget, ViewEvent};
use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, save_ofd};

use crate::vello_render_target::VelloRenderTarget;

/// Platform-agnostic editor state (no winit types). Owns the EditorComponent.
/// File I/O (load_ofd/save_ofd) lives here, not on EditorComponent.
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub modified: bool,
}

impl EditorApp {
    pub fn new(config: EditorConfig) -> Self {
        Self { component: EditorComponent::new(config), current_file: None, modified: false }
    }

    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), String> {
        let report = parse_ofd(bytes).map_err(|e| format!("parse failed: {e}"))?;
        self.component.load_document(report.document);
        self.modified = false;
        Ok(())
    }

    pub fn save_ofd(&self) -> Result<Vec<u8>, String> {
        let package = &self.component.document(); // need PackageHandle...
        // Problem: save_ofd needs &PackageHandle, but the component doesn't expose it.
        // For v1: use write_ofd (full write, no package) - the component's document is the source.
        rofd_io::write_ofd(self.component.document()).map_err(|e| format!("save failed: {e}"))
    }

    pub fn handle_event(&mut self, event: &ViewEvent) -> EventOutcome {
        let outcome = self.component.handle_event(event);
        if outcome.needs_repaint { self.modified = true; }
        outcome
    }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        self.component.render(target);
    }

    pub fn document(&self) -> &OfdDocument { self.component.document() }
    pub fn is_modified(&self) -> bool { self.modified }
    pub fn set_clock(&mut self, author: String, ts: i64) { self.component.set_clock(author, ts); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_ofd_then_document_has_pages() {
        // Build a minimal .ofd via the io fixture (reuse the pattern from rofd-io tests).
        // For simplicity, use write_ofd to create a round-trippable package, then load it.
        let mut doc = OfdDocument::default();
        // No pages - just test the load path doesn't panic.
        let bytes = rofd_io::write_ofd(&doc).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&bytes).unwrap();
        assert_eq!(app.document().pages.len(), 0);
        assert!(!app.is_modified());
    }

    #[test]
    fn save_ofd_round_trips() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        let doc = OfdDocument::default();
        app.component.load_document(doc);
        let saved = app.save_ofd().unwrap();
        assert!(!saved.is_empty());
    }
}
```

> **Note on save_ofd:** The component's `document()` returns `&OfdDocument`. `save_ofd` needs `(&OfdDocument, &PackageHandle)` for surgical save, but the component doesn't expose the PackageHandle (it was consumed by `load_document`). For v1, use `write_ofd` (full write, no package) - it constructs a fresh package from the model. This means save uses the full-write path (not surgical), which is acceptable for v1 (the surgical path is for preserving unmodelled body content; `write_ofd` emits only what the model holds). **If `write_ofd` is not public**, check `rofd_io`'s public API and use whatever is available.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-native-view editor_app`
Expected: FAIL - module not wired.

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/native-view/src/lib.rs`:
```rust
pub mod editor_app;
pub use editor_app::EditorApp;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-native-view`
Expected: PASS (file I/O tests + coord tests from Task 3).

- [ ] **Step 5: Commit**

```bash
git add crates/native-view/src/editor_app.rs crates/native-view/src/lib.rs
git commit -m "feat(native-view): EditorApp (component + file I/O)"
```

---

## Task 5: `examples/native-app` (winit ApplicationHandler + main)

**Files:**
- Create: `examples/native-app/Cargo.toml`, `examples/native-app/src/main.rs`
- Test: compile check + manual run

**Interfaces:**
- Consumes: `rofd_native_view::{EditorApp, VelloRenderTarget, WinitEventBridge}`, `rofd_component::EditorConfig`, `winit`.
- Produces: a runnable `native-app` binary that opens an OFD file (command-line arg), renders it, and handles keyboard/mouse events.

- [ ] **Step 1: Create `examples/native-app/Cargo.toml`**

```toml
[package]
name = "native-app"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-native-view = { workspace = true }
rofd-component = { workspace = true }
rofd-io = { workspace = true }
winit = { workspace = true }
```

- [ ] **Step 2: Write main.rs (ApplicationHandler)**

`examples/native-app/src/main.rs`:
```rust
use std::path::PathBuf;
use std::sync::Arc;

use rofd_component::EditorConfig;
use rofd_native_view::{EditorApp, VelloRenderTarget, WinitEventBridge};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct NativeApp {
    app: EditorApp,
    render_target: Option<VelloRenderTarget>,
    bridge: WinitEventBridge,
    window: Option<Window>,
    default_font_bytes: Arc<Vec<u8>>,
}

impl ApplicationHandler for NativeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }
        let window = event_loop.create_window(
            winit::window::WindowAttributes::default()
                .with_title("rofd - OFD Editor")
                .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0)),
        ).expect("failed to create window");

        // Seed scale factor from the primary monitor (ScaleFactorChanged only fires on changes).
        if let Some(monitor) = event_loop.primary_monitor() {
            self.bridge.set_scale_factor(monitor.scale_factor());
        }

        let size = window.inner_size();
        let render_target = VelloRenderTarget::new(&window, size.width, size.height)
            .expect("failed to create render target");
        self.render_target = Some(render_target);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::CloseRequested => { event_loop.exit(); return; }
            WindowEvent::ModifiersChanged(state) => { self.bridge.update_modifiers(state); return; }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.bridge.set_scale_factor(*scale_factor);
                return;
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(rt) = &mut self.render_target {
                    rt.resize(physical_size.width, physical_size.height);
                }
                let (w, h) = (physical_size.width as f64 / self.bridge.scale_factor,
                              physical_size.height as f64 / self.bridge.scale_factor);
                self.app.handle_event(&rofd_component::ViewEvent::Resize { width: w, height: h });
                if let Some(window) = &self.window { window.request_redraw(); }
                return;
            }
            WindowEvent::RedrawRequested => {
                if let Some(rt) = &mut self.render_target {
                    self.app.render(rt);
                }
                return;
            }
            _ => {}
        }

        // Translate winit event -> ViewEvent -> EditorApp.
        if let Some(view_event) = self.bridge.translate(&event) {
            let outcome = self.app.handle_event(&view_event);
            if outcome.needs_repaint {
                if let Some(window) = &self.window { window.request_redraw(); }
            }
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

fn main() {
    let default_font_bytes: Arc<Vec<u8>> = Arc::new(vec![]); // v1: empty (render uses default font fallback)

    let mut app = EditorApp::new(EditorConfig::new(default_font_bytes.clone()));
    app.set_clock("rofd".into(), 0);

    // Load file from command-line arg if provided.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        let path = PathBuf::from(&args[1]);
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(e) = app.load_ofd(&bytes) {
                    eprintln!("failed to load {}: {}", path.display(), e);
                }
            }
            Err(e) => eprintln!("failed to read {}: {}", path.display(), e),
        }
    }

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut native_app = NativeApp {
        app,
        render_target: None,
        bridge: WinitEventBridge::new(),
        window: None,
        default_font_bytes,
    };
    event_loop.run_app(&mut native_app).expect("event loop error");
}
```

> **API verification:** winit 0.30's `ApplicationHandler`, `EventLoop::new()`, `event_loop.run_app()`, `WindowAttributes`, `create_window`, `inner_size`, `request_redraw`, `primary_monitor`, `ScaleFactorChanged` - verify all via compile errors. The API changed significantly from winit 0.29.

> **Default font:** v1 uses empty bytes (the render engine's default font fallback will produce no glyphs for text - acceptable for a first runnable version). To render text, the host must register a real font (e.g., copy DejaVuSans.ttf as in Phase 2). For the plan, the host passes empty bytes; a follow-up can add font loading.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p native-app`
Expected: PASS (adapt winit API per verification notes). If fundamentally blocked, report the error.

- [ ] **Step 4: Verify workspace gates**

Run: `cargo test --workspace`
Expected: PASS (all tests; native-app has no tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add examples/native-app/ Cargo.toml Cargo.lock
git commit -m "feat(native-app): winit ApplicationHandler + runnable editor"
```

---

## Phase 4b Done - Definition of Done

- `rofd-native-view`: VelloRenderTarget (wgpu surface + vello renderer, implements RenderTarget), WinitEventBridge (winit -> ViewEvent + coord conversion), EditorApp (component + file I/O).
- `examples/native-app`: winit ApplicationHandler that creates a window, loads an OFD file (command-line arg), renders it, and handles keyboard/mouse/scroll/zoom events.
- Tests: WinitEventBridge coord conversion (3 tests), EditorApp file I/O (2 tests). VelloRenderTarget + native-app are compile-check + manual run.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green; `cargo build -p native-app` compiles.
- **Manual run:** `cargo run -p native-app -- path/to/file.ofd` opens a window showing the OFD.

## Deferred to Phase 5 (web-view + apps) / later

- **rofd_web_view:** wasm-bindgen + WebGpuRenderTarget + JS event bridge.
- **Cursor blink:** needs host timer (about_to_wait scheduling).
- **IME:** Chinese input (needs winit IME plumbing).
- **Toolbar / context menu / file dialogs:** UI chrome (needs a widget toolkit or custom rendering).
- **Click-to-position text cursor:** via caret_rect (needs the component to expose it; Phase 4a deferred).
- **Blit pipeline:** if direct render_to_texture to surface doesn't work on some GPUs, add an intermediate texture + blit pass.
- **Font loading from the host:** v1 uses empty default font bytes; the host should load a real font (e.g., DejaVuSans.ttf) and pass it via EditorConfig.
