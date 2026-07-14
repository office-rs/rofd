# rofd Cluster 3：交互式批注 UX 设计

- **日期**: 2026-07-14
- **状态**: Draft（待评审）
- **范围**: V1 收尾子项目 3/4 -- 批注的创建（WPS 6 种拖拽绘制）、选择、移动、缩放（手柄）、删除、右键菜单、5 个回调；加 Squiggly 到模型
- **前置**: Cluster 1（io 批注往返 GB/T 33190 合规）+ Cluster 2（手术刀保存调用链）已完成。C1/C2 揭示：component `handle_event` 丢弃 `PointerMove`/`PointerUp`（拖拽未实现）、无创建 UI、无选区手柄、无右键；render `composite` 不接 selection（无法画手柄）；`hit_test` 的 `AnnotationText` 从不返回；`caret_rect` 未用；`resize_annotation` 无 handle；仅 4 回调（缺 focus/interact/context_menu/page_change/zoom_change）
- **参考**: WPS OFD 阅读器创建功能仅高亮/下划线/删除线/波浪线/手写/矩形（无 TextBox/便签/印章/水印）

---

## 1. 背景与动机

v1 的"批注编辑器"目前只能编辑**已存在**批注的文本（末尾追加，cursor-at-end on click），无法**创建**批注、无法**拖拽移动/缩放**、无选区手柄、无右键、无创建工具栏。`PointerMove`/`PointerUp` 被 `handle_event` 的 `_` 兜底丢弃。这让"查看 + 批注编辑器"的"批注编辑"半壁缺失。

C3 补齐交互式批注 UX 的**主循环**：创建（拖拽绘制）-> 选择 -> 移动/缩放 -> 删除 -> undo。范围对齐 WPS OFD 阅读器（高亮/下划线/删除线/波浪线/手写/矩形 6 种创建），不做 TextBox/便签/印章/水印的创建（它们仍在模型里，C1 能 parse/render 外部文件）。

**明确推迟**（不在 C3）：文本光标（click-to-caret 定位到字符 + 可视光标 + 闪烁--v1 创建无文本批注，现有末尾追加够用）、样式编辑（颜色/线宽--创建用默认色，创建后不改）。这两项留 C4/后续。

## 2. 范围与成功标准

### 2.1 范围（in）

- **模型**：加 `AnnotationKind::Squiggly`（io Type=Highlight Subtype=Squiggly 精确映射，改 C1 的降级 Highlight）。
- **render**：`composite` 接 `selection` + `drag_preview` 画选区手柄（8 把柄+框）+ 拖拽预览；`hit_test` 加 `HitTarget::Handle(id, HandlePos)`。
- **component**：`tool: Tool`（Select | Create{kind}）+ `drag: Option<DragState>` 状态机；`handle_event` 实现 PointerDown/Move/Up（创建/移动/缩放）。
- **editor**：resize 几何（component 算新 rect，editor `resize_annotation(id, new_rect)` 不变）。
- **apps**：native + web 工具栏（6 工具 + 选择按钮）拖拽创建；右键菜单（Delete）。
- **回调**：补 `on_annotation_focus`/`on_annotation_interact`/`on_context_menu`/`on_page_change`/`on_zoom_change`。

### 2.2 范围（out / 推迟）

- 文本光标（click-to-caret + 可视光标 + 闪烁）。
- 样式编辑（颜色/线宽 UI）。
- TextBox/便签/印章/水印/椭圆/箭头/直线的**创建** UI（模型保留，parse/render 外部文件）。
- 自由绘制（Freehand）之外的笔迹输入（如压感）。

### 2.3 成功标准

- 创建：工具栏选"矩形" -> 在页面拖拽画框 -> 松开生成矩形批注（默认色）-> 自动选中。6 种都能创建。
- 选择：点击批注选中（8 把柄+框显示）。
- 移动：拖拽批注 body 平移。
- 缩放：拖拽 8 把柄之一缩放（rect-based 批注）。
- 删除：右键 -> Delete / Delete 键。
- undo/redo 全程可还原。
- 回调：focus/interact/context_menu/page_change/zoom_change 在对应事件触发。
- Squiggly：创建波浪线 -> save -> reopen -> 仍是 Squiggly（Type=Highlight Subtype=Squiggly）。

---

## 3. 设计

### 3.1 模型：Squiggly（已由 C1.5 完成）

C1.5 已加 `AnnotationKind::Squiggly`（Markup payload 复用）+ io `Type=Highlight Subtype=Squiggly` 精确映射（不降级）+ render 波浪 Q 曲线。**C3 不再重复模型/io/render 改动**；C3 仅在创建 UI（§3.5）把 Squiggly 作为 WPS 6 种可创建类型之一（`create_annotation(Squiggly, ...)` 已可用）。

### 3.2 render：手柄 + composite 接 selection + handle 命中

```rust
// crates/render/src/hit_test.rs
pub enum HandlePos { Nw, Ne, Sw, Se, N, S, E, W }  // 4 角 + 4 边中点

pub enum HitTarget {
    Annotation(AnnotationId),
    AnnotationText(AnnotationId, usize),  // 仍 reserved（文本光标推迟）
    Handle(AnnotationId, HandlePos),       // 新
    Page(PageId),
    Empty,
}
```
- `hit_test(doc, vp, selection, point)`：**先**查选中批注的手柄（若 point 在某选中批注的 8 把柄之一内 -> 返回 Handle）；再查批注/Page/Empty。加 `selection` 参数（只对选中批注查手柄）。
- 手柄几何：选中批注的 bounding rect（viewport 坐标）的 4 角 + 4 边中点；手柄为边长 `HANDLE_SIZE`（固定屏幕 px，如 8.0）的正方形，**不随 zoom 缩放**。命中半径 = `HANDLE_SIZE/2 + HIT_PAD`（如 +4.0）。

```rust
// crates/render/src/composite.rs
pub enum DragPreview {
    Create { kind: AnnotationKind, rect: Rect },       // 创建拖拽中（markup/rect 矩形预览）
    CreateFreehand { path: Vec<(f64,f64)> },            // 手写拖拽中（笔迹预览）
    Move { id: AnnotationId, rect: Rect },              // 移动中（轮廓预览）
    Resize { id: AnnotationId, rect: Rect },            // 缩放中（框预览）
}

pub fn composite(
    &self, doc: &OfdDocument, vp: &Viewport, fonts: &FontStore,
    selection: &AnnotationSelection, drag: Option<&DragPreview>,
) -> Scene
```
- 选中批注画选择框（描边 bounding rect）+ 8 把柄（小实心方块）。
- drag=Some 时画预览（Create 矩形/Freehand 笔迹/Move 轮廓/Resize 框），半透明。
- 手柄用 imaging Painter（fill 小方块），颜色固定（如黑/白对比）。
- `caret_rect` 仍不用（文本光标推迟）。

### 3.3 component：工具态 + 拖拽状态机

```rust
// crates/component/src/editor_component.rs
pub enum Tool { Select, Create(AnnotationKind) }

enum DragState {
    Create { kind: AnnotationKind, start: (f64,f64), current: (f64,f64), path: Vec<(f64,f64)> },
    Move { id: AnnotationId, last: (f64,f64) },
    Resize { id: AnnotationId, handle: HandlePos, anchor: (f64,f64), orig: Rect },
}

pub struct EditorComponent {
    // ... 现有字段
    tool: Tool,                   // 默认 Select
    drag: Option<DragState>,
}
```
- `set_tool(&mut self, tool: Tool)`（apps 工具栏调）。
- `handle_event`：
  - **PointerDown**（Left）：
    - `tool=Create{kind}` -> `drag = Create{kind, start=p, current=p, path=[p]}`（Freehand 起笔迹）。
    - `tool=Select` -> `hit_test(doc, vp, selection, p)`：
      - `Handle(id,h)` -> select(id) + `drag=Resize{id, handle=h, anchor=对角, orig=批注 rect}`。
      - `Annotation(id)` -> select(id) + `drag=Move{id, last=p}` + fire focus/interact。
      - `Page(_)` / `Empty` -> clear_selection + clear_cursor。
  - **PointerMove**：
    - `drag=Create` -> current=p（Freehand 追加 path.push(p)）。needs_repaint。
    - `drag=Move{id}` -> delta = p - last；editor.move_annotation(id, dx, dy)；last=p。needs_repaint。
    - `drag=Resize{id,handle,anchor,orig}` -> new_rect = compute_resize(handle, anchor, orig, p)；editor.resize_annotation(id, new_rect)。needs_repaint。
  - **PointerUp**：
    - `drag=Create{kind,start,current,path}` -> build payload from (start,current/path)：Markup{quad_points=[start,current], color=默认} / Freehand{path, color, width=默认} / Shape{Rect, rect=bbox, stroke/fill/width=默认}；editor.create_annotation(kind, page, payload)；select 新 id；tool 回 Select。fire focus/interact。
    - `drag=Move/Resize` -> 已实时 apply（PointerMove 里），PointerUp 仅 clear drag。
  - **PointerDown**（Right）-> fire `on_context_menu(p, target)`（target = hit_test 结果）。不改变选区。
- `build_scene`：调 `composite(doc, vp, fonts, &selection, drag_preview())`，`drag_preview()` 把 `self.drag` 映射成 `DragPreview`。
- 默认 `tool=Select`，`drag=None`。

### 3.4 editor：resize 几何（component 算，editor 不变）

- `resize_annotation(id, new_rect)` 签名不变（C1 现状）。component 在 `Resize` PointerMove 里用 `compute_resize(handle, anchor, orig, p)` 算新 rect：
  - 角把柄（Nw/Ne/Sw/Se）：新 rect = bbox(anchor, p)。
  - 边把柄（N/S/E/W）：固定对边，移动该边（如 E 把柄：x/y/h 不变，w = p.x - orig.x）。
- `move_annotation(id, dx, dy)` 已有。`create_annotation`/`delete_selected` 已有。

### 3.5 创建 UI（apps 工具栏）

- **native-app**（`examples/native-app/src/main.rs`）：工具栏加按钮：`选择` + `高亮` + `下划线` + `删除线` + `波浪线` + `手写` + `矩形`。点击 -> `editor.set_tool(Select)` 或 `set_tool(Create(Highlight))` 等。
- **web-app**（`examples/web-app` + SDK）：同样 7 按钮（DOM）-> `editor.setTool(...)`（wasm-bindgen 方法）。
- 拖拽创建：tool=Create 时 PointerDown 起点 -> Move 扩区域/笔迹 -> Up 提交（§3.3）。
- 创建后 tool 自动回 Select（§3.3 PointerUp）。或保持 Create（连续创建）--v1 选 PointerUp 后回 Select（简单，避免误连创）。
- **payload 默认值**：Markup color=`Rgb(255,255,0)`（高亮黄）/ `Rgb(0,0,255)`（下划线/删除线/波浪线蓝）；Freehand color=`Rgb(0,0,0)` width=1.5；Rect stroke=`Rgb(255,0,0)` fill=None width=2.0。常量定义。

### 3.6 右键菜单 + 删除

- `ViewEvent::PointerDown{button: Right, x, y}` -> component `hit_test` -> fire `on_context_menu(point=(x,y), target)`。
- native-app：注册 `on_context_menu` -> 弹 xilem/masonry 右键菜单（"Delete" 项，仅当选中批注时）-> `editor.delete_selected()`。
- web-app：注册 `on_context_menu` -> JS 弹菜单（"Delete"）-> `editor.deleteSelected()`。
- Delete 键已有（component handle_event Delete/Backspace 删选中）。
- 右键不改变选区（仅弹菜单）；若右键在未选中批注上，可先 select 再弹（v1：右键 = hit_test + fire context_menu，不自动 select--简单）。

### 3.7 回调（补 5）

```rust
// crates/component/src/callbacks.rs
pub struct Callbacks {
    // 现有 on_change/on_selection_change/on_cursor_change/on_save_request
    pub on_annotation_focus: Option<...Fn(AnnotationId)...>,
    pub on_annotation_interact: Option<...Fn(AnnotationId)...>,
    pub on_context_menu: Option<...Fn((f64,f64), HitTarget)...>,
    pub on_page_change: Option<...Fn(usize)...>,
    pub on_zoom_change: Option<...Fn(f64)...>,
}
```
- `on_annotation_focus(id)`：PointerDown 命中批注且该批注**此前未选中**（首次进入）-> fire focus。
- `on_annotation_interact(id)`：PointerDown 命中批注 -> fire interact（无论首次或重击）。**首次进入时 focus + interact 同发；重击已选中批注只发 interact**（照搬 reditor SDT 语义）。
- `on_context_menu(point, target)`：右键（§3.6）。
- `on_page_change(idx)`：Scroll/Resize 致当前可见页变化（component 跟踪 `current_page`，变化时 fire）。
- `on_zoom_change(z)`：Zoom 致 zoom 变化时 fire。
- 均加 setter（`on_annotation_focus(cb)` 等，cfg-gated Send）+ native/web 适配器 wiring。
- `HitTarget` 需 export（on_context_menu 签名用）或改用更简单的 ContextTarget enum（`Annotation(id)` / `Page` / `Empty`）。

### 3.8 错误处理

- 无新硬错（UX 交互不 fatal）。
- 创建/移动/缩放在空选区/无效 page 上 -> no-op（返 needs_repaint=false），不 panic。
- 回调 fire 失败（无 callback 注册）-> 静默跳过（callbacks 是 Option）。
- 无裸 unwrap。

---

## 4. 测试

### 4.1 render（场景结构断言，非像素）
- 手柄几何：选中批注 -> composite 画 8 把柄（断言 Scene 里 8 个 handle 填充块 + 1 个选择框描边）。
- hit_test：point 在选中批注的角把柄 -> Handle(id, Nw) 等；在边把柄 -> Handle(id, E) 等；在批注 body -> Annotation(id)；空 -> Empty。
- DragPreview：composite 接 Create/Move/Resize 预览 -> 断言预览图元存在。
- Squiggly 渲染：draw_squiggly 画波浪线（断言 path 命令是 Q 曲线序列）。

### 4.2 component（拖拽状态机）
- 创建：set_tool(Create(Rect)) -> PointerDown -> PointerMove -> PointerUp -> 批注创建 + 选中 + tool 回 Select。
- 移动：select 批注 -> PointerDown on body -> PointerMove(dx,dy) -> 批注 rect 平移。
- 缩放：select 批注 -> PointerDown on Nw 把柄 -> PointerMove -> 批注 rect 缩放（对角 anchor 固定）。
- 工具切换：set_tool(Create) -> set_tool(Select)。
- 右键：PointerDown Right -> on_context_menu fired。
- Freehand 创建：PointerDown -> 多个 PointerMove 追加路径 -> PointerUp -> Freehand 批注 path 含所有点。

### 4.3 editor
- resize 各把柄方向（Nw/Ne/Sw/Se/N/S/E/W）-> 新 rect 正确（component compute_resize 单测）。

### 4.4 io（Squiggly 往返）
- Squiggly 批注 serialize -> parse -> 全等（Type=Highlight Subtype=Squiggly）。加到 C1 的 annotation_roundtrip.rs。

### 4.5 回调
- focus/interact：PointerDown 命中批注 -> focus fired；重击选中 -> interact fired。
- context_menu：右键 -> fired with point+target。
- page_change：Scroll 致页变 -> fired with idx。
- zoom_change：Zoom -> fired with z。

### 4.6 集成（冒烟，可 #[ignore] 需 GUI）
- native-app：工具栏选矩形 -> 拖拽创建 -> 选中 -> 移动 -> 缩放 -> 右键 Delete -> undo（手动或 #[ignore]）。
- web-app 同。

---

## 5. caveat / 不在范围

- **文本光标推迟**：click-to-caret + 可视光标 + 闪烁不在 C3（v1 创建无文本批注；现有末尾追加编辑够用）。`HitTarget::AnnotationText` 仍 reserved。`caret_rect` 仍不用。
- **样式编辑推迟**：颜色/线宽 UI 不在 C3（创建用默认色常量，创建后不改）。editor 的 set_annotation_color/width 命令已有，但无 UI 触发。
- **Freehand 之外的笔迹**：无压感/平滑，v1 原始路径点。
- **连续创建**：PointerUp 后 tool 回 Select（不保持 Create）。连续创建需再点工具按钮。
- **右键不自动选区**：右键 = fire context_menu，不改变选区（Delete 菜单项删当前选中；若右键在未选中批注，Delete 无效--v1 接受，或后续加"右键先 select"）。
- **多选**：v1 单选（AnnotationSelection::Multi 不由 UI 产生）。Shift+点击多选推迟。

---

## 6. 决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | 方案 A：component 持 DragMode 状态机 + tool 态；render composite 接 selection+drag_preview | 统一拖拽状态机（Create/Move/Resize），比工具态门控分支散的方案 B 干净 |
| 2 | 创建范围 = WPS 6（高亮/下划线/删除线/波浪线/手写/矩形） | 对齐真实 OFD 阅读器（WPS）；TextBox/便签/印章/水印无创建 UI（模型保留 parse/render） |
| 3 | 创建流程 = 拖拽绘制 | WPS 6 种都是拖拽（markup 拖区域/手写拖笔迹/矩形拖框），统一 |
| 4 | 加 Squiggly 到模型 | WPS 有波浪线；C1 原降级 Highlight 改精确 Squiggly（Type=Highlight Subtype=Squiggly） |
| 5 | 文本光标推迟 | v1 创建无文本批注；现有末尾追加够用；留 C4 |
| 6 | 样式编辑推迟 | 创建用默认色常量；留 C4 |
| 7 | resize 几何在 component（editor 签名不变） | component 算新 rect（handle+anchor+orig），editor.resize_annotation(id, new_rect) 不变 |
| 8 | 手柄固定屏幕 px（不随 zoom） | 手柄交互尺寸稳定（8px），不受 zoom 影响 |
| 9 | on_context_menu 用简单 ContextTarget（Annotation/Page/Empty） | 不暴露完整 HitTarget（Handle 等是内部细节） |
| 10 | 右键不自动选区 | v1 简单；Delete 删当前选中 |

---

## 7. 对 v1 spec 的修订

- **§1.3 批注类型**：创建范围标注为 WPS 6（高亮/下划线/删除线/波浪线/手写/矩形）；TextBox/便签/印章/水印/椭圆/箭头/直线为 parse/render-only（模型保留）。
- **§4.2 / §6.4**：`composite` 签名加 selection + drag_preview；`hit_test` 加 Handle + selection 参数；补 5 回调。
- **§6.5 事件路由**：PointerMove/Up 实现（拖拽状态机）；右键路由；tool 态。
- **§4.8 / §3.2**：加 Squiggly kind。
- **§10**：文本光标（up/down 视觉行导航 + click-to-caret + 可视光标）标为推迟（C4/后续）。

---

## 8. 后续（非本 spec）

- **Cluster 4**：次要缺口收尾（ViewEvent 补 ScrollPage/ZoomAt/Ime、on_warning 回调、错误用例测试、SkippedObject 发出）+ 可能含文本光标 + 样式编辑（若 C3 推迟项挪到 C4）。
