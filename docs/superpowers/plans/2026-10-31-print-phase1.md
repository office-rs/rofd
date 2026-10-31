# rofd 打印模块一期实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地打印 spec 一期：rofd-export PDF 编码 crate + component PrintSession 内核 + web `exportPdf`/`planPages` SDK + tauri `print_pdf` IPC 与完整打印对话框 + xilem-app 另存 PDF。

**Architecture:** 页场景唯一来源是 component 的 `PrintSession`（owned，内容三开关过滤批注 kind）；新 crate `rofd-export` 只做 imaging Scene → krilla PDF 编码（从 rword `rword-export/src/pdf.rs` 移植）；提交底座复用外部库 office-rs-print（rev c2f99d5）；跨进程永远 PDF 字节。

**Tech Stack:** Rust（workspace 既有钉版 + office-rs-print git rev c2f99d5 + krilla 0.8 + swash 0.2）、TypeScript/Vue3（SDK vitest + web-app ant-design-vue + Playwright）。

**Spec:** [`docs/superpowers/specs/2026-10-31-print-design.md`](../specs/2026-10-31-print-design.md)（本计划从 spec 立论，执行者两份都读）

## Global Constraints

- 依赖钉版：`office-rs-print = { git = "https://github.com/office-rs/print.git", rev = "c2f99d5" }`、`krilla = { version = "0.8", features = ["pdf"] }`、`swash = "0.2"`、`base64 = "0.22"`、imaging 保持 `0.0.1`（spec §4）。
- rofd-export 依赖面**只有** imaging + krilla + swash + image；**不依赖任何 rofd crate**（spec §3）。
- component 新增依赖只有 office-rs-print（wasm 类型层安全，backend `cfg(not(wasm32))` 门控）；krilla 绝不进 component（spec §1）。
- 库内不取挂钟；PDF 不写日期（byte-deterministic，spec §2.7）。
- 打印页场景绝不含 caret/选区/handle/preedit/滚动条/tooltip（spec §2.3）。
- 内容默认值：watermark=false, annotations=false, seal=true（spec §0.2）。
- `PRINT_ZOOM = PX_PER_MM`；PDF px→pt ×0.75（spec §2.6）。
- commit message 与代码注释一律英文；conventional commits。
- 每任务后跑：`cargo build --workspace --exclude tauri-app && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`（tauri 壳任务除外，其验证命令在任务内给出）。

## Review Focus

spec 未逐条点名、但真实使用中最可能咬人的五类输入——每条已钉到归属任务的测试：

1. **字体仍在途时导出**（懒加载切片未到 → PDF 固化 tofu）：SDK `exportPdf` 必须先 flush `fontWatch.pending()` —— Task 5 Step 1 测试。
2. **页选择参数畸形**（越界页号/空列表/重复页号）：`validate_pages` 剔除越界、去重升序、全无效报错 —— Task 3 Step 1 测试。
3. **PrintSettings serde wire 形状错位**（JS 侧枚举大小写/结构不对 → Rust 反序列化失败或全默认）：对话框产出的 JSON 必须与 `PrintSettings` serde 形状逐字段一致（枚举 `PascalCase`，unit 变体纯字符串）—— Task 8 Step 1 + Task 7 Step 3 双向钉死。
4. **iframe 打印时序坑**（onload 晚于 appendChild / print 后立即 revoke → 预览瞬间消失）：Task 5 Step 3 用 spy 断言赋值顺序与不清理。
5. **对话框打开期间文档被编辑**（预览/计划页与实际不符）：`is_stale` 在批注变更后必须翻真（print_gen bump 位置唯一），对话框确认时重新 begin —— Task 3 Step 5 + Task 8 Step 1。

---

### Task 1: rofd-export crate（Scene → krilla PDF 编码器移植）

**Files:**
- Modify: `Cargo.toml`（workspace `members` 加 `"crates/export"`；`[workspace.dependencies]` 加 `office-rs-print`、`krilla`、`swash`，值见 Global Constraints）
- Create: `crates/export/Cargo.toml`、`crates/export/src/lib.rs`、`crates/export/src/pdf.rs`、`crates/export/tests/page_scenes.rs`

**Interfaces:**
- Produces（后续任务依赖的精确签名）:
  ```rust
  // rofd_export
  pub struct PdfPage { pub width_px: f32, pub height_px: f32, pub scene: imaging::record::Scene }
  pub enum ExportError { EmptyPages, Encode(String) }
  pub fn pdf_from_page_scenes(pages: &[PdfPage]) -> Result<Vec<u8>, ExportError>;
  ```

- [ ] **Step 1: 写失败测试 `crates/export/tests/page_scenes.rs`**

```rust
use imaging::record::Scene;
use rofd_export::{pdf_from_page_scenes, ExportError, PdfPage};

fn blank_page(w: f32, h: f32) -> PdfPage {
    let mut scene = Scene::new();
    { let mut p = imaging::Painter::new(&mut scene);
      p.fill_rect(imaging::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64),
                  imaging::peniko::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)); }
    PdfPage { width_px: w, height_px: h, scene }
}
fn mediabox_count(b: &[u8]) -> usize { b.windows(9).filter(|w| w == b"/MediaBox").count() }

#[test]
fn empty_pages_is_error() {
    assert!(matches!(pdf_from_page_scenes(&[]), Err(ExportError::EmptyPages)));
}

#[test]
fn two_pages_yield_two_mediaboxes_and_pdf_header() {
    let out = pdf_from_page_scenes(&[blank_page(200.0, 100.0), blank_page(200.0, 100.0)]).unwrap();
    assert!(out.starts_with(b"%PDF-"));
    assert_eq!(mediabox_count(&out), 2);
}

#[test]
fn encode_is_deterministic() { /* 同输入两次编码字节相等 */ }
```

再加字形嵌入测试（fixture 用 `crates/render/tests/fixtures/fonts/TestFont.ttf`，以 `include_bytes!` 相对路径 `../../render/tests/fixtures/fonts/TestFont.ttf` 引入）：

```rust
#[test]
fn glyph_run_embeds_fontfile_and_tounicode() {
    // painter.glyphs(&font_data, Color).font_size(32.0)
    //   .draw(&Style::Fill(imaging::peniko::Fill::NonZero), [record::Glyph { x: 0.0, y: 32.0, id: <TestFont 任一非 0 glyph id，如 'A' 的 id —— 用 swash 查或直接用 68（latin 'D'）> }])
    // 断言 bytes 含 b"/FontFile" 与 b"/ToUnicode"，且不含 "glyph" 告警路径
}
```

（`peniko::FontData { data: Blob::new(Arc::new(bytes)), index: 0 }`；imaging Painter glyphs API 见 `~/.cargo/registry/src/*/imaging-0.0.1/src/painter.rs:213`。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-export`
Expected: 编译失败（crate 不存在）

- [ ] **Step 3: 实现 —— 从 rword 移植 `pdf.rs`**

新建 crate（包名 `rofd-export`，edition 2021，deps：imaging/krilla/swash/image workspace）。`src/pdf.rs` **逐段移植 `D:/code/rword/crates/rword-export/src/pdf.rs`**，保留 `PdfSink` + `SinkContext` + `PdfPageSink`（`imaging::PaintSink` impl）+ 全部 helper（`split_color`/`affine_to_krilla`/`glyph_transform_for_surface`/`is_pure_x_shear`/`normalized`/`krilla_fill_rule`/`krilla_stroke`/`decode_image`/`font_key`/`image_key`）与常量 `PX_TO_PT=0.75`/`TOLERANCE=0.1`/`MAX_IMAGE_SIDE=30_000`，做且仅做这些适配：
  - 删 `ConvertSink` trait 与 `encode_pdf`/`pipeline` 依赖；公共入口 `pdf_from_page_scenes(pages: &[PdfPage]) -> Result<Vec<u8>, ExportError>`（内部即 rword `encode_pdf_from_page_scenes` 的形状：空列表 → `ExportError::EmptyPages`，逐页 `PageMeta{index:i, x:0.0, y:0.0, w, h}`）。
  - 错误类型 `ExportError { EmptyPages, Encode(String) }`（thiserror 可选，直接手写 Display）。
  - 无 metadata 入参（krilla `Document::new_with(SerializeSettings::default())` 不设日期，保 byte-determinism；rword 注释原样保留）。
  - `src/lib.rs` re-export 公共面；crate 注释注明移植来源与 spec 引用。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-export && cargo clippy -p rofd-export --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/export
git commit -m "feat: add rofd-export crate (Scene to krilla PDF encoder, ported from rword-export)"
```

---

### Task 2: PrintContent + 纸型映射 + PRINT_ZOOM（component 纯函数）

**Files:**
- Modify: `crates/component/Cargo.toml`（`office-rs-print = { workspace = true }`）、`crates/component/src/lib.rs`（`mod print_session; pub use print_session::{PrintContent, PRINT_ZOOM};`）
- Create: `crates/component/src/print_session.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct PrintContent { pub watermark: bool, pub annotations: bool, pub seal: bool } // Default: false,false,true; Clone,Copy,PartialEq,Eq,serde
  impl PrintContent { pub fn includes(&self, kind: &rofd_dom::AnnotationKind) -> bool }
  pub const PRINT_ZOOM: f64; // == rofd_render::PX_PER_MM
  pub(crate) fn paper_from_size_mm(w_mm: f64, h_mm: f64) -> office_rs_print::Paper;
  ```

- [ ] **Step 1: 写失败测试（`print_session.rs` 内 `mod tests`）**

```rust
#[test] fn print_content_default_is_clean_copy_with_seal() {
    let d = PrintContent::default();
    assert!(!d.watermark && !d.annotations && d.seal);
}
#[test] fn includes_maps_all_ten_kinds() {
    // Watermark→watermark；Stamp→seal；
    // Highlight/Underline/Strikeout/Squiggly/Freehand/Shape/Note/TextBox→annotations
    // 全 10 个 AnnotationKind 变体逐一断言（Shape(ShapeKind::Rect) 等）
}
#[test] fn paper_table_exact_and_normalized_and_closest() {
    assert_eq!(paper_from_size_mm(210.0, 297.0), Paper::A4);
    assert_eq!(paper_from_size_mm(297.0, 210.0), Paper::A4);   // 横放归一
    assert_eq!(paper_from_size_mm(215.9, 279.4), Paper::Letter); // ±1mm
    assert_eq!(paper_from_size_mm(300.0, 430.0), Paper::A3);   // 最近面积
}
#[test] fn print_zoom_is_px_per_mm() { assert_eq!(PRINT_ZOOM, rofd_render::PX_PER_MM); }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component print_session`
Expected: 编译失败（模块不存在）

- [ ] **Step 3: 实现**

`PrintContent` 派生 `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize` + 手写 `Default`；`includes` 按 spec §2.4 映射表 match；`paper_from_size_mm` 照抄 rword `print_session.rs:191-225`（表：A3/A4/A5/B4/B5/Letter/Legal，±1mm 命中，最近面积兜底）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component print_session`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "feat(component): PrintContent flags, paper size mapping, PRINT_ZOOM baseline"
```

---

### Task 3: PrintSession + print_gen 失效计数

**Files:**
- Modify: `crates/component/src/print_session.rs`（session 主体）、`crates/component/src/ofd_component.rs`、`crates/component/src/lib.rs`（导出 `PrintSession`）

**Interfaces:**
- Consumes: `rofd_render::page_body_scene(page, res, fonts, zoom)`、`rofd_render::annotation_scene::draw_annotations(painter, &[Annotation], res, fonts, origin, zoom)`（现签名，勿改）
- Produces:
  ```rust
  impl OfdComponent { pub fn begin_print(&self, content: PrintContent) -> PrintSession; }
  impl PrintSession {
      pub fn page_count(&self) -> usize;
      pub fn page_rect(&self, n: usize) -> (f64, f64);                       // physical_box * PRINT_ZOOM
      pub fn page_parameters(&self, n: usize) -> (Paper, Orientation);
      pub fn page_scene(&mut self, n: usize) -> Scene;                        // 惰性构建+缓存，返回 clone
      pub fn validate_pages(&self, pages: &[u32]) -> Result<Vec<usize>, String>; // 1-based：剔越界、去重升序；空→Err
      pub fn plan(&self, settings: &PrintSettings, current_page: usize) -> Result<Vec<usize>, String>;
      pub fn payload_for(&mut self, pages: &[usize]) -> PrintPayload;
      pub fn is_stale(&self) -> bool;
  }
  ```
  session 私有字段：`doc: OfdDocument`（整份 clone——resources 全 `Arc`，便宜）、`fonts: FontStore`（clone，Rc 内共享）、`content: PrintContent`、`print_gen: Arc<AtomicU64>`、`gen_at_begin: u64`、`page_cache: HashMap<usize, Scene>`。
  `OfdComponent` 新增字段 `print_gen: Arc<AtomicU64>`（`new()` 初始化）；**唯一 bump 点 = `notify_document_change()`**（`ofd_component.rs:2410`，`self.print_gen.fetch_add(1, Ordering::Relaxed)`）。

- [ ] **Step 1: 写失败测试（追加到 `print_session.rs` mod tests；fixture 仿 `component_with_markup` 的纯代码页 + 批注构造）**

```rust
// 页几何
#[test] fn page_count_and_rect_use_print_zoom() { /* 210x297mm 页 → (210.0*PRINT_ZOOM, 297.0*PRINT_ZOOM) */ }
// 内容过滤（结构断言，比对象数/类型，仿 tooltip 测试的 Command::Draw 过滤法）
#[test] fn annotations_off_yields_body_only_scene() {
    /* 同一组件：含 Highlight + Watermark + Stamp 批注；
       content={watermark:true,annotations:false,seal:false} 的 page_scene
       绘制对象数 == page_body_scene 基线（批注全部剔除） */
}
#[test] fn seal_on_keeps_stamp_image_draw() {
    /* content 默认（seal=true）：Stamp 批注页的 scene 含 Draw::Fill 带 image brush（图章解码为位图填充），
       关掉 seal 后该 draw 消失 */
}
#[test] fn watermark_toggle_controls_watermark_glyphs() { /* 开→有 glyph run；关→无 */ }
// 页选择
#[test] fn validate_pages_dedupes_sorts_and_rejects_empty() {
    /* [3,1,1,99,2] 于 3 页文档 → Ok([1,2,3])；[9] → Err；[] → Err */
}
// plan 委托（office_rs_print 语法）
#[test] fn plan_delegates_range_and_parity() {
    /* 5 页：All/All→1..=5；All/Odd→[1,3,5]；Custom "1-2,5"→[1,2,5]；Current(5)→[5]；Custom "x"→Err */
}
// staleness
#[test] fn is_stale_flips_after_annotation_mutation() {
    /* begin_print → !is_stale()；editor.create_annotation(...)（或经 handle_event 打字）→ is_stale() */
}
// payload
#[test] fn payload_for_packs_planned_pages_only() {
    /* 3 页文档 payload_for(&[1,3]) → Pages 变体 len==2，paper 来自 physical_box，scene 非空 */
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component print_session`
Expected: FAIL（`begin_print` 等未定义）

- [ ] **Step 3: 实现**

`begin_print(&self, content)`：clone `editor.document()` + `font_store`（惰性建，同 `update_layers` 首段的 `if self.font_store.is_none()` 套路）+ clone `print_gen`。`page_scene(n)`：`page_body_scene(page, &doc.resources, &fonts, PRINT_ZOOM)` → 同一 Scene 上以 `origin=(0.0,0.0)` 调 `draw_annotations(painter, &filtered, &doc.resources, &fonts, (0.0,0.0), PRINT_ZOOM)`，`filtered = doc.annotations.for_page(&page.id).iter().filter(|a| content.includes(&a.kind)).cloned().collect::<Vec<_>>()`；存 cache 后返回 clone。`page_parameters`：`paper_from_size_mm(physical_box.w, physical_box.h)` + 宽>高→Landscape。`plan`/`payload_for` 照 rword `print_session.rs:115-179`（payload 的 `PageScene{paper, orientation, width_px: f32, height_px: f32, scene: SceneWire}`）。`validate_pages`：过滤 `1..=page_count`、去重、升序；空 `Err("no printable pages")`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component`
Expected: PASS（含既有测试无回归）

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "feat(component): PrintSession kernel (begin_print, page scenes, plan, payload, staleness)"
```

---

### Task 4: web-view wasm 方法 `exportPdf` / `planPrintPages`

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`、`crates/web-view/Cargo.toml`（`rofd-export = { path = "../export" }`——注意 web-view 非 workspace-dep 表内 crate 用 path 直连即可，与 rofd-component 等写法一致用 workspace：先在根 `Cargo.toml` `[workspace.dependencies]` 加 `rofd-export = { path = "crates/export" }`）

**Interfaces:**
- Consumes: Task 1 `pdf_from_page_scenes`、Task 3 `begin_print/validate_pages/page_scene/page_rect`
- Produces（wasm-bindgen，web-view 无原生测试、逻辑已被 Task 3 覆盖，此任务为薄壳，由 Task 9 e2e 端到端验证）:
  ```rust
  #[wasm_bindgen(js_name = "exportPdf")]
  pub fn export_pdf(&mut self, watermark: Option<bool>, annotations: Option<bool>,
                    seal: Option<bool>, pages: Option<Vec<u32>>) -> Result<Vec<u8>, JsValue>;
  #[wasm_bindgen(js_name = "planPrintPages")]
  pub fn plan_print_pages(&self, range_kind: String, custom_spec: Option<String>,
                          parity: String, current_page: u32) -> Result<Vec<u32>, JsValue>;
  #[wasm_bindgen(js_name = "firstPagePaper")]
  pub fn first_page_paper(&self) -> Vec<String>;   // ["A4", "Portrait"] —— 对话框纸型/方向默认（spec §2.4 纸型权威）
  ```

- [ ] **Step 1: 实现两个方法**

`export_pdf`：三 bool 以 `PrintContent { watermark: w.unwrap_or(false), annotations: a.unwrap_or(false), seal: s.unwrap_or(true) }` 合成（默认值与 spec §0.2 一致）；`begin_print` → 页集合 = `pages.map(|p| session.validate_pages(&p)).transpose()?.unwrap_or(1..=n)` → 逐页 `PdfPage { width_px: page_rect.0 as f32, height_px: .., scene: page_scene(i) }` → `pdf_from_page_scenes` → `Vec<u8>`；错误转 `JsValue::from_str`。
`plan_print_pages`：`range_kind`/`parity` 字符串 match（`"All"|"Current"|"Custom"` / `"All"|"Odd"|"Even"`，未识别 → Err）；`Custom` 缺 spec → Err；组装 `PrintSettings{ range, parity, ..Default::default() }` → `session.plan(&s, current_page as usize)` → `Vec<u32>`。
`first_page_paper`：`begin_print(PrintContent::default()).page_parameters(0)` → `vec![format!("{:?}", paper), format!("{:?}", orientation)]`（`Paper`/`Orientation` 无 serde 字符串化，Debug 输出即 `"A4"`/`"Portrait"`——与 `PrintSettings` 的 PascalCase wire 同形，Task 8 直接当 wire 值用）。
`use rofd_export` 导入；wasm-bindgen 对 `Option<bool>`/`Option<Vec<u32>>` 参数原生支持，无需 JsValue 手解。

- [ ] **Step 2: 构建 wasm 验证编译**

Run: `cd crates/web-view && wasm-pack build --target web --out-dir sdk/dist 2>&1 | tail -3`
Expected: 构建 Success（wasm32 下 office-rs-print 类型层编译通过——backend 已 cfg 掉）

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock crates/web-view
git commit -m "feat(web-view): exportPdf and planPrintPages wasm methods"
```

---

### Task 5: SDK 面 —— exportPdf/planPages/print + 字体 flush 门 + iframe 打印

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`（`WasmOfd` interface 加两方法；`Ofd` 类加三方法）、`crates/web-view/sdk/src/fontFlow.ts`（`FontWatch` 增 `pending(): Promise<void>`）
- Create: `crates/web-view/sdk/src/print.ts`、`crates/web-view/sdk/src/__tests__/print.test.ts`

**Interfaces:**
- Consumes: Task 4 wasm 方法；`Ofd` 类既有 `fontWatch?: fontFlow.FontWatch`（index.ts:242）
- Produces:
  ```ts
  export interface PrintOptions { watermark?: boolean; annotations?: boolean; seal?: boolean; pages?: number[]; }
  // Ofd 类方法：
  exportPdf(options?: PrintOptions): Promise<Uint8Array>;
  planPages(range: { kind: 'All' | 'Current' | 'Custom'; spec?: string },
            parity: 'All' | 'Odd' | 'Even', currentPage: number): Promise<number[]>;
  print(options?: PrintOptions): Promise<void>;   // exportPdf → printViaIframe
  export function printViaIframe(pdf: Uint8Array): Promise<void>;  // print.ts
  export function toBase64(bytes: Uint8Array): string;             // print.ts（tauri 桥复用）
  // fontFlow：interface FontWatch { kick(): void; cancel(): void; pending(): Promise<void> }
  ```

- [ ] **Step 1: 写失败测试 `__tests__/print.test.ts`（vitest，mock wasm 仿 `ofd-wiring.test.ts` 的 makeOfd 套路）**

```ts
// ① flush 门：fontWatch.pending 未 resolve 前 wasm.exportPdf 不得被调；
//    resolve 后被调一次（用 deferred promise + spy 断言调用顺序）
it('exportPdf awaits pending font loads before calling wasm', async () => {...});
// ② 默认参数：exportPdf() 无 options → wasm.exportPdf(undefined, undefined, undefined, undefined)
it('exportPdf forwards options and defaults', async () => {...});
// ③ iframe 时序（happy-dom）：onload 赋值先于 appendChild（spy 顺序断言）；
//    print() 调用后 frame 仍在 document 中、URL 未 revoke；第二次调用才清理上一对
it('printViaIframe keeps frame alive after print and cleans previous pair', async () => {...});
// ④ toBase64 与 btoa 等价（含 >0x8000 分块路径，用 4096 长度随机字节比对）
it('toBase64 matches btoa on chunked input', () => {...});
// ⑤ pending()：无在途轮次立即 resolve
it('pending resolves immediately when idle', async () => {...});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npx vitest run src/__tests__/print.test.ts`
Expected: FAIL（模块/方法不存在）

- [ ] **Step 3: 实现**

`fontFlow.ts`：`scheduleFontWatch` 内部已按 debounce 轮次跑 fetch——给返回对象加 `pending(): Promise<void>`：记录「当前轮 promise」（kick 触发的 debounce+fetch 链），idle 时 `Promise.resolve()`；不改变既有 kick/cancel 语义。
`print.ts`：`printViaIframe` **逐行移植** rword `sessionManager.ts:250-307`（`lastPrintFrame`/`lastPrintUrl` 模块级持有、先清理上一对、onload 先赋值再 appendChild、`setTimeout(go, 700)` 兜底、print 后绝不 remove/revoke、`setTimeout(resolve, 100)`）；`toBase64` 移植 `sessionManager.ts:310-316`。
`index.ts`：`Ofd.exportPdf` = `await this.fontWatch?.pending()` → proxied `wasm.exportPdf(...)`（options 四字段展开传参）；`planPages` 直通；`print` = exportPdf + printViaIframe。导出 `PrintOptions` 类型。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd crates/web-view/sdk && npx vitest run`
Expected: PASS（全部既有 SDK 测试不回归）

- [ ] **Step 5: Commit**

```bash
git add crates/web-view/sdk/src
git commit -m "feat(sdk): exportPdf/planPages/print with font flush gate and iframe print"
```

---

### Task 6: web-app 打印入口（文件菜单 + 内容开关面板）

**Files:**
- Modify: `crates/web-app/src/App.vue`（文件下拉菜单加「打印…」与「导出 PDF」；打印面板）
- Create: `crates/web-app/src/print/panelOptions.ts`、`crates/web-app/src/print/panelOptions.test.ts`

**Interfaces:**
- Consumes: Task 5 `ofd.exportPdf/print`（实例经 App.vue 既有 ofd 句柄）
- Produces: `buildPrintOptions(watermark: boolean, annotations: boolean, seal: boolean): PrintOptions & { watermark: boolean; annotations: boolean; seal: boolean }`（面板三 checkbox → 完整 options；纯函数）

- [ ] **Step 1: 写失败测试 `panelOptions.test.ts`**

```ts
it('maps panel checkboxes to PrintOptions', () => {
  expect(buildPrintOptions(false, false, true)).toEqual({ watermark: false, annotations: false, seal: true });
  expect(buildPrintOptions(true, true, false)).toEqual({ watermark: true, annotations: true, seal: false });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-app && npx vitest run src/print`
Expected: FAIL

- [ ] **Step 3: 实现**

`panelOptions.ts` 纯函数；App.vue：文件菜单两项——「导出 PDF」直接 `ofd.exportPdf(buildPrintOptions(默认))` → 触发浏览器下载（Blob + a[download]）；「打印…」开 antd Modal（三 Checkbox，默认水印✗/批注✗/图章✔）→ 确认调 `ofd.print(buildPrintOptions(...))`。浏览器路径**不传 pages**（系统对话框管范围）。错误 catch 后 `console.error` + antd message 提示（spec §6 宿主 UI 行）。

- [ ] **Step 4: 验证**

Run: `cd crates/web-app && npx vitest run && npm run type-check`
Expected: PASS / 无类型错误

- [ ] **Step 5: Commit**

```bash
git add crates/web-app/src
git commit -m "feat(web-app): print entry with content-flag panel and PDF download"
```

---

### Task 7: tauri 壳打印 IPC（print.rs 移植）

**Files:**
- Modify: `crates/tauri-app/src-tauri/Cargo.toml`（`office-rs-print = { workspace = true }`、`base64 = "0.22"`）、`crates/tauri-app/src-tauri/src/main.rs`（`mod print;` + 注册四命令）
- Create: `crates/tauri-app/src-tauri/src/print.rs`

**Interfaces:**
- Consumes: office-rs-print native 面（`list_printers/print_pdf/JobHandle/JobState/join_all`）
- Produces（前端经 `__TAURI_INTERNALS__.invoke` 调用，参数名 camelCase 由 tauri 约定映射）:
  ```rust
  #[tauri::command] pub fn print_discover() -> Result<Vec<PrinterEntry>, String>;   // PrinterEntry{name,is_default,port} serde camelCase
  #[tauri::command] pub async fn print_pdf(app: AppHandle, pdf_b64: String,
      settings: office_rs_print::PrintSettings, title: String) -> Result<u32, String>;
  #[tauri::command] pub fn print_cancel(job_id: u32) -> Result<bool, String>;
  #[tauri::command] pub fn print_join_all();
  // 进度经 "print-progress" 事件（jobId/state/pagesDone/pagesTotal/error，200ms 轮询泵）
  ```

- [ ] **Step 1: 实现 `print.rs` —— 逐段移植 rword `print_ipc.rs` 全文（JOBS 表/进度泵/取消/join 语义原样）**

- [ ] **Step 2: 写 serde wire 测试（`print.rs` 内 `#[cfg(test)]`）**

```rust
#[test] fn print_settings_deserializes_dialog_wire_shape() {
    // 对话框（Task 8）产出的 JSON 形状逐字段钉死：枚举 PascalCase、unit 变体纯字符串
    let json = r#"{"printer":null,"copies":2,"collate":true,"duplex":"Off","color":"Color",
                   "paper":"A4","orientation":"Portrait",
                   "range":{"kind":"Custom","spec":"1-3,5"},"parity":"Odd"}"#;
    let s: office_rs_print::PrintSettings = serde_json::from_str(json).unwrap();
    assert_eq!(s.copies, 2);
    // range/parity 断言到具体变体
}
```
（若 `PrintSettings` 字段 wire 名与上不符——以 `D:/code/print/src/settings.rs` 的 serde 属性为准调整 JSON，测试钉的是「对话框 JSON 必须能反序列化成预期设置」这一事实。dev-dep 加 `serde_json`。）

- [ ] **Step 3: 跑测试 + 构建验证**

Run: `cargo test -p tauri-app && cargo build -p tauri-app`
Expected: PASS（Windows 本机）

- [ ] **Step 4: Commit**

```bash
git add crates/tauri-app/src-tauri
git commit -m "feat(tauri): print IPC bridge (discover/pdf/cancel/join with progress events)"
```

---

### Task 8: PrintDialog.vue 完整对话框 + 纯状态机

**Files:**
- Create: `crates/web-app/src/print/PrintDialog.vue`、`crates/web-app/src/print/printDialogState.ts`、`crates/web-app/src/print/printDialogServices.ts`、`crates/web-app/src/print/printDialogState.test.ts`
- Modify: `crates/web-app/src/App.vue`（tauri 环境（`'__TAURI_INTERNALS__' in window`）下「打印…」开本对话框替代浏览器面板；确认流程走 IPC）

**Interfaces:**
- Consumes: Task 5 `planPages/exportPdf/toBase64`、Task 7 四 IPC 命令
- Produces（状态机纯函数，形状抄 rword `print_dialog_state.rs` 的 advance 模式）:
  ```ts
  interface DialogState { printer: string | null; printers: PrinterEntry[]; copies: number; collate: boolean;
    duplex: 'Off'|'Duplex'|'Simplex'; color: 'Color'|'Mono'; paper: string; orientation: 'Portrait'|'Landscape'|'Auto';
    rangeKind: 'All'|'Current'|'Custom'; customSpec: string; parity: 'All'|'Odd'|'Even';
    watermark: boolean; annotations: boolean; seal: boolean;   // 默认 false/false/true
    status: 'idle'|'planning'|'error'|'submitting'; error: string | null; jobProgress: {done:number;total:number}|null; }
  type DialogEvent = {type:'printersLoaded',printers:PrinterEntry[]}|{type:'field',patch:Partial<DialogState>}
    |{type:'planResolved',pages:number[]}|{type:'planFailed',error:string}|{type:'submitting'}|{type:'progress',...}|{type:'done'}|{type:'reset'};
  function advance(s: DialogState, e: DialogEvent): DialogState;   // 纯函数
  function buildSettingsWire(s: DialogState): object;              // 产出与 Task 7 Step 2 JSON 逐字段一致的设置对象
  function clampCopies(n: number): number;                          // 1..=999
  ```

- [ ] **Step 1: 写失败测试 `printDialogState.test.ts`**

```ts
// ① wire 形状（Review Focus 3）：buildSettingsWire 输出与 Task 7 测试 JSON 逐字段一致
it('buildSettingsWire matches PrintSettings serde shape', () => {...});
// ② 范围语法经 planPages，错误进 status='error' 且保留原文
it('custom range errors surface and keep input', () => {...});
// ③ 份数钳制：0/1000/NaN→1/999/1
it('clamps copies', () => {...});
// ④ 默认内容开关 off/off/on；paper 默认取首页 page_parameters（services 注入 currentPagePaper）
it('content defaults are off/off/on', () => {...});
// ⑤ 提交流程：planResolved(pages) → submitting；确认时重新校验 staleness 不属于状态机（服务层），
//   状态机只保证 submit 用最新 plan
it('submit uses latest resolved plan', () => {...});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-app && npx vitest run src/print`
Expected: FAIL

- [ ] **Step 3: 实现**

`printDialogServices.ts`：tauri invoke 薄封装（`print_discover`/`print_pdf`/`print_cancel`/progress 事件订阅）+ `planPages` 注入 + `firstPagePaper` 注入（纸型/方向默认从文档读，spec §2.4 纸型权威——面板默认值读它，不是反过来）。`PrintDialog.vue`（antd：Modal + Select + InputNumber + Checkbox + Radio）：打开时 `print_discover` → 默认打印机 + `firstPagePaper` 设默认纸型/方向；预览区 = `exportPdf({pages:[当前页], ...content})` → blob URL `<iframe>` 展示；确认 = `planPages` → `exportPdf({pages,...})` → `toBase64` → `print_pdf(pdfB64, buildSettingsWire(state), title)` → 订阅 progress → 关闭/重试。

- [ ] **Step 4: 验证**

Run: `cd crates/web-app && npx vitest run && npm run type-check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/web-app/src crates/web-view
git commit -m "feat(web-app): full print dialog (pure state machine + tauri services)"
```

---

### Task 9: xilem-app 另存 PDF

**Files:**
- Modify: `crates/xilem-app/src/main.rs`（工具栏「导出 PDF」按钮 + 命令闭包 + 结果槽复用 `save_outcome` 模式）、`crates/xilem-app/Cargo.toml`（`rofd-export = { workspace = true }`）
- Create: `crates/xilem-app/src/host/export_pdf.rs`、`crates/xilem-app/tests/export_pdf.rs`

**Interfaces:**
- Consumes: Task 1 `pdf_from_page_scenes`、Task 3 `begin_print/PRINT_ZOOM`；host 模式仿 `host/document_io.rs`（`pub fn save_ofd` 的原子写）
- Produces: `pub fn export_pdf(component: &rofd_component::OfdComponent, path: &Path) -> Result<(), String>`（默认 `PrintContent::default()`，全部页，原子写）

- [ ] **Step 1: 写失败测试 `tests/export_pdf.rs`**

```rust
// 仿 c2_save.rs 的真实样例风格，但用纯构造文档即可（无 #[ignore]）：
// OfdComponent + 1 页 210x297 + 一条 Highlight 批注 → export_pdf(&c, &temp_path)
// 断言：文件存在、以 b"%PDF-" 开头、/MediaBox 计数 == 1
#[test] fn export_pdf_writes_one_page_pdf() { ... }
// 批注默认剔除：含 Highlight 的页与空页导出后 /MediaBox 都为 1，
// 且（可选加强）默认导出的内容 draw 数 == 手工 begin_print(all false except seal) 的页场景
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p xilem-app --test export_pdf`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现**

`host/export_pdf.rs`：`begin_print(PrintContent::default())` → 全页 `PdfPage` → `pdf_from_page_scenes` → 复用 `document_io` 的原子写落盘。`main.rs`：工具栏按钮 handler = `rfd::FileDialog::new().set_file_name("document.pdf").save_file()` → `push(app, move |c| { *outcome = Some(export_pdf(c, &path).map(|_| ())) })`，结果经 `save_outcome` 槽回传（复用 `consume_save_outcome` 的错误打印路径；导出成功不动 `modified`）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p xilem-app`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/xilem-app
git commit -m "feat(xilem-app): save-as-PDF toolbar action"
```

---

### Task 10: e2e 打印流 + CI libcups + 文档

**Files:**
- Create: `crates/web-app/e2e/print.spec.ts`
- Modify: `.github/workflows/rust.yml`（两处 apt install 各加 `libcups2-dev`——component 依赖 office-rs-print 后，linux native 链接必装，spec §4 硬约束）、`crates/web-app/README.md`（打印/导出一节：浏览器路径 iframe 三坑约束、tauri 路径 IPC 面）、`AGENTS.md`（§5 rofd-component 要点补 `begin_print`/`PrintSession` 一行 + §7 依赖钉版表加 office-rs-print/krilla 两行）

**Interfaces:**
- Consumes: Task 4/5 全部公共面；既有 `e2e/fonts.spec.ts` 的启动套路

- [ ] **Step 1: 写 Playwright spec `print.spec.ts`**

```ts
// 复用 fonts.spec.ts 的 app 启动方式（含 fonts stub 套路）
test('exportPdf returns valid PDF bytes', async ({ page }) => {
  // addInitScript stub window.print；到达编辑器后：
  // const bytes = await ofdInstance.exportPdf();
  // 断言 bytes 以 "%PDF-" 开头（经 page.evaluate 拿 Uint8Array 前缀）
});
test('planPages resolves range grammar', async ({ page }) => {
  // planPages({kind:'All'},'All',1) 长度 == 页数；{kind:'Custom',spec:'1'}→[1]
});
test('print mounts iframe and calls print', async ({ page }) => {
  // stub print 后调 ofd.print()：断言 document 里有 iframe[src^="blob:"]，
  // 且 window.print 被调（stub 记录）；再次调用后旧 iframe 被清理、新 iframe 存在
});
```

（ofd 实例句柄获取方式照 fonts.spec.ts 既有暴露路径；如无全局句柄，则经 SDK `Ofd.init` 返回值挂 `window.__ofd` 的既有测试钩子。）

- [ ] **Step 2: 跑 e2e**

Run: `cd crates/web-app && npm run build:sdk && npx playwright test e2e/print.spec.ts`
Expected: PASS

- [ ] **Step 3: CI 与文档修改**

rust.yml 两处 `sudo apt-get install -y` 行追加 `libcups2-dev`（build job 必需——component 经 office-rs-print 触发 linux native CUPS 链接，spec §4 硬约束；smoke job 只编 wasm 不链接 native，一并加上并留注释）。README 打印一节含**手测通道**（spec §7）：`npm run tauri dev` → 对话框选「Microsoft Print to PDF」打印机出 PDF 目检（中文/图片/图章/份数/范围/取消各一遍）；AGENTS 按上文补段。spec 状态行改「已实施（一期）」。

- [ ] **Step 4: 全量验证**

Run: `cargo build --workspace --exclude tauri-app && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cd crates/web-app && npm run type-check && npx vitest run`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add .github/crates/web-app docs
git commit -m "test: print e2e flow; ci: libcups2-dev for office-rs-print native link; docs"
```

---

## 二期接口预留（不在本计划内）

- `print_direct(component, printer, settings)`（xilem GDI 直打）消费 Task 3 的 `payload_for`，无需内核改动。
- tauri `print_pdf_to_file` 手测通道命令可随时按 rword `print_ipc.rs` 同款补一条。
