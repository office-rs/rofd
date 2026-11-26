# io: parse / preserving save / full write / write fidelity / IDs / error layering

> When to read: any task touching `crates/io`, the save path, write-fidelity
> tests, or the cross-repo (rword) API face. Authoritative specs:
> `2026-11-11-rofd-full-ofd-write-design.md` (full write + passthrough) and
> `2026-11-23-rofd-sign-design.md` §7 (preserving save).

## Errors are explicitly layered, never silently swallowed

- **Failure** = `rofd_io::OfdError` (`Zip`/`Xml`/`Schema`/`Io`, thiserror) —
  package/structural and unrecoverable, thrown straight up.
- **Degradable input** = `OfdWarning` (`MissingFeature` and other variants) —
  parsing continues, the model stays usable, and the warning is never lost:
  `parse_ofd` delivers it via
  `LoadReport { document, warnings }` (the document carries the package:
  `OfdDocument.package`, below); on the component side
  `fire_warnings(&[OfdWarning])` forwards to the `on_warning` callback (the
  component itself is io-free and never generates warnings — it only relays
  what the adapter hands it); on the convert side warnings are aggregated by
  kind (`(×N)` counts) and surfaced to all four runtimes via
  `ConvertOutput.warnings`.
- **`OfdWarning`/`ResourceKind` are defined in rofd-dom and merely
  re-exported by io** — so component can reference them without depending on
  io (the dependencies-point-upward invariant).
- The write path has no warning channel (malformed-input boundary): passthrough
  name collisions etc. are resolved by deterministic rules (below), without
  warning or failure.

## Parsing

`parse_ofd(bytes) -> Result<LoadReport, OfdError>`. `PackageHandle` plays two
roles: the entry source during parse, and the passthrough source during save
(its `consumed` set records the zip member names parse actually read into the
model). Unmodelled XML subtrees/attributes are captured verbatim at parse
time (`RawFragment`, with ancestor `xmlns` declarations synthesized into the
fragment); the DOM holds known/unknown-interleaved child sequences to
preserve positions.

**The document carries its package** (spec 2026-11-23 §17.11):
`parse_ofd` puts the handle into `OfdDocument.package`, and no host keeps a
second one that could drift from the document. `None` for a document that
has no file behind it (a new one) — it saves by the full write until it has
one. `PackageHandle`, `PkgEntry` and `EntryKind` are defined in dom
(`package.rs`; plain data) and re-exported here, so `rofd_io::PackageHandle`
still resolves. The field is not serialized.

Three functions serve the signing line on that package (spec §17.11,
§17.13):

- `refresh_document_level(&mut document)` rereads what the model keeps of
  OFD.xml, Document.xml and the signature entries from the document's own
  package — through the code `parse_ofd` uses — and leaves pages,
  resources and annotations alone. It also writes the handle's two
  signature flags (`signatures_declared`, `signatures_entry`) from what
  OFD.xml declares, the source a load fills them from; nothing keeps them
  in step by hand any more. A signature appended in place calls it, so the
  document-level fields, and the handle, are what loading the package would
  give.
- `generate_package(&document, &text_layout) -> GeneratedPackage { package,
  warnings }` is the full write as a handle, for a document that has no
  package or none that can take a patch to Document.xml; the document's own
  package is the passthrough source. The warnings are what parsing the
  package it wrote said — they are returned, not dropped, and the landing
  passes them to the host.
- `annotations_unwritable(&package, page_ids, &annotations) -> bool` is the
  decision behind `Preserved::blocked_by_signature`, shared with a signature
  that refuses the same document before anything is made.

## Preserving save (spec 2026-11-23)

Every save that has the original package goes through
`save_ofd(&document, &text_layout) -> Result<Preserved, OfdError>` — the
package is the document's own (`serialize/preserving.rs`);
`write_ofd_preserving(&package, …)` is the function below it, with the page
IDs and the ID seed as explicit inputs. What it does:

1. every entry of the original package is kept byte for byte under its
   original name;
2. only the annotation entry file and the page annotation files are
   rewritten, by the full write's own annotation serializer (the same
   annotation bytes either way), in place under their original names — a
   page annotated for the first time gets a new file that collides with
   nothing, a page emptied keeps its file as an empty `<ofd:PageAnnot>`.
   The entry file names each page file relative to its own directory —
   through `..` when a page file lives elsewhere, as in the old layout (a
   file in each page directory, no entry file); nothing is ever moved;
3. Document.xml is patched in at most two places, and only for a package
   that declares no signature: `MaxUnitID` is raised to the final ID when
   needed (values compared, never a flag), and the annotation entry is
   declared when the document has none (inserted where appendix A orders
   it). A package that declares signatures is never patched (spec I5);
4. without a package (a new document) `save_ofd` falls back to the full
   write.
5. a signature the document carries is written as it was made: its entries
   sit in the package, and the save returns them byte for byte with every
   other protected entry (spec §17.11). The save after a signature is an
   ordinary save.

**Locs** live in `paths.rs`: `resolve_loc` is the one way a loc becomes a
package path (`.` and `..` resolved, a leading `/` is package-absolute),
`relative_to` is its inverse. Never join a loc by hand and never write a
package path where a loc belongs: a reader resolves it against the
directory of the file that holds it.

`read_all_entries` reserves at most 16 MiB of the size a zip header
declares — it is the writer's word — and lets the buffer grow with what is
read.

The body is not an input (spec I6): nothing can hand a body change to it.
`Preserved` says what happened: `document_xml` (which patch),
`full_write_fallback` (an unsigned package had no anchor for a required
patch, so the whole package was written — observable, spec M20),
`blocked_by_signature` (the package declares signatures, has no annotation
entry, and annotations had to be written: `bytes` is the package unchanged
and the host decides — spec M16) and `reserve_exhausted` (a signed
package's new IDs went past the ID reserve — spec M14).

**ID reserve** (spec M14): signing raises an unsigned document's
`MaxUnitID` by 65 536 so that later annotations fit under it while
Document.xml stays as signed. On a signed document the serializer therefore
numbers new objects from the highest ID the model knows
(`preserving_id_seed`), not from `MaxUnitID`; on an unsigned one from
`object_id_seed` as before. The editor's `create_annotation` follows the
same rule.

The three scaffolds a signature needs — annotation entry, ID reserve,
OFD.xml's `<ofd:Signatures>` — are applied by `rofd-sign` as patches when
the first signature lands in a package, not by an option of the save
([sign.md](sign.md)).

## Full write (spec 2026-11-11): the generation path

`write_ofd_with_text_layout(doc, pkg: Option<&PackageHandle>, text_layout: &TextLayoutHints)`
is the generation entry — no original package (a new document, rword's
export) or an explicit re-pack; `write_ofd(doc)` is the pure-generation
convenience face. A host's save does not go through it (see above).

The four write-fidelity invariants (D2):

1. everything parsed into the model must be written back losslessly;
2. unmodelled subtrees/attributes/entries must pass through — **three
   layers** (D3/D12):
   - **package layer**: `pkg`'s unconsumed entries are transported verbatim
     under their original names; on a collision with an emitted canonical
     name, **emitted wins** (the original entry is dropped);
   - **element layer**: `RawFragment`s are re-emitted at their captured
     positions (four interleave sites: OFD.xml DocBody, the Document root,
     CommonData, the Page root);
   - **attribute layer**: unmodelled attributes on known elements pass
     through verbatim (name/value, prefix included).
3. parsed object IDs are stable; new objects are numbered from
   `MaxUnitID`+1;
4. "shadow representations" inside annotations go through a **drop list**
   (`AllRichText`, `ParagraphFormat`) — they are another representation of
   content rofd rewrites; passing them through would produce stale
   duplicates.

Deterministic output: same model, same bytes. Canonical layout naming
(`Pages/Page_{i}/Content.xml`, `Annots/…`, `Doc_0/Res/…`); **the loc chain
(OFD.xml → Document.xml → the loc/BaseLoc elements) is the sole source of
truth** — non-canonical names in the original package are renamed to the
canonical form accordingly (D12). OFD.xml's DocInfo is emitted when a
modelled field is set OR a captured DocInfo fragment exists, with a fixed
field order (D3).

## ID conventions

- ST_ID: object IDs are unsigned integers **unique across the whole file**
  (GB/T 33190); serialization uses one document-wide allocator that first
  serializes the annotations, then writes the final `<MaxUnitID>`.
- Object IDs parsed into the model are never rewritten; new objects (e.g.
  newly created annotations) are numbered from above `MaxUnitID`+1
  (`object_id_seed`).
- Entry addressing trusts only the loc chain, never entry names (D12 above).
- On a signed document `MaxUnitID` is the reserve written at signing time, a
  ceiling rather than the highest ID in use: new IDs are minted from the
  highest known ID, not from `MaxUnitID` (the ID reserve, above).

## Test duty

After changing io save logic these must stay green. `--test` names the
binary; without it the word is a name filter - `cargo test -p rofd-io
write_fidelity` matched no test and exited 0:

```bash
cargo test -p rofd-io --test write_fidelity   # parse→write→reparse model equivalence + passthrough bytes intact + stable IDs + same-model-same-bytes
cargo test -p rofd-io --test preserving_save  # the preserving save (spec 2026-11-23 PS series: byte-identical entries, the two patches, blocked / reserve flags)
cargo test -p rofd-io --test document_package   # the document carries its package; the three functions of the signing line
cargo test -p rofd-sign --features test-ca --test sign in_place   # a signature appended in place; the file verifies after an ordinary save
cargo test -p rofd-io --test preserving_save -- --ignored  # the real sample, through the preserving save
cargo test -p xilem-app --test c2_save -- --ignored   # the real sample through the host's save (#[ignore]; the preserving save too)
```

When a newly modeled object type (e.g. Clip, DrawParam dash) lands in dom,
add the round-trip assertions to `write_fidelity.rs` in the same change.

## Cross-repo reminder

`rofd-dom` + `rofd-io` are the frozen face for rword's ofd-export line (see
[architecture.md](architecture.md) §cross-repo frozen surface): a public API
change is a cross-repo break. One was taken on purpose on 2026-11-25, at
the owner's word (spec §17.11): `LoadReport` has no `package` field,
`save_ofd` has no `package` argument, and `OfdDocument` gained `package`.
rword uses none of the first two; its `OfdDocument` literal
(`crates/rword-export/src/ofd.rs`) takes what it leaves empty from
`Default` since its branch `fix/ofd-document-default-fields` (verified on
the tag and against this checkout), so it needs nothing when it moves off
its tag. `write_ofd` and `write_ofd_with_text_layout` are unchanged.
