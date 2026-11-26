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
- `OfdDocument.signatures` (`signature.rs`) is a **read-only projection**
  of the package's signatures — filled by io at parse time, never written
  from the model; the editor's commands and the hit test leave it alone
  (docs/agents/sign.md). `highest_known_object_id()` feeds the ID seed of a
  signed document's save; it counts every ID of the document — page
  objects, page IDs, document-level passthrough fragments, resource IDs,
  annotations — because an ID it misses is one a new annotation takes.
- `OfdDocument.package` (spec 2026-11-23 §17.11): the package the document
  came from. `PackageHandle` / `PkgEntry` / `EntryKind` are plain data,
  defined in `package.rs` and re-exported by io. Filled by `parse_ofd`,
  `None` for a document with no file behind it, never serialized. It is the
  document's own: a host keeps no second handle that could drift from it,
  a save preserves it, and a signature is appended to it in place
  (`rofd-sign`) — after which io's `refresh_document_level` rereads what
  the model keeps of OFD.xml, Document.xml and the signature entries. A
  document-level field added to `OfdDocument` goes into that refresh and
  into its tests (`crates/io/tests/document_package.rs`;
  `crates/sign/tests/sign/in_place.rs` pins that a signed document equals
  a reload), or a document that was just signed lags behind its package.
- **Cross-repo frozen-surface member** (rword's ofd-export line depends on
  rofd-dom + rofd-io via git tags): a public API change is a cross-repo
  break. Carries a `version` field (crates.io publish candidate). Adding a
  field is still a break for struct literals: the one rword has
  (`crates/rword-export/src/ofd.rs`) takes what it leaves empty from
  `Default` since its branch `fix/ofd-document-default-fields` (the `package`
  break was taken on purpose on 2026-11-25, at the owner's word), so a
  field added here costs it nothing - any other literal downstream would
  not compile.
- When modeling a new object type (e.g. Clip / DrawParam dash, rword spec
  D14), add the write-fidelity round-trip in the same change.
