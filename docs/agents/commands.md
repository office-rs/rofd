# Command reference

> When to read: before running anything beyond the core commands (root
> AGENTS.md); when you hit a dev-machine pitfall, see the end of this file.

The working directory is always the repo root `D:\code\rofd` unless stated
otherwise. The shell is bash (Unix syntax).

## Build / test / lint

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo test -p rofd-io                       # crate-scoped tests
cargo test -p rofd-io write_fidelity        # filter by name (write fidelity)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo llvm-cov --workspace                  # coverage (needs cargo-llvm-cov; target 80%)
```

## xilem native host

```bash
cargo run -p xilem-app                            # empty editor
cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd  # open an .ofd from the root test/ dir
cargo run -p xilem-app --example print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out
# ^ print manual-test channel (Pages payload lands in a file via GDI lpszOutput)
```

## gpui desktop host

```bash
cargo run -p gpui-app                             # empty editor (multi-document sessions)
cargo run -p gpui-app -- test/ru-yuan-ji-lu.ofd
cargo run -p gpui-app --example gpui_print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out
```

Manual checklist:
`docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md`.

## web host

```bash
rustup target add wasm32-unknown-unknown     # one-time
cd crates/web-app
npm install
npm run build:sdk       # wasm-pack build ../web-view --target web -> sdk/dist
npm run dev             # vite dev server
npm run build           # vite production build
npm run smoke:nogpu     # no-GPU smoke (headless Chrome --disable-gpu must land in the CPU fallback)
```

Font channel details: [fonts.md](fonts.md) and `crates/web-app/README.md`.

## tauri desktop host

```bash
cd crates/tauri-app
npm install
npm run build:sdk       # same as web-app: wasm-pack build ../web-view -> sdk/dist
npm run tauri dev       # dev: vite (1420) + the system WebView window
npm run tauri build     # package: vite build -> tauri desktop installer
```

Product naming is uniformly `rofd` (see release-tauri.yml). Platforms:
Windows (WebView2) works out of the box; macOS/Linux are gated on WebKit's
WebGPU progress.

## Convert kernel / CLI

```bash
cargo run -p rofd-convert-cli -- test/ru-yuan-ji-lu.ofd target/convert-probe.pdf --font C:/Windows/Fonts/simsun.ttc  # bin rofd-convert
cargo run -p rofd-convert --example convert_bench --release   # 3-scenario baseline
cargo test -p rofd-convert --test soak -- --ignored           # Linux RSS soak
cd crates/convert/fuzz && cargo +nightly fuzz run convert_ofd -- -runs=20000
```

For real samples with text, **pass `--font`**: under an empty font registry
the fallback system font may be unsupported by krilla's subsetter — since
phase-2 Task 1 this no longer fails wholesale; that font face is
**degraded** (one warning line + its glyphs skipped, exit code 0,
`encode_with_degradation`) but the text goes missing; `--font` guarantees
complete text (spec 2026-11-06 §8). Since phase 3, degradation warnings
surface on all four runtimes via `ConvertWarning`, and parse warnings are
aggregated by kind (a real sample: 739 lines → 3).

## Four bindings: build / demo acceptance

One acceptance bar: **pooled conversion == CLI bytes** (same machine, same
font state).

```bash
# Java (rofd-convert-java)
cargo build --release -p rofd-convert-java          # JNI cdylib rofdconvert(.dll)
cd crates/convert-java && mvn -DskipTests package   # thin jar (fat: -Dnative.dir=<staged> -Dskip.natives.check=true)
bash crates/convert-java/demo/run.sh                # two-act acceptance: JVM pool == CLI (needs %WINDIR%/Fonts/simsun.ttc)

# Node (rofd-convert-node, npm @office-rs/rofd-convert skeleton)
cargo build --release -p rofd-convert-node          # cdylib; the demo renames it rofd_convert_node.node
node crates/convert-node/demo/run.js                # worker_threads pool == CLI

# Python (rofd-convert-py, PyPI rofd-convert)
py -3.1 -m maturin build --release -m crates/convert-py/Cargo.toml -o target/wheels -i "C:/Users/flw/AppData/Local/Python/pythoncore-3.14-64/python.exe"
py -3.1 -m pip install --force-reinstall target/wheels/rofd_convert-*.whl
py -3.1 crates/convert-py/demo/run.py               # ThreadPoolExecutor pool == CLI
# ^ maturin >=1.16 and the explicit -i are required: the bare PATH python is 3.7 and cannot read 3.14 metadata

# wasm (rofd-convert-wasm, npm @office-rs/rofd-convert-wasm)
node crates/convert-wasm/sdk/build.mjs               # wasm-pack --target web -> sdk/dist/wasm
node crates/convert-wasm/demo/run.mjs                # browser main+Worker == CLI (needs Playwright: web-app npm install)
```

The release-lockstep checklist (on `rofd-convert-v*` tags):
[workflow.md](workflow.md).

## Dev-machine pitfalls

- **Python**: `py -3.1` is 3.14; the bare PATH `python`/`pip` pair is 3.7.
  maturin builds must pass the 3.14 interpreter explicitly via `-i`.
- **fuzz**: on Windows the MSVC ASan DLL directory must be on PATH, or the
  fuzz target won't start.
- **web font transitional dep**: needs a sibling checkout of
  `D:/code/fonts` (`file:` protocol).
- **Four worktree-session traps** (verified 2026-11-07):
  1. gitignored fixtures don't follow the worktree: copy
     `crates/web-app/public/fonts/` and `test/*.ofd` in from the main
     checkout before running tests, or gpui-view/component tests fail
     spuriously;
  2. the SDK's `file:` relative dep sits one level deeper in a worktree —
     junction-link `D:/code/fonts` in, then `npm install`; vitest also
     needs the main checkout's `sdk/dist` copied over;
  3. Windows excluded port ranges
     (`netsh interface ipv4 show excludedportrange protocol=tcp`) reject
     vite's 5173/5188 — use a free port inside an allowed range (e.g. 5250)
     + `--host 127.0.0.1`;
  4. D-drive pressure: the main checkout's `target/` can reach 168G+;
     linker LNK1108/LNK1318 ≈ disk-full signal — free space fast with
     `rm -rf target/debug/incremental`.
