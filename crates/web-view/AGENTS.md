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
- **Signing line** (spec 2026-11-23 §12, docs/agents/sign.md): wasm free
  functions `verifySignatures` / `signOfd` / `signPrepare` (+ `PreparedSign`,
  `StepResult`) / `certificateInfo` live in `src/sign_wasm.rs` — options
  and reports cross as JSON, parsed by plain functions that are tested
  natively (`cargo test -p rofd-web-view --lib sign_wasm`). `signOfd` /
  `signPrepare` are bytes in, bytes out, for a package that is not in an
  editor; the open document is signed through the instance methods (below).
  The SDK wraps the free functions as async module functions through one
  shared module loader (test seams `__testSetSignModule`,
  `__testSetSignLoader`); a failed load is not cached — the next caller
  loads again.
- **Preserving save**: `saveOfd` keeps the original package and rewrites
  only the annotation entries; `isSaveBlockedBySignature` /
  `isSaveReserveExhausted` / `isSaveFullWriteFallback` say what the last
  save had to do, `saveOfdFullWrite` is the explicit full write. They
  describe the last `saveOfd()` and nothing else: a host reads them right
  after it, not after `saveOfdFullWrite` or a document swap.
- **Seal placement**: `beginSealPlacement` / `cancelSealPlacement` /
  `setOnSealPlaced` / `setOnSealPlacementCancelled` (deferred to a
  microtask like every callback, so a handler may call back into rofd);
  `sealPictures` feeds the detail view's thumbnails.
- **The SDK is an exit of the library** (spec §17.13, the owner's ruling of
  2026-11-25): `rofd-component` is the other one, and the two agree —
  capability by capability, name by name (the table: sign.md §the two
  exits). Something signing-related added to one exit goes on the other in
  the same change, or the spec says why not.
- **On the instance** (spec §17.11, §17.13): methods on `WasmOfd`
  (`src/wasm_ofd.rs`), one for each of the component's — `sign(pfx,
  password, optionsJson)` (one call; the signature goes into the document
  that is open, the seal shows at the next frame, the view, the selection
  and the undo history stay), `verifySignatures(optionsJson)` (the document
  that is open, saved or not), `isSignBlocked()` (ask first: the file
  cannot take the annotations being edited), and `prepareSign` /
  `finishSign` for a signer elsewhere. An export is named as the SDK
  method is (`isSignBlocked`, like the `isSave*` family). `WasmOfd` keeps
  no package of its own: the document carries it, and `saveOfd` after a
  signature is an ordinary preserving save.
- **The two-phase form**: wasm `PreparedSignature` + `SignatureStepResult
  { more, reports }`; SDK `PreparedSignature` + `SignStep { done, next,
  reports }` — the component's names. The bytes side (`signPrepare`) is
  `PreparedSignOfd` + `SignOfdStep`. The plain half,
  `sign_wasm::step_result` (`StepOutcome`), is tested natively; it is
  compiled where its readers are (`cfg(any(wasm32, test))`), because this
  crate's `#[wasm_bindgen]` exports sit in
  `#[cfg(target_arch = "wasm32")]` modules and are never compiled natively.
  wasm-bindgen limits that shaped the surface: an exported struct cannot be
  declared inside a `#[wasm_bindgen] impl` block and a Rust enum is not
  exported, so `PreparedSignature` and `SignatureStepResult` sit at module
  scope. Each SDK class is good for one `apply` and says so itself
  (`#left` / `#take`): wasm has consumed the object by then. Native tests
  cannot build `WasmOfd`; the SDK wiring tests and the web-app e2e cover it.
  A test filter is a test name: `--lib a_finished_step`, not `step_result`.
- **Native tests cannot see wasm-only failures** — a wall-clock read in a
  dependency, a paint the CPU renderer refuses. The web-app e2e
  `e2e/sign.spec.ts` drives the real wasm on the WebGPU and on the CPU
  path: run it after touching `rofd-sign` or the seal preview.
