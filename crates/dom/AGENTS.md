# rofd-dom

> Auto-loaded when you work in this subtree. The pure data-model crate, no
> rofd-internal dependencies.

- `OfdWarning` / `ResourceKind` are defined here (io merely re-exports
  them) — so component can reference them without depending on io (the
  dependencies-point-upward invariant).
- Passthrough types: `RawFragment` plus the `raw_children` / `raw_attrs`
  slots on known nodes — parse captures unmodelled subtrees/attributes
  verbatim (namespace synthesis included), and write re-emits them
  (docs/agents/io.md).
- The annotation model: `Annotation` /
  `AnnotationPayload::TextBox { spans, .. }` / `TextLayoutHints` (save-time
  line geometry, consumed by io).
- **Cross-repo frozen-surface member** (rword's ofd-export line depends on
  rofd-dom + rofd-io via git tags): a public API change is a cross-repo
  break. Carries a `version` field (crates.io publish candidate).
- When modeling a new object type (e.g. Clip / DrawParam dash, rword spec
  D14), add the write-fidelity round-trip in the same change.
