# rofd convert ③ 期（Node/Py 绑定）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 `rofd-convert-node`（napi-rs cdylib + JS L2 池 + `@office-rs/rofd-convert` npm 包骨架含 `rofd-convert` bin）与 `rofd-convert-py`（PyO3 扩展 `rword 同型 lease + maturin abi3 wheel），各以"池化转换 == CLI 字节"demo 验收；并先清偿 ② 期终审判定的三条 ③ 期入口标准（D20 警告透出面、警告洪水治理、release 卫生批）。

**Architecture:** 两绑定都是 rofd-convert 内核的薄壳：Node 侧 `#[napi]` class `Converter`（每方法 catch_unwind——napi 边界 panic 会 abort Node）+ JS `index.js`/`pool.js`（worker_threads 每 worker 一实例）+ `cli.js`（bin 壳）；Python 侧 `#[pyclass]` + 内部 `Mutex<Option<Converter>>` lease（与 Java 句柄表同力学，GIL 释放期防同实例并发）+ `py.detach`。镜像参照 `D:/code/rword/crates/{convert-node,convert-py}`。

**Tech Stack:** napi 2 + napi-derive 2（Node）；pyo3 0.30（`Python::detach`，rword 同 pin）；maturin 1.15（本机经 `py -3.1`，Python 3.14）；Node 24 / npm 11（本机）。三者均为 MIT/Apache 宽松许可——本计划新增的全部外部依赖。

**Spec:** `docs/superpowers/specs/2026-11-06-rofd-convert-design.md`（§6.5 Node、§6.6 Python、§5 分层）。② 期终审裁定见 ledger/记忆：D20 透出面为本计划 Task 1。

## Global Constraints

- converter hosts 只 import rofd-convert(-node/-py) 的宿主语言面；绑定 crate 只依赖 rofd-convert + 自身绑定框架（napi/pyo3），互相不依赖。
- 每个跨 FFI 入口 panic 边界纪律：Node 侧 catch_unwind（napi panic=abort）；Py 侧 catch_panics + lease。错误=消息字符串，绝不跨 FFI 抛 Rust panic。
- 新增外部依赖白名单（过 §9 许可）：`napi = 2`、`napi-derive = 2`（workspace deps + build.rs）、`pyo3 = 0.30`（features abi3-py39）。除此之外零新增。
- npm 包名 `@office-rs/rofd-convert`（bin `rofd-convert` 随包，D6）；平台包槽位 `@office-rs/rofd-convert-<platform>` 骨架（实际发布属 release 工程）。PyPI `rofd-convert`（abi3-py39 wheel，不发 sdist）。
- Node addon 本地名 `rofd_convert_node.node`（cargo cdylib 名 `rofd_convert_node`，Windows 产物 `.dll` 复制改名 `.node`——rword 同流程）；Py 扩展模块名 `rofd_convert`。
- 英文代码注释/commit；conventional commits；验证基线 `cargo test --workspace --exclude tauri-app --exclude rofd-web-view` + clippy + fmt。
- 非目标：npm/PyPI 实际发布（release 工程另案）、musl、常驻服务。

## Review Focus

1. **napi 边界 panic 纪律**：任一 addon 方法内 panic 被 catch_unwind 转错误而非 abort Node → Task 4（单元测 + demo 全链）。
2. **Py lease 并发**：GIL 释放期同实例并发第二调用得 `ConvertError("converter in use")` 而非数据竞争 → Task 7（镜像 rword 测试形态）。
3. **worker 隔离与崩溃自愈**：pool worker 崩溃换新不丢队列语义（reject 排空 + respawn）→ Task 5/6。
4. **警告通道全通**：D20 后 export 降级警告与 io 警告在 CLI/Java/Node/Py 四面都可见且经洪水治理有界 → Task 1/2 测试 + Task 6/8 demo 输出。
5. **字节验收真实**：两 demo 各自与**独立 CLI 进程**产物逐字节相等（非自比）→ Task 6/8。

---

### Task 1: ③期入口(a)——D20 警告透出面（ConvertWarning 统一 + export detailed 返回）

**Files:**
- Modify: `crates/export/src/pdf.rs`（新增非破坏 detailed 入口）
- Modify: `crates/convert/src/lib.rs`（ConvertWarning 枚举 + 汇流）
- Modify: `crates/convert-cli/src/main.rs`（打印两类警告）
- Modify: `crates/convert-java/src/lib.rs`（nConvert 停靠随新类型小调）
- Test: `crates/export/tests/`、`crates/convert/tests/convert_api.rs`

**Interfaces:**
- export：`pub fn pdf_from_page_scenes_detailed(pages: &[PdfPage]) -> Result<PdfEncodeOutput, ExportError>`；`pub struct PdfEncodeOutput { pub bytes: Vec<u8>, pub warnings: Vec<String> }`（现有 `pdf_from_page_scenes` 保持签名、内部委托丢弃 warnings——健康路径字节不变）。
- convert：`pub enum ConvertWarning { Parse(rofd_dom::OfdWarning), Encode(String) }`（+ `Display`，non_exhaustive）；`ConvertOutput.warnings: Vec<ConvertWarning>`（**破坏性类型收紧**——仓内消费方本任务全改齐；这是 spec D20 原意的 ConvertWarning，①期裁决"复用 OfdWarning"在 D20 面前按预定回退路径演进）。

- [ ] **Step 1: RED**——export：detailed 入口在**人为坏字体 seam** 下返回字节+警告（扩 Task-① 的 seam 测试）；convert：`convert_output_carries_encode_warnings`（构造经 seam 的坏字体路径不可行于集成测试——改为单元级：`ConvertWarning` Display/汇流函数测试 + 依赖注入点直接测）。
- [ ] **Step 2: 实现两 crate 改动**——汇流顺序：先 io `Parse` 后 export `Encode`（稳定序）。
- [ ] **Step 3: CLI/Java 对齐**——CLI 逐行 `warning: {w}`（Display 已含语义前缀）；Java nConvert 停靠逻辑随类型编译修正（仍存 Display 字符串）。
- [ ] **Step 4: GREEN + 全量**（parity/concurrency 必须原样绿——汇流不改字节）。
- [ ] **Step 5: 手测**——裸 CLI 转真实样例：stderr 现在出现 `skipped font subsetting` 行（②期不可见的信号现在可见）。
- [ ] **Step 6: Commit** `feat(convert): unified ConvertWarning surfaces export degradation (spec D20)`

### Task 2: ③期入口(b)——警告洪水治理（真实样例 737 条/convert）

**Files:**
- Modify: `crates/convert/src/lib.rs`（聚合层）
- Test: `crates/convert/tests/convert_api.rs`

- [ ] **Step 1: 根因调查**——转 `test/ru-yuan-ji-lu.ofd` 抓全量警告样本归类（预期：`MissingFeature { feature: "PageBlock"… }` 每页重复）；把发现写进提交信息。
- [ ] **Step 2: RED**——聚合契约测试：N 条同 kind+feature 的 Parse 警告折叠为 1 条 `Parse` + 附加计数的表述（如 Display 后缀 `(×N)`，或聚合进枚举新形态——设计自由度给实现者，契约=**种类有界**：去重后条数 ≤ max(去重前种类数, 16)）；同 kind 不同 entry 采样前 5 条 + 计数。
- [ ] **Step 3: 实现聚合**（在 convert 汇流处，io 行为零变化——io 测试不动）。
- [ ] **Step 4: GREEN + 手测**——真实样例 stderr 从 ~737 行降到有界（记录前后行数）。
- [ ] **Step 5: Commit** `feat(convert): bounded warning aggregation for real-document floods`

### Task 3: ③期入口(c)——release 卫生批（NOTICE + 出处注记）

**Files:**
- Create: `NOTICE`（repo 根，第三方声明：krilla/zip/quick-xml/parley/fontique/swash/jni/napi/pyo3/krilla 依赖链——审计后如实列出）
- Create: `crates/convert/LICENSE`+`NOTICE`、`crates/convert-cli/LICENSE`+`NOTICE`、`crates/convert-java/LICENSE`+`NOTICE`（镜像 rword 每 crate 双文件；发布物打包用）
- Create: `crates/render/tests/fixtures/fonts/README.md`（出处注记：TestFont.ttf/TestFontCjk.otf/TwoFamily.ttc 的来源与许可——DejaVu（Bitstream Vera 派生）等，含 TwoFamily 生成方法）
- Modify: spec §9（NOTICE 已落位的一句更新）

- [ ] **Step 1: 依赖清单审计**——`cargo tree -p rofd-convert -p rofd-convert-cli -p rofd-convert-java`（+ 本计划后续两 crate 预留）梳理直接依赖 → NOTICE 草稿（名称+许可+版权行）。
- [ ] **Step 2: 落文件**（rword 对应文件为模板，语言/格式镜像）。
- [ ] **Step 3: Commit** `docs: NOTICE files, crate LICENSE/NOTICE pairs, font fixture provenance (phase-3 hygiene)`

### Task 4: rofd-convert-node crate（napi cdylib）

**Files:**
- Create: `crates/convert-node/Cargo.toml`（workspace 成员；`[lib] crate-type=["cdylib"] name="rofd_convert_node"`；deps rofd-convert + napi2/napi-derive2（workspace deps）；build.rs napi_build）
- Modify: 根 `Cargo.toml` members + workspace.dependencies（napi/napi-derive，注明与 rword 同 pin）
- Create: `crates/convert-node/src/lib.rs`
- Test: `crates/convert-node/src/lib.rs` 内单测（纯逻辑）+ 集成经 demo

**Interfaces（Task 5 JS 对端逐字对齐）:**
- `#[napi] pub struct Converter`：`new()`；`convertToPdf(ofd: Buffer, options?: Options) -> ConvertOutput`（`Options { watermark?: bool, annotations?: bool, seal?: bool, maxPages?: number, maxMediaBytes?: number }`，缺省=内核默认；`ConvertOutput { bytes: Buffer, warnings: string[] }`——警告经 Display 字符串）。
- 模块级：`registerFont(bytes: Buffer) -> bool`、`registerFontFile(path: string) -> boolean`（读失败抛错、坏字体 false——镜像内核契约）、`registerDefaultFont(bytes: Buffer) -> boolean`、`hasDefaultFont() -> boolean`、`registeredFamilies() -> string[]`、`version() -> string`。
- 每方法体 `catch_panics`（napi 边界 panic=abort Node，必须拦）。

- [ ] **Step 1: RED**——纯逻辑单测（options 映射 ≤0→None、format 枚举、错误 Display）。
- [ ] **Step 2: 实现**——镜像 `D:/code/rword/crates/convert-node/src/lib.rs`（JsConverter 骨架）+ rofd 扩展面。
- [ ] **Step 3: GREEN + `cargo build --release -p rofd-convert-node` 产 .dll → 复制改名 `rofd_convert_node.node`（demo 脚本做，本步只验证产物存在）**。
- [ ] **Step 4: Commit** `feat(convert-node): napi addon - Converter class, registry, panic boundary`

### Task 5: Node JS L2 + cli.js + package.json

**Files:**
- Create: `crates/convert-node/{index.js,index.d.ts,pool.js,pool-worker.js,cli.js,package.json}`（镜像 rword 同名文件；addon 加载三级：本地 .node → `../../target/release/rofd_convert_node.node` → 平台包槽位）
- Test: demo 覆盖（Task 6）；`node -e` 冒烟本步做

**要点：** pool.js/pool-worker.js 镜像 rword（worker_threads 每 worker 一实例、least-loaded 派发、崩溃 reject+respawn）；cli.js 对齐 rofd Rust CLI 全旗标集（`--font`/`--fonts-dir`/`--default-font`/`--no-watermark`/`--no-annotations`/`--no-seal`/`--max-pages`/`--max-media-bytes`/`--stdout`/`-h`/`-V`，退出码 0/2/3）；package.json `name:"@office-rs/rofd-convert"`、`bin: {"rofd-convert": "./cli.js"}`、optionalDependencies 平台槽位注释骨架。
- [ ] Step 1: 移植五文件。2: `node -e "require('./index.js')"` 冒烟 + `node cli.js --help`。3: Commit `feat(convert-node): JS L2 - pool workers, cli shell, npm package skeleton`

### Task 6: Node 验收 demo（== CLI 字节）

**Files:**
- Create: `crates/convert-node/demo/run.js`（镜像 rword：build release → CLI 产基准 → pool 8 worker 转 → `Buffer.equals` 断言 → 打印 `BYTES MATCH CLI`；含 sha256）

- [ ] Step 1: 写 demo。2: 全链跑通（记录哈希/字节数/线程数入提交信息）。3: Commit `test(convert-node): pooled converts byte-identical to CLI`

### Task 7: rofd-convert-py crate（PyO3 lease）

**Files:**
- Create: `crates/convert-py/Cargo.toml`（workspace 成员；cdylib `rofd_convert`；deps rofd-convert + pyo3 0.30 abi3-py39（workspace dep））
- Modify: 根 `Cargo.toml` members + workspace.dependencies（pyo3，注明 rword 同 pin）
- Create: `crates/convert-py/src/lib.rs`（镜像 rword：`create_exception! ConvertError`、`#[pyclass] Converter` 内部 `Mutex<Option<...>>` lease、`convert(ofd, *, watermark?, annotations?, seal?, max_pages?, max_media_bytes?)` kwargs、`py.detach` 释放 GIL、catch_panics；rofd 扩展：`register_default_font`、`has_default_font`；`__version__`）

- [ ] Step 1: RED（lease 并发语义单测 + kwargs 映射单测——镜像 rword 的测试思路）。2: 实现。3: `cargo test -p rofd-convert-py` + `py -3.1 -m maturin build -m crates/convert-py/Cargo.toml` 出 wheel（本机验证可装：`py -3.1 -m pip install --force-reinstall <wheel>` 到用户环境——装完即用即留（demo 需要），报告注明）。4: Commit `feat(convert-py): PyO3 extension - lease instance, kwargs options, GIL-released convert`

### Task 8: Py 验收 demo（== CLI 字节）

**Files:**
- Create: `crates/convert-py/demo/run.py`（镜像 rword：ThreadPoolExecutor 每 worker 一实例、转 fixture → 与 CLI 基准逐字节比对 → 打印 `BYTES MATCH CLI`；字体注册 simsun.ttc）

- [ ] Step 1: 写 demo。2: `py -3.1 crates/convert-py/demo/run.py` 全链绿（记录哈希入提交信息）。3: Commit `test(convert-py): pooled converts byte-identical to CLI`

### Task 9: AGENTS.md 同步（③期部分）

**Files:**
- Modify: `AGENTS.md`（§2 命令：node/npm 与 py/maturin 通道 + 两 demo；§3 布局表两行 + 图；§5 两小节（Node 池形态/Py lease 形态各一段）；§7 钉版表 napi 2/napi-derive 2/pyo3 0.30 三行；lockstep 清单补 convert-node/convert-py 两 Cargo.toml + package.json + pyproject.toml）

- [ ] Step 1: 增量。2: 全量绿 + 两 demo 复跑。3: Commit `docs: AGENTS.md convert-node/convert-py entries (phase 3)`
