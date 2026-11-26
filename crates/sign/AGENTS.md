# rofd-sign

> Auto-loaded when you work in this subtree. The full picture — shape,
> invariants, pipeline, pitfalls — is `docs/agents/sign.md`; the spec is
> `docs/superpowers/specs/2026-11-23-rofd-sign-design.md` (§4, §9, §10).

The platform-agnostic signing line, with two doors (spec §17.11): a
signature is appended **in place** to the package a document carries
(`in_place.rs` — `sign_in_place`, `sign_refusal`, `prepare_in_place`,
`verify_document`; what the component calls), and the bytes API for
a package that is not in an editor (`sign`, `prepare_sign`, `verify`:
bytes in, bytes out). Both run the same plan and the same rounds.

- **In place means nothing is made again** (spec §17.11): the plan holds
  references and scaffold patches, no entry copies, and the digests are
  taken where the bytes lie. A document without a package, or one whose
  Document.xml has no anchor for the first signature's patch, gets a
  generated package once (`Regenerate::Once`); the bytes API never
  generates one and is refused there instead, as it always was.
- **The landing is one step** (`SignatureToAppend::land`): the package as it
  will be is written beside the document — the generated one, or a copy of
  the handle (the entry buffers are shared) — then it takes the document's
  place and the document is read back from it; if the reread fails, the
  document's own package goes back, so the document is as it was. This
  rests on `rofd_io::refresh_document_level` reading everything that can
  fail before it writes a field (it says so); a fallible step added after
  its first write there would leave a half-refreshed model beside the old
  package. `land_with` takes the reread as an
  argument so that a test can make it fail (the crate's own tests, in
  `in_place.rs`). After the reread the model's document-level fields and
  the handle's two signature flags are what loading the package gives —
  the landing does not set the flags itself (`crates/sign/tests/sign/in_place.rs`
  pins the equality with a reload).
- **What the landing returns is `Landed { reports, warnings }`** (spec
  §17.13): the reports of every signature, and the warnings of parsing a
  package generated for the signature plus what the reread says about the
  signature that landed. The reread projects the whole list; what it says
  about signatures that were in the file already was reported at open and
  is not repeated (`Plan::is_about_the_signature`; a warning that names no
  entry is kept). Nothing goes to stderr — the component passes the
  warnings to the host's `on_warning`. The bytes API takes neither: no
  package is made on that path, and a signature rofd-sign wrote reads back.
- **This crate is below the exits** (spec §17.13): hosts meet the signing
  line through `rofd-component` and the SDK. The component re-exports the
  types a host needs (`rofd_component::signing`); a signing capability
  added here gets its entry on both exits in the same change.
- **A signature prepared on one document lands on no other** (`Seen`,
  compared in `SignatureToAppend::land`): every entry a signature covers
  and the list, by buffer for the document's own and by bytes for the same
  file read again. Left out is exactly what a signature leaves out
  (`package_util::annotation_layer`, one list with the References); a file
  that only looks like an annotation is protected, and compared. A document
  without a package is recognised by the model a package is generated from,
  the annotations and `max_unit_id` left out (the editor raises MaxUnitID
  as it numbers annotations). A difference is `SignError::DocumentChanged`,
  nothing written; annotation edits in between are no change. Three files
  (the list, OFD.xml, Document.xml) and a filter by entry name were both
  tried, and both let a signature land on another package.
- **No platform crates, and no wall clock anywhere in the tree.** The crate
  must build for `wasm32-unknown-unknown`, where `SystemTime::now()`
  panics — a dependency that reads the clock is a bug here
  (`keystore-core`'s keystore does; `pfx.rs` reads the certificate bags
  directly for that reason). Cryptography is pure Rust (RustCrypto `sm2`
  0.14 / `sm3` 0.5, the DSA only).
- **Verification reports, it never fails.** Its input is always the
  package — bytes, or the package a document carries (`verify_document`) —
  never the model (spec I2).
- **Signature files are never re-encoded for verification.** The SM2 check
  runs on the raw TLV bytes of the to-be-signed part (`DerNode::raw`).
- **The crate holds no keys and touches no network**: the signer is
  injected, `now` is an argument. A signature handed back by a signer is
  checked against its certificate before anything is written.
- **References of a new signature** (I4): every entry but the signatures
  and the annotation layer, plus its own `Seal.esl` — never zip directory
  entries (strict readers reject a `FileRef` that names a directory).
- **A reference list that names nothing fails layer 1**
  (`IntegrityResult.protected_files` is 0): the verdict is `Invalid`,
  whatever the signature value says.
- **One DER length encoder** (`der::push_tlv`) for the writer and for the
  PKCS#12 rewrite: a length takes as many octets as it needs. Both used to
  write three from 16 MiB up and drop the top byte.
- **`SignError::Password` only for a wrong password** (the file's own
  integrity check said so); a file that cannot be used is
  `SignError::Certificate`. Hosts must not report the second as the first.
- **A seal picture is bounded** (`MAX_SEAL_PICTURE_BYTES`, 16 MiB as PNG):
  refused, never embedded broken.
- **io's DER walker is not this module**: `rofd-io` carries a tiny reader
  that only lifts a seal picture; the full reader, the writer and the X.509
  subset live here.
- Real packages deviate from the textbook layout (relative or absolute
  locs, non-numeric ids, stamps sharing an id, a seal that exists only
  inside the signature value, a time stored as a BIT STRING or as a
  GeneralizedTime): tolerate them, report what cannot be read.
- **Native tests cannot see wasm-only failures.** After touching an
  encoder or the References logic, run the web-app e2e (`crates/web-app`:
  `npm run e2e -- e2e/sign.spec.ts`) and ofdrw's validator
  (`tools/ofdverify`).
