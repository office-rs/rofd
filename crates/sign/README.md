# rofd-sign

Electronic-seal and signature verification and creation for OFD
(GB/T 33190) packages. Part of [rofd](https://github.com/office-rs/rofd);
see `docs/agents/sign.md` in the repository for the full picture.

- **Verify** a package: a structured report for each signature (file
  integrity, seal consistency, signature value, certificate information).
  Verification never fails, it reports.
- **Sign a document that is open**: `sign_in_place` appends the signature
  to the package the document carries (`OfdDocument.package`) — no model
  is parsed again, no second document is made, nothing is written to a
  file. It returns the reports and the warnings of the landing (`Landed`).
  A host does not call it: the editor library does, behind its `sign`
  feature — `OfdComponent::sign` / `sign_pfx` / `verify_signatures`
  (rofd-component; the types are re-exported in
  `rofd_component::signing`, so a host need not name this crate).
- **Sign a package** that is not in an editor: append a seal signature (SES
  v4, GB/T 38540) or a digital signature (GB/T 35275). Bytes in, bytes out.
- **Certificate files** (`.pfx`, `.p12`) with a password, and a small
  in-memory store for the certificates a user imported.
- Pure Rust (SM2 / SM3 by RustCrypto), no platform crates: builds for
  `wasm32-unknown-unknown`.

The crate never holds keys (the host injects a signer), never reads the
clock (the host passes the time) and never touches the network.

## Use

```rust
use rofd_sign::{
    sign, sign_in_place, verify, PfxSigner, Placement, Pages, SealImage, SignOptions,
    VerifyOptions,
};

// On opening: what do the signatures of this file say?
// One report per signature; an empty list when the file is not signed.
let reports = verify(&ofd_bytes, &VerifyOptions::default());

// Signing. The host reads the clock — the library never does.
let mut signer = PfxSigner::open(&pfx_bytes, "the password")?;
let options = SignOptions {
    seal: Some(SealImage { png, size_mm: 40.0 }),            // None: a digital signature
    placement: Some(Placement::BottomRight {
        pages: Pages::Last,
        right_mm: 10.0,
        bottom_mm: 10.0,
    }),
    ..SignOptions::new("2026-11-24 10:30:00")                  // the signing time
};
let signed = sign(&ofd_bytes, &mut signer, &options)?;

// The document that is open (parsed with rofd-io; it carries its package):
// the signature goes into it, and the host's ordinary save writes the file.
let landed = sign_in_place(&mut document, &mut signer, &options)?;
let reports = landed.reports;
let warnings = landed.warnings; // what the landing has to tell the host
```

`PfxSigner::open` tells a wrong password (`SignError::Password`, the file's
own integrity check said so) from a file that cannot be used
(`SignError::Certificate`): show the second as what it is. A report whose
`integrity.protected_files` is 0 belongs to a signature that protects no
file; it reads `Invalid`.

A signer that is not in this process (a USB token, a remote service) takes
the two phases instead: `prepare_sign` hands out the bytes to sign,
`Prepared::apply` takes the signature back — for a document that is open
`prepare_in_place`, `PreparedInPlace::apply`, `SignatureToAppend::append_to`.
Such a document is not signed when its file cannot take its annotations
(`sign_refusal`: a signed file without an annotation entry). Documents
signed here can still gain annotations: a signature never protects the
annotation layer (GB/T 48666-2026), and rofd's save leaves everything else
byte for byte.

License: Apache-2.0.
