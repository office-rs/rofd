# gpui-app

> Auto-loaded when you work in this subtree. The adapter SDK:
> crates/gpui-view/AGENTS.md.

The gpui desktop host (multi-document session TabBar + toolbar/file
menu/print panel), **coexisting** with the xilem/tauri lines (spec
2026-10-31 R1 — no replacement duty). This crate holds io (io lives in the
host layer `src/host/document_io.rs`; the adapter depends on no io).

- The parity denominator is web-app; the per-id audit:
  `docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`; the manual
  checklist:
  `docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md`.
- Fonts: default SimSun (`%WINDIR%\Fonts\simsun.ttc`, empty config on read
  failure); declared families lazy-load local bytes through `font_index.rs`
  scanning the system font directories (memmap2 + ttf-parser — the same
  shape as the tauri desktop channel).
- File dialogs use gpui's native `prompt_for_paths` (no rfd).
- The gpui family pins `=0.3.10` / `=0.8.1` and bumps as one batch
  (docs/agents/workflow.md §pinned dependencies).
- Print manual test: `--example gpui_print_to_file`.
