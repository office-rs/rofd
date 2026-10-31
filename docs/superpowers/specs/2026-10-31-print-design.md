# rofd 打印模块设计（打印 + PDF 导出）

日期：2026-10-31
状态：已实施（一期）
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）
参照：rword 打印全周期（rword `docs/superpowers/specs/2026-10-20-print-design.md` 含 §14 重构修订）、[`docs/draft/2026-10-27-rword-print-implementation-notes.md`](../../draft/2026-10-27-rword-print-implementation-notes.md)（rword 实施全记录与踩坑清单）
复用库：`office-rs/print`（`office-rs-print = { git = "https://github.com/office-rs/print.git", rev = "c2f99d5" }`，与 rword 同 pin）

## 0. 总览

### 0.1 问题与目标

rofd 目前没有任何打印/导出能力。本设计给三个宿主形态补齐打印路径：

| 宿主 | 一期能力 | 验收 |
|---|---|---|
| web（web-view + SDK） | `exportPdf(options)` | 返回 PDF 字节；宿主 iframe print 出浏览器打印对话框 |
| tauri 桌面壳 | `print_pdf` IPC + 完整打印对话框 | 选打印机/份数/范围/奇偶/逐份/内容开关后直接出纸（Windows WinRT 光栅 / Linux CUPS） |
| xilem-app（native） | 另存 PDF | rfd 保存对话框 + 落盘 PDF 字节 |
| xilem-app（native） | 原生直打 + 默认对话框 | **二期**（GDI 矢量回放，`print_pages`） |

底座全部复用 office-rs-print（设置钳制/纸型映射/页计划/collate/JobHandle/Windows+Linux 后端），rofd 只写内核 session、PDF 编码器薄层与宿主接线。

### 0.2 已敲定决策（owner 裁决）

1. **优先级**：exportPdf（web + tauri）先行；xilem 原生 GDI 直打二期。
2. **内容三开关是打印接口入参**：`watermark` / `annotations` / `seal` 三个独立 bool，控制是否进打印产物。
3. **默认值**：水印 ❌ 不打印、批注 ❌ 不打印、图章 ✔ 打印（净版优先；章是法律要件必须上纸）。
4. **签章 v1 语义 = `AnnotationKind::Stamp`（图片图章）**。OFD 真正的 SEP 电子签章（Signature.xml + 印章包外观）rofd 目前无解析无渲染（io 仅字节保留 `EntryKind::Signature`），其外观渲染是独立新功能，**不在本设计范围**（见 §8 非目标）。
5. **tauri 完整打印对话框**：打印机列表、份数、页码范围、奇偶、逐份、双面、彩色、内容三开关全套进 v1。
6. **打印逻辑下沉 component**（对齐 rword D12 的 PrintSession 形状），且比 rword 更统一：**web 导出也走 session**（rword 的 web exportPdf 绕过 session 直调 `docx_to_pdf_with_engine` 另起路径；rofd 的字体事实来源是 component 的 `font_store`——含 SDK 懒加载切片——导出必须过 component，因此 session 是页场景唯一来源）。

### 0.3 rofd 相对 rword 的差异事实（已核实）

- **板式文档，零布局差异**：OFD 页面预排版，不存在 rword 的 `LayoutMode::Print` 变体布局，`needs_print_layout` 守卫退化为无（打印与屏幕的布局差异恒为 0；唯一差异 = 批注 kind 过滤）。
- **页场景天然页局部**：`page_body_scene(page, res, fonts, zoom)` 产出页局部 Scene；`draw_annotations(painter, anns, res, fonts, origin, zoom)` 的 `origin` 是参数，传 `(0,0)` 即页局部。不需要 rword 的整场打印 Scene + clip/translate 切页。
- **水印/图章已是批注种类**：`AnnotationKind::Watermark` / `AnnotationKind::Stamp` 在 dom/io/render 三层全通（解析、编辑、序列化、绘制），内容开关 = kind 过滤，零前置工作。
- **office-rs-print 类型层 wasm 可编译**：`backend` 模块 `#[cfg(not(target_arch = "wasm32"))]` 门控（GDI/CUPS 仅 native），公共面只依赖 imaging + serde——component 依赖它不破坏平台边界（rofd web-view 的 wasm 构建不受影响）。
- **krilla 0.8 wasm 可用**：rword 的 wasm exportPdf 即在 wasm32 内跑 `Scene → krilla` 编码（`rword-export/src/pdf.rs`，convert-wasm 注释明证）。
- **dom 无批注 revision 计数器**：只有 `body_revision`（body 又不可变）。session 改持批注快照（见 §2.5）。

### 0.4 rofd 特有补充（超出 rword 的部分）

- `PrintContent` 内容开关类型（rword 无此语义）。
- session 页场景惰性构建（rword begin_print 立即构建整场；rofd 逐页按需 + session 内缓存，对话框预览一次只看一页）。
- 导出页选择 API：`exportPdf` 增 `pages` 参数 + `planPages` 辅助方法（§5.1；rword 的 tauri 流硬编码 `range: All`，此缝在 rword 未被测过，rofd 显式补上）。

## 1. 架构总图

```
接口层（薄）
├─ web-view: exportPdf(options) ─── wasm 方法，返回 PDF 字节
├─ tauri IPC: print_pdf(pdf_b64, settings, title) + list_printers/caps/job 查询
│    （跨进程载荷纪律：永远 PDF 字节，绝不矢量 Pages 过 IPC —— rword D 教训）
└─ xilem-app: 一期另存 PDF；二期 print_direct(component, printer, settings)

rofd 内核新增
├─ rofd-component: PrintSession（§2，打印逻辑下沉的落点）
│    begin_print(flags) → page_count/page_rect/page_parameters/page_scene/
│    plan/payload_for/is_stale
├─ rofd-export（新 crate，§3）: pdf_from_page_scenes（Scene 列表 → krilla PDF）
│    依赖仅 imaging + krilla；component 不依赖它（krilla 不进内核）
└─ 依赖新增：rofd-component → office-rs-print（类型层）

底座（外部库，零改动复用）：office-rs-print rev c2f99d5
  settings（钳制+纸型映射）/ page_plan（"1-3,5" 语法+collate）
  payload（SceneWire serde）/ job（取消+进度+JOBS 表）
  后端（native）：Windows GDI 矢量回放 + Windows WinRT-PDF 光栅 + Linux CUPS
```

依赖方向（沿既有 5 层单向纪律）：

```
web-view ─┬─► component ─► render ─► dom        （component 新增 office-rs-print）
          └─► rofd-export（imaging + krilla）    （新叶子 crate，同层于 imaging 消费者）
xilem-app ─┬─► xilem-view ─► component
           └─► rofd-export + io + rfd            （另存 PDF：host document_io 模式）
tauri 壳(src-tauri) ─► office-rs-print（native 后端；非 rofd crate，不违 §4.9）
tauri 前端 ─ 复用 web-app 源码（PrintDialog.vue 共享）
```

## 2. 内核：PrintSession（rofd-component）

新文件 `crates/component/src/print_session.rs`，公共面经 `lib.rs` 导出。上层（适配器/宿主）只经 `begin_print` 取 session，不直接碰内部状态。

### 2.1 内容开关

```rust
/// Print content flags (spec 2026-10-31 §2.1). Each layer is independent;
/// defaults are the "clean copy" preset (watermark/annotations off, seal on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrintContent {
    pub watermark: bool,    // AnnotationKind::Watermark
    pub annotations: bool,  // 其余全部批注 kind（见 §2.4 映射表）
    pub seal: bool,         // AnnotationKind::Stamp（v1 图片章；SEP 见 §8）
}

impl Default for PrintContent {
    fn default() -> Self { Self { watermark: false, annotations: false, seal: true } }
}
```

### 2.2 session 形状

```rust
impl OfdComponent {
    /// 开始打印会话：快照批注 + FontStore clone（内部 Rc 共享，clone 便宜）。
    /// 返回 **owned** session（无生命周期借用——rword D12 形状），
    /// 对话框可跨帧持有而不锁 component 的 &mut。
    pub fn begin_print(&self, content: PrintContent) -> PrintSession;
}

pub struct PrintSession { /* 私有：pages/resources 引用或 Arc、fonts、content、
                             页场景惰性缓存、批注快照、print_gen 快照 */ }

impl PrintSession {
    pub fn page_count(&self) -> usize;
    /// 页 (w, h) px（PRINT_ZOOM 基线，见 §2.6）
    pub fn page_rect(&self, n: usize) -> (f64, f64);
    /// 纸型权威：页 physical_box（mm）→ 最近 Paper + 方向（§2.4）
    pub fn page_parameters(&self, n: usize) -> (Paper, Orientation);
    /// 页局部 Scene（白底 body + 按 content 过滤后的批注），惰性构建 + session 内缓存
    pub fn page_scene(&mut self, n: usize) -> Scene;
    /// 页计划：委托 office-rs-print resolve_page_list（宿主永不重写范围语法）
    pub fn plan(&self, settings: &PrintSettings, current_page: usize) -> Result<Vec<usize>, String>;
    /// native 提交载荷：PrintPayload::Pages（份数/collate 恒为任务参数，载荷不翻倍 —— rword D10 纪律）
    pub fn payload_for(&mut self, pages: &[usize]) -> PrintPayload;
    /// 批注自 begin_print 后是否变化（见 §2.5）
    pub fn is_stale(&self) -> bool;
}
```

### 2.3 页场景组装（核心等式）

```
print_page_scene(n) = 白底（page_body_scene 自带）
                    + draw_body(page, res, fonts, (0,0), PRINT_ZOOM)
                    + draw_annotations(painter, 过滤后的该页批注, res, fonts, (0,0), PRINT_ZOOM)
```

- 复用 `rofd_render::page_body_scene`（body 部分）与 `draw_annotations`（origin=(0,0)）——与屏幕同一绘制路径，**打印即所见（WYSIWYG）**，无打印专属样式分支。
- **绝不进入打印产物**：caret、body 文本选区高亮、批注选框 handle、拖拽预览、preedit、滚动条、tooltip——这些全是 overlay/组件 UI 态，session 不经过 `overlay_scene`。
- 惰性构建：首次 `page_scene(n)` 时构建并存入 session 内 `HashMap<PageIndex, Scene>`；session 生命周期短（一次导出/一次对话框会话），不做代际失效。

### 2.4 kind → 开关映射与纸型权威

| 开关 | AnnotationKind | 说明 |
|---|---|---|
| `watermark` | `Watermark` | 水印批注 |
| `seal` | `Stamp` | 图片图章（v1 签章语义） |
| `annotations` | `Highlight` `Underline` `Strikeout` `Squiggly` `Freehand` `Shape` `Note` `TextBox` | 其余全部 |

纸型映射：`physical_box`（mm，OFD 原生单位，无 rword 的 twips 换算）→ 纸型表（A3/A4/A5/B4/B5/Letter/Legal，±1mm 精确命中否则最近面积，**映射表照抄 rword `paper_from_size_mm`**）；方向 = 宽>高 ? Landscape : Portrait。纸型权威是文档页盒子，面板默认值从它读，不是反过来（rword §7 纪律）。

### 2.5 快照、代计数与 staleness（builder check 裁决修订）

session 是 **owned**（rword D12 形状），不借 component：

- 内容自洽：`begin_print` 时 clone `AnnotationModel`（批注量小）+ FontStore（内部 Rc 共享，clone 便宜——`update_layers` 已有同一注释佐证）。导出期间用户继续编辑不影响已开始的产物。
- staleness 信号：component 持 `print_gen: Arc<AtomicU64>`，在**批注变更 choke point**（`modified` 翻真 / on_change 触发的同一处，即编辑事务提交后）bump；`begin_print` 快照该计数，`is_stale` 比较计数。计数器住在 component，**不动 dom**（rword 计数器亦在 component 侧）。
  - 早先「快照 PartialEq 对比」方案作废：脱离 component 的 owned session 看不见当前批注，无参 `is_stale` 无法实现；计数器是已验证形状。
- 一期 `is_stale` 为**测试可见、UI 休眠**（每次 exportPdf 都 fresh begin，宿主无跨帧持有场景）；二期 xilem 对话框跨帧持 session 时启用（on_change → 提示重新开始）。
- body 不可变，无需快照。

### 2.6 打印坐标基线

- `PRINT_ZOOM = PX_PER_MM`（96/25.4 ≈ 3.7795 px/mm，与屏幕基线同款 96dpi 约定）。
- PDF 编码在 rofd-export 内做 px→pt ×0.75（与 rword `pdf.rs` 逐字相同），mm→pt 换算精确（96/25.4 × 0.75 = 72/25.4）。
- 二期 GDI 矢量回放由 office-rs-print 后端自行处理坐标（64× 亚像素），session 不感知。

### 2.7 时间纪律

session/导出路径不取任何时钟（§4.4）。PDF 元数据（创建日期等）若需要时间戳，由**宿主**注入参数，库内不取 now。

## 3. rofd-export（新 crate）

```
crates/export/  （crate 名 rofd-export）
  依赖：imaging 0.0.1 + krilla 0.8（features=["pdf"]），无 rofd 内部依赖
```

- 唯一公共函数族：`pdf_from_page_scenes(pages: &[PdfPage]) -> Result<Vec<u8>, ExportError>`，`PdfPage { width_px, height_px, scene: Scene }`（页局部）。
- 实现移植 `rword-export/src/pdf.rs`：krilla 每页一个 page，per-page transform px→pt（×0.75），glyph run → krilla 文本（字体嵌入），fill/stroke/图片照抄。
- 错误显式分层（§4.6）：`ExportError::{EmptyPages, Encode(String)}`，wasm 侧桥接为 JsValue 错误串。
- 放独立 crate 而非塞进 web-view 的理由：xilem-app 另存 PDF 同样要它；component 不依赖它（krilla 编码器是产物转换，不是内核状态）。

## 4. 依赖钉版（动之前必读）

| crate | 源 | 版本/rev |
|---|---|---|
| `office-rs-print` | git `github.com/office-rs/print` | `rev = c2f99d5`（与 rword 同 pin；workspace 统一声明） |
| `krilla` | crates.io | `0.8`（features `["pdf"]`，与 rword 同） |
| `imaging` | crates.io | `0.0.1` 不变（office-rs-print 公共面同基） |

CI 注意（**硬约束**，非「如需要」）：office-rs-print 无 cargo feature 可关后端——ubuntu 上任何 native 链接构建（clippy `--all-targets`、测试二进制）**必须** `apt install libcups2-dev`（其 `#[link(name="cups")]` 曾在 CI 才暴露）；rofd workspace 内 component 仅用类型层，但同仓库 tauri 壳在矩阵内编译即触发。

## 5. 宿主接线

### 5.1 web（web-view + SDK + web-app）

- **wasm 方法**（`wasm_ofd.rs` 新增两个）：
  - `exportPdf(options?) -> Vec<u8>`。options = `{ watermark?: boolean, annotations?: boolean, seal?: boolean, pages?: number[] }`（内容三 bool 缺省用 `PrintContent::default()`；`pages` 缺省 = 全部页，否则为 **1-based 页号列表**，越界页静默剔除、空结果报错）。实现：`component.begin_print(content)` → 逐计划页 `page_scene` → `rofd_export::pdf_from_page_scenes` → 字节。
  - `planPrintPages(rangeSpec: string, parity: string, currentPage: number) -> number[] | error`。委托 office-rs-print `parse_page_spec` + `resolve_page_list`——**范围语法只在 Rust 侧解析一次**，JS 宿主永不重写（rword D 纪律；rword tauri 流硬编码 `range: All`，此缝未被测过，rofd 显式补上）。
- **字体就绪门（builder check 裁决）**：SDK `exportPdf` 在调 wasm 前 **flush 在途字体加载**（fontFlow 的 pending 补片 await 完；基建现成）；某族整体不可用则照常导出（tofu 如实反映所见，与屏幕一致）。wasm 方法本身保持同步 `Vec<u8>`，不引入异步桥。
- **SDK**（`sdk/src/`）：`ofd.exportPdf(options?) => Promise<Uint8Array>`；`ofd.planPages(range, parity, current) => Promise<number[]>`；另有宿主便捷 `print()`：exportPdf → blob URL → iframe print。iframe 三坑是**硬约束**（rword §5.3 真金白银换来的）：① onload 赋值先于 appendChild；② Chromium 对 blob-PDF 不再发 load——固定延时兜底；③ `print()` 后绝不立即 remove/revoke——源常驻到下一次打印。
- 内容三开关 + pages 是纯 bool/number wire（无枚举），避开 rword 踩的 serde 枚举 PascalCase 坑；但 §5.2 的 `PrintSettings` 过 IPC 时仍是枚举 wire，JS 侧按 `PascalCase`（`All/Custom/Odd`）处理并在边界校验坏 JSON。
- web-app 打印入口：打印按钮 + 打印前小面板（三个内容开关 checkbox + 确认；浏览器路径页范围交给系统对话框，不传 `pages`），随后 iframe print；浏览器系统对话框接管打印机/份数/范围。

### 5.2 tauri（src-tauri 壳 + 共享前端）

- **壳新增依赖**：office-rs-print（native 后端）。壳不依赖任何 rofd crate 的 §4.9 现状不变（office-rs-print 是外部库）。
- **IPC 面**：
  - `print_pdf(pdf_b64: String, settings: PrintSettingsWire, title: String) -> job_id`（Linux=CUPS 原生；Windows=WinRT 光栅+GDI）
  - `list_printers() -> Vec<PrinterInfo>`
  - `printer_capabilities(name) -> PrinterCaps`
  - `job_status(job_id) / cancel_job(job_id)`（JobHandle 快照；进度 UI 最小实现，形状留给 plan）
- **前端**：`PrintDialog.vue`（放 web-app/tauri-app 共享源码树）——完整对话框：打印机下拉（含能力感知的纸型/双面/彩色可选项）、份数、范围（`1-3,5` 语法，office-rs-print 校验）、奇偶、逐份、内容三开关、当前页/总页数显示。**状态机 = TS 纯函数模块**（`advance(state, event)` 形状，抄 rword `print_dialog_state.rs` 的模式）+ services 注入（list_printers/caps 桥），可单测；不进任何 rofd Rust crate（rword 终版结论：UI 状态模型不属于 spooling 库）。
- 流程：打开对话框 → 预览（`exportPdf` 只传 `pages:[当前页]`，快速）→ 确认 → `planPages(range, parity, current)` 解析出计划页 → `exportPdf({ ...content, pages: 计划页 })` → base64 → IPC `print_pdf`。**office-rs-print 后端不施加 range**（载荷 = 恰好计划好的页；CUPS 不传 page-ranges、WinRT 光栅整 PDF spool）——页选择在导出时落入 PDF，份数/逐份仍由 settings 带给后端。

### 5.3 xilem-app（native）

- **一期（随本设计交付）**：另存 PDF。复用 host 模式（`src/host/document_io.rs` 旁新增 `export_pdf.rs`）：rfd 保存对话框 → `begin_print(PrintContent::default()` 或菜单三开关 UI 由宿主自定 → `rofd_export::pdf_from_page_scenes` → 原子写。工具栏入口「另存为 PDF」。
- **二期（独立 spec/plan）**：`print_direct(component, printer, settings)` 一步直打（session → `payload_for` → `print_pages` GDI 矢量）+ 可选默认打印对话框组件（`print_dialog_state.rs` 形状移植进 rofd-xilem-view，纯 `advance` 状态机 + PrintServices trait 注入）+ 打印到文件手测通道（`print_pages_to_file`）。

## 6. 错误处理（§4.6 分层）

| 层 | 错误 | 处理 |
|---|---|---|
| session | 页越界 / 空文档 | panic 于契约违背（内部一致）或 `page_count()==0` 由宿主先判 |
| plan | 范围语法错 | `Err(String)` 透传 office-rs-print 校验信息（对话框内联显示） |
| rofd-export | 空页列表 / krilla 编码失败 | `ExportError`，wasm 桥转 JsValue，SDK Promise reject 带信息 |
| IPC/壳 | 打印机不存在 / 驱动拒绝 | office-rs-print JobState 错误透传对话框；失败保持对话框打开 |
| 宿主 UI | iframe print 环境异常 | catch + console + 用户可见提示（rword 教训 6：init/promise 链必须 catch） |

字体缺失不阻断导出：FontStore 回退与屏幕一致（tofu 同款），产物如实反映所见。

## 7. 测试与验收

- **纯函数单测**：纸型映射（±1mm 命中/最近面积/横竖归一）、`PrintContent`→kind 过滤、`plan` 委托语义（all/odd/current/custom/去重升序/非法 spec）、`exportPdf` 的 `pages` 参数语义（缺省全页/越界剔除/空结果报错）、默认值。
- **session 场景结构断言**（对象数/类型，非像素，§6 渲染测试约定）：水印关→无 Watermark 绘制对象；批注关→无批注对象但 body 不变；图章开→Stamp 图像存在；预览/产物不含 caret/handle（结构断言 overlay 类对象为零）。
- **rofd-export**：PDF 字节结构断言（`/Type/Page`、`/MediaBox` 计数 = 页数，抄 rword `pdf_from_page_scenes_two_pages` / `empty_pages_is_error` 测试形状）。
- **staleness**：begin_print 后编辑批注 → `is_stale()==true` 且 `print_gen` choke point 被触发；未编辑 → false。
- **字体就绪门**：SDK 单测——在途补片未完时 `exportPdf` 等待 flush；补片完成后产物含正确字形（Playwright 或 vitest 视基建择一）。
- **手测通道**：tauri 壳直跑 office-rs-print `print_pdf_to_file` + 「Microsoft Print to PDF」出 PDF 目检（中文/图片/图章/份数/范围/取消）；`#[ignore]` 真实样例标定。
- **浏览器流 Playwright**（沿用 smoke 基建）：断言 exportPdf 字节合法（%PDF- 头 + 页数 = pages 参数长度）、`planPages("1-3,5")` 返回、iframe 挂载、`print` 被调（stub），不碰系统对话框。

## 8. 非目标

- SEP 电子签章外观解析/渲染（v1 图章 = Stamp 批注；若未来做 SEP，`seal` 开关语义扩展，接口不变）。
- 打印变体布局（OFD 板式，不存在布局差异）。
- SDK 内置浏览器打印面板（浏览器系统对话框自带打印机/份数/范围/奇偶，面板增量价值为零——rword 已全删过一轮）。
- 矢量 Pages 载荷过 IPC（永远 PDF 字节；矢量仅限同进程引擎路径，二期 xilem）。
- macOS 打印后端（office-rs-print 仅 Windows/Linux；tauri macOS 本就受 WebGPU 限制）。
- 打印样式与屏幕不同（WYSIWYG，同一绘制路径，无打印专属样式）。

## 9. 阶段划分

**一期（本设计交付）**：
1. workspace 钉版（office-rs-print / krilla）+ rofd-export crate（移植 pdf.rs + 结构测试）
2. PrintSession（component）+ 过滤/纸型/plan/staleness 单测
3. web-view `exportPdf`/`planPrintPages` wasm 方法 + SDK `exportPdf`/`planPages`/`print`（含字体 flush 门）+ iframe 约束 + web-app 入口与内容开关面板
4. tauri IPC 面 + `PrintDialog.vue` 完整对话框（TS 状态机纯函数 + 单测）
5. xilem-app 另存 PDF
6. Playwright 打印流 + 手测通道文档

**二期（独立 spec）**：xilem `print_direct` GDI 矢量直打 + 原生默认对话框组件 + `print_pages_to_file` 手测通道。

## 10. 留给 plan/实现的次要决策（builder 可定）

- PDF 元数据（标题/作者）是否从 doc 注入及注入项清单（宿主可覆盖）。
- tauri job 进度 UI 形状（轮询 vs 事件）。
- PrintDialog.vue 预览性能策略（只编当前页已定；位图 vs SVG 预览未定）。
- `PrintContent` 是否同时暴露给 xilem 另存菜单的 UI 形状（宿主自理）。
- wasm 侧大 PDF 字节的返回通道（直接 Vec<u8> vs 分段）——按 rword as-built 直接 Vec 起步，超限再改。
