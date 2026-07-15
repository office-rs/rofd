# Adapter Wiring Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Wire existing ViewEvent (ScrollPage/ZoomAt/Ime) + 5 callbacks to native winit + web JS adapters.

**Architecture:** Native winit bridge maps PageUp/PageDown/Ctrl+wheel/IME to ViewEvent; web SDK exposes 5 callbacks + ScrollPage/ZoomAt methods. Component/render unchanged.

**Spec:** adapter wiring design (approved inline).

## Global Constraints
- component/render 不改（API 已就绪）。
- commits：conventional commits，无 attribution 行。
- fmt：`cargo fmt -- <files>`；stage specific files。

---

## Task 1: Native wiring

**Files:** `crates/native-view/src/winit_bridge.rs`, `examples/native-app/src/main.rs`

- PageUp/PageDown -> `ViewEvent::ScrollPage { Up/Down }` (winit Bridge translate).
- Ctrl+MouseWheel -> `ViewEvent::ZoomAt { factor, center: (cursor_x, cursor_y) }` (替代当前 Zoom；用 bridge 的 cursor 位置).
- winit `WindowEvent::Ime(Ime::Commit(text))` -> `ViewEvent::Ime { text }`.
- Test: winit_bridge 单元测试（PageUp -> ScrollPage; Ctrl+wheel -> ZoomAt with center）.

## Task 2: Web wiring

**Files:** `crates/web-view/src/wasm_editor.rs`, `examples/web-app/src/main.ts`

- WasmEditor: 加 `set_on_warning`/`set_on_annotation_focus`/`set_on_annotation_interact`/`set_on_page_change`/`set_on_zoom_change` (wasm-bindgen, 同 set_on_change 模式).
- WasmEditor: 加 `handle_scroll_page(direction: &str)` + `handle_zoom_at(factor: f64, cx: f64, cy: f64)` (wasm-bindgen).
- SDK TS: 加上述方法/回调接口.
- main.ts: 注册 5 回调; PageUp/PageDown keydown -> handle_scroll_page; Ctrl+wheel -> handle_zoom_at.
- Verify: wasm32 compile + native compile.

## Task 3: Verify + commit
- `cargo test --workspace` (per-crate) green; clippy + fmt + wasm clean.
