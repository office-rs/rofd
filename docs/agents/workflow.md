# Workflow / pinned versions / releases / test conventions

> When to read: before committing, before touching dependency versions,
> before tagging a convert release, before writing tests.

## Git workflow

- **Single `main` branch, commit directly on main** (flw ruling; no
  dual-branch, no PR line).
- superpowers flow: **spec → plan → TDD → self-review → commit**, all
  conventional commits.
- Commit message format:

  ```
  <type>: <description>

  <optional body>
  ```

  Types: feat / fix / refactor / docs / test / chore / perf / ci. **Always
  English** (language ruling: [conventions.md](conventions.md)).
- Specs are date-named under `docs/superpowers/specs/` and append-only:
  revisions land as amendments; history is never rewritten.

## Test conventions

- Coverage target **80%** (`cargo llvm-cov --workspace`); TDD (red first,
  then green).
- **Write fidelity is the core test** (must stay green after any io save
  change — see [io.md](io.md)): `crates/io/tests/write_fidelity.rs` +
  `round_trip.rs`; real samples are `#[ignore]`:
  `cargo test -p xilem-app --test c2_save -- --ignored`.
- Rendering uses **scene structure assertions** (object counts/types), not
  pixels.
- The xilem-view headless harness (masonry_testing, 9 cases): typing,
  double-click word select, IME, wheel, ctrl+wheel, pure-horizontal
  ctrl+wheel, Ctrl+C, window-focus gating, fit-mode commands and exit.
- convert parity: `crates/convert/tests/parity.rs` pins byte-identical PDF
  output with the editor's export; the four binding demos' acceptance =
  pooled conversion == CLI bytes (one sha256 across five runtimes is on
  record).

## Pinned dependencies (read before touching)

| crate | source | version/rev |
| ----- | -- | --------- |
| `xilem` / `masonry_testing` | linebender/xilem git | `271a27a6d4a930f7878d404f9014e3c50a3a9b88` |
| `imaging` / `imaging_vello` / `imaging_vello_cpu` | crates.io | `0.0.1` |
| vello / parley | crates.io | `0.8` |
| wgpu | crates.io | `28` |
| `office-rs-print` | office-rs/print git | `72b67a9` (workspace-level `default-features = false`) |
| `jni` | crates.io | `0.21` (convert-java) |
| `napi` / `napi-derive` / `napi-build` | crates.io | `2` (convert-node) |
| `pyo3` | crates.io | `0.30` (convert-py, abi3-py39) |
| `krilla` | crates.io | `0.8` (feature `pdf`) |
| `gpui-pre` / `gpui-pre-platform` | crates.io | `=0.3.10` (zed@08cbb88 snapshot re-publish; the official crates.io gpui 0.2.2 lacks the Windows/IME fixes — do not swap) |
| `gpui-kit` / `gpui-kit-assets` | crates.io | `=0.8.1` (icons via `with_assets(AllAssets)`) |

- **The gpui family bumps as one batch, never singly**: kit 0.8.1 pins
  gpui-pre `=0.3.10`; an upgrade moves gpui-pre / gpui-pre-platform /
  gpui-kit / gpui-kit-assets together (the workspace `Cargo.toml` comments
  are the contract).
- Toolchain: `rust-toolchain.toml` pins `1.98.1`; newly pinned packages
  declare `rust-version = 1.96`.
- No direct masonry_winit/winit deps remain (masonry_winit uses winit
  internally).

## convert release lockstep (on `rofd-convert-v*` tags)

The version files that must agree:

- `crates/convert{,-cli,-java,-node,-py,-wasm}/Cargo.toml`
- `crates/convert-java/pom.xml`
- `crates/convert-node/package.json`
- `crates/convert-py/pyproject.toml`
- `crates/convert-wasm/sdk/package.json`

Before tagging, `cargo deny check licenses` (`deny.toml`) must be green.
Product naming is uniformly `rofd` (release-tauri.yml likewise). Runbook:
`docs/superpowers/specs/2026-11-08-rofd-convert-release-runbook.md`.
