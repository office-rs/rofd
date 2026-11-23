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
- **Annotation font family is not persisted in v1** (spec 2026-11-10,
  reaffirmed by the full-write spec D11): `FontId` stores the display
  family name and no Font resource is created (see
  [editing.md](editing.md)).
- **Single main branch, direct commits**: no dual-branch/PR line
  (**deliberately different** from rword's PR workflow).

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
