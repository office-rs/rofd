# rofd-component

> Auto-loaded when you work in this subtree. Annotation editing/format
> panel/IME: docs/agents/editing.md; fonts: docs/agents/fonts.md; print
> architecture: docs/agents/architecture.md.

**The only integration entry**, `OfdComponent`. `new_native()` /
`new_wasm()` (`#[cfg(target_arch = "wasm32")]`-gated, not feature-gated);
wasm callbacks are not `Send`. **No io dependency** (io-free; warnings only
relay from the adapter via `fire_warnings` — see docs/agents/io.md).

- `OfdConfig`: `default_font_bytes`, `page_gap` (default 20), `zoom`
  (initial PX_PER_MM, overridable via `with_zoom`).
- Scene dirty cache (final public API): `update_scene()` recomposes per the
  dirty flags, `scene() -> &Scene`, `set_viewport_size(f64,f64)`,
  `mark_scene_dirty` (pub(crate)).
- Text-editing API: `paste_text`, `delete_annotation`, `tick_blink`
  (unified on both targets; wasm is driven by `renderFrame`), and the
  preedit state machine (the component owns `Option<PreeditState>`).
- Creation tools / flowed layout / rich text / selection / format panel /
  save hints: docs/agents/editing.md (the 2026-11-09 / 11-10 spec series).
- Print kernel seam (spec 2026-10-31): `begin_print(PrintContent) ->
  PrintSession` (content toggles default all-on, D17); page scenes/paper/
  page plan all go through the session (`page_scene` / `page_parameters` /
  `plan`); native one-call direct print via `print_direct`
  (begin_print→plan→payload→`print_pages`).
- Seal placement (`seal_placement.rs`, spec 2026-11-23 §11):
  `begin_seal_placement` / `cancel_seal_placement` / `seal_placement_active`
  and the `on_seal_placed` / `on_seal_placement_cancelled` callbacks. Modal:
  `handle_event` offers every event to it first; only scrolling, page
  turns, zoom, resize and focus go through. The preview lives in the
  per-frame overlay with the opacity baked into its pixels. The component
  only reports the position — signing is one call on the component (next
  bullet).
- **The component is an exit of the library** (spec §17.13, the owner's
  ruling of 2026-11-25); the TS SDK is the other one, and the two agree —
  capability by capability, name by name (the table: sign.md §the two
  exits). A signing capability goes on both in the same change, or the spec
  says why not. Not on the component, on purpose: the save and its flags,
  the certificate store (the component is io-free).
- Signing (`signing.rs`, spec §17.11, §17.13) sits behind the **non-default
  Cargo feature `sign`**: a host that signs enables it, everyone else does
  not compile the cryptography. `sign_refusal` (ask first, to say it in the
  host's words), `sign(&mut signer, &options)` — one call, the signature is
  appended to the package the open document carries —, `sign_pfx(pfx,
  password, &options)` (the same with the certificate file opened inside;
  what the SDK's `ofd.sign` is), `verify_signatures(&VerifyOptions)` (the
  document that is open, saved or not; what the SDK's
  `ofd.verifySignatures` is), and `prepare_sign` / `finish_sign`
  (`PreparedSignature`, `SignStep`), the two-phase form for a signer that
  is not in this process. The types a host meets are rofd-sign's and are
  re-exported from `rofd_component::signing`. Three rules: **preparing
  changes nothing** (the document is first touched when `finish_sign` lands
  the signature); **two refusals, nothing written** — a file that cannot
  take the document's annotations (`AnnotationsUnwritable`) and a package
  that changed between the two phases (`DocumentChanged`; annotation edits
  in between are no change); **a signature is not an undo step** — its
  warnings go out through `fire_warnings` (the host's `on_warning`; one
  batch when there is something to say, never an empty one), then
  `notify_document_change` goes out like an annotation edit (modified,
  print sessions stale, `on_change`) and `invalidate_pages` raises
  `body_revision`, which is what makes the page scenes and the CPU
  fallback's page bitmaps stale; the view, the selection, the cursor being
  edited and the history stay. Never `load_document` for a signature — it
  is for another document and puts the view back at the top, drops the
  selection and clears the history (the owner noticed: signing on page 2
  jumped to page 1). Tests: `cargo test -p rofd-component --features sign`.
- `seal_pictures()` (no feature needed): the seal picture of every signature
  in list order, the model's own buffers — what the SDK's `sealPictures()` is;
  hosts read it instead of walking `document().signatures`.
- `font_families` / `set_available_font_families` / `annotation_text_style` /
  `annotation_format_anchor` / the three format setters: the panel data and
  command face (selection-aware); UI is adapter-built. The size table /
  color palettes live in `component::text_format`.
- `needs_frame() -> FrameNeed`: the single source of truth for web
  on-demand rendering (spec 2026-10-13).
