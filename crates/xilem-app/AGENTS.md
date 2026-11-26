# xilem-app

> Auto-loaded when you work in this subtree. Widget contracts:
> crates/xilem-view/AGENTS.md.

The xilem native host app (file dialogs + toolbar UI policy). **A host is
not a library deliverable** (golden rule 6); this crate holds io
(native-side io calls live in the host layer, `src/host/document_io.rs`).

- A pure `Xilem::new_simple` host. AppState is plain data (the command
  queue + file paths + modified + has_selection + warnings +
  context_menu) plus an `Arc<Mutex<Option<Result<(), String>>>>` save-result
  slot: save commands report their on-disk result there, `app_logic` drains
  it up front — only a success clears `modified`, a failure keeps the
  unsaved state. The queue's and the slot's locks are held only briefly.
- File I/O lives in `src/host/document_io.rs`:
  `LoadedOfd{document, warnings, signatures}`, `load_ofd`, `save_bytes`
  (the preserving bytes a host save writes) and `save_ofd`
  (the same bytes, written atomically; `tests/c2_save.rs` runs it on the
  real sample). Neither is the full write: a host reaches that only after
  the user's word on a blocked save (M16), through
  `write_ofd_with_text_layout`. A host save goes through
  `signing::flow::inspect_save`, which runs the pre-save check first;
  `HostOutcome::Save` carries the file's signatures and the save's notes
  (`SaveDone`), which reach the status bar. The package travels with the
  document (`document.package`); the app keeps no handle of its own.
- Signing line (spec 2026-11-23 §13, docs/agents/sign.md): `src/signing/` —
  the decisions in `mod.rs` (the same as the gpui host's), the
  command-queue flow in `flow.rs`, the views in `panel.rs`. The component
  is only reachable at drain time, so commands snapshot into slots and
  `app_logic` acts on them the next pass (`drain_signing`,
  `drain_save_candidate`). The password field shows bullets and rebuilds
  the password from them: the pinned xilem has no masked text input.
  Signing writes nothing (M13 as revised on 2026-11-24): `queue_sign`
  pushes one command that opens the signer and builds the options
  (`signing_material`), asks `sign_refusal` first and signs on the
  component in one call (`c.sign`); the outcome lands in
  `AppState.sign_outcome` (`SignOutcome`), and `drain_signing` stores the
  reports, sets `modified` and refreshes the seal pictures through
  `refresh_seal_pictures` (not `document_replaced`, which is for a swap).
  The signature is in the document that is open (spec §17.11): nothing is
  loaded again, so the view, the selection and the undo history stay, and
  the next save is an ordinary save; AppState keeps no package handle and
  no signed bytes.
- CLI path arguments load through `host::document_io::load_ofd`; the app
  starts with an empty font config (`OfdConfig::new(Arc::new(vec![]))`,
  glyphs left to system font fallback).
- Real-sample test of the preserving save through the host:
  `cargo test -p xilem-app --test c2_save -- --ignored`.
- Print manual-test channel: `--example print_to_file` (the Pages payload
  lands in a file via GDI lpszOutput).
