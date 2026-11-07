# rofd convert 模块对外交付 — 设计文档（OFD→PDF：FFI 绑定 + CLI + 多语言 SDK）

日期：2026-11-06
状态：待审查
来源：头脑风暴会话（决策全部由 flw 拍板；整体结构镜像 rword convert spec `2026-10-22-convert-module-design.md`，rofd 特有偏差在 §11 单独成表）。独立评审（builder check，对照两仓实际代码核验）意见已并入：D15 的 Rust 侧存放、D19-D21。

---

## 1. 背景与问题

rofd 的 PDF 编码能力已经存在，但只有编辑器内入口：

- **编码管线已通**：`rofd_export::pdf_from_page_scenes(&[PdfPage])`（`crates/export/src/pdf.rs:81`）纯函数编码；三个宿主（xilem-app / gpui-app / web-view `exportPdf`）都经**活的** `OfdComponent::begin_print` + `PrintSession::page_scene` 组装页场景。
- **场景组装已 headless-ready**：`rofd_render::page_body_scene` + `draw_annotations` 本身不依赖 component（`PrintSession` 只是 `doc + FontStore + PrintContent` 的载体，`crates/component/src/print_session.rs:167`），纯 CPU，无 WebGPU/窗口依赖。
- **对外交付为零**：没有"OFD 字节进、PDF 字节出"的单调用，没有 bin 命令，没有 Java/Node/Python/JS(web) 绑定，上层应用无法集成。
- **服务器语义缺失**：字体注册是每组件实例的（`OfdComponent::register_font`），服务器高并发需要"注册一次、进程共享、实例长期持有"。
- **默认预设变更**（本次决策 D17）：打印/导出/convert 的默认内容预设从"净版（水印✗/批注✗/图章✔）"改为"三层全开（✔✔✔）"。

调研对照沿用 rword convert spec §1 的结论：LibreOffice 系被迫进程隔离（慢/泄漏/非线程安全是病因不是优点）；OnlyOffice 每任务 spawn 子进程（C++ 核心不敢住进宿主）；商业标杆 Aspose.Words 走进程内 FFI，契约"一个线程一个实例、字体进程级共享"。rofd 内核是 Rust、缓存有界（shaped-glyph cache、PrintSession 页缓存）、OFD 输入按敌意数据处理（zip+quick-xml 显式错误分层），具备走 Aspose 模型的条件。

## 2. 目标

1. **六入口一内核**：Rust crate（仓内一等 API）、CLI bin、Java(jar)、Node.js、JS(web/wasm)、Python——同一个转换内核。
2. **服务器高并发**：进程内 FFI 为主，上层应用自持线程池/异步模型，长期实例 反复转换。
3. **公网交付**：Maven Central / npm / PyPI 发布，包内自带各平台二进制，源码不出仓。
4. **视觉零漂移（parity）**：同一份文档 + 同一组内容开关，convert 产出 == 编辑器"导出 PDF"产出（字节级）。

## 3. 非目标（v1）

- **图片输出**（PNG 页图/缩略图）：`OutputFormat` 只留枚举扩展位，不实现。
- **OFD→OFD**（读改写/批注拍平/净化重打包）：不碰，属 D 门（后端读改写）未来工作。
- **常驻服务/daemon/HTTP API**：上层应用有自己的服务框架，拿 FFI 自己包。
- **crates.io 发布**（见 D4）。
- **格式矩阵扩张**：输入只有 .ofd，输出只有 PDF；不做 PDF→OFD（C 门）、图片输出等。
- **编辑能力**：convert 是只读转换，不碰 editor。
- **系统字体扫描**：字体注入只经进程级注册表（D9）；扫描系统字体目录是宿主应用的责任（native 宿主已有 font_index.rs 先例）。
- **musl/Alpine、FreeBSD** 等平台：按需后补（D12）。

## 4. 决策记录

| # | 决策 | 选择 | 备选与否决理由 |
|---|---|---|---|
| D1 | 定位 | 库项目的转换模块，供上层应用集成；部署环境以 Linux 服务器（无头）为主 | 终端用户工具、SaaS 服务 |
| D2 | 并发模型 | **FFI 进程内为主**（Aspose 模型）：字体进程级注册一次、实例同一时刻只被一个线程碰；CLI 为辅（脚本/CI/验证用）；daemon 不做 | 每转换一进程（LibreOffice 的药，非优点）；进程池/常驻监听（上层应用自己包更合适） |
| D3 | API 分层 | **L1 基础 FFI**（内核侧一份实现，同步、实例级、错误进结果不抛异常）+ **L2 高级 API**（每种宿主语言一份胶水，含线程池）；**池不进 Rust** | Rust 内置池·阻塞/异步提交变体（唤醒中转、线程错配、异步/取消/背压语义要穿 FFI，否决；rword D3 同判）。转换是 CPU 工作：宿主侧"N 线程各持一实例"即池的全部本质 |
| D4 | Rust 发布 | **不上 crates.io**：Rust 集成方直接用本仓（私有 git 依赖）。理由：crates.io 强制上传源码；npm/Maven/PyPI 均可只发二进制 | 源码可见+专有许可；私有 registry |
| D5 | 分发通道 | 公网仓库 + **包内自带二进制**：Maven Central（jar 内嵌 .so/.dll，按平台分类器）、npm（napi-rs prebuilds 平台包）、PyPI（平台 wheel，不发 sdist）、web wasm（npm，编译产物） | 要求用户单独装二进制；内网制品 |
| D6 | CLI 分发 | **随 Node 绑定同包发布**：`@office-rs/rofd-convert` 一个 npm 包同时含编程 API 与 `rofd-convert` bin（JS 壳调内核）；平台二进制走 napi 平台包 | 独立 npm CLI 包；GitHub Releases（私库下载要鉴权） |
| D7 | crate 布局 | **新增 `rofd-convert` 会话层 crate**（实例生命周期/进程级字体注册表/FFI 边界纪律/格式枚举），依赖 **component + io + render + export + dom**（rofd 差异，见 D18；render 直入是因为组装循环直接用 `FontStore` 与 `PRINT_ZOOM`）；绑定 crate 各自薄壳 | ① 会话层长进 export（export 无 doc 级管线职责）；② 场景组装下沉 render 让 convert 只依赖 render（`PrintContent` 是 serde 线型、web-view/宿主多处引用，迁移面大；且 component 本就平台无关，依赖它不违反分层——§5） |
| D8 | 输出格式 | v1 仅 PDF；`OutputFormat` 枚举保留扩展位（未来 Png 等加臂不动 L1 签名） | 不抽象等接入时再改（返工更大——全部 L2 绑定要加参数） |
| D9 | 字体模型 | **进程级注册 API**（启动时一次，全局共享）+ 文档嵌入字体（`Resources.font_data`）+ 默认字体兜底，链路沿用 `FontStore::from_resources` + `register_font`；注册只对此后创建的实例生效（含文档说明）；v1 append-only 无注销 | 每实例独立注册（N 份内存）；恢复每调用注册（服务器模型下浪费）；内置系统字体扫描（越权——那是 native 宿主的能力，服务器上字体目录形态不可假设） |
| D10 | Java 目标 | JNI + **Java 8 字节码**兼容（国内服务器存量现实），手动 bindgen 出的 JNI 壳 | Panama FFM（Java 22+）；JNA（性能损耗） |
| D11 | Node 池形态 | **worker_threads 每 worker 一实例**（rofd 实例 !Send，见 D15，Puppeteer 式池是唯一安全形态） | 单线程队列（浪费）；libuv 线程池持 !Send 实例跨线程（UB 风险） |
| D12 | 平台矩阵 | linux-x64-gnu、linux-arm64-gnu、windows-x64-msvc、macos-x64、macos-arm64；musl 延后按需 | — |
| D13 | 版本与发布 | 各包锁步 0.x；独立 `rofd-convert-v*` tag + 手动 dispatch 触发发布工作流 | 跟随主仓 tag（耦合编辑器节奏；rword v0.1.2 事故已验证前车之鉴） |
| D14 | 命名 | crate `rofd-convert` / `rofd-convert-cli` / `rofd-convert-java` / `rofd-convert-node` / `rofd-convert-py` / `rofd-convert-wasm`；npm `@office-rs/rofd-convert`（含 CLI）；Maven `io.github.office-rs:rofd-convert`、jar 内 Java 包名 `io.github.officers`；PyPI `rofd-convert`（账号沿用 rword-convert 已打通的通道，见 §12 遗留）；bin 名 `rofd-convert` | 短名家族（组织身份不明显） |
| D15 | 实例线程契约 | **rofd 可直接预判：落"实例绑线程"分支**——`FontStore` 内部 `Rc<RefCell<parley::FontContext>>`（`crates/render/src/text/font.rs:36`）天生 !Send，Converter 持有它即 !Send。各语言 L2：Java ThreadLocal、Node worker_threads、Python threading.local、wasm 单线程天然满足。字体字节经 `Arc` 共享，多实例无 N 倍内存；每实例的 parley FontContext + shaped-glyph 缓存即实例固有开销。① 期第一个任务加静态断言锁死（§6.1），防未来 FontStore 改 Send 后契约静默漂移——若真改 Send 可放宽为槽池，L2 API 不变；**Rust 侧实例存放按语言自有设计**（评审补充）：Java thread-local 句柄表、Python pyo3 `unsendable`、Node 每 worker 一实例（③ 期验证 napi-rs class 对 !Send 字段的接受度）——rword 绑定的全局 `Mutex` 句柄表断言 Send，**不可照抄**。**计划期修订（2026-11-07）**：`FontStore` 固然 !Send，但按 §6.1 它**逐调用局部构建**、从不跨调用存活——`Converter` 只持 `Arc` 字节快照，**当前实际为 `Send`**；"实例绑线程"降级为 L2 **建议**形态（安全超集，兼容未来引入温引擎/共享 FontStore 的演进），不做 !Send 静态断言（两种未来都合法） | 实例 Sync 内部锁（掩盖数据竞争、吞吐损失）；槽池（!Send 下不可行） |
| D16 | 许可 | **Apache-2.0**（沿用 rword D16 拍板）：公网二进制包按 Apache-2.0 发布，与"源码不出仓"和"未来开源"都兼容，自带专利授权。各包内嵌 LICENSE 与第三方声明；依赖栈须全为兼容的宽松许可（① 期做一次依赖许可审计） | 专有 EULA；MIT（无明确专利条款） |
| D17 | **内容开关默认全开**（rofd 特有，flw 2026-11-06 拍板） | `PrintContent::default()` 从净版（水印✗/批注✗/图章✔）翻为**全开（✔✔✔）**；打印、导出、convert 三处默认一并生效。连带修改：web-app `exportPdf` 显式实参、打印面板初始勾选态、断言净版预设的测试（`default_export_is_clean_copy_preset` 等） | 保持净版默认（被否——用户要全开） |
| D18 | **parity 原则**（rofd 特有） | convert 复用 `PrintSession` 的场景组装：component 新增 headless 构造入口，convert 内部经它取页场景（同一 `page_body_scene` + `draw_annotations` + `PRINT_ZOOM`），保证与编辑器导出**字节一致**（验收见 §8） | convert 自行组装场景（~20 行，漂移风险：PRINT_ZOOM/过滤语义双份） |
| D19 | **默认字体入口**（评审补充） | 进程级新增 `register_default_font(bytes)`（显式默认字体锚点，后注册者生效；初始为 None）+ CLI `--default-font <path>`。与既有概念同源对齐：web SDK init 面的 `defaultFontFamily`（桌面 D5"宋体锚 default"）与 Rust 侧 `OfdConfig::default_font_bytes` 是同一语义的每实例形态，convert 无 TS 胶水层，门开在 facade 的进程级；`register_font` ↔ SDK `registerFont` 同理。回退边界如实文档化：native 构建存在 fontique 系统字体隐式回退（与编辑器空配置行为一致，输出随机器字体环境变化）；wasm 无任何回退，未注册且未嵌入的 CJK 呈豆腐。要求跨机器字节可复现的部署必须显式注册足够字体 | 依赖 `FontStore::register_font` 的"首个成功注册锚定 default"隐式语义（注册顺序变成语义）；convert 内置字体（rword 亦未把字体打进包——其 builtin 清单是商用许可字体，仅运行时读本机目录，缺则静默跳过；rofd 同判：分发负担与许可面）；强制 hermetic（须改 FontStore 的 fontique 配置，连带改编辑器行为，否决） |
| D20 | **warnings 范围**（评审补充） | v1 `ConvertWarning` 只汇 io 侧 `OfdWarning`（MissingFeature / FontSubstituted / ResourceNotFound 三类既有变体 + 消息）；export 编码期非致命降级（glyph run 丢弃等）暂不透出——`pdf_from_page_scenes` 公开面零改动，未来经新增带 warnings 的返回形态**非破坏性**接入 | ① 期就动 export 公开面（扩大 ① 期改动面；健康文档的降级通道基本静默，收益低） |
| D21 | **zip 炸弹姿态**（评审补充） | 接受 rword 同款姿态：预算旋钮在 parse 之后核查（页数/媒体字节），敌意 zip 的解压内存峰值先于预算发生；文档化为已知边界，未来需要时再给 io 加条目尺寸上限（届时不再是"io 零改动"） | ① 期改 io 加条目上限（动核心 crate 解析循环与测试，收益/风险比不划算） |

## 5. 架构总览

```
                 ┌───────────── 绑定/入口层（薄壳，无业务逻辑） ─────────────┐
                 │ rofd-convert-cli    rofd-convert-java   rofd-convert-node │
                 │ (bin rofd-convert)  (JNI cdylib + jar)  (napi-rs cdylib)  │
                 │ rofd-convert-py     rofd-convert-wasm                        │
                 │ (PyO3 cdylib)       (wasm-bindgen, 无 WebGPU)                │
                 └──────────────────────────┬────────────────────────────────┘
                                            │ 只依赖 rofd-convert
┌───────────────────────────────────────────┴──────────────────────────────────┐
│ rofd-convert（会话层 facade）：Converter 实例 / 进程级字体注册表（含默认字体，   │
│ D19）/ OutputFormat{Pdf} / ConvertOptions{content,max_pages,max_media_bytes} / │
│ catch_unwind 边界 / 错误模型（稳定 code + 消息）                              │
└───────────────────────────┬─────────────────────────────────────────────────┘
                            │ 依赖 component + io + render + export + dom
┌───────────────────────────┴─────────────────────────────────────────────────┐
│ component：PrintSession 新增 headless 构造；PrintContent::default 翻全开（D17）│
│ io（parse_ofd → OfdDocument + warnings，零改动）                              │
│ render（page_body_scene / draw_annotations / FontStore，唯一增量：            │
│   `FontStore::registered_family_names` 只读探针，服务 registered_families）   │
│ export（pdf_from_page_scenes，零改动）                                        │
└──────────────────────────────────────────────────────────────────────────────┘
```

分层规则（AGENTS.md 需同步修订）：

- rofd-convert 是 component/io/export 的**上邻 facade**：converter hosts（CLI/绑定/未来服务化）只 import rofd-convert，够不到 component/io/export——与"编辑器宿主只碰 component"同构（镜像 rword"converter hosts 只碰 rword-convert"）。
- rofd-convert 依赖 component **不违反** AGENTS §4.9/§4.1：component 平台无关（无 winit/web-sys/wgpu），convert 的依赖链全在平台无关核心层内，方向严格向上（component → render/editor → dom；io → dom；export 叶子）。
- 绑定 crate 依赖 rofd-convert 一层，互相不依赖；**wasm 绑定不依赖 web-view**（编辑器面）——headless 转换不需要 WebGPU/Canvas，任何浏览器、Web Worker 可用。
- 字体注入只此一门（进程级注册表），Converter 不开 per-call 字体口（镜像 rword 评审裁定：多租户"每文档上传字体"明确不支持，注册表会无界增长）。

## 6. 模块细则

### 6.1 `rofd-convert`（L1 核心，`crates/convert/`）

```rust
/// 进程级，启动时调用一次或若干次；线程安全（内部锁）。v1 append-only（无注销）。
pub fn register_font(bytes: Vec<u8>) -> Result<(), FontRegisterError>;
pub fn register_font_file(path: &Path) -> Result<(), FontRegisterError>;
pub fn registered_families() -> Vec<String>;          // 仅进程级注册的族名
pub fn register_default_font(bytes: Vec<u8>) -> Result<(), FontRegisterError>; // 显式默认字体锚点（D19）

/// 长期持有的转换器实例。同一时刻只被一个线程使用（D15：!Send，编译器强制）。
pub struct Converter { /* 注册表快照 + 默认字体等配置 */ }
impl Converter {
    pub fn new() -> Result<Converter, ConvertError>;  // 拿进程级字体快照（Arc 共享，无字节拷贝）
    pub fn convert(&mut self, format: OutputFormat, ofd: &[u8], opts: &ConvertOptions)
        -> Result<ConvertOutput, ConvertError>;       // 字节进字节出单调用
}

pub enum OutputFormat { Pdf }                          // 扩展位（D8）：加臂不动签名
pub struct ConvertOptions {
    pub content: PrintContent,                         // 默认全开（D17），复用 component 类型
    pub max_pages: Option<u32>,                        // 病态文档预算：防千页炸弹钉死池线程
    pub max_media_bytes: Option<usize>,                // 媒体字节总预算。None = 宽裕默认（① 期定值）
}
pub struct ConvertOutput { pub bytes: Vec<u8>, pub warnings: Vec<ConvertWarning> }
```

- **内部管线**：`parse_ofd(bytes)` → 预算核查（页数 / 媒体字节，超限 `ConvertError::BudgetExceeded`；核查时点在 parse 之后，zip 解压内存峰值先于预算发生，D21）→ `FontStore::from_resources(&doc.resources, default)`（`default` = `register_default_font` 的快照，None 则无默认，D19）+ 注册表字体逐个 `register_font`（复刻 `build_font_store` 的 ~10 行组装循环，不动 component）→ `PrintSession::headless(doc, fonts, content)` → 逐页 `page_scene(n)` → `pdf_from_page_scenes`。
- **FontStore 每次调用按文档重建**（shaped-glyph 缓存逐调用冷启）：实例长寿买到的是注册表共享与字体不重扫，**不是**温排形缓存（与 rword 温引擎的差异如实声明）；输出稳定性按 §8 的字节稳定定义约束。
- **单调用转换**而非 open/edit/close 三段式：转换无编辑态，三段式只会制造泄漏面（镜像 rword 裁定）。
- **FFI 边界纪律**（绑定 crate 共用）：每个入口 `catch_unwind`，panic → `ConvertError::Internal`（panic 是 bug、错误是数据）；错误 = 稳定 code + 消息字符串，不跨 FFI 抛 Rust panic/异常。
- **线程契约（D15 修订 2026-11-07）**：`FontStore` 逐调用局部构建，`Converter` 仅持 `Arc` 注册表快照——**当前为 `Send`**（测试钉住现状，见 ① 期 plan）；L2 仍按"实例绑线程"建议实现。不做 !Send 静态断言：若未来引入温引擎（实例持 FontStore/排形缓存），契约随实现收紧，届时各 L2 无需改 API。
- **超时/取消不进 L1**（镜像 rword 评审裁定）：L1 同步单调用；病态文档用预算旋钮挡住下限风险，wall-clock 超时与取消是宿主责任（Node AbortSignal→terminate worker、Java/Python 进程级 watchdog），各 L2 文档明示。预算旋钮必须 ① 期冻结进 API。
- **warnings**（D20）：v1 只汇 io 的 `OfdWarning`（模板缺失/字体替换/资源缺失三类既有变体），统一为 `ConvertWarning`（稳定种类 + 消息）随 `ConvertOutput` 返回；export 编码期非致命降级暂不透出（`pdf_from_page_scenes` 零改动），未来经新增返回形态非破坏性接入。

### 6.2 `rofd-component` 演进量（最小）

- `PrintSession::new` 现为 pub(crate)；新增 **pub headless 构造**：`PrintSession::headless(doc: OfdDocument, fonts: FontStore, content: PrintContent)`——内部 `print_gen` 用新建 `Arc<AtomicU64>`，`is_stale` 恒 false（headless 无并发变异源）。编辑器路径 `begin_print` 行为零变化。
- `PrintContent::default()` 翻全开（D17）。连带：xilem-app/gpui-app `export_pdf`（用 default，自动跟）、web-app `exportPdf` 显式实参 `(false,false,true)`→`(true,true,true)`、打印面板初始勾选态、翻转断言净版的测试。
- FontStore 组装循环**不下沉**：convert 复刻 ~10 行（D7 备选②的否决理由），避免为 convert 单方面搬动 component 内部结构。

### 6.3 `rofd-convert-cli`（`crates/convert-cli/`）

- `rofd-convert <input.ofd> <output.pdf>`；flags：`--font <path>`（可多次，等价进程级注册）、`--fonts-dir <dir>`（目录内全部文件注册，镜像 rword CLI）、`--default-font <path>`（D19）、`--no-watermark` / `--no-annotations` / `--no-seal`（默认全开，只提供关闭项）、`--max-pages <n>` / `--max-media-bytes <n>`。
- 退出码：0 成功；非 0 = 稳定错误 code；warnings 打到 stderr；镜像 rword CLI 语义（脚本/CI 友好，字节基准载体——各绑定验收的对照物）。

### 6.4 `rofd-convert-java`（`crates/convert-java/`，② 期）

- JNI 手动 bindgen 壳、Java 8 字节码（major 52）；thin jar（`-Djava.library.path`）+ zero-config fat jar（native 目录内嵌，沿用 rword 2026-11-05 zero-config native loading 设计）双形态。
- L2：`RofdConverter` 类，实例 ThreadLocal 持有（D15）；`convert(byte[], ConvertOptions)` 同步返回。
- 坐标：Maven `io.github.office-rs:rofd-convert`，Java 包名 `io.github.officers`（与 groupId 去连字符一致，镜像 rword）。
- 验收：demo 两幕（act 1 fallback tier、act 2 zero-config fat jar）输出 == CLI 字节。

### 6.5 `rofd-convert-node`（`crates/convert-node/`，③ 期）

- napi-rs cdylib；L2 提供 `createConverter()` + worker_threads 池 helper（每 worker 一实例，D11）。
- npm `@office-rs/rofd-convert`：编程 API + `rofd-convert` bin（JS 壳）同包（D6）；平台二进制走 napi optionalDependencies 平台包。
- 验收：pooled demo 输出 == CLI 字节。

### 6.6 `rofd-convert-py`（`crates/convert-py/`，③ 期）

- PyO3；L2 实例 `threading.local` 持有（D15）；maturin 构建平台 wheel（不发 sdist，D5）。
- PyPI `rofd-convert`；验收：pooled demo 输出 == CLI 字节。

### 6.7 `rofd-convert-wasm`（`crates/convert-wasm/`，④ 期）

- wasm-bindgen `--target web`；纯 CPU（rofd-render 场景录制无 GPU 依赖，render/io/export 均无平台 crate 依赖）——任何浏览器、Web Worker、Node 可用。
- npm 包（命名沿用 rword convert-wasm 形态：`@office-rs/rofd-convert-wasm`）；`sdk/build.mjs` 同型构建脚本；main + worker 双 demo。
- 验收：浏览器 demo 输出 == CLI 字节。

## 7. 分期

| 期 | 内容 | 出口判据 |
|---|---|---|
| ① | `rofd-convert` 内核 + `rofd-convert-cli`；D17 默认翻转（含三宿主与测试）；线程契约文档化（Send 现状测试 + 绑线程建议）；预算旋钮；parity 测试（§8）；fuzz 靶 `convert_ofd`；soak 骨架；依赖许可审计 | CLI 转真实样例绿；parity 字节一致测试绿；fuzz 20000 runs 无 panic；`cargo test/clippy/fmt` 全绿 |
| ② | `rofd-convert-java`：JNI 壳 + jar 双形态 + 两幕 demo + Maven 发布件 | demo 两幕 == CLI 字节 |
| ③ | `rofd-convert-node` + `rofd-convert-py`：napi/PyO3 + 平台包 + npm/PyPI 发布件 | pooled demo == CLI 字节（两语言） |
| ④ | `rofd-convert-wasm`：wasm 绑定 + sdk + main/worker demo | 浏览器 demo == CLI 字节 |

各期内部照 superpowers 工作流（spec → plan → TDD → 自审 → 提交）走；① 期完成后 AGENTS.md 一次性同步（§10）。

## 8. 质量与验收

- **byte-stable**：同（进程字体状态, 输入字节, 选项, **同一机器字体环境**——native 存在 fontique 系统隐式回退，见 D19）→ 同输出字节；多实例（各绑一线程）并发转换输出互不干扰且一致——Linux RSS soak 测试（镜像 rword `soak -- --ignored`，量级按 rofd 实测定）。
- **parity（D18）**：同一 fixture + 同 flags，`Converter::convert` 输出 == 编辑器导出路径（`begin_print` → `pdf_from_page_scenes`）输出，字节级断言，① 期钉死。为此 xilem-app `export_pdf` 的编码核心提为可测的返回字节形态（行为不变）。
- **fuzz**：`convert_ofd` 靶——随机/变异字节进 `Converter::convert`，断言不 panic、错误走 `ConvertError`（zip 容器 + XML 的敌意输入面）。
- **预算**：max_pages / max_media_bytes 触发 `BudgetExceeded` 的单元测试。
- **覆盖率** 80%（仓规）。
- **渲染保真边界沿用 v1 现状**：模板继承/JBIG2/瓦片图等桩处理经 warnings 透出，不在 convert 范围内解决。
- **系统回退字体 × krilla 子集化**（2026-11-07 Task 8 手测发现，宿主 Windows + ru-yuan-ji-lu.ofd）：native 构建的 fontique 系统回退字体可能不被 krilla 子集化支持——真实文本文档在空注册表下可能编码失败，CLI 以退出码 3 干净报告；要求跨机器可复现的部署按 D19 显式注册足够字体。

## 9. 发布工程

- **锁步清单**（`rofd-convert-v*` tag 时必须全部匹配，镜像 rword 11 文件制）：
  `crates/{convert,convert-cli,convert-java,convert-node,convert-wasm,convert-py}/Cargo.toml`、`crates/convert-java/pom.xml` + `example/pom.xml`、`crates/convert-node/package.json`、`crates/convert-wasm/sdk/package.json`、`crates/convert-py/pyproject.toml`。
- 工作流 `convert-release.yml` 同型：按 tag 前缀门控（`rofd-convert-v*`），手动 dispatch；与主仓 `v*` 发布线独立（rword v0.1.2 前车之鉴）。
- 各包内嵌 LICENSE（Apache-2.0）与第三方声明文件。
- 产物命名统一 `rofd-convert` 家族。
- **① 期依赖许可审计结论（2026-11-07，D16 收口）**：`rofd-convert` 运行时外部依赖 = **0**（`crates/convert/Cargo.toml` 的 `[dependencies]` 全为 workspace 内部 path 依赖）；`rofd-convert-cli` 运行时依赖仅 `rofd-convert`。dev-deps 不随产物分发：`rofd-convert` 的 `zip`（workspace 钉 2.2，与 rofd-io 同源，手工打包 fixture 复包）+ `rofd-convert-cli` 的 rofd-io/rofd-dom（fixture 组装）。传递栈与编辑器线一致——`Cargo.lock` 差异 bb4d05d..e37c927 仅新增 `rofd-convert`/`rofd-convert-cli` 两个内部包，**零新增外部包**；fuzz 侧 `libfuzzer-sys` 位于独立 fuzz workspace（自有 lock，commit e37c927 起跟踪），不进根运行图。结论：无新增许可面，依赖栈全宽松许可。

## 10. AGENTS.md 同步清单（① 期完成后）

- §2 常用命令：convert 系列（CLI 运行、fuzz、soak、各绑定 demo/构建）。
- §3 仓库布局：六个新 crate 行 + 分层图中 convert 的位置（component/io/export 的上邻 facade）。
- "两条历史偏离"区补第三条同型说明：converter hosts 只碰 rofd-convert。
- §4 不变量补一行：convert 不依赖任何平台 crate（同 component 及以下）。
- 依赖钉版表：无新增外部 pin（napi-rs/PyO3/wasm-bindgen 版本 ②③④ 期定）。
- 常见陷阱表：❌"在绑定 crate 里直接 import rofd-io/rofd-export" → ✅"只碰 rofd-convert"。

## 11. 与 rword convert spec 的镜像差异

| 维度 | rword（2026-10-22 spec） | rofd（本文档） |
|---|---|---|
| 输入 | .docx | .ofd（zip 容器 + XML，rofd-io 显式错误分层） |
| 输出 | PDF（OFD 预留 sink） | 仅 PDF（枚举扩展位预留 Png 等） |
| 内容开关 | 无此概念 | `PrintContent` 三层（水印/批注/图章），**默认全开（D17）**，随 `ConvertOptions` 传入 |
| 内核依赖 | rword-export 一层（管线都在 export） | component + io + export + dom（D7：headless PrintSession 保 parity） |
| Send 核查 | 待核查（① 期实测定分支） | **FontStore !Send 但逐调用局部构建 → Converter 当前 Send**（D15 计划期修订）；L2 仍建议实例绑线程；②③ 绑定的 **Rust 侧实例存放**自行设计（rword 句柄表断言 Send，不可照抄） |
| parity 验收 | 无（无编辑器导出对照需求） | **有（D18）**：convert == 编辑器导出，字节级 |
| warnings | export 管线 warnings | **v1 只汇 io `OfdWarning`（D20）**；export 降级透出延后非破坏性接入 |
| 默认字体 | builtin manifest＝9 个商用字体**文件名**的运行时目录读取（`RWORD_FONTS_DIR`/编译期 dev 回退路径；目录 gitignored，**发布包不带字体字节**，缺文件静默跳过退系统） | 显式 `register_default_font`（D19）+ CLI `--fonts-dir`；不做内置清单——**生产兜底形态与 rword 实际分发形态一致**（进程注册 + native 系统发现 + wasm 无回退） |
| 字体来源 | 注册表 + 系统 hybrid 兜底 | 注册表 + 文档嵌入 + 默认字体兜底（**无系统扫描**，非目标明示） |
| 其余（D1-D6、D8、D10-D14、D16、发布、fuzz/soak/预算/FFI 纪律） | — | L2 形态 1:1 镜像 |

## 12. 遗留问题

1. **排期**（已定，flw 2026-11-07）：convert 四期与 V1 收尾路线（C2 手术刀保存接线 / C3 交互体验 / C4 小尾巴）**本轮一并做掉**，不互相排队；convert 内部仍按 ①→②→③→④ 的依赖序执行，各期出各自 plan。
2. **PyPI 账号**：rword D14 当年"待确认"；rword-convert 已实际发布过（v0.1.x），rofd-convert 沿用同一账号通道，③ 期前确认一次即可。
3. **npm wasm 包名**：`@office-rs/rofd-convert-wasm` 待 ④ 期注册时确认无冲突。
