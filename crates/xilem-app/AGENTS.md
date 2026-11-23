# xilem-app

> Auto-loaded when you work in this subtree. Widget contracts:
> crates/xilem-view/AGENTS.md.

The xilem native host app (file dialogs + toolbar UI policy). **A host is
not a library deliverable** (golden rule 6); this crate holds io
(native-side io calls live in the host layer, `src/host/document_io.rs`).

- A pure `Xilem::new_simple` host. AppState is plain data (the command
  queue + file/package paths + modified + has_selection + warnings +
  context_menu) plus an `Arc<Mutex<Option<Result<(), String>>>>` save-result
  slot: save commands report their on-disk result there, `app_logic` drains
  it up front — only a success clears `modified`, a failure keeps the
  unsaved state. The queue's and the slot's locks are held only briefly.
- File I/O lives in `src/host/document_io.rs`:
  `LoadedOfd{document, package, warnings}`, `load_ofd`, `save_ofd` (full
  write + passthrough; atomic write). Save-As creates no new
  PackageHandle.
- CLI path arguments load through `host::document_io::load_ofd`; the app
  starts with an empty font config (`OfdConfig::new(Arc::new(vec![]))`,
  glyphs left to system font fallback).
- Real-sample save test: `cargo test -p xilem-app --test c2_save --
  --ignored`.
- Print manual-test channel: `--example print_to_file` (the Pages payload
  lands in a file via GDI lpszOutput).
