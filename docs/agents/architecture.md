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
crates/sign ─► io ─► dom                 (verify; append a signature to the package a document carries)
component ─► sign (feature `sign`)        (the component signs the open document; off by default)
xilem-app / gpui-app ─► sign             (status bar, pre-save check, certificate store; wasm-exported by web-view)
```

Converter hosts (CLI/bindings) only touch rofd-convert — isomorphic with
"editor hosts only touch component".

**Three deviations (as-built)**:

1. component's own code stays io-free; rofd-sign, and with it io, join the
   component only behind its `sign` feature (off by default — convert and
   its bindings do not compile it);
2. native-side io calls moved up into the hosts (`xilem-app/src/host/document_io.rs`,
   `gpui-app/src/host/document_io.rs`); the xilem-view / gpui-view adapters do
   not depend on io; web-view still holds io (the wasm host is itself the
   "host");
3. the native hosts depend on `rofd-sign` directly (spec 2026-11-23 M2) for
   everything around the signature — status bar, pre-save check,
   certificate store, `VerifyOptions`: the adapters carry only the seal
   placement state and never depend on `rofd-sign`; web-view holds the wasm
   exports of the signing line. The signature itself is made by the
   component in every host.

## Platform boundary (expansion of golden rules 6–7)

- The platform boundary sits **between component and the adapters**: the six
  crates at component and below (dom/io/render/editor/component/export) plus
  rofd-convert and rofd-sign are platform-agnostic — no
  winit/web-sys/wgpu/arboard deps.
- xilem-view / web-view / gpui-view are the **only** three platform bindings;
  adapters do binding only. Functionality is consolidated in the core layer —
  pushing io/business logic into an adapter is an anti-pattern.
- The `src-tauri` Rust shell only starts the window + registers plugins and
  the two commands — it **depends on no rofd crate**.

## Crate map

| crate | path | responsibility | deps (rofd-internal) |
| --- | --- | --- | --- |
| `rofd-dom` | `crates/dom` | pure data model, incl. `PackageHandle` (the package a document came from, carried on `OfdDocument.package`) | — |
| `rofd-io` | `crates/io` | `parse_ofd` / `save_ofd` (preserving save: original entries + rewritten annotations) / `write_ofd` (full write + unknown-content passthrough); `refresh_document_level` (document-level fields + the handle's signature flags, from OFD.xml) / `generate_package` (`GeneratedPackage { package, warnings }`) / `annotations_unwritable` for the signing line | dom |
| `rofd-export` | `crates/export` | Scene→krilla PDF encoder (print/export artifact conversion) | — (no rofd-internal deps) |
| `rofd-render` | `crates/render` | `imaging::record::Scene` building + hit_test/caret_rect | dom |
| `rofd-editor` | `crates/editor` | annotation selection, command pattern, Step/Transaction/History | dom |
| `rofd-component` | `crates/component` | **an exit of the library** (with the TS SDK): `OfdComponent`: ViewEvent, callbacks, dirty cache, the modal seal placement state, `seal_pictures`; behind the `sign` feature: `sign` / `sign_pfx` / `verify_signatures` / `sign_refusal` / `prepare_sign` / `finish_sign`, and the re-exported types of `rofd_component::signing` | dom + render + editor + office-rs-print (type layer) + rofd-sign (optional, `sign` feature; own code io-free) |
| `rofd-sign` | `crates/sign` | signature verification; seal / digital-signature creation (SES v4, GB/T 35275) appended in place to the package a document carries (`sign_in_place`), and bytes in, bytes out for a package outside an editor (`sign`); certificate files and store | io + dom + RustCrypto sm2/sm3 (**no platform dep**) |
| `rofd-convert` | `crates/convert` | conversion session-layer facade: `Converter` / process-level font registry / budgets / `catch_unwind`; converter hosts import only this crate | component + io + render + export + dom |
| `rofd-convert-cli` | `crates/convert-cli` | bin `rofd-convert`: zero-dependency argv parsing / exit codes 0-2-3 / font registration warn-and-continue | rofd-convert |
| `rofd-convert-java` | `crates/convert-java` | JNI cdylib `rofdconvert` + Java L1/L2 (`io.github.officers`), Maven `io.github.office-rs:rofd-convert` thin/fat jars | rofd-convert + jni 0.21 |
| `rofd-convert-node` | `crates/convert-node` | napi cdylib `rofd_convert_node` (catch_unwind per method) + JS L2 (worker_threads pool/cli.js), npm `@office-rs/rofd-convert` skeleton | rofd-convert + napi 2 |
| `rofd-convert-py` | `crates/convert-py` | PyO3 extension `rofd_convert.rofd_convert_native` (leased instance + GIL-released convert), PyPI `rofd-convert` (abi3-py39 wheel) | rofd-convert + pyo3 0.30 |
| `rofd-convert-wasm` | `crates/convert-wasm` | wasm-bindgen `--target web`, pure CPU (warm instance + registerFont invalidation semantics), npm `@office-rs/rofd-convert-wasm` skeleton | rofd-convert + wasm-bindgen/js-sys |
| `rofd-xilem-view` | `crates/xilem-view` | thin masonry/xilem adapter: `OfdWidget` + `ofd()`/`ofd_with_config()` | component |
| `rofd-web-view` | `crates/web-view` | thin WASM adapter and **the other exit of the library**: `WasmOfd` + `WebGpuRenderTarget` + TS SDK `@office-rs/rofd` (signing on the instance and as module functions; it agrees with the component, spec §17.13) + the wasm exports of the signing line | component + io + editor + dom + rofd-export + office-rs-print (type layer) + vello + imaging + imaging_vello + wgpu + web-sys + rofd-convert (shared page assembly) + rofd-sign |
| `rofd-gpui-view` | `crates/gpui-view` | gpui adapter SDK: `OfdGpuiView` (Entity/Element) + `GpuiOfdHandle` + IME bridge + print-dialog state machine / discover-only services | component + dom + imaging + gpui-pre/gpui-pre-platform + office-rs-print (list_printers) + ttf-parser + image (**no io dep**) |
| `xilem-app` | `crates/xilem-app` | xilem host app (file dialogs + toolbar UI policy + signing UI) | xilem-view + component + io + dom + xilem + rfd + rofd-convert (shared page assembly) + rofd-sign |
| `gpui-app` | `crates/gpui-app` | gpui host app (multi-document session TabBar + toolbar/file menu/print panel/signing panel) | gpui-view + io + export + component + dom + office-rs-print + gpui family/kit + memmap2/ttf-parser + rofd-convert (shared page assembly) + rofd-sign |
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

## Signing line (since 2026-11-23, spec 2026-11-23)

`rofd-sign` verifies signatures and appends new ones - in place, to the
package the document carries (spec §17.11 of the signing design); dom carries
the read-only signature projection and the package itself, io the
preserving save (a signed document can still gain annotations), render draws
the seals, component owns the modal seal placement state and, behind its
`sign` feature, signs and verifies the open document in one call. **The
component and the TS SDK are the exits of the library** (spec §17.13, the
owner's ruling of 2026-11-25): a signing capability is on both, under the
same name, or the spec says why not. The picture, the
invariants and the pitfalls: [sign.md](sign.md); the save: [io.md](io.md).

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

One break was taken on purpose on 2026-11-25, at the owner's word (signing
spec §17.11): `LoadReport` lost its `package` field, `save_ofd` lost its
`package` argument, and `OfdDocument` gained `package`. rword uses neither
of the first two; its `OfdDocument` literal
(`crates/rword-export/src/ofd.rs`) takes what it leaves empty from
`Default` since its branch `fix/ofd-document-default-fields` (made
2026-11-25; the owner pushes it and opens the PR), so it needs nothing
when it moves off its tag. `write_ofd` and `write_ofd_with_text_layout`
are unchanged ([io.md](io.md) §cross-repo reminder).
