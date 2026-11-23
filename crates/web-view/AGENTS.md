# rofd-web-view (+ TS SDK `@office-rs/rofd`)

> Auto-loaded when you work in this subtree. Font channels:
> docs/agents/fonts.md; render scheduling/CPU fallback:
> docs/agents/rendering.md; the SDK lives in `sdk/`, built with wasm-pack
> `--target web`.

The WASM adapter: `WasmOfd` (wasm-bindgen) + `WebGpuRenderTarget` + the JS
event bridge; factory `create_wasm_ofd`. This crate is **the only adapter
holding io** (the wasm host is itself the "host"), plus editor/dom/export
(type layer) and rofd-convert (shared page assembly — byte-identical with
the editor's PDF export).

- **CPU fallback ordering invariant** (spec 2026-10-18): `create_wasm_ofd`
  probes the adapter (`compatible_surface: None`) BEFORE `create_surface` —
  once a canvas has a webgpu context bound it can never get a 2d context
  and the fallback breaks. Guarded by `npm run smoke:nogpu`; do not revert.
- **IME bridge** (2026-11-07): a canvas is not editable and the OS IME will
  not compose into it — the SDK uses a 1px transparent
  `<input data-rofd-ime>` to carry keyboard focus and composition
  (`compositionupdate` → `handleImePreedit`; `compositionend` →
  `handleImeCommit` + clear preedit + clear value; keydowns during
  composition (`isComposing`/`keyCode 229`) are dropped to prevent double
  insertion); the input is anchored at the caret via `caretRect()` (the
  candidate window follows the caret); the whole focus model moved from the
  canvas to that input.
- **Render scheduling** (spec 2026-10-13): the kernel's `needs_frame()` is
  the single source of truth; the wasm fused export `renderFrame()` returns
  `0` / `>0`ms / `-1`. The SDK self-suspends its scheduler
  (`sdk/src/scheduler.ts` `RenderScheduler` + `with-kick.ts` Proxy kick +
  watchdog): `0` → immediate rAF re-render; `>0` → a single `setTimeout`
  wake (blink deadline); `-1` → fully suspended. Any wasm method call via
  the Proxy kick schedules exactly one rAF (`renderFrame` exempt);
  `visibilitychange` fully silences a hidden tab; `pauseRendering` /
  `resumeRendering` are the host's policy handles. **Never** start a
  constant rAF loop.
- **Font flow** (`sdk/src/fontFlow.ts`): npm slices + local slices + the
  name-only common catalog + Chromium Local Font Access (details in
  docs/agents/fonts.md §web channel); `formatPanel.ts` reads the family
  list live.
- Entry `Ofd.init(container, config?)`; the SDK ships as the npm package
  `@office-rs/rofd` (the `sdk/` directory; `sdk/dist` is the wasm-pack
  output).
