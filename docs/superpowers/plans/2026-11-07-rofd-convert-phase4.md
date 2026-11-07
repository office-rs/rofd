# rofd convert ④ 期（wasm 绑定）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 `rofd-convert-wasm`（wasm-bindgen `--target web`，纯 CPU）+ npm 包 `@office-rs/rofd-convert-wasm` 骨架 + main/worker 浏览器 demo，验收 = **浏览器输出哈希 == 同机 CLI 基线哈希**（spec §7 ④）。

**Architecture:** 镜像 `D:/code/rword/crates/convert-wasm`（lib.rs 暖实例静态 + registerFont 失效语义；sdk/build.mjs；demo HTTP 服务 + Playwright 驱动）。rofd 偏差：注册面带 `registerDefaultFont`/`hasDefaultFont`、注册结果 bool 映射、warnings 经 `ConvertWarning` Display、**无需** rword 的 getrandom/wgpu 图门（workspace uuid 已带 `js`；内核图无 wgpu），需 web-view 同款 `wasm-opt = false` 元数据门。

**Tech Stack:** wasm-bindgen 0.2 / js-sys 0.3（workspace 已有）；wasm-pack 0.14（本机）；Playwright（web-app node_modules，本机）。

**Spec:** `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.7、§7 ④、§12-3（npm 包名待注册确认）。

## Global Constraints

- 依赖：`rofd-convert` + `wasm-bindgen`/`js-sys`（workspace）——**零新增外部依赖**。
- `panic = abort` 姿态（spec §6.1 注记）：`catch_panics` 在 wasm 编译为无害守卫，错误仍走 `Result → JsError`。
- 字体只经 `registerFont(bytes)`（浏览器无系统发现）；成功注册使暖实例失效（下轮 convert 重建快照——比 native D9 略宽，rword 同型，doc 注明）。
- 单线程/realm 语义：无 lease（与 Node 绑定同裁定）；main 与每个 Worker 各自独立模块实例。
- 英文注释/commit；验证：`cargo test -p rofd-convert-wasm`（native 单测）+ `cargo check -p rofd-convert-wasm --target wasm32-unknown-unknown` + demo。
- npm 包名 `@office-rs/rofd-convert-wasm`（骨架，不实发布）。

## Review Focus

1. **暖实例失效语义**：registerFont 成功后，下一 convert 能看到新字体（demo 断言 fontEmbedded）。
2. **main == worker**：两 realm 独立实例产同字节（demo 断言）。
3. **== CLI 字节**：demo driver 侧跑同字体 CLI 基线，与页面 mainSha 相等（spec §7 ④ 的真验收）。
4. **wasm 图编译**：无 wgpu/getrandom 门的情况下 wasm32 check 通过（若需要门，按 web-view 先例加并注明）。
5. **错误面**：坏输入 → JS Error（含 zip 字样），demo 不因之挂。

---

### Task 1: rofd-convert-wasm crate + lib.rs
- Create: `crates/convert-wasm/Cargo.toml`（workspace 成员；cdylib；workspace wasm-bindgen/js-sys；`[package.metadata.wasm-pack.profile.release] wasm-opt = false` 同 web-view 注记）
- Create: `crates/convert-wasm/src/lib.rs`：`ConvertResult{bytes: Uint8Array, warnings: Vec<String>}`（getter 克隆）；`registerFont(&[u8])->bool`（Result 映射 + 失效暖实例）、`registerDefaultFont`、`hasDefaultFont`、`registeredFamilies`、`convertToPdf(&[u8])->Result<ConvertResult,JsValue>`（默认选项；暖实例 take/put；catch_panics；warnings Display 串）、`version()`。纯逻辑（warnings 映射）native 单测 RED→GREEN。
- 验证：native 测试 + `cargo check --target wasm32-unknown-unknown`。
- Commit: `feat(convert-wasm): wasm-bindgen surface - warm instance, registry, warnings`

### Task 2: sdk 构建 + npm 骨架 + README/LICENSE/NOTICE
- Create: `crates/convert-wasm/sdk/build.mjs`（镜像 rword：wasm-pack build --target web --release → dist/wasm）+ `sdk/package.json`（@office-rs/rofd-convert-wasm）+ `crates/convert-wasm/pkg/package.json` 若 rword 有则镜像；`README.md`/`README.zh-CN.md`（用法/字体语义/无选项面注明）；LICENSE/NOTICE 对（根复制）。
- 验证：`node sdk/build.mjs` 产 pkg。
- Commit: `feat(convert-wasm): sdk build script, npm package skeleton, bilingual READMEs`

### Task 3: demo（main+worker，== CLI 哈希）
- Create: `crates/convert-wasm/demo/index.html`（镜像 rword：载 pkg、registerFont(assets/font.bin)、主线程 + Worker 各 convert fixtures sample（ru-yuan-ji-lu.ofd）、sha256、写 `window.__demo{magic,deterministic,workerDeterministic,sameAsWorker,fontRegistered,fontEmbedded,mainSha,workerSha,warnings}`；Worker 内独立 import pkg）
- Create: `crates/convert-wasm/demo/run.mjs`（镜像 rword driver：wasm-pack build → stage fixture+`C:/Windows/Fonts/simsun.ttc`（fail-fast）→ HTTP serve → Playwright chromium → 取 __demo → **跑 CLI 基线（--font simsun.ttc）比对 mainSha == 基线 sha256** → 打印 `wasm == CLI BYTES MATCH`）
- 验证：`node crates/convert-wasm/demo/run.mjs` 全绿。
- Commit: `test(convert-wasm): browser demo - main+worker byte-identical to CLI`

### Task 4: AGENTS.md 同步
- §2 命令（build.mjs、demo）、§3 表 + 图、lockstep 清单补两文件（convert-wasm Cargo.toml + sdk package.json）。
- 验证：workspace 全绿 + demo 复跑。
- Commit: `docs: AGENTS.md convert-wasm entries (phase 4)`
