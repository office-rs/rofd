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
- `font_families` / `set_available_font_families` / `annotation_text_style` /
  `annotation_format_anchor` / the three format setters: the panel data and
  command face (selection-aware); UI is adapter-built. The size table /
  color palettes live in `component::text_format`.
- `needs_frame() -> FrameNeed`: the single source of truth for web
  on-demand rendering (spec 2026-10-13).
