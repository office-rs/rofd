# rofd-io

> Auto-loaded when you work in this subtree. **The main rules live in
> docs/agents/io.md** (preserving save / full write / write fidelity /
> three-layer passthrough / ID conventions / error layering; specs
> 2026-11-11-rofd-full-ofd-write and 2026-11-23-rofd-sign §7) — read it
> before touching this crate. Signing: docs/agents/sign.md.

Subtree essentials:

- `parse_ofd` delivers `LoadReport { document, warnings }` — the package is
  the document's own (`document.package`; `PackageHandle` is defined in dom
  and re-exported here, so `rofd_io::PackageHandle` still resolves).
  `PackageHandle`'s `consumed` set is the package-layer passthrough
  filter; its layout fields (`document_entry`, `annotations_entry`,
  `annotation_files`, `signatures_declared` / `signatures_entry`) tell the
  preserving save where the structural files live, under their original
  names.
- **Two writes.** A host's save is `save_ofd(&document, &text_layout)`
  (`serialize/preserving.rs`): original entries byte for byte, only the
  annotation entries rewritten, Document.xml patched in at most two places
  and never on a package that declares signatures. `write_ofd(_with_text_layout)`
  is the generation path (no original package).
- Signature entries are only projected (`parse/signature.rs`): never
  consumed, never re-encoded (spec 2026-11-23 I1).
- **Three functions serve the signing line** (spec §17.11, §17.13), all on
  the package the document carries:
  `refresh_document_level(&mut document)` rereads the document-level fields
  from it (through the code `parse_ofd` uses) and writes the handle's two
  signature flags from what OFD.xml declares — the same source a load fills
  them from, so nothing keeps them in step by hand;
  `generate_package(&document, &text_layout)` is the full write as a handle
  and returns `GeneratedPackage { package, warnings }` — do not drop the
  warnings, the caller passes them to the host;
  `annotations_unwritable(&package, page_ids, &annotations)` is
  the decision behind `Preserved::blocked_by_signature` and behind a
  signature's refusal — one function, so the two cannot disagree.
- Locs between files go through `paths.rs` (`resolve_loc`, `relative_to`):
  a page annotation file can live outside the entry file's directory (the
  old layout) and is then named through `..`; never join or strip a path by
  hand.
- `read_all_entries` reserves at most 16 MiB of an entry's declared size.
- After changing save logic, `cargo test -p rofd-io --test write_fidelity`
  and `cargo test -p rofd-io --test preserving_save` must stay green, and
  after changing the three functions above
  `cargo test -p rofd-io --test document_package`.
  `--test` matters: without it the words are a name filter, and a bare
  `cargo test -p rofd-io write_fidelity` runs zero tests and exits 0
  (real samples:
  `xilem-app --test c2_save -- --ignored`, which runs the preserving save).
- `rofd-io` + `rofd-dom` are rword's cross-repo frozen surface (git
  dependency following tags): a public API change is a cross-repo break —
  think about the rword side first (docs/agents/architecture.md
  §cross-repo frozen surface). One was taken on purpose on 2026-11-25, at
  the owner's word: `LoadReport.package`, the `package` argument of
  `save_ofd`, and `OfdDocument.package` (docs/agents/io.md).
