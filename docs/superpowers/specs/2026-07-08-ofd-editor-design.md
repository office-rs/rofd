# rofd — OFD 查看 + 批注编辑器 设计

- **日期**: 2026-07-08
- **状态**: Draft（待评审）
- **参考项目**: `D:/code/reditor`（Rust + GPU 的 OOXML 编辑器库）
- **范围**: 本 spec 覆盖 **v1：view + annotate editor**（前端，native + WASM）。后端生成、PDF 转换、后端读改写为后续子项目，架构为其留门。

---

## 1. 目标与范围

### 1.1 目标

构建一个 OFD（GB/T 33190）**查看 + 批注**编辑器**库**，参考 reditor 的分层架构。主文档（body）只读渲染；批注是唯一可编辑层。库以 `EditorComponent` 为唯一集成入口（类比 `<textarea>`），宿主控制消息循环并转发事件。

### 1.2 子系统分解

项目最终包含四个子系统，共享 `rofd-dom` + `rofd-io`：

| 子系统 | 部署 | 写入路径 | spec 覆盖 |
|---|---|---|---|
| **A. view + annotate editor** | 前端（native + WASM） | 手术刀（`save_ofd`） | ✅ v1（本 spec） |
| **B. 后端 OFD 生成** | 服务端 | 全量（`write_ofd`） | 后续 |
| **C. PDF -> OFD 转换** | 服务端 | 全量（`write_ofd`） | 后续 |
| **D. 后端读 OFD -> 修改 -> 输出新 OFD** | 服务端 | 手术刀（`save_ofd`） | 后续 |

`rofd-render` / `rofd-editor` / `rofd-component` 为 A 专有；B/C/D 只用 dom + io（+ 未来的 `rofd-pdf`）。

### 1.3 v1 功能面

- **渲染保真度**：常见子集——基础 Text/Image/Path/Composite 对象 + CTM + 常见字体 + JPG/PNG。模板继承 / JBIG2 / 瓦片图 / 冷门特性按需补（v1 桩处理：能简单展开就展开，否则跳过 + warning）。
- **批注类型**：标注笔迹（高亮 / 下划线 / 删除线 / 自由绘制）、图形批注（矩形 / 椭圆 / 箭头 / 直线 / 文本框）、便签评论（带回复线程）、印章水印（静态图章 / 水印）。
- **电子签名**：v1 只渲染已有签名/印章（只读显示），不做签名/验签，不引入国密。
- **平台**：双平台 native 优先。Rust + Vello GPU + Parley 文字 + native（xilem/winit）+ WASM/WebGPU，三层缓存、命令/撤销、`EditorComponent` 门面。

### 1.4 非目标（v1）

- 编辑 body 内容（文本/图片/路径对象的增删改）——body 只读。
- 创建加密电子签名 / 验签。
- 全保真渲染真实复杂公文（模板/JBIG2/瓦片图）。
- 实时协同 / 多用户。
- up/down 视觉行导航（见 §10 已知边界）。

---

## 2. 架构与 crate 分层

复刻 reditor 的"5 层严格单向依赖 + 格式 crate + 薄适配器"骨架，按 OFD 固定版式替换。依赖**严格向上**，反向边禁止。

### 2.1 Workspace crates

| rofd crate | 路径 | 对应 reditor | 职责 |
|---|---|---|---|
| `rofd-dom` | `crates/dom` | reditor-dom | 纯数据模型：`OfdDocument`（PageModel 只读 body + AnnotationModel 可变批注 + 包元数据）。依赖仅 serde/uuid。无 `Format` trait |
| `rofd-io` | `crates/io` | reditor-ooxml | `parse_ofd` / `save_ofd`（手术刀）/ `write_ofd`（全量）自由函数 + `PackageHandle` + `zip_surgical` 模块。依赖 dom + zip + quick-xml |
| `rofd-render` | `crates/render` | reditor-renderer + reditor-layout（瘦身后） | Vello 场景构建（body 对象 + CTM + 批注 overlay）+ `text/` 子模块（font/glyph/shape）+ `hit_test`/`caret_rect` |
| `rofd-editor` | `crates/editor` | reditor-editor | 批注选区、命令模式 + Step/Transaction/History |
| `rofd-component` | `crates/component` | reditor-component | **唯一集成入口** `EditorComponent`：`new_native()`/`new_wasm()`、`ViewEvent`、`RenderTarget`、callbacks、脏缓存 |
| `rofd-native-view` | `crates/native-view` | reditor-native-view | 薄适配器：`EditorApp` + `WinitEventBridge` + `VelloRenderTarget` |
| `rofd_web_view` | `crates/web-view` | reditor-web-view | WASM 薄适配器：`WasmEditor` + `WebGpuRenderTarget` + TS SDK |

### 2.2 依赖图

```
┌────────────────────────────────────────────────────────┐
│              Example Apps                                │
│   examples/native-app        │   examples/web-app       │
└──────────┬───────────────────┼──────────────┬───────────┘
           │                   │              │
┌──────────▼──────┐ ┌──────────▼──────────────▼─────────┐
│  native-view    │ │         web-view                    │
│ (VelloRender-   │ │  (WebGpuRenderTarget + WASM JS 绑定) │
│  Target, Bridge)│ │                                     │
└────────┬────────┘ └────────────────┬────────────────────┘
         └────────────┬───────────────┘
                      │
┌─────────────────────▼───────────────────┐
│  component  (EditorComponent 门面,       │
│              ViewEvent, RenderTarget,    │
│              Callbacks, 脏缓存)          │  同时依赖 io（load_ofd/save_ofd 便利方法）
└──┬───────┬──────────────────┬────────┘
   │       │                  │
┌──▼──────┐ ┌──▼────┐ ┌───────▼──────┐
│ render  │ │editor │ │     io       │  render 内含 text/ 子模块（font/glyph/shape）；
│ (Vello  │ │(命令) │ │ (解析+手术刀+│  render/editor/io 均依赖 dom；
│  +text/)│ │       │ │  全量保存)   │  component 另依赖 io 的 load/save 便利方法
└────┬────┘ └──┬────┘ └───────┬──────┘
     │         │              │
     └─────────┴──────────────┘
                      │
┌─────────────────────▼───────────────────┐
│  dom  (OfdDocument: PageModel +         │
│        AnnotationModel + 元数据)         │
└─────────────────────────────────────────┘
```
┌─────────────────────▼───────────────────┐
│  dom  (OfdDocument: PageModel +         │
│        AnnotationModel + 元数据)         │
└─────────────────────────────────────────┘
```

### 2.3 与 reditor 的关键差异

1. **无独立 `layout` crate**：OFD 固定版式，对象坐标已在文档中，无段落回流。reditor-layout 的"段落排版"消失，字体加载 + 文字整形下沉为 `rofd-render` 内的 `text/` 子模块（不足以单列 crate）。
2. **dom 双模型**：`PageModel`（body，加载后只读）与 `AnnotationModel`（批注，可变）分离。
3. **`rofd-io` 双写入路径**：手术刀（`save_ofd`，A+D）+ 全量（`write_ofd`，B+C），非 reditor-ooxml 的全量序列化。
4. **无 `Format` trait**：单格式项目，YAGNI。JSON 测试 fixture 由模型 derive serde 后用 `serde_json` 直接获得。
5. **editor 操作对象是批注**，不是文本光标；选区是对象级。

---

## 3. 数据模型与 io 层

### 3.1 `rofd-dom`：`OfdDocument`

所有结构 `#[derive(Debug, Clone, Default)]` + public 字段；读写区分靠约定，类型不锁死（生成/D 可自由构造）。

```rust
pub struct OfdDocument {
    pub meta: DocMeta,                  // 版本、页面物理尺寸基准、单位
    pub pages: Vec<Page>,               // body 页模型（editor 加载后只读；生成/D 可构造）
    pub resources: Resources,           // 字体/图片/DrawParam，Arc 共享
    pub annotations: AnnotationModel,   // 可变批注面（editor 唯一改这里）
}

pub struct Page {
    pub id: PageId,
    pub physical_box: Rect,             // OFD Area
    pub layers: Vec<Layer>,             // Body/Foreground/Background
    pub template: Option<TemplateRef>,  // v1 桩
}

pub struct Layer { pub layer_type: LayerType, pub objects: Vec<PageObject> }

pub enum PageObject {                   // 类型化 enum，对齐 reditor ParagraphContent
    Text(TextObject),
    Image(ImageObject),
    Path(PathObject),
    Composite(CompositeObject),
}

pub struct TextObject {
    pub id: ObjectId, pub boundary: Rect, pub ctm: Option<Affine>,  // kurbo::Affine（6 值）
    pub font: FontRef, pub size: f32, pub fill: Option<Color>,
    pub codes: Vec<TextCode>,           // 字形 ID + delta 定位（不排版）
}
pub struct TextCode { pub glyph_ids: Vec<u32>, pub deltas: Vec<(f32, f32)> }
pub struct ImageObject { pub id, boundary, ctm, image: ImageRef }
pub struct PathObject  { pub id, boundary, ctm, fill, stroke, line_width, data: PathData }  // M/L/C/Q/A/Z
pub struct CompositeObject { pub id, boundary, ctm, unit: CompositeUnitRef }

pub struct Resources {
    pub fonts: HashMap<FontId, Arc<Vec<u8>>>,       // Arc 共享（CJK 字体大）
    pub images: HashMap<ImageId, Arc<Vec<u8>>>,
    pub draw_params: HashMap<DrawParamId, DrawParam>,
}
```

### 3.2 `AnnotationModel`：唯一可变面

```rust
pub struct AnnotationModel { pub by_page: HashMap<PageId, Vec<Annotation>> }

pub struct Annotation {
    pub id: AnnotationId,              // uuid v4
    pub kind: AnnotationKind,
    pub page: PageId,
    pub creator: String,
    pub created: i64, pub modified: i64,       // 调用方传入 ts（库不取系统时间）
    pub reply_to: Option<AnnotationId>,        // InReplyTo，便签回复线程
    pub payload: AnnotationPayload,
}

pub enum AnnotationKind {
    Highlight, Underline, Strikeout,
    Freehand, Shape(ShapeKind),         // Rect/Ellipse/Arrow/Line
    Note, TextBox,
    Stamp, Watermark,
}

pub enum AnnotationPayload {
    Markup   { quad_points: Vec<Point>, color: Color },                  // 高亮/下划线/删除线
    Freehand { path: PathData, color: Color, width: f32 },
    Shape    { kind: ShapeKind, rect: Rect, stroke: Color, fill: Option<Color>, width: f32 },
    Note     { rect: Rect, color: Color, content: String, icon: NoteIcon },
    TextBox  { rect: Rect, content: String, font: FontRef, size: f32, color: Color },
    Stamp    { rect: Rect, image: ImageRef },                            // 静态图片图章
    Watermark{ rect: Rect, content: String, opacity: f32, angle: f32, font: FontRef, size: f32, color: Color },
}
```

- **Appearance 不存模型**：批注渲染形（overlay 几何）由 render 从 `payload` 实时算，不预存——省冗余、编辑时天然一致。
- **可变/可构造**：`pages` 在 editor 里只读是运行约定（io 的 save 逻辑编码"重写批注、保留 body 原样"保证），类型不限制；生成/D 直接构造 `Page`/`PageObject`。
- **ID**：`ObjectId`/`PageId` 用 OFD ID 字符串 newtype；`AnnotationId` 用 uuid v4。

### 3.3 `rofd-io`：双路径 + 条目驱动手术刀

```rust
pub fn parse_ofd(bytes: &[u8]) -> Result<LoadReport, OfdError>;
pub fn save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError>;  // 手术刀：A+D
pub fn write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError>;                      // 全量：B+C

pub struct LoadReport { pub document: OfdDocument, pub package: PackageHandle, pub warnings: Vec<OfdWarning> }
pub struct PackageHandle { /* io 内部不透明：原始条目字节(Arc) + name->位置 + 条目分类索引 */ }
```

**手术刀保存机制（核心）**：

- `parse_ofd` 时，body 页 XML / 签名 / 资源条目的原始字节以 `Arc` 保留在 `PackageHandle`（图片/字体与模型里的 `Arc` 共享，不重复拷贝）。
- `save_ofd`：**批注条目从 `AnnotationModel` 重新序列化；其余条目原样拷字节**。-> 未建模 body 内容（模板/JBIG2/冷门对象）和签名**字节级保留**。这就是 subset 模型仍能保真的原因。
- v1 的"脏集"= 所有批注条目。D 来了之后脏集扩展到被改的 body 页条目——同一套条目驱动逻辑，只是脏集变大。body 页一旦被改，该页 `Page_N.xml` 从 subset 模型重序列化，**该页未建模对象丢失**（逐页生效，不影响其他页）。

**全量 `write_ofd`**：不依赖 `PackageHandle`，从模型从头打包。只发出模型里有的东西，无"原始"可丢。

---

## 4. 渲染管线

### 4.1 渲染模型：paper-on-desk + 离散分页

灰底视口 `#E0E0E0`，白页居中竖向堆叠，可滚动。每页尺寸来自 `Page.physical_box`，整体按 `zoom` 缩放。最终变换 = `viewport(平移+zoom) × page_origin × object_ctm`。

### 4.2 场景构建：body 稳定 + 批注 overlay

每页产出两个独立 `vello::Scene`：

```rust
impl RenderEngine {
    fn body_scene(&self, page: &Page, res: &Resources) -> Scene;            // 只算一次，缓存（body 只读，永不失效）
    fn annotation_scene(&self, anns: &[Annotation], res: &Resources) -> Scene; // 从 payload 实时算几何
    fn composite(&self, doc, viewport, selection, cache: &mut PageSceneCache) -> Scene;
}
```

- **body 对象**：`TextObject` 按字形 ID + delta 逐字画轮廓（不排版）；`PathObject` 走 Vello path；`ImageObject` 贴图；`CompositeObject` 展开。每个套 `ctm`。
- **批注 overlay**：`Markup` 画半透明矩形/下划线/删除线；`Freehand`/`Shape` 画 path；`Note` 画图标+边框（文字内容在弹层）；`TextBox`/`Watermark` 经整形后画字；`Stamp` 贴图章。选区手柄（8 把柄 + 框）叠最上层。

### 4.3 `text/` 子模块（并入 render）

```rust
// rofd-render/src/text/
mod font   { FontLoader; FontCache; Arc<Vec<u8>> 共享 }     // 解析 OTF/TTF
mod glyph  { glyph_outline(font, glyph_id) -> Path }        // body 文字按 ID 取轮廓，无整形
mod shape  { shape(text, font, size) -> PositionedGlyphs }  // Parley，仅批注文本用
```

body 文字用 `glyph`（文档已给字形 ID + delta）；批注文字（TextBox/Note/Watermark）用 `shape`。

### 4.4 脏缓存：body 稳定 + 批注脏

reditor 三层 `layout脏 -> scene脏 -> 重绘`。OFD 无回流，layout 层消失：

| 层 | body | 批注（非文本） | 批注（文本） |
|---|---|---|---|
| shape | - | - | 文本编辑 -> 该批注 shape 脏 |
| scene | **一次构建后稳定缓存** | 批注改动 -> overlay scene 脏 | shape 脏 -> scene 脏 |
| repaint | 每帧 | 每帧 | 每帧 |

`PageSceneCache`（component 持有）按页缓存 `(body_scene, annotation_scene)`。批注编辑只让对应页的 annotation_scene 失效，body_scene 不动。GPU immediate mode 全帧重绘。

### 4.5 交互 API：render 算几何，editor 保持逻辑

```rust
pub enum HitTarget { Annotation(AnnotationId), AnnotationText(AnnotationId, usize), Page(PageId), Empty }
impl RenderEngine {
    fn hit_test(&self, doc, viewport, point: Point) -> Option<HitTarget>;
    fn caret_rect(&self, doc, viewport, ann_id, offset: usize) -> Option<Rect>;
}
```

像素事件经 render 命中/换算成逻辑量（id、offset）喂 editor；editor 只存 `(AnnotationId, offset)`，不碰像素。

---

## 5. editor：选区、命令、撤销/重做

### 5.1 所有权与作用域

```rust
pub struct Editor {
    document: OfdDocument,              // 拥有整个 doc；只通过 API 改 .annotations
    selection: AnnotationSelection,
    text_cursor: Option<TextCursor>,
    history: History,                   // cap 100
    author: String,
    current_ts: i64,                    // 宿主 set_clock 更新；库不取系统时间
}
```

D（后端读改写）不用 rofd-editor——程序化修改直接改模型 + 调 `save_ofd`，不要 undo/选区。

### 5.2 选区模型（对象级）

```rust
pub enum AnnotationSelection { None, Single(AnnotationId), Multi(Vec<AnnotationId>) }
pub struct TextCursor { annotation: AnnotationId, offset: usize, preferred_x: Option<f32> }
```

点击文本类批注内部 -> `hit_test` 命中 `AnnotationText(id, off)` -> 同时设 `Single(id)` + `TextCursor`。

### 5.3 Step / Transaction / History

```rust
pub trait Step: Send { fn apply(&self, anns: &mut AnnotationModel); fn revert(&self, anns: &mut AnnotationModel); }
pub struct InsertAnnotationStep { pub annotation: Annotation }
pub struct DeleteAnnotationStep  { pub annotation: Annotation }
pub struct ReplaceAnnotationStep { pub id: AnnotationId, pub before: Annotation, pub after: Annotation }

pub struct Transaction {
    pub steps: Vec<Box<dyn Step>>,
    pub selection_before: AnnotationSelection,  pub selection_after: AnnotationSelection,
    pub text_cursor_before: Option<TextCursor>, pub text_cursor_after: Option<TextCursor>,
}
pub struct History { /* 栈，cap 100 */ }
```

文本编辑产生 `ReplaceAnnotationStep`（before/after 整个 annotation）。批注文本通常短，逐键克隆可接受；后续有性能问题再下沉为细粒度 `SetTextStep`。

### 5.4 命令清单

| 命令 | 产生 | 说明 |
|---|---|---|
| `create_annotation(kind, page, payload, now)` -> id | Insert | author/ts 来自 editor |
| `delete_annotation(id)` / `delete_selected()` | Delete | |
| `move_annotation(id, dx, dy)` | Replace | 平移 boundary/quad_points |
| `resize_annotation(id, handle, new_rect)` | Replace | 手柄缩放 |
| `set_annotation_style(id, color/width/fill/...)` | Replace | |
| `insert_text(id, offset, chars)` / `delete_text(id, offset, len)` | Replace | 批注内文本编辑 |
| `set_annotation_text(id, text)` | Replace | 整段替换 |
| `reply_to(parent_id, content, now)` -> id | Insert | 新建 Note，`reply_to=parent` |
| `undo()` / `redo()` / `can_undo()` / `can_redo()` | - | 弹/压 History |

### 5.5 author / 时间戳

库不调 `Date::now()`。宿主 `editor.set_clock(author, ts)` 更新，命令用 `self.current_ts` 填 `created`/`modified`。

### 5.6 变更信号 -> 缓存失效

命令 apply 后经 `on_change(affected_pages: Vec<PageId>)` 回调发出受影响页集合，component 据此只让对应页的 annotation_scene 失效。`on_cursor_change` / `on_selection_change` 在 text_cursor / selection 变化时触发。

### 5.7 editor 保持逻辑

left/right = ±1 char（纯逻辑）；home/end = 行首/末（需 render line-map）；up/down 视觉行需 render 按 `preferred_x` 算（见 §10 边界）。

---

## 6. component 门面 + 集成面 + 平台适配器

### 6.1 `EditorComponent`

```rust
pub struct EditorComponent {
    editor: Editor,
    render: RenderEngine,
    package: Option<PackageHandle>,     // Some=从文件加载(surgical); None=生成/new(full)
    cache: PageSceneCache,
    viewport: Viewport,
    callbacks: Callbacks,
    config: EditorConfig,
}

impl EditorComponent {
    #[cfg(not(target_arch = "wasm32"))] pub fn new_native(cfg: EditorConfig) -> Self;
    #[cfg(target_arch = "wasm32")]     pub fn new_wasm(cfg: EditorConfig) -> Self;

    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<Vec<OfdWarning>, OfdError>;
    pub fn load_document(&mut self, doc: OfdDocument);
    pub fn new_document(&mut self);
    pub fn save_ofd(&self) -> Result<Vec<u8>, OfdError>;   // package 有->手术刀, 无->全量
    pub fn handle_event(&mut self, e: &ViewEvent) -> EventOutcome;
    pub fn render(&mut self, target: &mut dyn RenderTarget);
    pub fn register_font(&mut self, data: Vec<u8>);
}
```

构造目标门控（`#[cfg]` 非 feature）。`save_ofd` 按 `package` 有无自动选手术刀/全量。

### 6.2 `RenderTarget` trait

```rust
pub trait RenderTarget { fn draw_scene(&mut self, s: &Scene); fn size(&self) -> (u32, u32); }
```

native 由 `VelloRenderTarget`、wasm 由 `WebGpuRenderTarget` 实现。

### 6.3 `ViewEvent`

Key{key,modifiers} / Pointer{Down,Move,Up}{x,y,button,modifiers} / Scroll{dx,dy} / ScrollPage / Zoom{factor} / ZoomAt{factor,center} / Resize{w,h} / Ime{...} / FocusGained/Lost。

### 6.4 Callbacks

| 回调 | 触发 | 用途 |
|---|---|---|
| `on_change(affected_pages)` | 批注变更 | 失效对应页 annotation_scene |
| `on_selection_change(&selection)` | 选区变 | 状态栏/属性面板 |
| `on_cursor_change(&Option<TextCursor>)` | 文本光标变 | 光标显隐 |
| `on_annotation_focus(id)` | 进入批注 | 高亮/状态跟踪（对应 reditor on_sdt_focus） |
| `on_annotation_interact(id)` | 激活批注 | 打开便签弹层/进入文本编辑（对应 on_sdt_interact） |
| `on_context_menu(point, target)` | 右键 | 批注/页面右键菜单 |
| `on_save_request()` | Ctrl+S | 宿主触发保存 |
| `on_page_change(idx)` / `on_zoom_change(z)` | 滚动/缩放 | 页码/缩放指示器 |
| `on_warning(Vec<OfdWarning>)` | 加载/渲染降级 | 提示"部分内容未显示" |

`focus` 与 `interact` 首次进入同发；批注内重击只重发 `interact`；批注内键盘移动两者都不发（照搬 reditor SDT 语义）。`Send` bound 在 native 上、wasm 上不加（target-gated）。

### 6.5 事件路由

```
PointerDown ─► render.hit_test ─► AnnotationText(id,off) ─► editor.set_cursor+select
                                 │  Annotation(id)        ─► editor.select
                                 └  Empty                 ─► editor.clear
Drag        ─► render 算 handle 几何 ─► editor.move/resize
KeyDown     ─► text_cursor? editor.insert_text/delete_text : 选区方向键
           └─► editor op ─► on_change(affected_pages) ─► cache 失效
下一帧 render ─► 重建脏 scene ─► composite ─► RenderTarget.draw_scene
```

### 6.6 native-view 三层（Bridge 不进 EditorApp）

| 层 | 拥有 | 不感知 |
|---|---|---|
| **Host**（examples/native-app） | masonry/xilem 状态、canvas WidgetId、右键菜单策略、渲染循环 | - |
| **WinitEventBridge** | modifiers、cursor(物理px)、scale_factor、canvas_origin | masonry/文档 |
| **EditorApp** | EditorComponent、文件路径、modified 标志 | winit/masonry/xilem |

坐标链：`winit CursorMoved(物理px) ÷ scale_factor − canvas_origin -> 画布逻辑px -> ViewEvent::PointerMove`。Bridge 不是 EditorApp 字段（框架无关、host 可前置拦截、生命周期不同）。HiDPI 首帧 `resumed()` 显式 `set_scale_factor`。

### 6.7 web-view

`WasmEditor`（wasm-bindgen）+ `WebGpuRenderTarget` + JS 事件桥。`wasm-pack --target web`；SDK 在 `crates/web-view/sdk/`，发布为 npm 包。入口 `Rofd.init(container, config) -> Promise<Editor>`。默认字体 NotoSans + NotoSansCJKsc，`Arc<Vec<u8>>` 共享，`warmup()` 预编译 shader。

---

## 7. 数据流

**加载**
```
.ofd bytes
 └ rofd_io::parse_ofd -> LoadReport{document, package, warnings}
 └ component.load_ofd -> editor.document=doc; component.package=pkg; render lazy 建 body_scene
```

**交互批注**
```
PointerDown -> render.hit_test -> HitTarget -> editor.select/set_cursor
KeyDown('x') -> editor.insert_text -> ReplaceAnnotationStep -> Transaction 入 History
             -> on_change([page]) -> cache 失效该页 annotation_scene -> needs_repaint
下一帧 -> render 重建脏 scene -> composite -> draw_scene
```

**保存（editor / D，手术刀）**
```
save_ofd(doc, pkg): 批注条目 <- 重序列化; 其余 <- 原样拷字节; 重打 zip -> Vec<u8>
（仅批注条目与原文不同；签名/未建模 body 字节级保留）
```

**保存（生成 B / 转换 C，全量）**
```
write_ofd(doc): 从模型建全部条目 -> zip -> Vec<u8>
```

**后端读改写（D）**
```
parse_ofd -> (doc, pkg) -> 程序化改 -> save_ofd(doc, pkg) -> 新 .ofd
```

**PDF->OFD（未来 C）**
```
.pdf -> rofd_pdf::parse_pdf -> OfdDocument -> write_ofd -> .ofd
```

---

## 8. 错误处理

显式、分层、UI 友好 + 服务端详尽、绝不静默吞。

**结构化错误**（偏离 reditor boxed，因退化需分类）：
```rust
pub enum OfdError {
    Zip(zip::result::ZipError),
    Xml { entry: String, loc: String, source: quick_xml::Error },
    Schema { entry: String, reason: String },
    ResourceNotFound { id: String, kind: ResourceKind },
    Io(std::io::Error),
}
```

**可降级问题走 warning，不致命**：
```rust
pub enum OfdWarning {
    MissingFeature { feature: String, entry: String },   // 模板继承/JBIG2/瓦片图
    SkippedObject { page: PageId, reason: String },
    FontSubstituted { requested: String, used: String },
}
```

模板未展开 / JBIG2 / 未知对象 -> 跳过 + warning，不 fail 整个加载。`component` 经 `on_warning` 回调上抛。

| 场景 | 处置 |
|---|---|
| 加载硬错（ZIP 损坏/XML 畸形） | `Err(OfdError)` -> 宿主"无法打开：{reason}" |
| 加载可降级 | warning，继续加载 |
| 保存错 | `Err` -> 宿主"保存失败：{reason}" |
| 渲染缺字体 | warning + 回退字体，不崩 |
| editor 非法操作 | 返回 `false`/`Result`，不 panic |

绝不静默：所有 `?` 带 context 上抛；warning 全记录；无裸 `unwrap`/`ignore`。输入校验在 io 边界 fail-fast。

---

## 9. 测试

80% 覆盖，unit + integration + e2e，TDD（先红后绿）。

**Unit（每 crate）**

| crate | 关键测试 |
|---|---|
| `rofd-dom` | 构造/Default/Clone；serde JSON 往返（fixture）；ID 生成 |
| `rofd-io` | **手术刀字节保留**（parse -> save_ofd -> 未触碰条目字节逐字节相等，**核心测试**）；解析正确性；`write_ofd` 全量往返；错误用例（坏 ZIP/坏 XML/缺资源）；warning 用例（模板/JBIG2 -> warning 且不 fatal） |
| `rofd-render` | body 场景结构断言（对象数/类型，非像素）；每类 `AnnotationPayload` -> 期望 overlay 几何；`hit_test`；`caret_rect` |
| `rofd-editor` | **每命令 apply->undo->还原**；History cap=100 溢出丢最旧；选区/光标转移；author/ts 盖戳 |
| `rofd-component` | 事件路由；**脏缓存只失效受影响页**；`save_ofd` 路径选择（package 有->手术刀，无->全量） |

**Integration**：真实 .ofd fixture（纯文本/含图/含路径/已签名/已有批注）-> 加载 -> 批注 -> 保存 -> 重开 -> 断言批注保留 + body 字节一致。

**E2E**：native-app 开文件->加批注->undo->存->重开（冒烟）；web-app 同流程（Playwright）。

**Fixture 手段**：模型 derive serde -> `serde_json` fixture，免费且跨 crate。

**Golden 像素**：GPU 快照易 flaky，v1 用场景结构断言；native 侧可选 golden，非阻塞 CI。

**覆盖**：`cargo llvm-cov`，目标 80%；TDD 节奏在 writing-plans 阶段细化到每个任务。

---

## 10. 已知边界与风险

- **up/down 视觉行导航**：批注内多行文本的上下行移动需 render line-map 查询。v1 实现 left/right + 点击定位 + home/end；up/down 降级为移到首/末行，或补 `render.line_map(id)` 查询。
- **body 编辑保真度 caveat（D 场景）**：subset 模型未建模模板/JBIG2/冷门对象。D 改某页 body 时，该页 `Page_N.xml` 从 subset 模型重序列化，**该页未建模对象丢失**。批注级修改不受影响。body 编辑保真度随模型覆盖度增长而提升。
- **签名完整性**：v1 只渲染已有签名，不验签。手术刀保存保留签名条目字节，但若原签名覆盖了批注层，保存仍可能影响其有效性——v1 不做校验，文档此 caveat；默认建议 Save As 到新文件，不覆盖原件。
- **Linebender alpha 风险**：vello/parley/xilem 仍 alpha/beta，bump 时预期 breaking change（同 reditor）。
- **WebGPU only**：无 Canvas2D 回退，Chrome/Edge 113+。首帧 shader 编译延迟靠 `warmup()` 前置。
- **IME**：中文输入依赖 xilem IME plumbing（上游仍在完善）。

---

## 11. v1 留门清单（为 B/C/D 不砌死墙）

1. 模型可构造 + 可变（public 字段 + `Default`）——服 A/B/C/D。
2. `rofd-io` 双写入路径（`save_ofd` 手术刀 + `write_ofd` 全量）——v1 只用 `save_ofd`。
3. dom/io 无 GUI 依赖——服 B/C/D 后端。
4. 留 `rofd-pdf` crate 位置（C）。
5. 手术刀设计为条目驱动选择性重写（v1 只重写批注条目，为 D 的 body 重写留路）+ 写明 body 编辑保真度 caveat。

---

## 12. 关键决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | v1 = view + annotate（非全功能编辑器） | 用户定位；批注收敛为可变面，复杂度可控 |
| 2 | 渲染常见子集，非全保真 | 务实可迭代；手术刀保存补足未建模 body 的保真 |
| 3 | 电子签名只渲染已有 | v1 不引入国密 |
| 4 | 双平台 native 优先 | 对齐 reditor 开发顺序，调试友好 |
| 5 | 方案 1：手术刀式批注编辑器 | body 只读 scope 与手术刀天然契合；保签名、body 零损耗、批注规范在包内 |
| 6 | `rofd-text` 并入 `rofd-render` 的 `text/` 子模块 | OFD 无回流，text 关注点质量不足单列；render 是唯一消费者；editor 保持逻辑不反向依赖 |
| 7 | `rofd-format` 单列但更名 `rofd-io`，砍 `Format` trait | "format" 暗示多格式抽象；单格式 YAGNI；JSON fixture 由 serde 直接获得；dom 保持纯净 |
| 8 | `rofd-io` 双写入路径（手术刀 + 全量） | A+D 往返用手术刀，B+C 生成用全量 |
| 9 | 四子系统分解，本 spec 仅覆盖 A | 多子系先分解；架构为 B/C/D 留门 |
| 10 | 错误结构化 enum + LoadReport warning 分流 | 退化需分类处置；可降级问题不 fatal 以支持部分渲染 |
