# io: parse / full write / write fidelity / IDs / error layering

> When to read: any task touching `crates/io`, the save path, write-fidelity
> tests, or the cross-repo (rword) API face. Authoritative spec:
> `2026-11-11-rofd-full-ofd-write-design.md` (full write + passthrough).

## Errors are explicitly layered, never silently swallowed

- **Failure** = `rofd_io::OfdError` (`Zip`/`Xml`/`Schema`/`Io`, thiserror) —
  package/structural and unrecoverable, thrown straight up.
- **Degradable input** = `OfdWarning` (`MissingFeature` and other variants) —
  parsing continues, the model stays usable, and the warning is never lost:
  `parse_ofd` delivers it via
  `LoadReport { document, package, warnings }`; on the component side
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

## Full write (spec 2026-11-11; the surgical save is retired, D1)

`write_ofd_with_text_layout(doc, pkg: Option<&PackageHandle>, text_layout: &TextLayoutHints)`
is the only write entry; `write_ofd(doc)` is the pure-generation convenience
face (no package, no hints). The SDK's external signature is unchanged (D8:
the hosts' `Some(pkg)`/`None` dual path merged into the full write).

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

## Test duty

After changing io save logic these must stay green:

```bash
cargo test -p rofd-io write_fidelity   # parse→write→reparse model equivalence + passthrough bytes intact + stable IDs + same-model-same-bytes
cargo test -p xilem-app --test c2_save -- --ignored   # real samples (#[ignore])
```

When a newly modeled object type (e.g. Clip, DrawParam dash) lands in dom,
add the round-trip assertions to `write_fidelity.rs` in the same change.

## Cross-repo reminder

`rofd-dom` + `rofd-io` are the frozen face for rword's ofd-export line (see
[architecture.md](architecture.md) §cross-repo frozen surface): a public API
change is a cross-repo break.
