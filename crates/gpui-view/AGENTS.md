# rofd-gpui-view

> Auto-loaded when you work in this subtree. The gpui line coexists with
> xilem/tauri (spec 2026-10-31 R1 — no replacement duty). Fonts:
> docs/agents/fonts.md §gpui channel.

The gpui adapter SDK: `OfdGpuiView` (Entity/Element) + `GpuiOfdHandle` +
the IME bridge + the print-dialog state machine / discover-only services.
**No io dependency** (io lives in the gpui-app host layer).

- The adapter's seven files follow the rword shape (gpui_sink / font_cache
  copied and adapted from rword@4e0b25b6; the print state machine/services
  copied from the xilem-view precedent).
- The parity denominator is web-app; the per-id audit lives at
  `docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`.
- Fonts: default SimSun (simsun.ttc, empty config on read failure); declared
  families lazy-load local bytes through `font_index.rs` scanning the system
  font directories (mmap + all-languages name table extraction).
- File dialogs use gpui's native `prompt_for_paths` (no rfd).
- The annotation format-panel UI is implemented here (contrast:
  xilem-view's is pending).
- Pins gpui-pre `=0.3.10` / gpui-kit `=0.8.1` — **batch-bump only, never
  singly** (docs/agents/workflow.md §pinned dependencies).
- Print manual test: `cargo run -p gpui-app --example gpui_print_to_file`.
