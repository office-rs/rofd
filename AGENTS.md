# AGENTS.md

Guidance for AI coding agents (Claude Code, Codex, Cursor, …) working in this
repo. Read this before touching code; human contributors should read
[README.md](README.md) instead. The design specs are the ultimate source of
truth: [`docs/superpowers/specs/`](docs/superpowers/specs/) is date-named and
append-only — the newest wins. This file tracks the as-built state; when it
conflicts with code + spec, code + spec win, then come back and fix this file.

## Project Overview

**rofd** is an OFD (GB/T 33190) **view + annotate** editor **library**, in
Rust, dual-platform (native + WASM).

- **v1 scope**: view + annotate + sign. The body renders read-only;
  annotations are the mutable layer; signatures (seals and digital
  signatures) are appended, never edited, and verified on opening.
  Non-goals: editing the body, full-fidelity rendering (template
  inheritance / JBIG2 / tiled images are on-demand, v1 stubs), real-time
  collaboration. Doors left open for the B/C/D scenarios, not implemented.
- **Library shape**: `OfdComponent` is the only integration entry (think
  `<textarea>`); the host owns the message loop and forwards events. The
  library never reads the system clock and depends on no GUI framework.
  Host apps (xilem-app / web-app / tauri-app / gpui-app) are not library
  deliverables.
- **convert line**: headless OFD→PDF via `rofd-convert` (CLI +
  Java/Node/Python/Wasm bindings).
- **Cross-repo frozen surface**: rword's docx→OFD export line depends on this
  repo's `rofd-dom` + `rofd-io` (git dependency following tags; the frozen
  surface is exactly those two — spec 2026-11-11 D4/D5). The as-built
  reference project is `D:/code/rword` (the same-shaped OOXML editor lib).

## Golden Rules

0. Nested `crates/*/AGENTS.md` auto-load when you work in that subtree;
   cross-cutting topics under `docs/agents/*.md` are read on demand (Task
   Routing below). They extend these rules, never override them.
1. **Dependencies point strictly upward; reverse edges are forbidden.**
   Editor hosts only touch `rofd-component`; converter hosts (bindings/CLI)
   only touch `rofd-convert`. Reaching past a facade is a bug — file a
   redesign instead.
2. **Annotations are mutable, signatures are append-only, and a save never
   touches what a signature protects** (the editor only mutates
   `.annotations`, never pages; signature files travel byte for byte).
3. **Two writes, both faithful**: a save keeps the original package and
   rewrites only the annotation entries; a generation (no original package)
   writes everything the model holds, with everything unmodelled passing
   through (docs/agents/io.md). After changing io save logic, the
   write-fidelity tests (`crates/io/tests/write_fidelity.rs`) and the
   preserving-save tests (`preserving_save.rs`) must stay green.
4. **The library never reads the wall clock**: no `Date::now()` /
   `SystemTime::now()`; hosts inject via `set_clock(author, ts)`. Sole
   exception: the monotonic `Instant` (`web_time` on wasm) drives the caret
   blink animation.
5. **Rendering produces `imaging::record::Scene`**, not `vello::Scene`
   (render only uses the imaging Painter API).
6. **The platform boundary sits between component and the adapters**:
   component and below (incl. export, convert) are platform-agnostic — no
   winit/web-sys/wgpu/arboard deps; xilem-view / web-view / gpui-view are
   the only three platform bindings.
7. **convert and below depend on no platform crate** (same law as
   component); the component's own code never touches io (rofd-sign joins
   it only behind the `sign` feature), and native-side io calls live in the
   host layer.
8. **Errors are explicitly layered, never silently swallowed**: parse
   failures are `OfdError`, degradable inputs are `OfdWarning` (surfaced to
   hosts via callbacks/warning streams) — details in docs/agents/io.md.
9. **Single `main` branch, commit directly on main** (no dual-branch/PR
   line); superpowers flow: spec → plan → TDD → self-review → commit, all
   conventional commits.
10. **Language**: commit messages, code comments, and the whole AGENTS system
    (root + `docs/agents/` + nested) are always English. Chinese is
    reserved for design docs under `docs/superpowers/` and user-facing UI
    strings. No WPS strings in code comments, identifiers, or file names.
11. **Doc audit after every major task**: usage docs + spec amendment (append,
    never rewrite history) + the whole AGENTS system (root + `docs/agents/` +
    nested, routing table entries included) updated in the same batch. Root
    `AGENTS.md` stays ≤150 lines — a rule earns a line there only if it
    matters in EVERY session; everything else goes to `docs/agents/`
    (cross-cutting) or a nested `crates/*/AGENTS.md` (subtree-specific).

## Commands

Full reference (all four hosts, four bindings, demo acceptance, fuzz,
coverage, release lockstep, dev-machine pitfalls):
[docs/agents/commands.md](docs/agents/commands.md).

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo test -p rofd-io --test write_fidelity --test preserving_save --test document_package   # io saves (`--test` names a binary; a bare filter runs nothing)
cargo test -p rofd-component --features sign   # the component's signing tests (off without the feature)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd   # xilem native host
cargo run -p gpui-app  -- test/ru-yuan-ji-lu.ofd   # gpui native host
cd crates/web-app && npm run dev                   # web host
cargo run -p rofd-convert-cli -- test/ru-yuan-ji-lu.ofd target/convert-probe.pdf --font C:/Windows/Fonts/simsun.ttc
```

## Architecture at a Glance

Strict five-layer one-way dependencies + three platform adapters + the
convert session layer. Full diagram, crate table, print architecture:
[docs/agents/architecture.md](docs/agents/architecture.md).

```
component ─► render ─► dom          io ─► dom        export (Scene→PDF leaf, no rofd-internal deps)
component ─► editor ─► dom
convert (session layer) ─► component + io + render + export + dom
sign ─► io ─► dom                    (verify + create signatures, in place on the package a document carries)
component ─► sign (feature `sign`)    (the component signs the open document; the host lends the key and the time)
xilem-view / web-view / gpui-view ─► component (web-view also holds io/editor/export/convert/sign)
xilem-app / gpui-app ─► sign         (status bar, pre-save check, certificate store; adapters never sign)
hosts: xilem-app / gpui-app / web-app / tauri-app (not deliverables)
```

## Task Routing

Read the matching file BEFORE working in an area:

| Task area | Read first |
|---|---|
| Any `crates/` subtree | that crate's nested `AGENTS.md` (auto-loaded) |
| Layering / crate boundaries / print architecture / cross-repo surface | [docs/agents/architecture.md](docs/agents/architecture.md) |
| io full write / preserving save / write fidelity / passthrough / ID conventions / error layering | [docs/agents/io.md](docs/agents/io.md) |
| Signing / verification / seal placement / certificate files / pre-save check | [docs/agents/sign.md](docs/agents/sign.md) |
| Annotation editing / flowed text layout / rich text / format panel / IME | [docs/agents/editing.md](docs/agents/editing.md) |
| Fonts (any crate) | [docs/agents/fonts.md](docs/agents/fonts.md) |
| Render pipeline / Scene IR / CPU fallback | [docs/agents/rendering.md](docs/agents/rendering.md) |
| Git / commits / release lockstep / pinned versions / test conventions | [docs/agents/workflow.md](docs/agents/workflow.md) |
| Full command reference / dev-machine pitfalls | [docs/agents/commands.md](docs/agents/commands.md) |
| Rulings / doc-audit duty / known risks | [docs/agents/conventions.md](docs/agents/conventions.md) |
| gpui Windows manual checklist | [docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md](docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md) |
| Signing manual acceptance (the reader is the arbiter) | [docs/superpowers/manual/2026-11-24-sign-reader-checklist.md](docs/superpowers/manual/2026-11-24-sign-reader-checklist.md) |

## Environment Gotchas (this dev machine)

- `py -3.1` is Python 3.14; the bare `python` on PATH is 3.7 — the
  convert-py build/acceptance must pass the interpreter explicitly via `-i`.
- Running fuzz on Windows: the MSVC ASan DLL directory must be on PATH.
- The web font transitional dep needs a sibling checkout of `D:/code/fonts`
  (`file:` protocol; switch to `^1.0.0` once the npm package ships).
- Four worktree-session traps (gitignored fixtures / fonts junction /
  Windows excluded port ranges / D-drive pressure):
  [docs/agents/commands.md](docs/agents/commands.md) §dev-machine pitfalls.
- Linker LNK1108/LNK1318 ≈ D-drive-full signal; free space fast with
  `rm -rf target/debug/incremental`.
