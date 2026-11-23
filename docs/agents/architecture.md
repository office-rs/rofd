# Architecture

> When to read: any task that changes crate boundaries, public APIs, or
> crosses layers; when you need the full crate map.

## Five layers — dependencies point strictly upward, reverse edges forbidden

Each crate's `[dependencies]` is the single source of truth for dependency
direction.

```
crates/convert-cli   ─┐
                      │
crates/convert-java  ─┤
                      ├─► rofd-convert ──┐
crates/convert-node  ─┤                  │
                      │                  │
crates/convert-py    ─┤                  │
                                        │
crates/convert-wasm  ─┘                  │
                                        │
crates/xilem-app      ─┐                │
                       ├─► xilem-view ─┤
                       │                ├──► component ─┬─► render ──┐
crates/web-app ─► web-view ─────────────┤                ├─► editor ──┴─► dom
   (JS + Vite,         (wasm-bindgen    │                │
    not a cargo        adapter)         │                │
    member)                             │                │
                                        │                └─► io ────────┘
crates/gpui-app ─► gpui-view ───────────┘
```

Converter hosts (CLI/bindings) only touch rofd-convert — isomorphic with
"editor hosts only touch component".

**Two historical deviations (as-built)**:

1. component stays io-free — io deps never sink into the component;
2. native-side io calls moved up into the hosts (`xilem-app/src/host/document_io.rs`,
   `gpui-app/src/host/document_io.rs`); the xilem-view / gpui-view adapters do
   not depend on io; web-view still holds io (the wasm host is itself the
   "host").

## Platform boundary (expansion of golden rules 6–7)

- The platform boundary sits **between component and the adapters**: the six
  crates at component and below (dom/io/render/editor/component/export) plus
  rofd-convert are platform-agnostic — no winit/web-sys/wgpu/arboard deps.
- xilem-view / web-view / gpui-view are the **only** three platform bindings;
  adapters do binding only. Functionality is consolidated in the core layer —
  pushing io/business logic into an adapter is an anti-pattern.
- The `src-tauri` Rust shell only starts the window + registers plugins and
  the two commands — it **depends on no rofd crate**.

## Crate map

| crate | path | responsibility | deps (rofd-internal) |
| --- | --- | --- | --- |
| `rofd-dom` | `crates/dom` | pure data model | — |
| `rofd-io` | `crates/io` | `parse_ofd` / `write_ofd` (full write + unknown-content passthrough) + `PackageHandle` | dom |
| `rofd-export` | `crates/export` | Scene→krilla PDF encoder (print/export artifact conversion) | — (no rofd-internal deps) |
| `rofd-render` | `crates/render` | `imaging::record::Scene` building + hit_test/caret_rect | dom |
| `rofd-editor` | `crates/editor` | annotation selection, command pattern, Step/Transaction/History | dom |
| `rofd-component` | `crates/component` | **the only integration entry** `OfdComponent`: ViewEvent, callbacks, dirty cache | dom + render + editor + office-rs-print (type layer) (**no io dep**) |
| `rofd-convert` | `crates/convert` | conversion session-layer facade: `Converter` / process-level font registry / budgets / `catch_unwind`; converter hosts import only this crate | component + io + render + export + dom |
| `rofd-convert-cli` | `crates/convert-cli` | bin `rofd-convert`: zero-dependency argv parsing / exit codes 0-2-3 / font registration warn-and-continue | rofd-convert |
| `rofd-convert-java` | `crates/convert-java` | JNI cdylib `rofdconvert` + Java L1/L2 (`io.github.officers`), Maven `io.github.office-rs:rofd-convert` thin/fat jars | rofd-convert + jni 0.21 |
| `rofd-convert-node` | `crates/convert-node` | napi cdylib `rofd_convert_node` (catch_unwind per method) + JS L2 (worker_threads pool/cli.js), npm `@office-rs/rofd-convert` skeleton | rofd-convert + napi 2 |
| `rofd-convert-py` | `crates/convert-py` | PyO3 extension `rofd_convert.rofd_convert_native` (leased instance + GIL-released convert), PyPI `rofd-convert` (abi3-py39 wheel) | rofd-convert + pyo3 0.30 |
| `rofd-convert-wasm` | `crates/convert-wasm` | wasm-bindgen `--target web`, pure CPU (warm instance + registerFont invalidation semantics), npm `@office-rs/rofd-convert-wasm` skeleton | rofd-convert + wasm-bindgen/js-sys |
| `rofd-xilem-view` | `crates/xilem-view` | thin masonry/xilem adapter: `OfdWidget` + `ofd()`/`ofd_with_config()` | component |
| `rofd-web-view` | `crates/web-view` | thin WASM adapter: `WasmOfd` + `WebGpuRenderTarget` + TS SDK | component + io + editor + dom + rofd-export + office-rs-print (type layer) + vello + imaging + imaging_vello + wgpu + web-sys + rofd-convert (shared page assembly) |
| `rofd-gpui-view` | `crates/gpui-view` | gpui adapter SDK: `OfdGpuiView` (Entity/Element) + `GpuiOfdHandle` + IME bridge + print-dialog state machine / discover-only services | component + dom + imaging + gpui-pre/gpui-pre-platform + office-rs-print (list_printers) + ttf-parser + image (**no io dep**) |
| `xilem-app` | `crates/xilem-app` | xilem host app (file dialogs + toolbar UI policy) | xilem-view + component + io + dom + xilem + rfd + rofd-convert (shared page assembly) |
| `gpui-app` | `crates/gpui-app` | gpui host app (multi-document session TabBar + toolbar/file menu/print panel) | gpui-view + io + export + component + dom + office-rs-print + gpui family/kit + memmap2/ttf-parser + rofd-convert (shared page assembly) |
| web-app | `crates/web-app` | Vite + TS host app (not a cargo member) | `@office-rs/rofd` (= `crates/web-view/sdk`) |
| `tauri-app` | `crates/tauri-app` | Tauri desktop host (the cargo member is `src-tauri`) | the tauri shell depends on no rofd crate; the frontend reuses web-app |

## The gpui line (since 2026-11-01, spec 2026-10-31)

rofd-gpui-view + gpui-app **coexist** with the xilem/tauri hosts (R1: no
replacement duty). The adapter's seven files follow the rword shape
(gpui_sink/font_cache copied and adapted from rword@4e0b25b6; the print
state machine/services copied from xilem-view). The parity denominator is
web-app — the per-id audit lives at
`docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`, the manual
checklist at `docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md`.

## Print architecture

The kernel seam sits in component (spec 2026-10-31):
`begin_print(PrintContent) -> PrintSession` with three content toggles
(watermark/annotations/seals, **all on by default** — spec 2026-11-06 D17:
`PrintContent::default()` is true for all three; hosts and convert both
derive via default). Page scenes/paper/page plan all go through the session
(`page_scene`/`page_parameters`/`plan`; the `1-3,5` range syntax is parsed
by office-rs-print — hosts never re-parse). Native one-call direct print is
`print_direct` (begin_print→plan→payload→`print_pages`).
`office-rs-print` is `default-features = false` at the WORKSPACE level
(its default feature is a link-time libcups dep on Linux); backend users
(component print-backend, the print_services adapters) enable the feature
explicitly, type-layer consumers inherit as-is.

## Cross-repo frozen surface (the rword ofd-export line)

rword's docx→OFD export (rword spec 2026-11-12-ofd-export) depends on this
repo's **rofd-dom + rofd-io** (git dependency following tags, spec
2026-11-11 D4); Scene→OFD replay lives in rword's own export crate (D5).
Consequence: **rofd-dom / rofd-io public faces are a cross-repo
commitment** — think about the rword side before changing these two crates'
public APIs; rofd modeled Clip / DrawParam dash etc. for that line (commit
8f35a32, D14). dom/io/render/editor carry `version` fields (crates.io
publish candidates); the rest stay path-only because they transitively
depend on git repos.
