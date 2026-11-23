# rofd-render

> Auto-loaded when you work in this subtree. Rendering invariants:
> docs/agents/rendering.md; font system: docs/agents/fonts.md (the
> FontStore/slice chains/TTC anchoring all live under this crate's `text/`).

- Produces `imaging::record::Scene` (imaging Painter API), **never
  vello::Scene** (golden rule 5); backend conversion lives in the adapters.
- `text/font.rs` FontStore: `register_font` (refuses whole-font shadowing
  when a slice chain anchors the family), `register_font_slice` (internal
  name `__rofs_<key>_<sliceId>`); any successful registration clears the
  glyph/wrapped caches.
- `text/family.rs`: the `family_list_for` fallback chain + `PICKER_ALIASES`
  (listed only when the canonical family is deliverable).
- `text/shape.rs`: TTC multi-family names anchor to the canonical name by
  the lowest face index — **cross-process byte determinism** (fontique's
  HashMap iteration order is per-process random); convert parity depends
  on it.
- `layers.rs` `page_body_scene`: assembles the scene per OFD layer
  semantics; equivalence is pinned by `tests/layers_equivalence.rs`. Tests
  use scene structure assertions (object counts/types), not pixels.
