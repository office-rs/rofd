# AGENTS.md

本文件为在本仓库工作的 AI 编程代理（以及人类协作者）提供上下文。先读这里，再动代码。

设计 spec 是事实的最终来源：最新一份 [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md)。本文档与代码现状对齐；二者冲突时以代码 + spec 为准，并回来修本文档。

---

## 1. 项目是什么

**rofd** 是一个 OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，双平台（native + WASM）。as-built 参照项目是 `D:/code/rword`（OOXML 编辑器库，同型改造已全部落地）。

- **v1 范围**：查看 + 批注。主文档（body）**只读渲染**；批注是**唯一可变层**。
- **库形态**：以 `OfdComponent` 为唯一集成入口（类比 `<textarea>`），宿主控制消息循环并转发事件。库本身不取系统时间、不直接依赖 GUI 框架。
- **平台边界**：库 = 平台无关核心（dom/io/render/editor/component）+ 三个平台适配器（xilem-view/web-view/gpui-view）；`crates/xilem-app`、`crates/web-app`、`crates/tauri-app` 与 `crates/gpui-app` 是宿主应用，不是库的交付物（见 §4.9）。
- **非目标**：编辑 body 内容、创建/验签电子签名、全保真渲染（模板继承/JBIG2/瓦片图按需补，v1 桩处理）、实时协同。
- 为 B（后端生成）/ C（PDF→OFD）/ D（后端读改写）留门，但不实现。

---

## 2. 常用命令

> 工作目录始终为仓库根 `D:\code\rofd`，除非另说明。Shell 为 bash（Unix 语法）。

### 构建 / 测试

```bash
cargo build --workspace          # 全量编译
cargo test  --workspace --exclude tauri-app --exclude rofd-web-view
cargo test  -p rofd-io           # 单 crate 测试
cargo test  -p rofd-io surgical  # 按名过滤（手术刀字节保留测试）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

覆盖率目标 80%：`cargo llvm-cov --workspace`（需安装 `cargo-llvm-cov`）。

### 运行 native 宿主

```bash
cargo run -p xilem-app                           # 空编辑器
cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd  # 打开根目录 test/ 下的 .ofd
cargo run -p xilem-app --example print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out  # 打印手测通道（Pages 载荷经 GDI lpszOutput 落文件）
```

命令行路径参数经宿主 `host::document_io::load_ofd` 加载。native 宿主以空字体配置启动（`OfdConfig::new(Arc::new(vec![]))`，见 `crates/xilem-app/src/main.rs`），字形交给系统字体回退；仓库自带的 CJK 字体位于 `crates/web-app/public/fonts/`（git 跟踪，无需下载）。

### 运行 gpui 桌面宿主

```bash
cargo run -p gpui-app                            # 空编辑器（多文档会话制）
cargo run -p gpui-app -- test/ru-yuan-ji-lu.ofd  # 打开根目录 test/ 下的 .ofd
cargo run -p gpui-app --example gpui_print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out  # 打印手测通道（同 xilem-app 先例）
```

gpui 宿主与 xilem/tauri 两线**并存**（spec 2026-10-31 R1，不承担替代责任）。默认字体宋体（`%WINDIR%\Fonts\simsun.ttc`，读取失败退空配置）；文档声明族经 `font_index.rs` 懒扫描系统字体目录（mmap + name 表全语言提取）按需加载本机字节（tauri 桌面通道同型）。文件对话框走 gpui 原生 `prompt_for_paths`（无 rfd）。手测清单见 `docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md`，功能对齐审计见 `docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`。

### 构建 / 运行 web 宿主

```bash
rustup target add wasm32-unknown-unknown          # 一次性
cd crates/web-app
npm install
npm run build:sdk         # wasm-pack build crates/web-view -> sdk/dist
npm run dev               # vite 开发服务器
npm run build             # vite 生产构建
npm run smoke:nogpu       # 无 GPU 冒烟（headless Chrome --disable-gpu 必须落入 CPU 回退）
```

`build:sdk` 等价于 `cd ../web-view && wasm-pack build --target web --out-dir sdk/dist`。
字体通道：Noto 两族经 `@office-rs/font-runtime/vite` 从 npm 切片按需加载，商用六族走本机切片 `public/fonts-sliced/`（gitignored，裸 `Ofd.init` 不再默认拉 CDN 字体）——详见 `crates/web-app/README.md`。`file:` 过渡依赖需要 sibling 检出 `D:/code/fonts`（spec 2026-10-17 §7：npm 包上线后切 `^1.0.0`，届时移除本注记）。

### 构建 / 运行 tauri 桌面宿主

```bash
cd crates/tauri-app
npm install
npm run build:sdk         # 同 web-app：wasm-pack build crates/web-view -> sdk/dist
npm run tauri dev         # 开发：起 vite(1420) + 系统 WebView 窗口
npm run tauri build       # 打包：vite build -> tauri 打成桌面安装包
```

tauri-app 前端**复用 web-app 源码**（`main.ts` 从 `../web-app/src` 导入 App.vue）；仅在挂载前用 `setFileHost` 注入原生文件桥（`src/host/tauri.ts`，走 tauri-plugin-dialog + tauri-plugin-fs 的原生打开/保存对话框）。字体与 sample.ofd 从 web-app 的 `public/` 本地加载（vite `publicDir` 指向它），脱离 CDN。**桌面字体通道**（draft 2026-10-18，纯系统通道）：tauri 桥提供 `listSystemFonts`+`readSystemFont` 双能力，Rust 壳启动时扫描 `%WINDIR%\Fonts`+用户字体目录建索引（`src-tauri/src/font_index.rs`，mmap+name 表全语言提取），SDK 按文档声明族**懒加载**本机字节（零再分发、无白名单），桌面 manifest 集为空（Noto 切片不进桌面，vite 插件 `families: []`），默认字体宋体；浏览器路径逐字节不变。`src-tauri` 的 Rust 壳只启动窗口 + 注册插件与双命令，**不依赖任何 rofd crate**（§4.9）。平台支持：Windows（WebView2）开箱即用；macOS/Linux 受 WebKit 的 WebGPU 进度限制。

产物命名统一为 `rofd`（详见 release-tauri.yml）。

### 转换内核 / CLI（headless convert）

```bash
cargo run -p rofd-convert-cli -- test/ru-yuan-ji-lu.ofd target/convert-probe.pdf --font C:/Windows/Fonts/simsun.ttc  # bin rofd-convert
cargo run -p rofd-convert --example convert_bench --release  # 3 场景基线
cargo test -p rofd-convert --test soak -- --ignored          # Linux RSS soak
cd crates/convert/fuzz && cargo +nightly fuzz run convert_ofd -- -runs=20000
# ^ Windows：MSVC ASan DLL 目录须在 PATH 上
# Java 绑定（rofd-convert-java）
cargo build --release -p rofd-convert-java     # JNI cdylib rofdconvert(.dll)
cd crates/convert-java && mvn -DskipTests package   # thin jar（fat：-Dnative.dir=<staged> -Dskip.natives.check=true）
bash crates/convert-java/demo/run.sh           # 两幕验收：JVM 池化转换 == CLI 字节（需 %WINDIR%/Fonts/simsun.ttc）
# Node 绑定（rofd-convert-node，npm 包 @office-rs/rofd-convert 骨架）
cargo build --release -p rofd-convert-node     # cdylib → demo 自动改名为 rofd_convert_node.node
node crates/convert-node/demo/run.js           # 验收：worker_threads 池 == CLI 字节
# Python 绑定（rofd-convert-py，PyPI 包 rofd-convert）
py -3.1 -m maturin build --release -m crates/convert-py/Cargo.toml -o target/wheels -i "C:/Users/flw/AppData/Local/Python/pythoncore-3.14-64/python.exe"
py -3.1 -m pip install --force-reinstall target/wheels/rofd_convert-*.whl
py -3.1 crates/convert-py/demo/run.py          # 验收：ThreadPoolExecutor 池 == CLI 字节
# ^ maturin ≥1.16 且须显式 -i：PATH 裸 python 是 3.7，读不动 3.14 元数据
```

带文字的真实样例**建议给 `--font`**：空字体注册表下系统回退字体可能不被 krilla 子集化支持——②期 Task 1 起不再整体失败，该字体面**降级**（一行 warning + 跳过其字素、退出码 0，`encode_with_degradation`），但文字会缺，`--font` 保证文字完整（spec 2026-11-06 §8 边界的现行形态）。③期 Task 1 起降级 warning 经 `ConvertWarning` 透出到全部四端（CLI stderr/Java takeWarnings/Node·Py warnings）；③期 Task 2 起解析警告按种类聚合（真实样例 739 行 → 3 行，`(×N)` 计数）。

发布锁步清单（`rofd-convert-v*` tag 时）：`crates/convert{,-cli,-java,-node,-py}/Cargo.toml` + `crates/convert-java/pom.xml` + `crates/convert-node/package.json` + `crates/convert-py/pyproject.toml`（版本须一致）；`cargo deny check licenses` 应绿（`deny.toml`）。

---

## 3. 仓库布局与分层

严格 5 层单向依赖，**反向边禁止**。每个 crate 的 `[dependencies]` 是依赖方向的唯一事实来源。**平台边界线画在 component 与适配器之间**：component 及以下六个 crate（含叶子 rofd-export）平台无关（不依赖 winit/web-sys/wgpu/arboard 等平台 crate），xilem-view / web-view / gpui-view 是仅有的三层平台绑定；rofd-convert（转换会话层）同属平台无关核心，是 component/io/render/export 的上邻。

```
crates/convert-cli  ─┐
                     ├─► rofd-convert ──┐
crates/convert-java ─┘                  │
                                        │
crates/xilem-app      ─┐                │
                       ├─► xilem-view ─┤
                       │                ├──► component ─┬─► render ──┐
crates/web-app ─► web-view ─────────────┤                ├─► editor ──┴─► dom
   (JS + Vite,         (wasm-bindgen    │                │
    非 cargo 成员)      适配器)          │                └─► io ────────┘
                                        │
crates/gpui-app ─► gpui-view ───────────┘
```

converter hosts（CLI/未来绑定）只碰 rofd-convert，与"编辑器宿主只碰 component"同构。

| crate            | 路径                 | 职责 | 依赖（rofd 内部） |
| ---------------- | -------------------- | --- | ----------------- |
| `rofd-dom`       | `crates/dom`         | 纯数据模型 | — |
| `rofd-io`        | `crates/io`          | `parse_ofd` / `save_ofd`（手术刀）/ `write_ofd`（全量）+ `PackageHandle` | dom |
| `rofd-export`    | `crates/export`      | Scene→krilla PDF 编码器（打印/导出产物转换，imaging+krilla 叶子 crate） | —（无 rofd 内部依赖） |
| `rofd-render`    | `crates/render`      | `imaging::record::Scene` 构建 + hit_test/caret_rect | dom |
| `rofd-editor`    | `crates/editor`      | 批注选区、命令模式、Step/Transaction/History | dom |
| `rofd-component` | `crates/component`   | **唯一集成入口** `OfdComponent`：ViewEvent、Callbacks、脏缓存 | dom + render + editor + office-rs-print（类型层）（**不依赖 io**） |
| `rofd-convert`   | `crates/convert`     | 转换会话层 facade：`Converter`/进程级字体注册表/预算/`catch_unwind`；converter hosts 只 import 此 crate | component + io + render + export + dom |
| `rofd-convert-cli` | `crates/convert-cli` | bin `rofd-convert`：零依赖 argv 解析/退出码 0-2-3/字体注册 warn-and-continue | rofd-convert |
| `rofd-convert-java` | `crates/convert-java` | JNI cdylib `rofdconvert` + Java L1/L2（`io.github.officers`），Maven `io.github.office-rs:rofd-convert` thin/fat jar | rofd-convert + jni 0.21 |
| `rofd-convert-node` | `crates/convert-node` | napi cdylib `rofd_convert_node`（每方法 catch_unwind）+ JS L2（worker_threads 池/cli.js），npm `@office-rs/rofd-convert` 骨架 | rofd-convert + napi 2 |
| `rofd-convert-py` | `crates/convert-py` | PyO3 扩展 `rofd_convert.rofd_convert_native`（lease 实例 + GIL 释放 convert），PyPI `rofd-convert`（abi3-py39 wheel） | rofd-convert + pyo3 0.30 |
| `rofd-xilem-view`| `crates/xilem-view`  | masonry/xilem 薄适配器：`OfdWidget` + `ofd()`/`ofd_with_config()` | component |
| `rofd-web-view`  | `crates/web-view`    | WASM 薄适配器：`WasmOfd` + `WebGpuRenderTarget` + TS SDK | component + io + editor + dom + rofd-export + office-rs-print（类型层） + vello + imaging + imaging_vello + wgpu + web-sys + rofd-convert（共享页组装） |
| `rofd-gpui-view` | `crates/gpui-view`   | gpui 适配层 SDK：`OfdGpuiView`（Entity/Element）+ `GpuiOfdHandle` + IME 桥 + 打印对话框状态机/discover-only services | component + dom + imaging + gpui-pre/gpui-pre-platform + office-rs-print（list_printers）+ ttf-parser + image（**不依赖 io**） |
| `xilem-app`      | `crates/xilem-app`   | xilem 宿主应用（文件对话框 + 工具栏 UI 策略） | xilem-view + component + io + dom + xilem + rfd + rofd-convert（共享页组装） |
| `gpui-app`       | `crates/gpui-app`    | gpui 宿主应用（多文档会话 TabBar + 工具栏/文件菜单/打印面板） | gpui-view + io + export + component + dom + office-rs-print + gpui 家族/kit + memmap2/ttf-parser + rofd-convert（共享页组装） |
| web-app          | `crates/web-app`     | Vite + TS 宿主应用（非 cargo 成员） | `@office-rs/rofd`（= `crates/web-view/sdk`） |
| `tauri-app`      | `crates/tauri-app`   | Tauri 桌面宿主应用 | tauri 壳不依赖 rofd crate；前端复用 web-app |

> **两条历史偏离（改造后的 as-built）**：
> 1. component 保持 io-free——io 依赖从不下沉到组件；
> 2. native 侧 io 调用已**上移到宿主**（`xilem-app/src/host/document_io.rs`），适配器 rofd-xilem-view 不依赖 io；web-view 侧仍持 io（wasm 宿主自己就是"宿主"）；gpui 线同律——rofd-gpui-view 不依赖 io，io 在 `gpui-app/src/host/document_io.rs`。
>
> **gpui 线（2026-11-01 起，spec 2026-10-31）**：rofd-gpui-view + gpui-app 与 xilem/tauri 宿主**并存**（R1，不承担替代责任）。适配层七文件照 rword 同型（gpui_sink/font_cache 拷贝改写自 rword@4e0b25b6，打印状态机/services 拷贝自 xilem-view）；功能对齐分母 = web-app，逐 id 审计见 `docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`。

---

## 4. 不可违反的不变量

### 4.1 依赖严格向上，反向边禁止

### 4.2 body 只读；批注是唯一可变面

### 4.3 手术刀保存：未触碰条目字节级保留

`save_ofd(doc, pkg)`：批注条目从 `AnnotationModel` 重新序列化；`Document.xml` 字节级打补丁（`<MaxUnitID>` + 缺失时插入 `<Annotations>` loc，严格阅读器只经此引用发现批注）；其余条目原样拷字节。改 io 保存逻辑后，核心测试（`crates/io/tests/round_trip.rs`、`save_surgical.rs`）必须仍绿。宿主层另有 `crates/xilem-app/tests/c2_save.rs`（#[ignore]，真实样例）。

### 4.4 库不取系统时间

库内**绝不**调 `Date::now()` / `SystemTime::now()`。宿主通过 `set_clock(author, ts)` 注入；命令用注入时间填 `created`/`modified`。**例外（2026-09 明确，2026-10-13 修订）**：单调 `Instant`（非挂钟、不可持久化、不随系统时钟跳变）可用于动画计时——caret blink 的 deadline/`tick_blink` 在两目标统一使用。wasm 侧经 `web_time::Instant` cfg 别名（`performance.now()` 支撑；`std::time::Instant::now()` 在 wasm32-unknown-unknown 是硬 panic），native 侧仍为 std。

### 4.5 渲染产出 `imaging::record::Scene`，不是 `vello::Scene`

### 4.6 错误显式分层，绝不静默吞

### 4.7 没有 `Format` trait

### 4.8 ID 约定

### 4.9 平台边界：功能内聚核心层，适配器只做绑定

### 4.10 convert 及以下不依赖任何平台 crate（与 component 同律）；converter hosts 只碰 rofd-convert

---

## 5. 各 crate 工作要点

### rofd-component

- 唯一入口 `OfdComponent`：`new_native()`/`new_wasm()`（`#[cfg(target_arch = "wasm32")]` 门控，非 feature）、`handle_event`。
- `OfdConfig`：`default_font_bytes`、`page_gap`（默认 20）、`zoom`（初始 PX_PER_MM，`with_zoom` 可覆盖）。
- 场景脏缓存（终态公共 API）：`update_scene()` 按脏标志重合成、`scene() -> &Scene`、`set_viewport_size(f64,f64)`、`mark_scene_dirty`（pub(crate)）。
- 文本编辑：`paste_text`、`delete_annotation`、`tick_blink`（两目标统一，wasm 由 `renderFrame` 驱动，见 §4.4）、preedit 状态机（组件自持 `Option<PreeditState>`，简化 caret overlay，不做 ghost-document 回流）。
- 打印内核缝（spec 2026-10-31）：`begin_print(PrintContent) -> PrintSession`，内容三开关（水印/批注/图章，默认**全开 ✔✔✔**——spec 2026-11-06 D17 翻转，commit 052f528；`PrintContent::default()` 三项皆 true，宿主与 convert 均经 default 派生）；页场景/纸型/页计划全经 session（`page_scene`/`page_parameters`/`plan`，`1-3,5` 范围语法由 office-rs-print 解析，宿主不重写）；native 一键直打 `print_direct`（begin_print→plan→payload→`print_pages`）。

### rofd-convert（headless 转换线，spec 2026-11-06）

- 会话层 facade：converter hosts（CLI/未来 java/node/py/wasm 绑定）只 import 此 crate，够不到 component/io/render/export。长期 `Converter` 实例 + 单调用 `convert(format, ofd_bytes, opts)` 字节进字节出（无 open/edit/close 三段式）；`catch_unwind` 边界（panic → `ConvertError::Internal`，`catch_panics` pub 供绑定共用 FFI 纪律）。
- 进程级字体注册表（D9/D19）：`register_font`/`register_font_file`/`registered_families`/`has_default_font`/`register_default_font`（显式默认字体锚点，**后注册者生效**）；**注册必须先于 `Converter::new()`**——实例持注册表快照（Arc 共享零拷贝），之后的注册要换新实例才可见。append-only 无注销；同字节幂等 no-op。字体注入只此一门，无 per-call 口。
- 预算旋钮：`ConvertOptions{content, max_pages, max_media_bytes}`，默认 `DEFAULT_MAX_PAGES`=10000 / `DEFAULT_MAX_MEDIA_BYTES`=1GiB，超限 `BudgetExceeded` fail-fast（核查在 parse 之后，zip 解压内存峰值先于预算——D21 已知边界）。
- parity 原则（D18）：convert 与三宿主（xilem-app/gpui-app/web-view）②期 Task 2 起同经共享页组装 `pdf_pages`/`pdf_pages_at`（单一组装循环，落在本 crate），与编辑器"导出 PDF"**字节级一致**（`crates/convert/tests/parity.rs` 钉死）。
- 字体面选择跨进程确定（②期 Task 2b）：TTC 多个 family 名按**最低面索引**锚定规范名（`render/text/shape.rs`，fontique 的 HashMap 迭代序逐进程随机，不锚定则同一转换两次跑出不同字节）。
- FontStore 逐调用重建：无跨调用温排形缓存（实例长寿买到的是注册表共享，不是温缓存）；`Converter` 当前为 `Send`（D15 修订 2026-11-07，测试钉住现状非承诺），L2 仍按"实例绑线程"建议实现。
- CLI（`rofd-convert-cli`，bin `rofd-convert`）：零依赖 argv 解析；退出码 0 成功 / 2 用法错误 / 3 转换失败；`--font`/`--fonts-dir`/`--default-font` 注册失败 warn-and-continue；`--no-watermark`/`--no-annotations`/`--no-seal`（默认全开 D17，只提供关闭项）。
- 警告流（③期 D20 收口）：`ConvertOutput.warnings: Vec<ConvertWarning>`（`non_exhaustive`：`Parse(OfdWarning)` / `ParseRepeats{representative,count}` / `Encode(String)`）——io 解析警告按种类聚合（`(×N)` 计数，真实样例 739 行 → 3 行）+ export 降级行，`Display` 一行一条；四端一致：CLI stderr、Java `takeWarnings`、Node/Py `warnings` 字符串数组。

### rofd-convert-node / rofd-convert-py（Node·Python 绑定，③期）

- convert-node（napi cdylib `rofd_convert_node`）：每方法 `catch_unwind`（napi 边界 panic 会 abort Node）；`Converter.convertToPdf(ofd, options?)` + 注册五件套；JS L2 `index.js`/`pool.js`（worker_threads 每 worker 一实例、崩溃 reject+respawn）；`cli.js` 为 npm bin `rofd-convert`（@office-rs/rofd-convert，平台包 optionalDependencies 槽位）。
- convert-py（PyO3 `rofd_convert.rofd_convert_native`，abi3-py39 wheel）：`#[pyclass] Converter` 内部 `Mutex<Option<…>>` lease（与 JNI 句柄表同力学）——GIL 释放期（`py.detach`）同实例并发第二调用抛 `ConvertError("converter in use")`；`convert(ofd, *, watermark=…, max_pages=…)` kwargs；吞吐用 `threading.local` 每线程一实例。构建须 maturin ≥1.16 + 显式 `-i` 3.14 解释器（PATH 裸 python=3.7）。
- 三绑定 demo 验收统一：**池化转换 == CLI 字节**（同机同字体状态；③期实测三运行时同 sha256 `82c3b352…`）。

### rofd-convert-java（JNI 绑定，②期）

- JNI 面 10 入口（NativeLink 的 `nNewConverter`…`nTakeWarnings`）全经 `catch_panics`——Rust panic 绝不跨 FFI，统一抛 `io.github.officers.OfdConvertException`。
- 租约句柄表：每次 JNI 调用 `take` 借出 Converter、调用结束 `put` 归还；`InUse`（并发占用）/`Stale`（已关/未知 id）为稳定异常前缀，id 单调递增无 ABA。
- L2（Java）：`OfdConverter`(+Options) AutoCloseable，线程契约为**一次一线程**（D15，共享请走池）；PhantomReference cleaner 线程兜底回收漏 close 的句柄（Java 8 无 Cleaner API）；`OfdConverterPool` 固定槽位池（`borrow()` 阻塞直至空闲）。
- NativeLoader 三级零配置加载：`rofd.native.path`/`ROFD_NATIVE_PATH`（显式覆盖）→ classpath `natives/<platform>/<libfile>`（fat jar 内嵌）→ `System.loadLibrary` 兜底；Java 8 字节码（major 52）。
- Maven `io.github.office-rs:rofd-convert`（thin/fat 双 jar，fat 需四平台 natives 齐全）；两幕 demo 验收 = 池化 JVM 转换与 CLI **字节一致**（`demo/run.sh`）。

### rofd-xilem-view（as-built 九条契约）

见 §"Native Ofd Widget"。

### rofd-web-view

- `WasmOfd`（wasm-bindgen）+ `WebGpuRenderTarget` + JS 事件桥。工厂 `create_wasm_ofd`。wasm-pack --target web。
- CPU 回退（spec 2026-10-18）：WebGPU 不可用时 SDK 落 `create_wasm_ofd_cpu`（vello_cpu + 页位图缓存）于**同一 canvas**——前提是 `create_wasm_ofd` **先探 adapter（`compatible_surface: None`）后 `create_surface`**（canvas 一旦绑了 webgpu context 就永远拿不到 2d context，回退必挂）；该链路由 `npm run smoke:nogpu` 冒烟守护，勿倒回"先绑 surface 再探 adapter"。
- SDK 在 `crates/web-view/sdk/`，入口 `Ofd.init(container, config?)`，发布为 npm 包 `@office-rs/rofd`。
- 渲染调度（spec 2026-10-13 web-render-on-demand）：内核 `needs_frame() -> FrameNeed` 是唯一真值；wasm 融合导出 `renderFrame()` 返回 `0` / `>0`ms / `-1`。SDK 自挂起调度器（`scheduler.ts` `RenderScheduler` + `with-kick.ts` Proxy kick + watchdog）：`0`→rAF 立即重渲、`>0`→唯一 `setTimeout` 唤醒（blink deadline，web 的 WaitUntil）、`-1`→完全挂起；任何 wasm 方法调用经 Proxy kick 恰预约一个 rAF（`renderFrame` 豁免）；`visibilitychange` 隐藏完全静默；`pauseRendering`/`resumeRendering` 为宿主政策层手柄。

### xilem-app

- 纯 `Xilem::new_simple` 宿主。AppState 为 plain data（命令队列 + file/package 路径 + modified + has_selection + warnings + context_menu），另持一个 `Arc<Mutex<Option<Result<(), String>>>>` 保存结果槽：保存命令回传落盘结果，`app_logic` 开头排空，成功才清 modified、失败保持未保存态。命令队列与结果槽的锁仅在瞬间持有。
- 文件 I/O 落 `src/host/document_io.rs`：`LoadedOfd{document, package, warnings}`、`load_ofd`、`save_ofd`（Some(pkg) 手术刀 / None 全量；原子写）。Save-As 不产生新 PackageHandle。

---

## Native Ofd Widget（masonry/xilem 适配器）

`rofd-xilem-view` 把编辑器嵌为一等 masonry widget。宿主不碰 winit——focus、pointer capture、IME session、clipboard 路由全部经 masonry：

```
ofd(queue) / ofd_with_config(queue, config)    xilem View (ofd_view.rs)
  │ build: OfdWidget::new(config); actions ↔ .on_change/.on_context_menu/… handlers
  │ rebuild: drains `queue` → OfdWidget::with_component (host→widget commands)
  ▼
OfdWidget (masonry Widget)                      owns OfdComponent, pending queue,
  │                                               effective focus, pointer cursor
  │ masonry events → masonry_events.rs → ViewEvent
  │ component callbacks → pending queue → ctx.submit_action (after every touch)
  │ paint: component.update_scene() + painter.replay(scene)
  ▼
OfdComponent (rofd-component)
```

Key contracts:

1. **Host→widget commands**：`OfdCommand = Arc<dyn Fn(&mut OfdComponent) + Send + Sync>`；`OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>`，rebuild 时经 `with_component` 排空。任何被处理的回调返回 `MessageResult::Action(())` → 重跑宿主逻辑 → rebuild → 排空。命令必须 `Fn`（按钮重复触发），捕获数据在闭包内 clone。
   命令排空非空时提交一次内部 `OfdWidgetAction::HostCommandWake`（不映射任何宿主 handler，仅触发再一拍 app_logic），使宿主能消费命令结果；下一拍队列已空、不再提交 wake，故有界、不产生无限重渲染。
2. **Widget→host events**：`OfdWidgetAction` 镜像组件回调面；`PointerCursorChanged` 内部消费（驱动 `get_cursor`），绝不 submit。
3. **有效焦点** = widget 键盘焦点 ∧ 窗口焦点；组件出生未聚焦（首次点击前 caret 隐藏）。`Ime::Disabled → FocusLost` 强制提交 preedit，带防御性 regain。
4. **IME**：`accepts_text_input` 自动起停 session；每个触点末尾从 `caret_rect()` 刷新 `set_ime_area`。
5. **剪贴板**：Ctrl+V = `TextEvent::ClipboardPaste` → `paste_text`；Ctrl+C 在 widget 拦截经 `ctx.set_clipboard` 写出。Ctrl+X 在 rofd 为 copy-only（运行期 body 只读、无 selection extent，绝不删除）；宿主菜单无 Cut 项。
6. **Zoom（rofd 有意差异，无镜像）**：组件 viewport 直接乘法缩放（`zoom *= factor`，基线 `PX_PER_MM = 96/25.4`）；ctrl+wheel 发 `ZoomAt{factor, center}`（×1.1 / ×0.9，center = 指针视口位置），钳制 `[PX_PER_MM×0.25, PX_PER_MM×3.0]`。宿主/widget 不保留 f32 zoom 镜像。fit 缩放模式（适应宽度/高度/页面，spec 2026-10-10）：`set_zoom_mode(ZoomMode)`，基准页=当前页；resize/换文档自动重适配、翻页不重适配；手动缩放退出回 Custom 并 fire `on_zoom_mode_change`。fit 结果钳 `[FIT_MIN_ZOOM, MAX_ZOOM]`（5%~300%）；手动缩放下限动态——zoom 已低于 25% 时下限放宽到 5%（防上拽）。
7. **Blink**：`on_anim_frame` 在聚焦期间保活动画；500ms 时序归组件 `tick_blink`（单调 Instant）。
8. **Scroll**：wheel 事件 set_handled——滚动与自绘滚动条归组件；无 portal、无 CANVAS_CONTENT_HEIGHT。
9. **无障碍**：`Role::Document`。

If you're tempted to reintroduce a winit-layer bridge、canvas-origin 记账、或每帧 force_repaint 排序——don't；widget 就是整个平台适配器。

---

## 6. 测试约定

- 目标 80% 覆盖，TDD（先红后绿）。
- 手术刀字节保留是核心测试；真实样例测试保持 `#[ignore]`：
  - `cargo test -p xilem-app --test c2_save -- --ignored`
- 渲染用场景结构断言（对象数/类型，非像素）。
- xilem-view 无头 harness（masonry_testing，9 个）：打字、双击选词、IME、滚轮、ctrl+wheel、ctrl+wheel 纯横向、Ctrl+C、窗口焦点门控、fit 模式命令与退出。

---

## 7. 依赖钉版（动之前必读）

| crate | 源 | 版本/rev |
| ----- | -- | --------- |
| `xilem` | linebender/xilem git | `271a27a6d4a930f7878d404f9014e3c50a3a9b88` |
| `masonry_testing` | 同上（dev-dep） | 同 rev |
| `imaging` / `imaging_vello` | crates.io | `0.0.1` |
| vello / parley | crates.io | `0.8` |
| wgpu | crates.io | `28` |
| `office-rs-print` | office-rs/print git | `c2f99d5`（与 rword 同 pin；gpui-view 的 list_printers + gpui-app 的 join_all 同源） |
| `jni` | crates.io | `0.21`（convert-java；rword 同 pin） |
| `napi` / `napi-derive` / `napi-build` | crates.io | `2`（convert-node；rword 同 pin） |
| `pyo3` | crates.io | `0.30`（convert-py，abi3-py39；rword 同 pin） |
| `krilla` | crates.io | `0.8`（feature `pdf`，PDF 编码） |
| `jni` | crates.io | `0.21`（与 rword convert-java 同 pin；rofd-convert-java 的唯一外部依赖） |
| `gpui-pre` / `gpui-pre-platform` | crates.io | `=0.3.10`（zed@08cbb88 快照重发布；官方 crates.io gpui 0.2.2 缺 Windows/IME 修复，不可换用） |
| `gpui-kit` / `gpui-kit-assets` | crates.io | `=0.8.1`（图标走 `with_assets(AllAssets)`） |

gpui 家族（gpui-pre / gpui-pre-platform / gpui-kit / gpui-kit-assets）**整批 bump、不可单动**——kit 0.8.1 钉 gpui-pre `=0.3.10`，升级时四个一起动（workspace `Cargo.toml` 注释即契约）。

Toolchain：`rust-toolchain.toml` 钉 `1.98.1`；新 pin 的包声明 `rust-version = 1.96`。已无 masonry_winit/winit 直接依赖（masonry_winit 内部使用 winit）。

---

## 8. 工作流

superpowers 工作流：spec → plan → TDD → 自审 → 提交，全部 conventional commits。

---

## 9. 常见陷阱

| ✅ Do | ❌ Don't |
| ----- | -------- |
| render 用 imaging Painter API | 在 render 里直接构造 vello::Scene |
| 宿主 set_clock（Instant 仅用于 blink 动画） | 库内调挂钟 |
| editor 只改 .annotations | 在 editor 里改 pages |
| xilem-view 只依赖 component | 往适配器塞 io/业务逻辑 |
| 改 io 后跑手术刀测试 | 只断言模型相等 |
| converter 绑定/CLI 只碰 rofd-convert | 绑定/CLI 直接 import rofd-io/rofd-export/component |
| SDK 渲染调度经 `RenderScheduler`/kick（真值 = `needs_frame`） | 在 SDK 起恒定 rAF 循环；往 tick 里加经 Proxy 的 wasm 读而不依赖 `inLoopTick` 兜底 |

---

## 10. 编码规范

- 设计文档可以保留 WPS 字样，但是代码注释、代码变量、代码文件命名不要存留 WPS 字样。
- **语言约定（2026-10-20 起）**：commit message 与代码注释一律用英文；中文仅用于 spec/plan/draft 等设计文档（`docs/superpowers/`）及面向用户的 UI 文案。

---

## 11. 参考项目

- **`D:/code/rword`** — Rust + GPU 的文档编辑器库，rofd 本次 masonry/xilem 改造与更名的 as-built 参照：适配器三文件（word_widget/masonry_events/word_view）、宿主（xilem-app + host/document_io）、更名 commit 序列。
- **`D:/code/reditor`** — 更早的 OOXML 编辑器库，rofd 分层骨架的历史来源。
