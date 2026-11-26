# Conventions, rulings, risks

> When to read: before adding public API, before closing out a major task
> (doc audit), and whenever a rule's authority matters.

## Language (ruling 2026-10-20, extended 2026-11-21)

- **Commit messages, code comments, and the whole AGENTS system** — root
  `AGENTS.md`, `docs/agents/*.md`, nested `crates/*/AGENTS.md` — **are
  always English**.
- Chinese is reserved for design docs under `docs/superpowers/`
  (specs/plans/drafts) and user-facing UI strings.
- Design docs may keep WPS strings, but **code comments, identifiers, and
  file names never carry them**.
- Write new code with English comments; convert Chinese comments on lines
  you are already touching; do not mass-rewrite untouched ones.

## Rulings

- **No `Format` trait**: rofd is a single-format (OFD) library and carries
  no format-agnostic codec abstraction — do not imitate rword's `Format`
  trait (rword needs one for its OOXML/HTML dual-format clipboard; rofd
  does not). Parsing/writing lives directly in `rofd-io`; introducing a
  trait indirection here is over-design.
- **The library never reads the system clock**: no `Date::now()` /
  `SystemTime::now()` inside the library; hosts inject via
  `set_clock(author, ts)` and commands fill `created`/`modified` from it.
  **Exception** (ruled 2026-09, amended 2026-10-13): the monotonic
  `Instant` (not a wall clock, not persistable, immune to clock jumps) may
  drive animation timing — the caret-blink deadline/`tick_blink` uses it on
  both targets; wasm aliases it via `web_time::Instant`
  (`performance.now()`; `std::time::Instant::now()` is a hard panic on
  wasm32-unknown-unknown).
- **Print content toggles default to all-on** (spec 2026-11-06 D17, commit
  052f528): `PrintContent::default()` is true for watermark/annotations/
  seals; hosts and convert both derive via default; the CLI only offers
  `--no-*` opt-outs.
- **Annotations are mutable, signatures are append-only, a save never
  touches what a signature protects** (spec 2026-11-23): the editor save
  is the preserving save; the full write is the generation path. A host
  never decides to invalidate a signature for the user — it asks (M16,
  M17).
- **The no-clock rule covers the dependency tree**: a library dependency
  that reads `SystemTime::now()` panics on wasm32-unknown-unknown (found
  in `keystore-core`'s `KeyStore::insert`, 2026-11-24), and native tests
  cannot see it. `rofd-sign` therefore reads certificate files without a
  keystore, and the web-app e2e `sign.spec.ts` guards the class.
- **Annotation font family is not persisted in v1** (spec 2026-11-10,
  reaffirmed by the full-write spec D11): `FontId` stores the display
  family name and no Font resource is created (see
  [editing.md](editing.md)).
- **Single main branch, direct commits**: no dual-branch/PR line
  (**deliberately different** from rword's PR workflow).
- **The component and the SDKs are the exits of the library** (ruling
  2026-11-25, signing spec §17.13): `rofd-component` for a native host, the
  TS SDK `@office-rs/rofd` for a web one. They agree with each other and
  with the person using them - a capability is on both, under the same
  name, or the spec says why not; what a host needs to use one call must
  not force a second dependency or a detour (verifying the open document
  used to need a save first). `rofd-sign` and `rofd-io` are below the exits.
  The table of the signing surface: [sign.md](sign.md) §the two exits.

## Doc audit (at the close of every major task)

When a major task/feature lands (its code commits), check the doc layers
for drift and update them **in the same batch**:

1. **Usage docs**: crate READMEs, the root README, SDK docs;
2. **Design docs**: append an amendment to the implementing spec (never
   rewrite history);
3. **The AGENTS system**: root `AGENTS.md` + `docs/agents/*.md` + nested
   `crates/*/AGENTS.md` (routing table entries included). A code-only
   change that leaves any layer stale is an incomplete task.

**Size budget (anti-rebloat)**: root `AGENTS.md` ≤ 150 lines; topic/nested
files stay lean — link to the authoritative spec instead of restating it.

## Do / Don't

| ✅ Do | ❌ Don't |
| ----- | -------- |
| render uses the imaging Painter API | constructing vello::Scene in render |
| hosts set_clock (Instant only for blink animation) | the library reading the wall clock |
| editor only mutates .annotations | editor touching pages |
| xilem-view / gpui-view depend only on component | pushing io/business logic into adapters |
| run write-fidelity tests after io changes | asserting only model equality (passthrough loss is invisible) |
| converter bindings/CLI touch only rofd-convert | bindings/CLI importing rofd-io/rofd-export/component directly |
| SDK render scheduling via `RenderScheduler`/kick (truth = `needs_frame`) | a constant rAF loop in the SDK; adding proxied wasm reads to ticks without the `inLoopTick` guard |
| think about the rword consumer before changing dom/io public faces | treating dom/io as private in-repo API |
| verify / sign over package bytes (`rofd-sign`); append signatures | re-encoding a signature file; editing or deleting a signature |
| ask before a save that would break a signature (`blocked_by_signature`, `newly_invalidated`) | writing silently, or refusing to save at all |
| a signing capability on both exits at once, same name (component + SDK; sign.md §the two exits) | adding it to one exit and leaving the other to work around it |
| hand warnings to the caller / the host's warning channel (`Landed.warnings`, `GeneratedPackage.warnings`, `on_warning`) | printing them, or dropping them where they are raised |

## Known risks

- **Linebender alpha churn**: xilem is pinned to a git rev (alpha), vello
  0.8 (beta), parley 0.8 (alpha) — expect breaking changes on bumps.
- **gpui-pre third-party snapshot pace**: the gpui line rides Longbridge's
  `gpui-pre =0.3.10` re-publish of zed@08cbb88 (the official crates.io gpui
  0.2.2 lacks the Windows/IME fixes). Batch-bump contract:
  [workflow.md](workflow.md).
- **convert D21 known boundary**: the zip-decompression memory peak happens
  before the budget check (the fail-fast on over-budget fires after parse)
  — a malicious package can burn memory before being rejected.
- **tauri macOS/Linux**: gated on WebKit's WebGPU progress.
- **xilem-view format panel pending** (spec 2026-11-10 §5.3): gpui-view and
  the web SDK are done; the xilem line is not.
- **v1 stubs**: template inheritance / JBIG2 / tiled images unimplemented
  (degradation warnings, never silent).
- **Reader acceptance of rofd's signatures is manual and pending** (spec
  2026-11-23 W1–W5b): ofdrw's validator accepts everything rofd signs
  (`tools/ofdverify`); the reference reader has not been tried yet. Two
  details are unconfirmed against a real seal: the seal property's `type`
  (rofd writes `1`, ofdrw's v4 sample carries `3`) and certificate files
  from real CAs.
- **A signed package from another tool has no ID reserve** (M14): IDs of
  annotations added afterwards run above `MaxUnitID`.
- **sky-ofd's seal encoder writes the certificate bare** where the
  structure wants a list of OCTET STRINGs; ofdrw's validator refuses it.
  Worth telling whoever maintains that archive chain.
- **Final review of the signing line** (2026-11-24, one fresh reviewer over
  the whole line): two Critical and seven Important findings were fixed,
  each with a test that failed first; what was deferred is in the signing
  spec §17.7.
- **Final review of signing in place** (2026-11-25, a self-review and then
  one fresh reviewer): no Critical; two Important and one Minor were fixed,
  each with a test that failed first (spec §17.12). The six Minors it left
  open were fixed the same day under spec §17.13, each with a test; the
  changelog entry was written. What is still the owner's call: whether dom
  and io get a version bump before the next `rofd-io-v*` tag (their public
  surface broke on purpose on 2026-11-25).
- **A signature goes into the document that is open, in place** (spec
  §17.11, the owner's design of 2026-11-25 after three rounds: M13 revised
  on 2026-11-24 and §17.10 superseded): the component signs
  (`component.sign`, `ofd.sign`), the signature is appended to the package
  the document carries, and nothing else of the document is made again -
  no model, no package handle, no signed copy of the file. No host loads a
  signed package as a new document (`load_document` puts the view back at
  the top) and none keeps a package handle or a baseline. Nothing is
  written until the user saves, and that save is an ordinary save. "What
  was signed is what is stored" holds per file: signature files as they
  were made, protected files as hashed. `sign` / `signOfd` stay pure
  bytes-in-bytes-out for a package that is not in an editor.
- **The document carries its package** (`OfdDocument.package`, dom owns
  `PackageHandle`): this broke the rword frozen surface on purpose on
  2026-11-25 (`LoadReport.package`, the `package` argument of `save_ofd`);
  rword's `OfdDocument` literal takes new fields from `Default` on its
  branch `fix/ofd-document-default-fields` (push and PR are the owner's).
- **A no-change save is not byte-stable on a signed file with annotations**
  (found with §17.10, not fixed): the appearance object IDs are minted from
  `MaxUnitID` by the unsigned save before the signature and from the
  highest known ID by the signed save after it. PS1's byte-for-byte claim
  holds only without appearance objects; nothing breaks (annotation entries
  are not protected, and the signature's own files are written as made).
- **The SDK's save flags describe the last `saveOfd()`**: read them right
  after it, and not after `saveOfdFullWrite` or a document swap.
- **The renderer ignores a physical box origin other than 0 0** for the
  body (older than the signing line): GB/T 33190 table 7 makes that corner
  the origin of the page space. Signatures are written in page space
  anyway (`sign` adds the origin), so they are right wherever the reader
  honours the box.
- **Signing does not run the pre-save check** (M17 is about saves): on a
  file whose other tool protects the annotation entries, signing the
  document as it stands in the editor invalidates that signature, and the
  status bar says so afterwards. An open decision (spec §17.7).
