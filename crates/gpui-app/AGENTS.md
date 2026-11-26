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
- Signing line (spec 2026-11-23 §13, docs/agents/sign.md): `src/signing/` —
  the decisions in `mod.rs` (dialog validation, the pre-save check, the
  status / detail texts), the shell's methods in `flow.rs`, the panels in
  `panel.rs` (rendered the way `print_dialog.rs` is), the store file in
  `store_file.rs` (`%LOCALAPPDATA%/rofd/certificates.json`). Every save runs
  the pre-save check (`save_checked`) and asks before it would break a
  signature; `save_notes` (an exhausted ID reserve, a fallback to the full
  write) join the status line of a saved file. Signing writes nothing
  itself (M13 as revised on 2026-11-24): `sign_now` (`flow.rs`) is one call
  on the component — `signing_material` opens the certificate file and
  builds the options, `sign_refusal` is asked first so that the refusal is
  said in the dialog's words, then `component.sign` — and the signature
  lands in the document that is open (spec §17.11): nothing is loaded
  again, so the view, the selection and the undo history stay; the session
  is modified, and the next save is an ordinary save. The session keeps no
  package handle and no signed copy (`DocumentSession` reads the document
  through the component's save snapshot,
  `host::document_io::save_bytes`). The modal backdrops `occlude()`:
  without it the editor under a panel takes the focus back on every press.
  `tests` cover the flow in `TestAppContext`, including a frame that draws
  every panel.
