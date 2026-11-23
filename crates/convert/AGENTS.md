# rofd-convert

> Auto-loaded when you work in this subtree. The full line picture
> (CLI/bindings/demos/release lockstep): docs/agents/commands.md and
> docs/agents/workflow.md; spec 2026-11-06-rofd-convert.

The headless OFD→PDF conversion session-layer facade: converter hosts (CLI
+ the java/node/py/wasm bindings) **import only this crate** — component/
io/render/export are out of reach (golden rule 7).

- A long-lived `Converter` instance + the single-call
  `convert(format, ofd_bytes, opts)` — bytes in, bytes out (no
  open/edit/close three-phase); the `catch_unwind` boundary (panic →
  `ConvertError::Internal`; `catch_panics` is pub for the bindings' shared
  FFI discipline).
- **Process-level font registry** (D9/D19): registration must precede
  `Converter::new()` — the instance holds a registry snapshot
  (Arc-shared, zero-copy); append-only, no deregistration; identical bytes
  are an idempotent no-op; this is the single font-injection door (details
  in docs/agents/fonts.md §convert).
- Budget knobs: `ConvertOptions { content, max_pages, max_media_bytes }`,
  defaults `DEFAULT_MAX_PAGES` = 10000 / `DEFAULT_MAX_MEDIA_BYTES` = 1GiB;
  over-limit is a `BudgetExceeded` fail-fast (the check runs after parse —
  the zip-decompression memory peak precedes the budget check, the known
  D21 boundary).
- **Parity principle** (D18): convert and the three hosts share the page
  assembly `pdf_pages` / `pdf_pages_at` (the single assembly loop lives in
  this crate), byte-identical with the editor's "export PDF"
  (`tests/parity.rs` pins it).
- Cross-process-deterministic font-face selection: TTC anchored by the
  lowest face index (render/text/shape.rs — see docs/agents/fonts.md).
- The FontStore is rebuilt per call: no cross-call warm shaping cache (a
  long-lived instance buys registry sharing, not a warm cache); `Converter`
  is currently `Send` (D15 amended 2026-11-07 — tests pin the status quo,
  not a promise); L2 implementations should still treat instances as
  thread-bound.
- Warning stream (phase-3 D20): `ConvertOutput.warnings:
  Vec<ConvertWarning>` (non_exhaustive: `Parse(OfdWarning)` /
  `ParseRepeats{representative,count}` / `Encode(String)`) — parse warnings
  aggregated by kind (`(×N)` counts) plus export degradation lines,
  identical across all four runtimes.
