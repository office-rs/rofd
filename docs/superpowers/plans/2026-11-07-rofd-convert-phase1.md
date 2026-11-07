# rofd convert ① 期（内核 + CLI）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 headless OFD→PDF 转换内核 `rofd-convert`（会话层 facade：进程级字体注册表 + Converter 单调用转换 + 预算旋钮 + FFI 纪律）与 `rofd-convert-cli`（bin `rofd-convert`），并完成 D17 内容开关默认全开翻转与 D18 parity 字节验收。

**Architecture:** `rofd-convert` 依赖 component + io + render + export + dom（依赖严格向上，全平台无关）；内部管线 `parse_ofd → 预算核查 → FontStore（嵌入+默认+注册表快照）→ PrintSession::headless → 逐页 page_scene → pdf_from_page_scenes`；FontStore 逐调用局部构建（Converter 只持 Arc 快照，当前为 `Send`，D15 修订）。CLI 为零依赖薄壳。② 期（Java）、③ 期（Node/Py）、④ 期（wasm）各自另出 plan。

**Tech Stack:** Rust（workspace 现有栈：zip/quick-xml/parley/krilla 均已钉版，无新增外部依赖）；CLI 手写 argv 解析（镜像 rword convert-cli）。

**Spec:** `docs/superpowers/specs/2026-11-06-rofd-convert-design.md`（已批准；D15 计划期修订 2026-11-07 已并入）。镜像参照仓：`D:/code/rword` 的 `crates/convert`、`crates/convert-cli`。

## Global Constraints

- 工具链 `rust-toolchain.toml` 钉 1.98.1；新 crate 不新增任何外部依赖（内部 crate + thiserror 都不在新增集——错误类型手写 Display，不引 thiserror）。
- 依赖方向：`rofd-convert` → rofd-component + rofd-io + rofd-render + rofd-export + rofd-dom；CLI 只依赖 rofd-convert（converter hosts 不得越过 facade，spec §5）。
- 库内不取挂钟（AGENTS §4.4）：krilla `SerializeSettings::default()` 不设日期（现状，保证字节确定）；bench/soak 用单调 `Instant`。
- 代码注释与 commit message 一律英文；中文仅限 docs/ 下设计文档。
- 提交走 conventional commits（feat/fix/test/docs/chore）。
- 验证命令基线：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`。
- 预算默认值（spec §6.1）：`DEFAULT_MAX_PAGES = 10_000`、`DEFAULT_MAX_MEDIA_BYTES = 1 << 30`（镜像 rword）。

## Review Focus

1. **敌意/损坏字节**（非 zip、截断 XML）→ `ConvertError::Decode`，绝不 panic → Task 5 `corrupt_bytes_is_decode_error` + Task 10 fuzz 靶。
2. **零页文档** → `pdf_from_page_scenes` 的 `EmptyPages` 映射为 `ConvertError::Encode`，不 panic → Task 5 `zero_page_doc_is_encode_error`。
3. **页数/媒体炸弹** → `BudgetExceeded` 快速失败（`what:"pages"` / `what:"media_bytes"`）→ Task 5 `pages_budget_exceeded`、`media_budget_exceeded`。
4. **内容三开关各自真实生效**（水印/批注/图章独立开关产出不同字节）→ Task 5 `content_flags_independently_change_output`（fixture 含 Highlight+Stamp+Watermark 三种批注）。
5. **长期实例跨调用无状态泄漏**（先转 A 再转 B 再转 A，两次 A 字节一致）→ Task 7 `sequence_is_history_independent`；`--stdout` 二进制安全（无多余换行）→ Task 8 `stdout_is_binary_safe`。

---

### Task 1: D17 — PrintContent 默认全开（component + 三宿主波及）

**Files:**
- Modify: `crates/component/src/print_session.rs:27-35`（Default impl）及同文件测试 `:436`（`print_content_default_is_clean_copy_with_seal`）
- Modify: `crates/web-app/src/App.vue:593`（printPanel 初始态）、`crates/web-app/src/App.vue:847`（exportPdf 实参）
- Modify: `crates/web-app/src/print/printDialogState.ts:150`（tauri 打印对话框初始 `watermark: false`）
- Modify: `crates/xilem-app/tests/export_pdf.rs:120`（`default_export_is_clean_copy_preset`）

**Interfaces:**
- Produces: `PrintContent::default() == { watermark: true, annotations: true, seal: true }`（serde 线型不变；Task 5/6 依赖此默认）。

- [ ] **Step 1: 写失败测试（component）**

改 `print_content_default_is_clean_copy_with_seal` 为全开断言并更名 `print_content_default_is_all_layers_on`：

```rust
#[test]
fn print_content_default_is_all_layers_on() {
    let c = PrintContent::default();
    assert!(c.watermark && c.annotations && c.seal, "D17: all layers on by default");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-component print_content_default`
Expected: FAIL（watermark/annotations 仍为 false）

- [ ] **Step 3: 翻转 Default impl**（`watermark: false→true`、`annotations: false→true`；`seal` 保持 true）

- [ ] **Step 4: 运行 component 测试通过**

Run: `cargo test -p rofd-component`
Expected: PASS（同文件其余用 `watermark: false/true` 的测试都是显式构造，不受影响——若个别断言默认值则随改）

- [ ] **Step 5: 宿主波及翻转**

`App.vue:593` → `reactive({ open: false, busy: false, watermark: true, annotations: true, seal: true })`；`App.vue:847` → `buildPrintOptions(true, true, true)`；`printDialogState.ts:150` 起的初始 content 三值全 true（读上下文对齐 annotations/seal 两邻位）；`xilem-app/tests/export_pdf.rs:120` 测试改为断言全开预设并更名 `default_export_is_all_layers_preset`。

- [ ] **Step 6: 全量验证**

Run: `cargo test -p xilem-app -p gpui-app` 与 `cd crates/web-app && npx vitest run src/print`
Expected: 全 PASS（`panelOptions.test.ts` 测的是显式实参，无需改）

- [ ] **Step 7: Commit** `feat: flip PrintContent default to all layers on (convert spec D17)`

---

### Task 2: PrintSession::headless 公开构造（component）

**Files:**
- Modify: `crates/component/src/print_session.rs`（`PrintSession::new` 旁新增 pub 构造）
- Test: 同文件 `#[cfg(test)]`（镜像现有 print_session 测试的 doc fixture）

**Interfaces:**
- Produces: `pub fn PrintSession::headless(doc: OfdDocument, fonts: rofd_render::FontStore, content: PrintContent) -> PrintSession`——语义：`Self::new(doc, fonts, content, Arc::new(AtomicU64::new(0)))`；`is_stale()` 恒 false。Task 5 消费。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn headless_session_renders_pages_and_never_stale() {
    let fonts = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    let session = PrintSession::headless(two_page_doc(), fonts, PrintContent::default());
    assert_eq!(session.page_count(), 2);
    let _ = session.page_scene(0);               // 构建不 panic
    assert!(!session.is_stale(), "no component can bump the generation");
}
```

（`two_page_doc` 若无现成 helper，镜像同文件既有测试的双页 fixture 构造。）

- [ ] **Step 2: 运行确认失败**（`headless` 不存在，编译错）
Run: `cargo test -p rofd-component headless_session`
- [ ] **Step 3: 实现 headless 构造**（doc 注释写明：headless 无并发变异源，print_gen 恒 0）
- [ ] **Step 4: 运行通过** Run: `cargo test -p rofd-component`
- [ ] **Step 5: Commit** `feat(component): pub headless PrintSession constructor for the convert facade`

---

### Task 3: rofd-convert crate 骨架（类型面 + 错误模型 + FFI 纪律）

**Files:**
- Create: `crates/convert/Cargo.toml`（workspace 新成员；字段风格镜像 `crates/export/Cargo.toml`；deps：rofd-component/rofd-io/rofd-render/rofd-export/rofd-dom 全 path/workspace-deps）
- Modify: 根 `Cargo.toml:3` members 追加 `"crates/convert"`；`[workspace.dependencies]` 追加 `rofd-convert = { path = "crates/convert" }`
- Create: `crates/convert/src/lib.rs`
- Test: `crates/convert/tests/convert_api.rs`（本任务先放骨架测试）

**Interfaces:**
- Produces（Task 4/5/8 及 ②③④ 期依赖的稳定类型面）:

```rust
pub enum OutputFormat { Pdf }                                    // D8 扩展位
pub struct ConvertOptions {
    pub content: rofd_component::PrintContent,                   // 默认全开（Task 1 后 Default 即是）
    pub max_pages: Option<u32>,                                  // None = DEFAULT_MAX_PAGES
    pub max_media_bytes: Option<usize>,                          // None = DEFAULT_MAX_MEDIA_BYTES
}
pub struct ConvertOutput { pub bytes: Vec<u8>, pub warnings: Vec<rofd_dom::OfdWarning> }
pub const DEFAULT_MAX_PAGES: u32 = 10_000;
pub const DEFAULT_MAX_MEDIA_BYTES: usize = 1 << 30;
pub enum ConvertError { Decode(String), Encode(String),
    BudgetExceeded { what: &'static str, limit: u64, actual: u64 }, Internal(String) }  // 手写 Display
pub enum FontRegisterError { Io(std::io::Error), InvalidFont }   // 手写 Display
pub struct Converter { /* Task 4 填充快照字段；本任务占位 `_priv: ()` */ }
impl Converter { pub fn new() -> Converter }                     // 本任务空实现
pub fn catch_panics<T>(f: impl FnOnce() -> Result<T, ConvertError>) -> Result<T, ConvertError>
```

- [ ] **Step 1: 写失败测试**

`tests/convert_api.rs`：

```rust
use rofd_convert::{catch_panics, ConvertError, Converter};

#[test]
fn catch_panics_converts_to_internal() {                 // 镜像 rword 同名测试
    let r = catch_panics(|| Err::<(), _>(ConvertError::Decode("x".into())));
    assert!(matches!(r, Err(ConvertError::Decode(_))));
    let r = catch_panics(|| -> Result<(), ConvertError> { panic!("boom") });
    assert!(matches!(r, Err(ConvertError::Internal(m)) if m.contains("boom")));
}

#[test]
fn converter_is_send_today() {                           // D15 修订：钉住现状；温引擎引入时此测试随契约一起改
    fn assert_send<T: Send>() {}
    assert_send::<Converter>();
}
```

- [ ] **Step 2: 建 crate + 挂 workspace，运行确认失败**（crate 不存在）
Run: `cargo test -p rofd-convert`
- [ ] **Step 3: 实现 lib.rs 类型面**——`catch_panics` 体镜像 rword（`AssertUnwindSafe` + payload downcast `&str`/`String`）；`ConvertOptions::default()`（content 取 `PrintContent::default()`，预算 None）；doc 注释写明线程契约（Send 现状 + L2 绑线程建议）与"converter hosts 只 import 本 crate"。
- [ ] **Step 4: 运行通过** Run: `cargo test -p rofd-convert`
- [ ] **Step 5: Commit** `feat(convert): rofd-convert crate skeleton - error model, options, panic boundary`

---

### Task 4: 进程级字体注册表（D9/D19）

**Files:**
- Create: `crates/convert/src/fonts.rs`（`pub(crate) fn registry_snapshot()` 供 Converter）
- Modify: `crates/convert/src/lib.rs`（re-export 四个 pub fn）
- Modify: `crates/render/src/text/font.rs`（新增 `pub fn registered_family_names(&self) -> Vec<String>`——返回整字体注册成功的族名集合，spec §5 render 唯一增量）
- Test: `crates/convert/tests/convert_api.rs` 追加

**Interfaces:**
- Produces:

```rust
pub fn register_font(bytes: Vec<u8>) -> Result<(), FontRegisterError>;            // 指纹去重：重复字节幂等 Ok
pub fn register_font_file(path: impl AsRef<Path>) -> Result<(), FontRegisterError>; // Io 变体透传读失败
pub fn register_default_font(bytes: Vec<u8>) -> Result<(), FontRegisterError>;    // D19：后注册者生效
pub fn registered_families() -> Vec<String>;
pub fn has_default_font() -> bool;                                                // CLI 反馈用
pub(crate) struct RegistrySnapshot { pub fonts: Vec<Arc<Vec<u8>>>, pub default_font: Option<Arc<Vec<u8>>> }
pub(crate) fn registry_snapshot() -> RegistrySnapshot;                             // Task 5 消费
```

- [ ] **Step 1: 写失败测试**

```rust
const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

#[test]
fn register_font_records_family_and_is_idempotent() {
    rofd_convert::register_font(FONT.to_vec()).unwrap();
    rofd_convert::register_font(FONT.to_vec()).unwrap();          // 指纹去重，不二次注册
    let fams = rofd_convert::registered_families();
    assert!(!fams.is_empty(), "probe must surface the test font's family");
    assert_eq!(fams.len(), fams.iter().collect::<std::collections::HashSet<_>>().len());
}

#[test]
fn invalid_font_bytes_rejected() {
    assert!(matches!(rofd_convert::register_font(vec![0u8; 16]), Err(rofd_convert::FontRegisterError::InvalidFont)));
}

#[test]
fn default_font_flag_roundtrip() {
    assert!(!rofd_convert::has_default_font());
    rofd_convert::register_default_font(FONT.to_vec()).unwrap();
    assert!(rofd_convert::has_default_font());
}
```

- [ ] **Step 2: 运行确认失败**
- [ ] **Step 3: 实现**——存储 `static OnceLock<Mutex<Registry>>`；`Registry { fonts: Vec<Arc<Vec<u8>>>, families: Vec<String>, default_font: Option<Arc<Vec<u8>>>, fingerprints: HashSet<(usize, u64)> }`；指纹 = `(bytes.len(), DefaultHasher)`（镜像 rword fonts.rs，防重复面扰动 fallback 选取）；验证探针：临时 `FontStore::from_resources(&Resources::default(), Arc::new(vec![]))` + `register_font(Arc::new(bytes))`，false → `InvalidFont`，成功则经 `registered_family_names()` 记族名。render 侧新增 `registered_family_names`（只读，收集 `families` 值 + default 族名）。
- [ ] **Step 4: 运行通过** Run: `cargo test -p rofd-convert` 与 `cargo test -p rofd-render`
- [ ] **Step 5: Commit** `feat(convert): process-level font registry with fingerprint dedup and default-font anchor`

---

### Task 5: Converter::convert 主管线（预算 + warnings + Review Focus 1-4）

**Files:**
- Modify: `crates/convert/src/lib.rs`（Converter 实字段 + convert + `pub fn media_bytes`）
- Test: `crates/convert/tests/common/mod.rs`（fixture helpers，Task 6 共享）+ `crates/convert/tests/convert_api.rs` 追加

**Interfaces:**
- Consumes: Task 2 `PrintSession::headless`；Task 4 `registry_snapshot()`；既有 `rofd_io::parse_ofd`、`rofd_render::FontStore`、`rofd_export::{pdf_from_page_scenes, PdfPage}`。
- Produces: `pub fn convert(&mut self, format: OutputFormat, ofd: &[u8], opts: &ConvertOptions) -> Result<ConvertOutput, ConvertError>`

- [ ] **Step 1: 写失败测试**（fixture 依赖 rofd-component/rofd-io/rofd-dom，均为本 crate 已有依赖；无字体环境依赖——页面无文本对象时场景不需要字体。helpers 一律放 `tests/common/mod.rs`，Task 6 复用）

fixture helpers：

```rust
fn page(id: &str) -> rofd_dom::Page {   // 210x297mm 单层空页（镜像 component 测试构造）
    rofd_dom::Page { id: rofd_dom::PageId::new(id),
        physical_box: rofd_dom::Rect { x: 0.0, y: 0.0, w: 210.0, h: 297.0 },
        layers: vec![rofd_dom::Layer::default()], template: None }
}
fn two_page_doc() -> rofd_dom::OfdDocument { /* pages: [page("P0"), page("P1")] */ }
fn fixture_bytes() -> Vec<u8> { rofd_io::write_ofd(&two_page_doc()).unwrap() }
fn annotated_fixture() -> Vec<u8> {   // Highlight + Stamp + Watermark 各一条（三开关各有靶子）
    let mut c = rofd_component::OfdComponent::new(rofd_component::OfdConfig::new(std::sync::Arc::new(vec![])));
    c.load_document(two_page_doc());
    c.set_clock("t".into(), 1);
    c.create_annotation(rofd_dom::AnnotationKind::Highlight, rofd_dom::PageId::new("P0"),
        rofd_dom::AnnotationPayload::Markup { quad_points: vec![rofd_dom::Point{x:10.0,y:10.0}, rofd_dom::Point{x:100.0,y:100.0}], color: rofd_dom::Color::Rgb(255,255,0) });
    c.create_annotation(rofd_dom::AnnotationKind::Stamp,  rofd_dom::PageId::new("P0"), /* payload 镜像 component 测试的 Stamp fixture */);
    c.create_annotation(rofd_dom::AnnotationKind::Watermark, rofd_dom::PageId::new("P1"), /* 同上 */);
    rofd_io::write_ofd(c.document()).unwrap()
}
```

（Stamp/Watermark 的 payload 形状读 `crates/component/src/print_session.rs` 既有测试与 `crates/dom/src/annotation.rs` 的 `AnnotationPayload` 定义后补全。）

```rust
#[test] fn converts_to_pdf_bytes() {          // %PDF 魔数 + 两页
    let out = Converter::new().convert(OutputFormat::Pdf, &fixture_bytes(), &ConvertOptions::default()).unwrap();
    assert!(out.bytes.starts_with(b"%PDF")); assert!(!out.bytes.is_empty());
}
#[test] fn corrupt_bytes_is_decode_error() { assert!(matches!(convert(b"not a zip"), Err(Decode(_)))); }
#[test] fn zero_page_doc_is_encode_error() { /* write_ofd(空 doc) → Err(Encode(_)) */ }
#[test] fn pages_budget_exceeded() { /* 3 页 doc + max_pages Some(2) → Err(BudgetExceeded{what:"pages", limit:2, actual:3})（端到端：write_ofd 序列化页数） */ }
#[test] fn media_bytes_sums_images_and_font_data() {      // 媒体预算走独立检查函数（write_ofd 不落资源字节，端到端 fixture 造不出——镜像 rword check_media_budget 的独立可测姿态）
    let mut doc = two_page_doc();
    doc.resources.images.insert(rofd_dom::ImageId::new("I1"), std::sync::Arc::new(vec![0u8; 4096]));
    doc.resources.font_data.insert(rofd_dom::FontId::new("F1"), std::sync::Arc::new(vec![0u8; 512]));
    assert_eq!(rofd_convert::media_bytes(&doc), 4096 + 512);
}
#[test] fn content_flags_independently_change_output() {   // Review Focus 4
    let bytes = annotated_fixture();
    let base = convert_all_on(&bytes);
    let no_wm = convert_with(&bytes, PrintContent { watermark: false, annotations: true, seal: true });
    let no_ann = ...; let no_seal = ...;
    let all = [base, no_wm, no_ann, no_seal];  // 两两不同
}
#[test] fn warnings_passthrough_equals_parse_report() {
    let (a, b) = (convert(&fixture_bytes()).warnings, rofd_io::parse_ofd(&fixture_bytes()).unwrap().warnings);
    assert_eq!(a, b);
}
```

- [ ] **Step 2: 运行确认失败**
- [ ] **Step 3: 实现 convert**——顺序：`parse_ofd` → `Decode` 映射；页数预算（`doc.pages.len()`）；媒体预算（`pub fn media_bytes(doc) -> usize` = `resources.images` 与 `resources.font_data` 值字节和，独立可测，镜像 rword `check_media_budget` 的 pub 姿态）；`FontStore::from_resources(&doc.resources, snapshot.default_font.unwrap_or(Arc::new(vec![])))` + 逐个 `register_font(Arc)`（注册表快照）；`PrintSession::headless(doc.clone(), fonts, opts.content)`；`0..page_count` 收集 `PdfPage { width_px: w as f32, height_px: h as f32, scene: session.page_scene(i) }`；`pdf_from_page_scenes` → `Encode` 映射；返回 `ConvertOutput { bytes, warnings: report.warnings }`。`OutputFormat::Pdf` 穷举匹配。
- [ ] **Step 4: 运行通过** Run: `cargo test -p rofd-convert`
- [ ] **Step 5: Commit** `feat(convert): one-call OFD-to-PDF conversion with budgets and warning passthrough`

---

### Task 6: parity 字节验收（D18）

**Files:**
- Test: `crates/convert/tests/parity.rs`

**Interfaces:**
- Consumes: Task 5 `Converter::convert`；`rofd_component::{OfdComponent, OfdConfig, PrintContent, begin_print 路径}`；`rofd_export::pdf_from_page_scenes`。

- [ ] **Step 1: 写测试**（两组内容开关各断言一次；fixture 复用 Task 5 的 annotated_fixture——helper 提到 `tests/common/mod.rs` 共享）

```rust
fn editor_path_pdf(bytes: &[u8], content: PrintContent) -> Vec<u8> {
    let doc = rofd_io::parse_ofd(bytes).unwrap().document;
    let mut c = OfdComponent::new(OfdConfig::new(std::sync::Arc::new(vec![])));
    c.load_document(doc);
    let mut s = c.begin_print(content);
    let mut pages = Vec::new();
    for i in 0..s.page_count() {
        let (w, h) = s.page_rect(i);
        pages.push(rofd_export::PdfPage { width_px: w as f32, height_px: h as f32, scene: s.page_scene(i) });
    }
    pdf_from_page_scenes(&pages).unwrap()
}

#[test]
fn convert_bytes_equal_editor_export_all_on() {
    let bytes = common::annotated_fixture();
    let conv = Converter::new().convert(OutputFormat::Pdf, &bytes,
        &ConvertOptions { content: PrintContent { watermark: true, annotations: true, seal: true }, ..Default::default() }).unwrap();
    assert_eq!(conv.bytes, editor_path_pdf(&bytes, PrintContent { watermark: true, annotations: true, seal: true }));
}
// 同型第二条：全关 { false, false, false } 两边一致
```

- [ ] **Step 2: 运行** Run: `cargo test -p rofd-convert --test parity`
Expected: PASS（两侧同 FontStore 输入、同 session 组装、同编码器；若失败即管线有旁路状态，修 convert 而非测试）
- [ ] **Step 3: Commit** `test(convert): byte-level parity with the editor export path (spec D18)`

---

### Task 7: 并发与历史无关字节稳定（spec §8）

**Files:**
- Test: `crates/convert/tests/concurrency.rs`（镜像 rword `concurrency.rs`）

- [ ] **Step 1: 写测试**

```rust
#[test]
fn eight_threads_byte_stable() {          // 每线程各自 Converter::new()，输出 == 主线程基线，逐线程 assert_eq
}
#[test]
fn sequence_is_history_independent() {    // Review Focus 5：A,B,A → 两次 A 字节相等（B 用不同 fixture）
}
```

- [ ] **Step 2: 运行** Run: `cargo test -p rofd-convert --test concurrency`（release 下再跑一次：`cargo test -p rofd-convert --release --test concurrency`）
Expected: PASS
- [ ] **Step 3: Commit** `test(convert): 8-thread byte stability and history independence`

---

### Task 8: rofd-convert-cli（bin `rofd-convert`）

**Files:**
- Create: `crates/convert-cli/Cargo.toml`（`[[bin]] name = "rofd-convert"`；deps 仅 `rofd-convert` + 测试用 `rofd-io`/`rofd-component`/`rofd-dom` dev-deps）
- Modify: 根 `Cargo.toml` members 追加 `"crates/convert-cli"`
- Create: `crates/convert-cli/src/args.rs`、`crates/convert-cli/src/main.rs`、`crates/convert-cli/README.md`、`crates/convert-cli/README.zh-CN.md`

**Interfaces:**
- Consumes: Task 4 注册 API、Task 5 `Converter::convert`。
- Produces: `pub(crate) fn run(args: Args) -> i32`（main 只 `std::process::exit(run(args))`；测试不打进程）；`Args { input, output: OutputTarget::{File,Stdout}, fonts, fonts_dirs, default_font, no_watermark/no_annotations/no_seal, max_pages, max_media_bytes }`；退出码 0 成功 / 2 用法 / 3 转换失败；USAGE 常量（用户可见文案，按下稿逐字）：

```text
Usage: rofd-convert <input.ofd> <output.pdf|--stdout> [options]

Options:
  --font <file>          Register a font file into the process registry (repeatable)
  --fonts-dir <dir>      Register every file in the directory (repeatable)
  --default-font <file>  Set the default fallback font
  --no-watermark         Exclude the watermark layer (default: all layers on)
  --no-annotations       Exclude annotation layers (default: on)
  --no-seal              Exclude the seal layer (default: on)
  --max-pages <n>        Page budget (default 10000)
  --max-media-bytes <n>  Embedded-media byte budget (default 1073741824)
  -h, --help             Print usage
  -V, --version          Print version
```

- [ ] **Step 1: 写失败测试（args.rs 单元测试，镜像 rword args.rs 测试集）**

覆盖：双位置参解析、`--stdout`、缺 output 报错、flags 累积（`--font` 两次 + `--fonts-dir` + `--default-font`）、三个 no-* 旗标、`--max-pages`/`--max-media-bytes` 数值解析与非数拒收、未知旗标拒收、非 UTF-8 拒收、`-h`/`-V` 拦截。

- [ ] **Step 2: 运行确认失败**（模块不存在）
- [ ] **Step 3: 实现 args.rs**（零依赖手写解析，结构镜像 rword `crates/convert-cli/src/args.rs`）
- [ ] **Step 4: args 测试通过** Run: `cargo test -p rofd-convert-cli`
- [ ] **Step 5: 写失败测试（main 集成）**

```rust
#[test] fn run_converts_fixture_to_file() {   // write_ofd fixture → tempdir out.pdf；run()==0；文件以 %PDF 开头 }
#[test] fn run_convert_failure_exits_3() {    // 坏字节输入文件 → 3 }
#[test] fn run_usage_error_exits_2() {        // 缺 output → 2 }
#[test] fn stdout_is_binary_safe() {          // main.rs 抽出 pub(crate) fn write_output(target: &OutputTarget, bytes: &[u8]) -> io::Result<()>；
                                             // 对 Stdout 分支断言写入字节 == 输入字节（无追加换行），File 分支写 tempdir 后回读相等 }
```

- [ ] **Step 6: 实现 main.rs**——`register_fonts`（逐 `--font`/`--fonts-dir`/`--default-font`：读失败与 InvalidFont 仅 eprintln warning 后继续，镜像 rword CLI 姿态）；convert 经 `catch_panics`；warnings 逐行 stderr；写文件走直接 `fs::write`（CLI 无原子写需求）；`--stdout` 用 `io::stdout().lock().write_all`。
- [ ] **Step 7: 集成测试通过 + 手测真实样例** Run: `cargo test -p rofd-convert-cli` 与 `cargo run -p rofd-convert-cli -- test/ru-yuan-ji-lu.ofd target/convert-probe.pdf`
Expected: 测试 PASS；probe 文件生成且为有效 PDF
- [ ] **Step 8: 双语 README**（结构镜像 rword convert-cli README：用法、旗标、退出码、字体注册说明——`--font`/`--fonts-dir` 注册、失败仅警告继续兜底渲染）
- [ ] **Step 9: Commit** `feat(convert-cli): rofd-convert bin - args, exit codes, font registration`

---

### Task 9: soak 骨架 + bench example

**Files:**
- Create: `crates/convert/tests/soak.rs`（`#![cfg(target_os = "linux")]`，`#[ignore]`）
- Create: `crates/convert/examples/convert_bench.rs`

- [ ] **Step 1: 写 soak 测试**——单 Converter 循环转换 2000 次合成文档（fixture 每 500 次换一变体防 JIT 型缓存侥幸），每 500 次读 `/proc/self/status` 的 `VmRSS`，首尾差 < 64 MiB 断言（spec §8：量级按 rofd 实测定，注释写明可调）。
- [ ] **Step 2: 写 bench example**——3 档合成文档（1/10/50 页 × 含批注），每档 N=20 次取总耗时，`Instant::elapsed` 计时，stdout 打表（不进测试，基准数字记入提交信息，镜像 rword convert_bench 姿态）。
- [ ] **Step 3: 验证可编译 + 本机快跑** Run: `cargo test -p rofd-convert --test soak -- --list` 与 `cargo run -p rofd-convert --example convert_bench --release`
Expected: 列出忽略项；bench 打表完成
- [ ] **Step 4: Commit** `test(convert): RSS soak skeleton and 3-scenario bench example`

---

### Task 10: fuzz 靶 convert_ofd

**Files:**
- Create: `crates/convert/fuzz/Cargo.toml`（**独立 `[workspace]`，不进主 workspace**；结构镜像 `D:/code/rword/crates/convert/fuzz/Cargo.toml`：libfuzzer-sys + path 依赖 rofd-convert）
- Create: `crates/convert/fuzz/fuzz_targets/convert_ofd.rs`
- Modify: 根 `.gitignore` 追加 `crates/convert/fuzz/target/`

- [ ] **Step 1: 写靶**——`fuzz_target!(|data: &[u8]| { let _ = rofd_convert::Converter::new().convert(rofd_convert::OutputFormat::Pdf, data, &rofd_convert::ConvertOptions::default()); });`（错误是数据、panic 是 bug——任何 panic 都会被 fuzzer 抓到）
- [ ] **Step 2: 验证构建** Run: `cd crates/convert/fuzz && cargo check`
Expected: 编译通过（运行需 nightly + cargo-fuzz，属手动验证：`cargo +nightly fuzz run convert_ofd -- -runs=20000`，Windows 需 MSVC ASan DLL 目录在 PATH，镜像 rword 注记）
- [ ] **Step 3: Commit** `test(convert): libFuzzer target convert_ofd`

---

### Task 11: AGENTS.md 同步 + 依赖许可审计

**Files:**
- Modify: `AGENTS.md`（§2 命令区、§3 布局表 + 分层图、常见陷阱表）
- Modify: `docs/superpowers/specs/2026-11-06-rofd-convert-design.md`（§9 追加审计结论）

- [ ] **Step 1: AGENTS.md 增量**——§2 追加：`cargo run -p rofd-convert-cli -- input.ofd output.pdf`、`cargo test -p rofd-convert --test soak -- --ignored`、`cd crates/convert/fuzz && cargo +nightly fuzz run convert_ofd -- -runs=20000`、`cargo run -p rofd-convert --example convert_bench --release`；§3 表加 `rofd-convert`（crates/convert，会话层 facade，依赖 component+io+render+export+dom）与 `rofd-convert-cli` 两行，分层图补 convert 上邻位（converter hosts 只碰 rofd-convert）；陷阱表加一行：❌ 绑定/CLI 直接 import rofd-io/rofd-export → ✅ 只碰 rofd-convert。
- [ ] **Step 2: 依赖许可审计**——确认 `rofd-convert`/`rofd-convert-cli` 直接外部依赖为零（全内部 path 依赖）；传递新增面（相对编辑器线）为空。把结论一句写入 spec §9（"① 期审计：无新增外部依赖，传递栈与编辑器线一致，全宽松许可"）。
- [ ] **Step 3: 全量绿**

Run: `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
Expected: 全 PASS

- [ ] **Step 4: Commit** `docs: AGENTS.md convert entries + phase-1 license audit note`
