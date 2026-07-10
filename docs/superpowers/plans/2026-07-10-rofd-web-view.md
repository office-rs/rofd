# rofd Phase 5 (rofd_web_view + web-app) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd_web_view` (WASM/WebGPU adapter) + `examples/web-app` (Vite web app) - the web counterpart to the native editor, completing v1 dual-platform support.

**Architecture:** `WebGpuRenderTarget` creates a wgpu surface from a `web_sys::HtmlCanvasElement` and renders the vello::Scene via WebGPU. `WasmEditor` is a `#[wasm_bindgen]` struct wrapping `EditorComponent` + `WebGpuRenderTarget`, exposing simple methods (`handle_keydown`, `handle_pointerdown`, `render`, `load_ofd`) that JS calls from DOM event listeners. The TS SDK wraps the wasm exports in an idiomatic `Editor` class. The Vite web-app loads the wasm, creates a canvas, wires DOM events, and provides file open/save.

**Tech Stack:** Rust 2021 (`cdylib` + `rlib`), `wasm-bindgen`, `web-sys`, `js-sys`, `vello` (0.8, WebGPU backend), `wasm-pack` (`--target web`), TypeScript, Vite, Node.js/npm.

## Global Constraints

- **`rofd_web_view` deps = `rofd-component` + `rofd-io` + `rofd-dom` + `vello` + `wasm-bindgen` + `web-sys` + `js-sys`.** The crate is a `cdylib` (for wasm-bindgen export) + `rlib` (for Rust tests if any).
- **WASM prerequisites:** `rustup target add wasm32-unknown-unknown`, `cargo install wasm-pack`, Node.js + npm. **Task 1 verifies these are available** - if not, report BLOCKED.
- **Existing crates must compile for wasm32.** The `zip` crate (rofd-io) and `image` crate (rofd-render) should be wasm-compatible (they operate on bytes, not files). If any crate fails to compile for wasm32, cfg-gate the platform-specific code. The `rofd-io` `write_document_atomic` (tempfile) is already `#[cfg(not(target_arch = "wasm32"))]`-gated.
- **WebGpuRenderTarget mirrors VelloRenderTarget** (Phase 4b) but creates the surface from `web_sys::HtmlCanvasElement` instead of a winit Window. Same vello `render_to_texture` path.
- **WasmEditor exposes simple methods** (not the full ViewEvent enum) - JS calls `handle_keydown(key, ctrl, shift)`, `handle_pointerdown(x, y, button)`, `handle_scroll(dx, dy)`, `handle_resize(w, h)`, `render()`, `load_ofd(bytes)`, `save_ofd()`. The WasmEditor internally builds ViewEvents from these params.
- **JS drives the event loop** (requestAnimationFrame -> render). No Rust event loop.
- **Commits:** conventional commits, NO Co-Authored-By attribution line.
- **TDD where testable:** WasmEditor's ViewEvent construction from params is testable (unit test in Rust). WebGpuRenderTarget + web-app are compile-check + manual run (needs a browser).
- **Gate:** `cargo build -p rofd-web-view --target wasm32-unknown-unknown` compiles. `wasm-pack build` succeeds. `cd examples/web-app && npm install && npm run build` succeeds.

### Risk: WASM compilation of existing crates

The existing crates (dom, io, render, editor, component) were built for native. They should be wasm-compatible (pure Rust, no platform-specific code except rofd-io's tempfile gate). But the `zip` crate may have wasm issues. **Task 1 includes a wasm32 compile-check of the workspace** - if any crate fails, fix it (cfg-gate platform-specific code) before proceeding.

---

## File Structure

```
rofd/
├── Cargo.toml                      # add crates/web-view + examples/web-app to members
├── crates/
│   └── web-view/                   # NEW crate (cdylib + rlib)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # wasm-bindgen start + re-exports
│           ├── webgpu_render_target.rs  # WebGpuRenderTarget (canvas -> surface -> vello)
│           └── wasm_editor.rs      # WasmEditor (wasm-bindgen surface)
└── examples/
    └── web-app/                    # NEW Vite app
        ├── package.json
        ├── vite.config.ts
        ├── tsconfig.json
        ├── index.html
        └── src/
            └── main.ts             # app entry: load wasm, create canvas, wire events
```

---

## Task 1: WASM prerequisite check + `rofd_web_view` scaffold

**Files:**
- Modify: `Cargo.toml` (workspace members + deps)
- Create: `crates/web-view/Cargo.toml`, `crates/web-view/src/lib.rs`

**Interfaces:**
- Produces: empty `rofd_web_view` crate that compiles for `wasm32-unknown-unknown`.

- [ ] **Step 1: Verify WASM prerequisites**

Run:
```bash
rustup target list --installed | grep wasm32-unknown-unknown
wasm-pack --version
node --version
npm --version
```
If any is missing, report BLOCKED with what's missing and how to install it:
- `rustup target add wasm32-unknown-unknown`
- `cargo install wasm-pack`
- Install Node.js from https://nodejs.org/

- [ ] **Step 2: Verify existing crates compile for wasm32**

Run: `cargo check --workspace --target wasm32-unknown-unknown`
Expected: PASS. If any crate fails (e.g., `zip` using std::fs), cfg-gate the platform-specific code. **Report what needed fixing.**

- [ ] **Step 3: Add workspace member + deps**

Root `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io", "crates/render", "crates/editor", "crates/component", "crates/native-view", "crates/web-view", "examples/native-app"]

[workspace.dependencies]
# ... existing ...
rofd-web-view = { path = "crates/web-view" }
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = ["HtmlCanvasElement", "Window", "Document", "Element", "Performance", "ResizeObserver", "ResizeObserverEntry", "KeyboardEvent", "MouseEvent", "WheelEvent", "EventTarget"] }
js-sys = "0.3"
```

- [ ] **Step 4: Create `crates/web-view/Cargo.toml`**

```toml
[package]
name = "rofd-web-view"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
rofd-component = { workspace = true }
rofd-io = { workspace = true }
rofd-dom = { workspace = true }
vello = { workspace = true }
wasm-bindgen = { workspace = true }
web-sys = { workspace = true }
js-sys = { workspace = true }
```

- [ ] **Step 5: Create `crates/web-view/src/lib.rs`**

```rust
//! rofd_web_view - WASM/WebGPU adapter for rofd.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
```

Add `console_error_panic-hook = "0.1"` to `[dependencies]` in `crates/web-view/Cargo.toml`.

- [ ] **Step 6: Verify it compiles for wasm32**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/web-view/ Cargo.lock
git commit -m "chore: scaffold rofd_web_view crate (wasm32)"
```

---

## Task 2: WebGpuRenderTarget (canvas -> surface -> vello render)

**Files:**
- Create: `crates/web-view/src/webgpu_render_target.rs`
- Modify: `crates/web-view/src/lib.rs`
- Test: compile-check only (WebGPU needs a browser)

**Interfaces:**
- Consumes: `rofd_component::RenderTarget`, `vello::Scene`, `vello::Renderer`, `web_sys::HtmlCanvasElement`.
- Produces: `WebGpuRenderTarget` implementing `RenderTarget` (draw_scene renders vello::Scene to the WebGPU canvas; size returns canvas dimensions; resize reconfigures).

- [ ] **Step 1: Write WebGpuRenderTarget**

`crates/web-view/src/webgpu_render_target.rs`:
```rust
use rofd_component::RenderTarget;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

/// Owns the WebGPU device/queue/surface + vello renderer for a canvas element.
pub struct WebGpuRenderTarget {
    device: vello::wgpu::Device,
    queue: vello::wgpu::Queue,
    surface: vello::wgpu::Surface<'static>,
    config: vello::wgpu::SurfaceConfiguration,
    renderer: Renderer,
    width: u32,
    height: u32,
}

impl WebGpuRenderTarget {
    /// Create from a canvas element. Async (wgpu adapter request is async on WASM).
    pub async fn new(canvas: &HtmlCanvasElement, width: u32, height: u32) -> Result<Self, JsValue> {
        let instance = vello::wgpu::Instance::new(&vello::wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(canvas)
            .map_err(|e| JsValue::from_str(&format!("surface creation failed: {e}")))?;

        let adapter = instance.request_adapter(&vello::wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: vello::wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }).await
            .map_err(|e| JsValue::from_str(&format!("no adapter: {e}")))?;

        let (device, queue) = adapter.request_device(&vello::wgpu::DeviceDescriptor {
            label: Some("rofd web device"),
            required_features: vello::wgpu::Features::empty(),
            required_limits: vello::wgpu::Limits::default(),
            ..Default::default()
        }).await
            .map_err(|e| JsValue::from_str(&format!("device request failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats.get(0).copied().ok_or_else(|| JsValue::from_str("no surface formats"))?);

        let config = vello::wgpu::SurfaceConfiguration {
            usage: vello::wgpu::TextureUsages::RENDER_ATTACHMENT | vello::wgpu::TextureUsages::STORAGE_BINDING,
            format,
            width,
            height,
            present_mode: vello::wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(vello::wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, RendererOptions {
            use_cpu: false,
            antialiasing_support: vello::AaSupport::area_only(),
            num_init_threads: std::num::NonZeroUsize::new(1),
            pipeline_cache: None,
        }).map_err(|e| JsValue::from_str(&format!("vello renderer failed: {e}")))?;

        Ok(Self { device, queue, surface, config, renderer, width, height })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 { return; }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }
}

impl RenderTarget for WebGpuRenderTarget {
    fn draw_scene(&mut self, scene: &Scene) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => { self.surface.configure(&self.device, &self.config); return; }
        };
        let view = frame.texture.create_view(&vello::wgpu::TextureViewDescriptor::default());
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

> **API verification:** `instance.create_surface(canvas)` - on WASM, wgpu creates a WebGPU surface from the canvas. Verify the exact signature (may need `canvas.into()` or a wrapper). The `request_adapter`/`request_device` are async on WASM (use `.await`). The `Surface<'static>` lifetime may work differently on WASM (the canvas is owned by JS, not Rust). Adapt as needed via compile errors. If `create_surface` doesn't accept `&HtmlCanvasElement` directly, check if wgpu 28 has a `create_surface_from_canvas` or if the canvas needs to be wrapped in a `SurfaceTarget`.

- [ ] **Step 2: Wire into lib.rs**

`crates/web-view/src/lib.rs`:
```rust
//! rofd_web_view - WASM/WebGPU adapter for rofd.

use wasm_bindgen::prelude::*;

pub mod webgpu_render_target;

pub use webgpu_render_target::WebGpuRenderTarget;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
```

- [ ] **Step 3: Verify it compiles for wasm32**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: PASS (adapt wgpu/vello WASM API per verification notes). If fundamentally blocked, report BLOCKED.

- [ ] **Step 4: Commit**

```bash
git add crates/web-view/src/
git commit -m "feat(web-view): WebGpuRenderTarget (canvas -> WebGPU surface -> vello)"
```

---

## Task 3: WasmEditor (wasm-bindgen surface + event handling + file I/O)

**Files:**
- Create: `crates/web-view/src/wasm_editor.rs`
- Modify: `crates/web-view/src/lib.rs`
- Test: inline (ViewEvent construction from params is testable)

**Interfaces:**
- Consumes: `rofd_component::{EditorComponent, EditorConfig, ViewEvent, Key, Modifiers, MouseButton, EventOutcome}`, `rofd_io::{parse_ofd, write_ofd}`, `WebGpuRenderTarget` (Task 2).
- Produces: `WasmEditor` (`#[wasm_bindgen]` struct) with methods: `new` (async, takes canvas + font bytes), `handle_keydown`, `handle_pointerdown`, `handle_pointerup`, `handle_scroll`, `handle_resize`, `render`, `load_ofd`, `save_ofd`, `can_undo`, `can_redo`, `set_clock`.

- [ ] **Step 1: Write WasmEditor**

`crates/web-view/src/wasm_editor.rs`:
```rust
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use rofd_component::{EditorComponent, EditorConfig, EventOutcome, Key, Modifiers, MouseButton, ViewEvent};

use crate::webgpu_render_target::WebGpuRenderTarget;

#[wasm_bindgen]
pub struct WasmEditor {
    component: EditorComponent,
    render_target: WebGpuRenderTarget,
}

#[wasm_bindgen]
impl WasmEditor {
    #[wasm_bindgen(constructor)]
    pub async fn new(canvas: HtmlCanvasElement, font_bytes: Vec<u8>) -> Result<WasmEditor, JsValue> {
        let config = EditorConfig::new(Arc::new(font_bytes));
        let component = EditorComponent::new(config);
        let width = canvas.client_width() as u32;
        let height = canvas.client_height() as u32;
        let render_target = WebGpuRenderTarget::new(&canvas, width, height).await?;
        Ok(Self { component, render_target })
    }

    pub fn handle_keydown(&mut self, key: &str, ctrl: bool, shift: bool) -> bool {
        let key = parse_key(key);
        let modifiers = Modifiers { control: ctrl, shift, ..Default::default() };
        self.dispatch(ViewEvent::KeyDown { key, modifiers })
    }

    pub fn handle_pointerdown(&mut self, x: f64, y: f64, button: u32) -> bool {
        let btn = match button { 0 => MouseButton::Left, 1 => MouseButton::Middle, 2 => MouseButton::Right, _ => return false };
        self.dispatch(ViewEvent::PointerDown { button: btn, x, y, modifiers: Modifiers::default() })
    }

    pub fn handle_pointerup(&mut self, x: f64, y: f64, button: u32) -> bool {
        let btn = match button { 0 => MouseButton::Left, 1 => MouseButton::Middle, 2 => MouseButton::Right, _ => return false };
        self.dispatch(ViewEvent::PointerUp { button: btn, x, y })
    }

    pub fn handle_pointermove(&mut self, x: f64, y: f64) -> bool {
        self.dispatch(ViewEvent::PointerMove { x, y })
    }

    pub fn handle_scroll(&mut self, dx: f64, dy: f64) -> bool {
        self.dispatch(ViewEvent::Scroll { dx, dy })
    }

    pub fn handle_resize(&mut self, width: f64, height: f64) -> bool {
        self.render_target.resize(width as u32, height as u32);
        self.dispatch(ViewEvent::Resize { width, height })
    }

    pub fn render(&mut self) {
        self.component.render(&mut self.render_target);
    }

    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.component.load_ofd(bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn save_ofd(&self) -> Result<Vec<u8>, JsValue> {
        rofd_io::write_ofd(self.component.document())
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    pub fn can_undo(&self) -> bool { self.component.can_undo() }
    pub fn can_redo(&self) -> bool { self.component.can_redo() }
    pub fn set_clock(&mut self, author: String, ts: i64) { self.component.set_clock(author, ts); }

    fn dispatch(&mut self, event: ViewEvent) -> bool {
        self.component.handle_event(&event).needs_repaint
    }
}

fn parse_key(s: &str) -> Key {
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
        _ if s.len() == 1 => Key::Char(s.chars().next().unwrap()),
        _ => Key::Unidentified,
    }
}

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
```

> **Note:** `load_ofd` calls `self.component.load_ofd(bytes)` - but `EditorComponent` doesn't have a `load_ofd` method (it has `load_document`). The `load_ofd` is on `EditorApp` (native-view), not `EditorComponent`. For the web-view, either: (a) call `rofd_io::parse_ofd` + `self.component.load_document(doc)` directly, or (b) add a `load_ofd` method to EditorComponent. Option (a) is simpler (no component change). Fix: replace `self.component.load_ofd(bytes)` with `let report = rofd_io::parse_ofd(bytes)?; self.component.load_document(report.document);`.

- [ ] **Step 2: Fix the load_ofd call**

In `wasm_editor.rs`, replace the `load_ofd` method:
```rust
    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let report = rofd_io::parse_ofd(bytes)
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        self.component.load_document(report.document);
        Ok(())
    }
```

- [ ] **Step 3: Wire into lib.rs**

Add to `crates/web-view/src/lib.rs`:
```rust
pub mod wasm_editor;
pub use wasm_editor::WasmEditor;
```

- [ ] **Step 4: Run tests (native, for parse_key)**

Run: `cargo test -p rofd-web-view`
Expected: PASS (3 parse_key tests; these run on native, not wasm).

- [ ] **Step 5: Verify wasm32 compiles**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/web-view/src/
git commit -m "feat(web-view): WasmEditor (wasm-bindgen surface + event handling + file I/O)"
```

---

## Task 4: TS SDK + wasm-pack build config

**Files:**
- Create: `crates/web-view/sdk/package.json`, `crates/web-view/sdk/tsconfig.json`, `crates/web-view/sdk/build.mjs`, `crates/web-view/sdk/src/index.ts`
- Test: `wasm-pack build` succeeds

**Interfaces:**
- Produces: `@rofd/sdk` TypeScript package wrapping the wasm exports. `Rofd.init(container, config) -> Promise<Editor>`.

- [ ] **Step 1: Create the SDK package**

`crates/web-view/sdk/package.json`:
```json
{
  "name": "@rofd/sdk",
  "version": "0.1.0",
  "main": "dist/rofd_web_view.js",
  "types": "dist/rofd_web_view.d.ts",
  "scripts": {
    "build:wasm": "wasm-pack build --target web --out-dir sdk/dist ../",
    "build": "npm run build:wasm"
  }
}
```

`crates/web-view/sdk/src/index.ts`:
```typescript
import init, { WasmEditor } from '../dist/rofd_web_view.js';

export class Editor {
  private editor: WasmEditor;

  private constructor(editor: WasmEditor) {
    this.editor = editor;
  }

  static async create(canvas: HTMLCanvasElement, fontBytes: Uint8Array): Promise<Editor> {
    await init();
    const editor = await WasmEditor.new(canvas, fontBytes);
    return new Editor(editor);
  }

  loadOfd(bytes: Uint8Array): void {
    this.editor.load_ofd(bytes);
  }

  saveOfd(): Uint8Array {
    return this.editor.save_ofd();
  }

  handleKeydown(key: string, ctrl: boolean, shift: boolean): boolean {
    return this.editor.handle_keydown(key, ctrl, shift);
  }

  handlePointerDown(x: number, y: number, button: number): boolean {
    return this.editor.handle_pointerdown(x, y, button);
  }

  handlePointerUp(x: number, y: number, button: number): boolean {
    return this.editor.handle_pointerup(x, y, button);
  }

  handlePointerMove(x: number, y: number): boolean {
    return this.editor.handle_pointermove(x, y);
  }

  handleScroll(dx: number, dy: number): boolean {
    return this.editor.handle_scroll(dx, dy);
  }

  handleResize(width: number, height: number): boolean {
    return this.editor.handle_resize(width, height);
  }

  render(): void {
    this.editor.render();
  }

  get canUndo(): boolean { return this.editor.can_undo(); }
  get canRedo(): boolean { return this.editor.can_redo(); }
}
```

- [ ] **Step 2: Verify wasm-pack build**

Run: `cd crates/web-view && wasm-pack build --target web --out-dir sdk/dist`
Expected: PASS (produces `rofd_web_view.js` + `rofd_web_view_bg.wasm` + `rofd_web_view.d.ts` in `sdk/dist/`). If it fails, check the wasm32 compilation + wasm-bindgen export.

- [ ] **Step 3: Commit**

```bash
git add crates/web-view/sdk/
git commit -m "feat(web-view): TS SDK (@rofd/sdk) + wasm-pack build"
```

---

## Task 5: `examples/web-app` (Vite app)

**Files:**
- Create: `examples/web-app/package.json`, `examples/web-app/vite.config.ts`, `examples/web-app/tsconfig.json`, `examples/web-app/index.html`, `examples/web-app/src/main.ts`
- Modify: `Cargo.toml` (add examples/web-app to members - but it's not a Rust crate, so don't add it as a workspace member; just create the directory)

**Interfaces:**
- Produces: a Vite web app that loads the wasm, creates a canvas, wires DOM events, and provides file open/save.

- [ ] **Step 1: Create the Vite app**

`examples/web-app/package.json`:
```json
{
  "name": "rofd-web-app",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "build:sdk": "cd ../../crates/web-view && wasm-pack build --target web --out-dir sdk/dist"
  },
  "dependencies": {
    "@rofd/sdk": "file:../../crates/web-view/sdk"
  },
  "devDependencies": {
    "typescript": "^5.0.0",
    "vite": "^5.0.0"
  }
}
```

`examples/web-app/index.html`:
```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>rofd - OFD Editor</title>
  <style>
    body { margin: 0; overflow: hidden; background: #E0E0E0; }
    canvas { display: block; width: 100vw; height: 100vh; }
    #file-input { position: absolute; top: 10px; left: 10px; z-index: 10; }
  </style>
</head>
<body>
  <input type="file" id="file-input" accept=".ofd" />
  <canvas id="canvas"></canvas>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

`examples/web-app/src/main.ts`:
```typescript
import { Editor } from '@rofd/sdk';

async function main() {
  const canvas = document.getElementById('canvas') as HTMLCanvasElement;
  const fileInput = document.getElementById('file-input') as HTMLInputElement;

  // Resize canvas to window.
  const resize = () => {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
  };
  resize();

  // Create the editor (empty font bytes for v1 - text won't render).
  const editor = await Editor.create(canvas, new Uint8Array(0));

  function render() { editor.render(); }

  // File open.
  fileInput.addEventListener('change', async () => {
    const file = fileInput.files?.[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    editor.loadOfd(bytes);
    render();
  });

  // Keyboard.
  canvas.tabIndex = 0;
  canvas.focus();
  canvas.addEventListener('keydown', (e) => {
    const key = e.key;
    if (editor.handleKeydown(key, e.ctrlKey || e.metaKey, e.shiftKey)) { render(); }
    if (e.key === 'Tab' || e.key === 'Backspace' || e.key.startsWith('Arrow')) { e.preventDefault(); }
  });

  // Mouse.
  canvas.addEventListener('pointerdown', (e) => {
    if (editor.handlePointerDown(e.offsetX, e.offsetY, e.button)) { render(); }
  });
  canvas.addEventListener('pointerup', (e) => {
    if (editor.handlePointerUp(e.offsetX, e.offsetY, e.button)) { render(); }
  });
  canvas.addEventListener('pointermove', (e) => {
    if (editor.handlePointerMove(e.offsetX, e.offsetY)) { render(); }
  });

  // Scroll.
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (editor.handleScroll(e.deltaX, e.deltaY)) { render(); }
  });

  // Resize.
  window.addEventListener('resize', () => {
    resize();
    if (editor.handleResize(canvas.width, canvas.height)) { render(); }
  });

  // Initial render.
  render();
}

main();
```

`examples/web-app/vite.config.ts`:
```typescript
import { defineConfig } from 'vite';

export default defineConfig({
  server: { fs: { allow: ['..'] } },
  optimizeDeps: { exclude: ['@rofd/sdk'] },
});
```

`examples/web-app/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "lib": ["ESNext", "DOM", "DOM.Iterable"]
  }
}
```

- [ ] **Step 2: Build the SDK + install deps + build the app**

Run:
```bash
cd examples/web-app
npm run build:sdk
npm install
npm run build
```
Expected: PASS (Vite builds the app). If `npm install` fails (missing Node.js), report BLOCKED. If the wasm module fails to load (missing wasm-pack build), ensure `build:sdk` ran first.

- [ ] **Step 3: Commit**

```bash
git add examples/web-app/
git commit -m "feat(web-app): Vite web app (canvas + DOM events + file open)"
```

---

## Phase 5 Done - Definition of Done

- `rofd_web_view`: WebGpuRenderTarget (WebGPU canvas + vello renderer, RenderTarget impl), WasmEditor (wasm-bindgen surface + event handling + file I/O), TS SDK (@rofd/sdk).
- `examples/web-app`: Vite app that loads the wasm, creates a canvas, wires DOM events, file open.
- Tests: WasmEditor parse_key (3 unit tests, native). WebGpuRenderTarget + web-app are compile-check + manual run (needs a browser).
- `cargo check -p rofd-web-view --target wasm32-unknown-unknown` compiles. `wasm-pack build --target web` succeeds. `cd examples/web-app && npm run build` succeeds.
- **Manual run:** `cd examples/web-app && npm run build:sdk && npm install && npm run dev` opens a browser at localhost:5173 with the OFD editor.

## v1 Complete!

After Phase 5, rofd v1 is feature-complete:
- **Phase 1:** dom + io (parse/surgical save/write)
- **Phase 2:** render (Vello scene + annotation overlay + hit_test/caret)
- **Phase 3:** editor (annotation CRUD + undo/redo)
- **Phase 4a:** component (EditorComponent facade)
- **Phase 4b:** native-view + native-app (runnable native editor)
- **Phase 5:** web-view + web-app (runnable web editor)

Dual-platform (native + WASM), view + annotate, common-subset render, document-font text rendering, surgical save. The spec's v1 scope is complete.
