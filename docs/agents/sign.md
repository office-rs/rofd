# Signing: verification, creation, seal placement, preserving save

> When to read: any task touching `crates/sign`, the signature projection
> in dom / io / render, the seal placement state in component, a host's
> signing UI, or a host's save path. Authoritative spec:
> `2026-11-23-rofd-sign-design.md` (with its as-built amendment); the
> preserving save also amends `2026-11-11-rofd-full-ofd-write-design.md`.
> The save path itself: [io.md](io.md).

## Shape

| where | what |
|---|---|
| `rofd-dom` | `OfdDocument.signatures: SignatureSet` — read-only projection (id, kind, BaseLoc as written, stamps, seal picture). Never written from the model. `OfdDocument.package: Option<PackageHandle>` — the package the document came from (`package.rs` lives here since spec §17.11); a signature is appended to it. |
| `rofd-io` | `parse/signature.rs` lifts the projection; `serialize/preserving.rs` is the preserving save; `serialize/patch.rs` holds the byte patches; `paths.rs` resolves and writes the locs between files (`.` and `..` included). For the signing line: `refresh_document_level` (the model's document-level fields **and the handle's two signature flags**, from OFD.xml), `generate_package -> GeneratedPackage { package, warnings }`, `annotations_unwritable`. |
| `rofd-render` | `draw_seals` / `draw_seals_at`: seals are page content (cached with the page scene), drawn after the body and before annotations. |
| `rofd-component` | **an exit of the library.** Always: the modal seal placement state (`seal_placement.rs`), `seal_pictures()`, `on_warning`. Behind the `sign` feature (`signing.rs`): `sign`, `sign_pfx`, `verify_signatures`, `sign_refusal`, `prepare_sign` / `finish_sign`; the types a host meets are re-exported there (`rofd_component::signing`). |
| `rofd-sign` | `sign_in_place` / `prepare_in_place` / `verify_document` / `sign_refusal` (`in_place.rs`) for a document that is open — the landing returns `Landed { reports, warnings }`; bytes in, bytes out for a package that is not: `verify`, `sign`, `prepare_sign` + `Prepared`; `PfxSigner`, `CertificateStore`, the `Signer` trait, `flow.rs`. Test CA behind the `test-ca` feature. |
| `rofd-web-view` | wasm exports `verifySignatures` / `signOfd` / `signPrepare` / `certificateInfo`, and on the instance `sign` / `verifySignatures` / `isSignBlocked` / `prepareSign` / `finishSign` / `sealPictures`; the TS SDK wrappers (`verifySignatures` on the instance too, `SignStep { done, next, reports }`, `PreparedSignOfd` / `SignOfdStep`, `newlyInvalidated`), and the instance methods of the placement state. |
| `rofd-gpui-view`, `rofd-xilem-view` | the placement state only (verbs + `SealPlaced` / `SealPlacementCancelled`). They do **not** depend on `rofd-sign`. |
| `gpui-app`, `xilem-app`, `web-app` | the native two depend on `rofd-sign` directly (spec M2) for what is not on the component: verifying file bytes, the pre-save check, the certificate store. They sign through the component in one call and read seal pictures through it; none of them keeps a package handle (the document carries it). web-app goes through the SDK alone. |

The signing line's dependency edges: `sign -> io -> dom`; `component ->
sign`, optional behind the `sign` feature (off by default: convert, its
bindings and the two native adapters do not compile the cryptography).

## The two exits (spec §17.13)

The component and the SDKs are the exits of the library; they agree with
each other and with the person using them (the owner's principle,
2026-11-25). A capability belongs on both or on neither. What a host meets:

| To | `rofd-component` (feature `sign`, except seal pictures) | `@office-rs/rofd` |
|---|---|---|
| verify the document that is open | `component.verify_signatures(&VerifyOptions) -> Vec<SignatureReport>` | `await ofd.verifySignatures(options?) -> SignatureReport[]` |
| verify a file's bytes | `rofd_sign::verify(bytes, &opts)` | `await verifySignatures(bytes, options?)` (module function) |
| sign with a certificate file | `component.sign_pfx(pfx, password, &opts) -> Result<Vec<SignatureReport>, SignError>` | `await ofd.sign(pfx, password, options) -> SignatureReport[]` |
| sign with a signer of the host's own | `component.sign(&mut signer, &opts)` (`Signer` trait, one call) | `ofd.prepareSign(certificateDer, options)`, then `apply` per round |
| know whether signing is possible | `component.sign_refusal() -> Result<(), SignError>` | `ofd.isSignBlocked() -> boolean` |
| two-phase: prepare / land | `component.prepare_sign(cert_der, &opts) -> PreparedSignature`; `component.finish_sign(prepared, signature) -> SignStep` | `ofd.prepareSign(certificateDer, options) -> PreparedSignature`; `prepared.apply(signature) -> SignStep` |
| seal pictures | `component.seal_pictures() -> Vec<Option<Arc<Vec<u8>>>>` | `ofd.sealPictures() -> (Uint8Array \| null)[]` |
| placement state | `begin_seal_placement` / `cancel_seal_placement` / `seal_placement_active` / `on_seal_placed` / `on_seal_placement_cancelled` | `beginSealPlacement` / `cancelSealPlacement` / `setOnSealPlaced` / `setOnSealPlacementCancelled` |
| warnings | `on_warning` | the `onWarning` handler given to `Ofd.init(container, config)` — a load's warnings and a landing's both |

The two-phase types have the same names on both exits: `PreparedSignature`
and `SignStep` (`More(PreparedSignature) | Done(reports)` on the component;
`{ done, next, reports }` in the SDK, `reports` filled once `done`). In the
SDK each round is good for one `apply`, accepted, refused or failed — wasm
takes it with the call — so a refused answer means `prepareSign` again, not
a retry on the same object. The
SDK's bytes-side two-phase types carry `Ofd` (`PreparedSignOfd`,
`SignOfdStep`). The two-phase form exists for a signer whose answer does
not come back synchronously (an HTTP signing service; a `Signer` that can
be called in place covers a vendor DLL or a UKey). The bytes-side one is
for a package that is not open in an editor, and on the web it is the only
entry for that with a signer outside the page; it stays (the owner,
2026-11-26). Mind the one name left below the exits: `rofd_sign::SignStep`
is the bytes side of rofd-sign (`More(Prepared) | Done(Vec<u8>)`), not the
component's. It stays under that name too - the owner's ruling of
2026-11-26: a host does not use both forms (the editor with an external
signer, and headless bytes with an asynchronous one), so the names do not
meet; one that does imports aliases a name. Not on
the component, on purpose: the save and its flags
and the certificate store — the component is io-free, and where a
certificate file is kept is the host's business (the native hosts use
rofd-io and rofd-sign for them; the SDK has `saveOfd` and the `isSave*`
flags).

When something signing-related is added to one exit, add it to the other in
the same change, under the same name, or write down in the spec why not.

Golden rules that bind here: the body is read-only and signatures are only
ever appended (rule 2); the library never reads the clock — signing time
and the time a certificate is judged at are arguments (`now`), hosts read
the clock; `rofd-sign` is platform-agnostic and must build for
`wasm32-unknown-unknown`.

## Invariants (spec §4) and where they hold

- **I1 signature files are never re-encoded.** io projects `Signatures.xml`
  and `Sign_N/*` without marking them consumed, so they travel byte for
  byte; `rofd-sign` verifies against the raw TLV slices (`DerNode::raw`).
  The one exception is the list itself: GB/T 33190 18.1 keeps it out of
  the protected set precisely so signatures can be appended, so `sign`
  rewrites `Signatures.xml` whole (existing records kept as written).
- **I2 verification reads the package**, never the model (amended, spec
  §17.13): the package's bytes, or the package a document carries. A
  document's own package is what `verify_document` and
  `component.verify_signatures` read — no save is made for it.
- **I3 what was signed is exactly what is stored** — per file (spec §17.11):
  the signature's own files are written as they were made (never
  re-encoded), and the protected files are the very bytes that were hashed
  (the preserving save copies them). The zip container is made at save time
  and is not what a signature covers.
- **I4 References** (rofd's own signatures): every entry of the package
  except the signatures and the annotation layer, plus the signature's own
  `Seal.esl` — GB/T 48666-2026 8 c). Never its own `Signature.xml` or
  `SignedValue.dat`.
- **I5 a signed Document.xml is not touched** by a save or by a later
  signing.
- **I6 the preserving save cannot write body changes** — its inputs are the
  original package and the annotation model.
- **I7 a private key lives in memory until the signature is done**; the
  key type erases itself on drop.
- **I8 verification reports, signing fails.**

## Verification (`rofd_sign::verify`)

Three layers per signature, in list order: integrity (SM3 of every file the
References name), seal consistency (standalone `Seal.esl` == the seal
embedded in the signature value), signature value (the recorded data hash
== SM3 of `Signature.xml`, and SM2 over the raw to-be-signed bytes). The
certificate is reported, never judged beyond an optional validity period;
the chain and revocation are out of scope and the UI says so.

Verdicts: `Valid`, `Invalid`, `Unverified` (a layer could not run for lack
of material), `Unparseable` (the signature's own files are missing or
broken), `Unsupported` (recognised format, not supported — SES v5).
`Invalid` requires a check that actually failed: a layer that did not run
never makes a signature `Invalid`. One empty thing is a failure, though: a
signature whose reference list is empty or absent protects no file, so
layer 1 fails on it (`integrity.protected_files` is 0) whatever its
signature value says.

`flow::summarize` reduces the reports to
the status bar's phrase, `flow::newly_invalidated` names the signatures a
save takes from valid to anything else (both re-exported by
`rofd_component::signing`; the SDK has `newlyInvalidated`).

Where verification is entered from, and which one to use:

| you have | call |
|---|---|
| the document that is open | `component.verify_signatures(&opts)` / `await ofd.verifySignatures()` — the status bar speaks about the current document (M12 as it stands since spec §17.13: saved or not, a signature just made included) |
| a file that is not open, or the bytes a save is about to write | `rofd_sign::verify(&bytes, &opts)` / `await verifySignatures(bytes, options)` |

The pre-save check verifies the second kind: it is about the bytes that
would be written.

## Creating a signature (`rofd_sign`)

Two doors, one implementation (spec §17.11):

- **A document that is open**: `sign_in_place(&mut document, signer, opts)`
  appends the signature to the package the document carries and returns
  [`Landed`](#signing-in-place-spec-1711) — the reports of every signature
  afterwards and the warnings of the landing. `prepare_in_place` +
  `PreparedInPlace::apply` + `SignatureToAppend::append_to` are the same
  round by round for a signer outside the process (a token, a remote
  service). Hosts do not call these: the component does (`signing.rs`).
- **A package that is not in an editor** (bytes in, bytes out): `sign(ofd,
  signer, options) -> Result<Vec<u8>, SignError>`, and `prepare_sign` +
  `Prepared::apply` for the rounds. Built on the first door: parse, sign in
  place, zip. One difference: a package that cannot take the first
  signature's patch to Document.xml is **refused** here, where an open
  document is given a generated package first.

With a seal picture the signer signs twice (the seal's `eSealInfo`, then the TBS that
holds the seal and SM3 of `Signature.xml`); without one, once — SM3 of
`Signature.xml`, packed as a GB/T 35275 signed message with `Type="Sign"`.
A signature handed back by the signer is checked against the signer's
certificate before anything is kept.

What happens to the package, in order:

1. **Scaffolds**, when the signature list holds no record yet (a real
   first signature — also when OFD.xml declares a list the package does not
   carry): the annotation entry and its list file when the document has
   none — the list names the page annotation files the package already
   carries, wherever they are (a package in the old layout keeps its files
   in the page directories and lists them through `..`), `MaxUnitID`
   raised to the highest known ID + 65 536 (the ID
   reserve, M14), and `<ofd:Signatures>` in OFD.xml. A package that already
   has a signature is left as it is (I5).
2. **References** by I4, sorted by path. Zip **directory entries are not
   referenced** — strict readers reject a `FileRef` that names a directory.
   "Under the signature directory" is left out only when the list sits in a
   directory of its own; next to the document's own files, only the
   directories that hold a signature are.
3. **`Signature.xml`**: `sm2sign-with-sm3`, `now` as given, one StampAnnot
   per stamped page.
4. **IDs**: the signature ID is one above the largest numeric `MaxSignId`
   and `Signature@ID`; stamps follow. The list is rewritten with `MaxSignId`
   first (the schema's order; ofdrw writes it last).

Everything is hashed after step 1 and before any file of the new signature
exists. The scaffolds are made as **patches** first (`package_util.rs`:
`Scaffolds`, `Patch`) and nothing is written until the signature lands, in
one step (`SignatureToAppend::land`, below).

**Placement**: `At { page (from 1), boundary }`, or `BottomRight { pages,
right_mm, bottom_mm }` measured from the page's physical box. A seal that
would start outside the page is refused. The seal's long side is `size_mm`
(default 40 mm); the other side follows the pixel ratio; any image the
`image` crate reads is re-encoded as PNG. `MAX_SEAL_PICTURE_BYTES` (16 MiB)
bounds that PNG: a larger picture is refused, never embedded — it travels
inside every copy of the document.

**Certificate files** (`PfxSigner::open`): the modern layout (PBES2 / AES)
and the older one (3DES, RC2-40), including files written as BER with
indefinite lengths, which several CA tools still produce. A key is trusted
only after the public key derived from it equals the certificate's. Two
errors come out of it: `SignError::Password` — only when the file's own
integrity check rejected the password — and `SignError::Certificate`, a
file that cannot be used at all. A host shows the first as a wrong
password and never the second (spec section 16).

**Certificate store**: `CertificateStore` is in memory (`import` opens the
file once with the password and keeps the encrypted file, never the key);
`to_bytes` / `from_bytes` move it; `certificate_file` hands the file back
for `sign`. Where those bytes live is the host's business (`rofd/certificates.json`
in the application data directory on the native hosts, IndexedDB on the web).

**Host flow** (spec §13 and §17.11): check the password with the certificate
file (M7) → sign right away (no picture, or auto-stamped) or enter the seal
placement state → then, in every host, **one call** on the component:
`component.sign(&mut signer, &options)` (`component.sign_pfx(pfx, password,
&options)` is the same with the certificate file opened inside; on the web
`ofd.sign(pfx, password, options)`). The signature goes **into the document
that is open** — the seal is on its page, the view / selection / undo
history stay — and **nothing is written**. The document is unsaved, and
the user's save is an ordinary save: the signature's files are already in
the package the document carries. A document swap drops the unwritten
signature. No host keeps a package handle, a signed copy of the file, or a
baseline any more.

Before the call each host asks `component.sign_refusal()` (`ofd.isSignBlocked()`)
and says the one refusal in its own words:
"该文件没有批注入口，编辑中的批注无法签进文件：请先处理保存".

- **gpui-app**: `sign_now` (`signing/flow.rs`) signs on the session's editor
  and stores the returned reports; `signing_material` (`signing/mod.rs`)
  opens the signer from the certificate file.
- **xilem-app**: the component is only reachable at drain time, so the
  signature is one queued command (`queue_sign`); its outcome lands in the
  `sign_outcome` slot and `drain_signing` shows it.
- **web-app**: `signDocument` in `App.vue`; the status bar shows 签章未保存
  until the save (`signatureUnsaved`, a plain flag). The 验签 button and
  the status chip verify the document that is open
  (`ofd.verifySignatures()`, spec §17.13), as `verifyDocument` does on open
  and after a save; the file's bytes are verified only in the pre-save
  check, over what a save would write.

## Signing in place (spec §17.11)

The signature goes into the package the document carries; no model is
parsed again, no package is made again, and no signed copy of the file
exists before the user saves. What made the owner ask for it: a signature
used to end with loading the signed package as a new document, which put
the view back at the first page and dropped the selection and the undo
history — never `load_document` / `loadOfd` for a signature.

- **The plan holds no entry** (`sign.rs::Plan`): the references are
  digests, and the scaffolds are the few entries the first signature of a
  package replaces or adds. The digests are taken where the bytes lie; the
  only other thing the plan keeps is a shared handle on each protected
  entry (`Seen`), to know the package again when the signature lands.
- **A fixed order** (`in_place.rs::plan_for`): what can be refused is
  refused first, with nothing changed (`sign_refusal`: the file declares
  signatures and declares no annotation entry, and the document has
  annotations to write — `rofd_io::annotations_unwritable`, the same
  decision as the save's `blocked_by_signature`); a document without a
  package gets one made, once (`generate_package`: the full write, parsed
  into a handle — and what parsing it warned about is kept, `GeneratedPackage
  { package, warnings }`); so does a package whose first signature could not
  patch Document.xml (no `MaxUnitID`, no place for the annotation entry) —
  what a save of such a file falls back to.
- **One step** (`SignatureToAppend::land`): the package as it will be is
  written beside the document — the one made for the signature, or a copy of
  the document's own handle (`set_entry` into the copy: replaced or appended;
  the entry buffers are shared, not copied). Then it takes the document's
  place and `rofd_io::refresh_document_level` rereads what the model keeps of
  OFD.xml, Document.xml and the signature entries — so they are what loading
  the package would give — without one page being read. If that reading
  fails, the document's own package goes back and nothing was changed. Pages,
  resources and annotations are not touched: annotations are no part of a
  signature (I4), so they are neither read nor written. What comes back is
  `Landed { reports, warnings }`.
- **The handle says what OFD.xml says**: the landing does not set
  `signatures_declared` / `signatures_entry` itself; the reread writes both
  from the declaration in OFD.xml, the source `parse_ofd` fills them from. A
  list with records that OFD.xml never declares (a package another tool got
  wrong) therefore leaves the handle saying "not declared", as the model and
  a reload do.
- **Prepared earlier, landed later**: a signature prepared for a signer
  outside the process lands only on the document it was prepared on. The
  plan remembers every entry a signature covers and the list (`Seen`) — by
  buffer for the document's own, byte for byte for the same file read
  again. What is left out is exactly what a signature leaves out: the
  annotation layer as the package locates it and the directory entries (one
  list, `package_util::annotation_layer`, for the References and for this
  check). A file that merely looks like an annotation and is not located is
  protected, so it is compared. A document without a package is remembered
  by the model a package is generated from — the annotations left out, and
  `max_unit_id` with them: the editor raises it as it numbers an annotation
  on an unsigned document, and the first signature's scaffold then writes
  the ID reserve above it. Three files are not enough (found in
  self-review and in the final review): two packages can share OFD.xml,
  Document.xml and the list and differ in a page, and a signature made for
  one of them reads `Invalid` in the other; a document that was never saved
  had nothing at all to be recognised by. Another signature landing, or
  another document opened in the meantime (the same file saved and opened
  again included): `SignError::DocumentChanged`, nothing written. Annotation
  edits in the meantime are no change — made by the editor too, on a
  document with a package or without one.
- **In the component** (`signing.rs`, behind the `sign` feature): the
  signature lands through the editor's `revise_document`; the landing's
  warnings go out through `fire_warnings` (the host's `on_warning`, where a
  load's warnings go) — one batch when there is something to say, and no
  call when there is not (a host that keeps the last batch would read an
  empty one as "no warnings"); then `invalidate_pages` (the seals are page content:
  every cached page scene and every CPU-fallback page bitmap is stale) and
  `notify_document_change`, like any edit (modified, print sessions stale,
  `on_change`). The viewport, the selection, the text cursor and the
  history are not touched: a signature is no edit of the annotations.
  `OfdComponent::sign` and `finish_sign` return the reports, as before.
- **The editor's seams**: `Editor::revise_document` lends the document for
  a change that is neither an annotation command nor another document;
  `Editor::invalidate_pages` moves the body revision.
- **Pinned by tests** (`crates/sign/tests/sign/in_place.rs`): only the
  signature's own entries change and untouched entries keep their bytes
  where they lie (`Arc::ptr_eq`); the signed model is what a reload gives;
  in-place and bytes signing write the same signature; a second signature
  sees the first; the anchorless file; the refused file; edits and saves
  afterwards keep the signature's files; a signature prepared for one
  document lands neither on a twin of its file (the same OFD.xml,
  Document.xml and list, another page) nor on a twin that differs only in
  an annotation-looking file no page uses, nor on another document without
  a package, while the same file read again is still the same package; an
  annotation made between the phases — the editor's way, `max_unit_id`
  included — does not stop a signature of a document that was never saved;
  and the signature a refusal leaves in the document is the one that was
  there, not the refused one in its place; the handle says what a reload
  says for a list OFD.xml never declares; the warnings of a regenerated
  package come back, and those of the signatures that were already in the
  file do not. The crate's own tests (`in_place.rs`) pin that a failed
  reread leaves the document as it was (through `land_with`, which takes the
  reread as an argument) and that only the reread's warnings about the
  signature that landed come back. The component pins its own
  (`crates/component/src/signing.rs`). If
  `sign` ever writes anywhere but
  the signature entries, OFD.xml and Document.xml, or `OfdDocument` gains a
  document-level field, the reload test fails: extend
  `rofd_io::refresh_document_level`, do not loosen the test.
- **Warnings** go to the host (golden rule 8), nothing to stderr. The
  landing returns two kinds in `Landed.warnings`: what parsing a package
  generated for the signature warned about (an element rofd does not model,
  kept verbatim — the same thing the document's load said, said once more
  because the package was made again), and what the reread says about the
  signature that landed. The reread projects the whole signature list, and
  what it says about signatures that were in the file already (a seal
  picture that is not a PNG, a missing description) was reported when the
  file was opened: it is not repeated. A warning that names no entry cannot
  be attributed, and is kept. A signature rofd-sign wrote reads back, so the
  second kind is empty unless something is wrong - which is the point of
  carrying it.

## Seal placement state (component)

`begin_seal_placement(preview) -> bool`, `cancel_seal_placement()`,
`seal_placement_active()`, callbacks `on_seal_placed(SealPlacement)` and
`on_seal_placement_cancelled()`. While it lasts the component is modal:
nothing can be selected, edited, typed or opened from a right click;
scrolling, page turns and zooming go on. Esc or the right button cancels
and reports; a document swap ends it silently. The preview follows the
pointer, centred, kept whole on the page; the reported boundary is in the
millimetres the editor shows the page in, cut at a thousandth.
`Placement::At` takes it there and `sign` writes it in page space, whose
origin is the top-left corner of the physical box (GB/T 33190 table 7) —
the same space the automatic placement counts in. The two differ only on a
page whose box does not start at 0 0 (the renderer ignores that origin for
the body, a known limitation).

The preview's opacity (0.6) is multiplied into the picture's alpha
channel: the CPU fallback renderer (vello_cpu) refuses opacity on an image
draw with a panic.

## The save of a signed document

See [io.md](io.md#preserving-save-spec-2026-11-23). Two flags are the
host's to act on: `blocked_by_signature` (the package has no annotation
entry — ask before the full write, M16) and, before writing,
`newly_invalidated` on the candidate bytes (ask before a save that would
invalidate signatures, M17 — real for files signed by a tool that protects
the annotation entries). Never decide a broken signature on the user's
behalf. A signature has the same refusal before it is made
(`sign_refusal` / `isSignBlocked`): a file that cannot take the document's
annotations is not signed.

Two more flags are said, not acted on: `reserve_exhausted` (M14) and
`full_write_fallback` (M20) reach the status bar / a message after a
successful save; the normal save is silent.

## Pitfalls (all learned the hard way)

- **Real packages deviate**: relative or absolute `BaseLoc`, non-numeric
  `MaxSignId` (`s1`) and `Signature@ID` (`s001`), stamps sharing an ID,
  References that omit OFD.xml or run to images and fonts. Verify what the
  References list; ignore IDs that are not numbers when allocating.
- **Never re-encode before verifying.** The SM2 check runs on the bytes as
  they sit in the file.
- **SES v1 stores the time as a BIT STRING holding ASCII**; v4 uses a
  GeneralizedTime.
- **`SES_ESPropertyInfo.certList` is a `SEQUENCE OF OCTET STRING`**, not the
  certificate's own SEQUENCE. ofdrw's validator refuses the bare form —
  which is what sky-ofd's encoder writes. `seal_has_the_shape_of_the_ofdrw_sample`
  pins the whole tag tree.
- **No wall clock anywhere in the wasm dependency tree.** `keystore-core`'s
  `KeyStore::insert` stamps entries with `SystemTime::now()`, which panics
  on wasm32-unknown-unknown ("time not implemented"). Native tests cannot
  see this class of bug: the web-app e2e (`e2e/sign.spec.ts`) drives the
  real wasm on the WebGPU and on the CPU path.
- **A signed package without an annotation entry cannot take annotations**:
  the save reports `blocked_by_signature` and leaves the file untouched.
- **A signed package from another tool has no ID reserve**: annotations
  added afterwards take IDs above `MaxUnitID` (M14's known limitation).
- **Unconfirmed**: rofd writes `1` as the seal property's `type`; ofdrw's
  v4 sample carries `3`. Validators do not check it — confirm against a
  real seal when one is at hand.
- **A DER length has as many octets as it needs.** From 16 MiB on a third
  octet is not enough; one shared encoder writes it (`der::push_tlv`).
  Both writers used to drop the top byte there, and a big seal picture
  produced a seal no parser could read while `sign` returned `Ok`.
- **A reference list that names nothing is not a pass.** With zero
  references the integrity layer had nothing to fail on, and a signature
  value over such a description read `Valid` on a changed page.
- **Anything can sit where a certificate time belongs.** `parse_certificate`
  reports a time only when it has the shape of one (digits, optional
  fraction and `Z`); the native detail panels used to slice that text by
  bytes and panicked on a crafted file.
- **Page annotation files can live outside the entry file's directory.**
  The old layout has a file in each page directory and no entry file. A
  save or a first signature then writes the entry file with locs through
  `..` (GB/T 33190 table 2 allows it) and never moves the page files;
  `relative_to` in `rofd_io::paths` makes them, `resolve_loc` reads them.
  Listing such a file by its package path unhooks every annotation of the
  document.
- **A zip header's size is the writer's word.** `read_all_entries` reserves
  at most 16 MiB of it and lets the buffer grow with what is read.
- **Every ID of the document counts**, not only the page objects':
  `highest_known_object_id` includes page IDs, document-level passthrough
  fragments and resource IDs, or a new annotation takes a page's ID.
- **A no-change save is not byte-stable on a signed file that carries
  annotations.** The unsigned save before the signature numbers the
  appearance objects from `MaxUnitID` (`object_id_seed`); a save of the
  signed document numbers them from the highest ID the model knows
  (`preserving_id_seed`), and the two differ on a real file
  (`test/ru-yuan-ji-lu.ofd`, `test/text_annotation.ofd`). PS1's "every
  entry byte-identical" holds only for a package without appearance
  objects. Nothing breaks: annotation entries are not protected, and a
  signature's own files are written as they were made whatever a save does
  to the annotation files. Fixing it means stable IDs for appearance
  objects (the half of M21 that was left; spec §17.10).
- **Never load a signed package into the editor that shows the document.**
  `load_document` / `loadOfd` is for another document: the view goes back
  to the top, the selection and the undo history are dropped. A signature
  goes in through `component.sign` / `ofd.sign` (spec §17.11).
- **A document can lack a package it can sign.** One that was never saved
  has none; one whose Document.xml has no `MaxUnitID` cannot take the first
  signature's patch. Signing in place generates a package once, as a save
  falls back to the full write; the bytes API refuses instead, as it
  always did.
- **A boundary is page space.** GB/T 33190 table 7: the top-left corner of
  the physical box is the origin of the page space. `bottom_right` counted
  from there and `Placement::At` did not, so a seal placed by click was
  displaced on a page whose box does not start at 0 0; both do now, and
  "outside the page" is measured from the box's edges.
- **A failed load of the wasm chunk is not cached** by the SDK: the next
  caller loads again.

## Independent cross-check

`tools/ofdverify` validates packages with ofdrw's own validator (the
reference implementation of the archive chain). It caught two defects that
rofd's own tests could not see (a seal structure ofdrw refuses, a `FileRef`
naming a directory). Its reader is the arbiter for anything that has to be
*found* in a package rofd wrote: `new OFDReader(path).getAnnotationEntities()`
counted the annotations of an old-layout package after a save and after a
signature, which settled that locs through `..` are read. Run it after
touching the encoders or the References logic:

```bash
cargo run -p rofd-sign --example gen_builtin_seals -- <tmp>/seals   # or use any seal picture
cd tools/ofdverify && mvn -q compile dependency:build-classpath -Dmdep.outputFile=cp.txt
javac -encoding UTF-8 -cp "$(cat cp.txt)" -d out src/main/java/Verify.java
java -Dfile.encoding=UTF-8 -cp "out;$(cat cp.txt)" Verify <file.ofd>...
```

Needs a JDK, Maven and ofdrw-sign 2.3.9 from Maven Central or `~/.m2`.

## Test duty

```bash
cargo test -p rofd-sign --features test-ca   # the crate, with the test CA's own tests
cargo test -p rofd-sign --features test-ca --test sign in_place   # signing in place; the landing's unit tests are in `--lib in_place`
cargo test -p rofd-io --test preserving_save --test document_package   # the preserving save; the document carries its package
cargo test -p rofd-component seal_placement
cargo test -p rofd-component --features sign   # the component's signing tests (10 in signing.rs; the feature is off by default)
cargo test -p rofd-web-view --lib sign_wasm   # the wasm options / report JSON, and the two-phase step's plain half
cargo test -p gpui-app signing; cargo test -p xilem-app signing
cd crates/web-view/sdk && npm test       # SDK wrappers (stubbed wasm), `newlyInvalidated`
cd crates/web-app && npm test && npm run e2e -- e2e/sign.spec.ts
```

A test filter is a test name: `cargo test -p rofd-web-view --lib step_result`
runs nothing and exits 0 (`a_finished_step` is the test's name).

Manual acceptance (the reader is the arbiter):
`docs/superpowers/manual/2026-11-24-sign-reader-checklist.md`.
