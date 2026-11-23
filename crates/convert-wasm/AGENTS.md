# rofd-convert-wasm

> Auto-loaded when you work in this subtree. The convert-line picture:
> crates/convert/AGENTS.md; build/demo: docs/agents/commands.md §four
> bindings.

The wasm binding (phase 4): wasm-bindgen `--target web`, **pure CPU, no
WebGPU**; the npm package `@office-rs/rofd-convert-wasm` skeleton. Depends
only on rofd-convert + wasm-bindgen/js-sys.

- The **warm instance** is reused across calls; a successful registration
  **invalidates** it (the next call rebuilds the snapshot — slightly looser
  than native D9; a single browser realm has no lease).
- Browser fonts arrive only via `registerFont(bytes)` (no system
  discovery).
- `panic = abort` posture (`catch_panics` compiles to a harmless guard).
- The `wasm-opt = false` metadata gate matches web-view (multi-table wasm
  compatibility).
- Acceptance: `node demo/run.mjs` — browser main + Worker == CLI bytes
  (needs Playwright).
