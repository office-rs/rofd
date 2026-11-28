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
cargo test -p rofd-io --test write_fidelity # one test binary by name (`--test`); a bare `write_fidelity` is a name filter and runs nothing
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

## Signing line

```bash
cargo test -p rofd-sign --features test-ca               # the crate (verification, encoders, signing, certificate files)
cargo test -p rofd-sign --features test-ca --test sign in_place   # a signature appended in place: the model equals a reload, the file verifies after an ordinary save
cargo test -p rofd-io --test preserving_save --test document_package   # the preserving save (PS series); the document carries its package
cargo test -p rofd-component seal_placement              # the modal seal placement state
cargo test -p rofd-component --features sign             # the component signs and verifies the open document (the module is compiled out without the feature)
cargo test -p rofd-component --test seal_visibility      # a file's seal is painted
cargo test -p rofd-web-view --lib sign_wasm              # the wasm options / report JSON
cargo test -p rofd-web-view --lib a_finished_step        # the two-phase step carries the reports (a filter is a test name: `--lib step_result` runs nothing, exit 0)
cargo test -p gpui-app signing && cargo test -p xilem-app signing   # the hosts' signing flow
cargo build -p rofd-web-view --target wasm32-unknown-unknown         # rofd-sign must build for wasm32
cargo run -p rofd-sign --example gen_builtin_seals -- crates/web-app/public/seals   # redraw the built-in seal pictures (then copy to crates/*/assets/seals)
```

The signing e2e (`cd crates/web-app && npm run e2e -- e2e/sign.spec.ts`)
runs the wasm in `crates/web-view/sdk/dist`: rebuild it first
(`npm run build:sdk` in web-app), or it tests the previous build. The e2e
and the dev server run the SDK's TypeScript source (a Vite alias), so a
change to `sdk/src` needs no build; a type check of the web app (`vue-tsc`)
does — it reads the compiled wrapper in `sdk/dist`, so run `npm run
build:ts` in `crates/web-view/sdk` first, or it reports members that exist.

The reader-side acceptance (does the reference reader show and accept rofd's
signatures) is manual:
`docs/superpowers/manual/2026-11-24-sign-reader-checklist.md`.

## Independent cross-check of signatures (ofdrw)

Validates packages with ofdrw's own validator — the reference
implementation of the archive chain. Needs a JDK, Maven and ofdrw-sign from
Maven Central (or `~/.m2`). Run it after touching the encoders or the
References logic; it has caught what rofd's tests could not:

```bash
cd tools/ofdverify
mvn -q dependency:build-classpath -Dmdep.outputFile=cp.txt
javac -encoding UTF-8 -cp "$(cat cp.txt)" -d out src/main/java/Verify.java   # mkdir -p out first
java -Dfile.encoding=UTF-8 -cp "out;$(cat cp.txt)" Verify <file.ofd>...      # VALID via <container> / INVALID
```

## web host

```bash
rustup target add wasm32-unknown-unknown     # one-time
cd crates/web-app
npm install
npm run build:sdk       # wasm-pack build ../web-view --target web -> sdk/dist
npm run dev             # vite dev server
npm run build           # vite production build
npm run smoke:nogpu     # no-GPU smoke (headless Chrome --disable-gpu must land in the CPU fallback)
npm run e2e -- e2e/sign.spec.ts   # signing on the real wasm: WebGPU and CPU fallback paths
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
     checkout before running tests, or component tests fail spuriously
     (gpui-view's font tests needed `public/fonts/` until 2026-11-27; they
     now read the tracked fixtures under
     `crates/render/tests/fixtures/fonts/`);
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
- **vitest reports below the counts** (verified 2026-11-24): a listener
  that throws is printed as `Errors 1 error` under `Tests N passed` and
  makes the run exit 1. A `| grep "Tests"` filter hides that line and the
  pipe hides the exit code: the SDK suite was red for two weeks while
  every note said "98 green". Take each command's exit code; when a log
  has to be grepped, use `grep -a` (a test that prints a control byte
  turns the log "binary" otherwise).
- **Commit messages with backticks**: write them to a file and commit with
  `-F`; inside `-m "..."` the shell runs what the backticks enclose.
- **Linux CI failures don't reproduce here** (verified 2026-11-27): the WSL
  Ubuntu distro has no build libraries (`pkg-config`, the fontconfig /
  cups / xkbcommon `-dev` packages) and `sudo` needs a password, so a
  native Linux link cannot be run locally — verify Linux-only failures
  with a PR-branch workflow run instead (system deps: [workflow.md](workflow.md) §CI).
