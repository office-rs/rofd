# rofd xilem 重构与改名设计

日期：2026-09-22
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）
任务手册：[`tmp/2026-09-22-rofd-xilem-refactor-and-rename-playbook.md`](../../../tmp/2026-09-22-rofd-xilem-refactor-and-rename-playbook.md)
as-built 参照：`D:/code/rword`（姐妹项目，已完成同型重构 + 改名）

## 0. 总览

参照 rword 项目已完成的改造，对 rofd 做三项有序变换，严格按 **A → B → C** 推进；每项独立提交、独立编译可测。

- **A：架构重写。** 删除 winit 桥接层（`WinitEventBridge` + `EditorApp`）；把 native 适配器重写为 masonry `Widget` + xilem `View`；`main.rs` 重写为纯 `Xilem::new_simple` 宿主。
- **B：crate 改名。** `crates/native-view`（`rofd-native-view`）→ `crates/xilem-view`（`rofd-xilem-view`）；`crates/native-app`（`native-app`）→ `crates/xilem-app`（`xilem-app`）。
- **C：类型族改名。** 界面/组件家族 `Editor* → Ofd*`（Rust + JS/TS + Vue），含 SDK JS 类 `Editor → Ofd`、`WasmEditor → WasmOfd`。

### 0.1 已敲定的决策（owner 裁决）

1. **io 分层对齐 rword**：`parse_ofd`/`save_ofd`/`PackageHandle` 调用从适配器上移到宿主 `xilem-app/src/host/document_io.rs`；xilem-view 最终依赖仅 `rofd-component + xilem`。web-view 侧不动（wasm 的宿主是 JS，无 Rust host 层）。
2. **SDK 直接改名，版本 0.1.6**：JS `Editor → Ofd`，不留过渡别名；`crates/web-view/sdk/package.json` 版本 `0.2.0 → 0.1.6`（接续已发布的 0.1.2–0.1.5）。
3. **前缀 `Ofd*`**：`OfdComponent`/`OfdConfig`/`OfdWidget`/`OfdView`/`OfdCommand`/`WasmOfd`。
4. **命名时机（owner 裁决）**：A 阶段新适配器文件**直接使用最终名**（`OfdWidget`/`OfdView`/`ofd()`/`ofd_with_config()`）；核心层在 A 阶段仍为 `EditorComponent`/`EditorConfig`，C 完成后全链一致。A 内存在有意中间态 `OfdCommand = Arc<dyn Fn(&mut EditorComponent)>`。

### 0.2 路线方案

采用**方案 1：严格复刻 rword as-built，三阶段 A→B→C，差异点最小语义适配**。

否决的备选：先改名后重写（中间态难编译难审）；借重写把 rofd 事件名/zoom 全面对齐 rword（冲击 web 侧已发布行为，超出范围）。

### 0.3 rofd 相对 rword 的差异事实（已核实，均为有意差异）

- rofd 事件名为 `PointerDown/PointerMove/PointerUp`（rword 为 `Mouse*`），坐标 f64（rword 为 f32）——web-view 依赖，不改名不改类型。
- rofd 现有 `ViewEvent` 无 `ImePreedit/ImeCommit`，只有提交式 `Ime{text}`。
- rofd 组件当前无焦点处理臂、不绘制文本光标、无 preedit、无场景缓存。
- zoom 语义为乘法累积（`viewport.zoom *= factor`），基线 `PX_PER_MM = 96/25.4 ≈ 3.78`（rword 为 1.0 基线绝对缩放）。
- 宿主需保留 `Option<PackageHandle>`（rofd io 比 rword 厚）。

---

## 1. A0：依赖升级（对齐 rword commit `5d05e32`，单独提交）

根 `Cargo.toml` `[workspace.dependencies]`：

| 依赖 | 现状 | 目标 |
|---|---|---|
| `xilem` | git rev `bf81712d44e3` | git rev **`271a27a6d4a930f7878d404f9014e3c50a3a9b88`** |
| `masonry_testing` | — | **新增**，与 xilem 同 git 同 rev（dev-dep） |
| `masonry_winit` | git rev `bf81712d44e3` | **删除** |
| `imaging` / `imaging_vello` | git rev `0eea0499…` | **crates.io `0.0.1`** |

vello 0.8 / parley 0.8 / winit 0.30 / wgpu 28 / rfd 0.15 维持。

新增仓库根 `rust-toolchain.toml`：

```toml
# Linebender git deps (xilem/masonry @ 271a27a6) declare rust-version 1.96.
[toolchain]
channel = "1.98.1"
```

A0 只做调用点最小修正（masonry `Length` 坐标等 breaking changes、imaging 0.0.1 API 差异），不删除旧架构、不做架构变更。门禁：全量 build/test/clippy/fmt，手术刀字节保留测试自然保持绿色。

---

## 2. A1 核心侧：组件层前置改造

全部落在 `crates/component`（命名在 A 阶段仍是 `Editor*`），render 少量增补。native 与 wasm 同时受益。

### 2.1 ViewEvent（event.rs）

新增：

```rust
ImePreedit { text: String, cursor: Option<(usize, usize)> },
ImeCommit  { text: String },
```

保留现有 `Ime{text}`（web-view 调用），内部与 `ImeCommit` 共用同一提交处理函数。`Pointer*` 名称与 f64 坐标不动。

### 2.2 EditorConfig（config.rs）

新增 `zoom: f64`，`new()` 默认 `PX_PER_MM`；构造 viewport 使用该字段。构造函数签名不变。

### 2.3 构造门控

`new` 收为私有；新增 `new_native` / `new_wasm`（`#[cfg(target_arch = "wasm32")]` 门控，非 feature）。当前二者函数体相同（`RenderEngine` 平台无关），门控是未来分化的接缝。web-view 唯一调用点改 `new_wasm`。

### 2.4 焦点状态

新增字段 `focused: bool`（出生 false）、`cursor_visible: bool`（初始 true）及 blink 计时字段。

- FocusGained：`focused=true`，重置 blink 锚点（光标立即可见），重绘。
- FocusLost：`focused=false`；preedit 在途则强制提交（对齐 `Ime::Disabled → FocusLost`），隐藏光标，重绘。

### 2.5 Blink

- `pub fn tick_blink(&mut self) -> bool`：native 用单调 `std::time::Instant`，500ms 翻转，仅 focused 推进，返回可见性是否变化。
- wasm 在 `update_scene` 内按帧计数推进（16ms/帧，`#[cfg(wasm32)]`），不在 wasm 触碰 `Instant`（wasm32-unknown-unknown 下会 panic）。
- **对 AGENTS §4.4 的界定**：单调 `Instant` 是动画计时器，不是挂钟；创作时间戳仍只能经 `set_clock` 注入。

### 2.6 光标绘制与 caret_rect

- 新增组件级 `crates/component/src/caret_rect.rs`：`CaretRect { x, y, width, height }`（f32，viewport 空间）。
- `EditorComponent::caret_rect(&mut self) -> Option<CaretRect>` 委托现有 `rofd_render::caret_rect(doc, vp, fonts, ann_id, offset)`；无文本光标返回 `None`。
- **新能力**：场景构建时，`focused && cursor_visible` 且有文本光标 → 填充该 rect（1px 宽已含 zoom），append 在场景末端（与 tooltip 同层）。

### 2.7 场景缓存：update_scene / scene

- 新增 `cached_scene: Option<Scene>` + `scene_dirty: bool`。变更类事件后置脏；`update_scene()` 仅脏时重建（blink 翻转也置脏：每 500ms 一次全量重建，可接受）。
- `scene(&self) -> Option<&Scene>` 供 widget paint。
- 现有 `render(&mut self, &mut RenderTarget)` 保留给 web-view；native 不再走 `RenderTarget`。
- `set_viewport_size(w, h)`：逻辑等同现有 Resize 臂（viewport.size + clamp_scroll + page change）。

### 2.8 文本编辑方法

- `paste_text(&mut self, text) -> bool`：有批注文本光标 → 走现有 `editor.insert_text(ann, offset, text)` 路径（与 Ime 臂相同）；无光标 false。body 拖选永不作为粘贴目标。
- `cut_selection(&mut self) -> Option<String>`：文本光标内有选区 → 快照文本、经既有删除路径删区（与 Backspace 同路径）、返回文本；否则 None。
- 复制取值顺序：**先批注文本光标内选区，再 body 拖选文本**（现有 `selected_text`）。

### 2.9 Zoom 归属与钳制

- 组件 Zoom 臂加钳制：`viewport.zoom` 夹在 `[PX_PER_MM×0.25, PX_PER_MM×3.0]`，常量 `MIN_ZOOM`/`MAX_ZOOM` 定义在 render `viewport.rs`。widget 只发 ×1.1/×0.9 步进；实际值经既有 `on_zoom_change` 上抛。
- wasm 同步获得钳制（此前无界），记为有意行为变更，两端一致。

### 2.10 Preedit 状态机

- 新增 `crates/component/src/preedit.rs`，移植 rword `PreeditState { text, caret: Option<(usize,usize)>, format 快照, started_with_deleted_selection }`。
- `ImePreedit`：空串=取消；非串=开始/更新；开始时若文本光标内有选区，先把"删除选区"作为**独立 undo step**；合成文本提交前绝不进入 dom。
- `ImeCommit`/`Ime`：合成中→提交 preedit 文本（一个 undo step）；无合成→现有直接插入路径。
- 强制提交/丢弃移植 `requires_force_commit`：焦点丢失→提交；`load_document`→丢弃不提交；外部变更入口先 force-commit。按 rofd 事件集合裁剪。
- **渲染简化（有意差异）**：不做 rword 的 ghost document 重排；批注文本为固定版式，preedit 以 parley/FontStore 整形后在光标处叠加绘制合成文本 + 实心合成光标（OFD 文本框裁切，接受不推挤后续文字），提交后才真正入文档。

### 2.11 组件内测试

preedit 全状态机（开始/更新/空串取消/带选区删除/只读拒绝）、提交路径、force-commit 事件表、paste/cut、焦点门控光标可见性、zoom 钳制、caret_rect 随滚动/缩放跟踪。

---

## 3. A1 适配器侧：xilem-view 三文件

A 阶段目录仍为 `crates/native-view`、包名仍 `rofd-native-view`（B 才移动改名）；内容整体重写，新文件**直接使用最终名**。

### 3.1 Cargo.toml

- dependencies：**`rofd-component` + `xilem`**（仅此两个；render/io/dom、winit、arboard 全部移除）。
- dev-dependencies：`masonry_testing`（同 xilem pin）、`rofd-dom`、`dpi`。

### 3.2 命令通道（host→widget）

```rust
pub type OfdCommand = Arc<dyn Fn(&mut EditorComponent) + Send + Sync>;
pub type OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>;
pub fn command_queue() -> OfdCommandQueue;
```

无命令 enum；闭包必须 `Fn`（按钮重复触发），克隆捕获数据。不设 rword 的 `Widget::zoom` 静态方法（rofd 保留乘法 Zoom/ZoomAt 事件，命令直达组件）。静态 API 仅：`with_component`、`copy_selection`、`cut_selection`。

### 3.3 OfdWidgetAction（widget→host，载荷 owned）

| 变体 | 对应组件回调 | 备注 |
|---|---|---|
| `Changed` | on_change | 无载荷，宿主按需经队列查文档 |
| `SelectionChanged(AnnotationSelection)` | on_selection_change | |
| `CursorChanged(Option<TextCursor>)` | on_cursor_change | |
| `SaveRequested` | on_save_request | |
| `ContextMenu { pos: (f64,f64), target: ContextTarget }` | on_context_menu | |
| `AnnotationFocus(AnnotationId)` / `AnnotationInteract(AnnotationId)` | 同名回调 | |
| `PageChanged(usize)` | on_page_change | |
| `ZoomChanged(f64)` | on_zoom_change | |
| `TextSelectionChanged(Option<BodyTextSelection>)` | on_text_selection_change | |
| `Warnings(Vec<OfdWarning>)` | on_warning | rofd 专有 |
| `PointerCursorChanged(PointerCursor)` | on_pointer_cursor | **内部消费**：转 `get_cursor` + 请求光标变更，绝不 submit |

组件 `on_copy` 不在此适配器接线（Ctrl+C/X 在 widget 内拦截）；保留给 web-view。

### 3.4 OfdWidget（移植 word_widget.rs，约 600–650 行）

- 字段：`component`、`pending: Vec<OfdWidgetAction>`、`cursor: PointerCursor`、`widget_focused`、`window_focused`（出生 true）、`component_focused`（出生 false）、`size: (f64,f64)`。无 zoom 镜像。
- `new(config)`：`EditorComponent::new_native(config)`，接线 12 个回调为 push owned action（含 on_warning；除 on_copy 与 tooltip_formatter）；构造后发初始 `ViewEvent::FocusLost`。
- 宏 `refresh_effective_focus!`：effective = widget 焦点 ∧ 窗口焦点，与 component_focused 比对，差异时发 FocusGained/FocusLost，聚焦时启动 anim frame。
- 宏 `after_touch!`：take pending → PointerCursorChanged 内部消化，其余 submit_action；按 `caret_rect()` 刷新/清除 IME area；请求重绘。
- `accepts_focus` / `accepts_text_input` → true。
- measure 返回 offered；layout 调 `component.set_viewport_size(w,h)`（f32→f64）并设置 clip path。
- paint：`component.update_scene()` → `painter.replay(component.scene())`；scene 为 None 跳过。
- `get_cursor` 由 `cursor` 字段映射；`Role::Document`。

### 3.5 事件映射（masonry_events.rs，约 215 行 + 9 单测）

移植：`rofd_modifiers`、`named_key`（Enter/Backspace/Delete/Tab/Escape/方向键/Home/End/PageUp/PageDown）、`key_down_events`、`mouse_button`、`scroll_deltas`。边界处坐标 f32→f64；`SCROLL_LINE_PX = 20.0`；PixelDelta ÷ scale_factor。

| masonry 事件 | 处理 |
|---|---|
| PointerDown | request_focus + 左键 capture_pointer + `PointerDown{..., click_count}` |
| PointerMove / PointerUp | `PointerMove` / `PointerUp` |
| Scroll，无 Ctrl | set_handled + `Scroll{dx,dy}`，**y 取反**（winit 正 y=向上；组件按 web 惯例正 y=向下，与现有 `winit_bridge.rs` 一致） |
| Scroll，Ctrl 按住 | set_handled + **`ZoomAt{factor, center:(x,y)}`**（rofd 专有增强；factor 1.1/0.9） |
| KeyDown | `KeyDown`（Ctrl 不预过滤） |
| Ctrl+C / Ctrl+X | `on_text_event` 拦截：`copy_selection`/`cut_selection`（先批注光标选区后 body 拖选）→ `ctx.set_clipboard`，不下发组件 |
| ClipboardPaste（Ctrl+V） | `component.paste_text(text)` |
| WindowFocusChange / update FocusChanged | 经 effective focus 宏；`Ime::Disabled` → FocusLost，带防御性重新获取 |
| anim frame | request_anim_frame + `tick_blink()`，true 则重绘 |

菜单 Cut/Copy/Paste 不碰剪贴板（由命令显式调静态方法或 paste 命令）。

### 3.6 OfdView（移植 word_view.rs，约 280–320 行）

- `ofd()` / `ofd_with_config()` 构造；链式 `.on_*` handler 对应 3.3 表中除 PointerCursorChanged 外全部变体（`Handler<State,Payload> = Box<dyn Fn(&mut State,Payload)+Send+Sync>`）。
- build：`ctx.with_action_widget(|ctx| ctx.create_pod(OfdWidget::new(config)))`。
- rebuild：drain 队列 → `OfdWidget::with_component`。
- message：取 `OfdWidgetAction` 分发；handled → `MessageResult::Action(())`（宿主逻辑重跑 → rebuild → drain），否则 Nop/Stale。
- `lib.rs`（约 14 行）：导出 `ofd/ofd_with_config/OfdView/OfdWidget/OfdWidgetAction/OfdCommand/OfdCommandQueue/command_queue`。

### 3.7 旧测试迁移与删除

| 旧测试 | 去向 |
|---|---|
| `c2_save.rs`（#[ignore]） | `crates/xilem-app/tests/`（宿主层保存工作流） |
| `hit_coverage.rs`、`sample_ctm_hit.rs`（#[ignore]） | `crates/render/tests/`（render hit_test/composite；dev-dep 已有 io） |
| `sample_drag_select.rs`（#[ignore]） | `crates/component/tests/`（component dev-dep 增加 rofd-io，仅测试边） |

删除 `editor_app.rs`、`winit_bridge.rs`；`EditorApp`/`WinitEventBridge` 类型消失。

---

## 4. A3：xilem-app 宿主重写

### 4.1 Cargo.toml

依赖：`rofd-native-view`（B 后为 `rofd-xilem-view`）、`rofd-component`、**`rofd-io`（升为正常依赖）**、`rofd-dom`、`xilem`、`rfd`。删除 `winit`、`masonry_winit`。

### 4.2 host/document_io.rs（约 60–80 行）

```rust
pub struct LoadedOfd {
    pub document: OfdDocument,
    pub package: Option<PackageHandle>,
    pub warnings: Vec<OfdWarning>,
}

pub fn load_ofd(path: &Path) -> Result<LoadedOfd, String>;

/// package 存在 → save_ofd（手术刀，未触碰条目字节保留）；
/// package 为 None（新建文档）→ write_ofd（全量）。字节写回目标文件。
pub fn save_ofd(document: &OfdDocument,
                package: Option<&PackageHandle>,
                path: &Path) -> Result<(), String>;
```

写入失败不改动内存状态；warnings 由 AppState 接收并呈现（至少 stderr）。

### 4.3 AppState（plain data，无锁）

```rust
struct AppState {
    commands: OfdCommandQueue,
    file: Option<PathBuf>,
    package: Option<PackageHandle>,
    modified: bool,
    has_selection: bool,
    warnings: Vec<OfdWarning>,
    context_menu: Option<ContextMenuState>,
}
```

`push(app, f)` 助手推闭包。默认装配：加载后命令 `set_clock("rofd".into(), 0)`；安装默认 tooltip formatter（`default_tooltip_lines(ann, 0)`，UTC）；默认字体为空（系统字体回退，保持现状）。取代现有 `Arc<Mutex<…>>` + AtomicBool flag 轮询。

### 4.4 文件操作

- New：命令 `new_document()`；清空 file/package/modified/menu。
- Open：FileDialog → load_ofd → 推命令 `load_document(doc)`（闭包内克隆 document）；存 package、收 warnings、清 modified、更新标题。
- Save：file 存在 → 推命令快照 `c.document().clone()` → document_io 按 package 路由 → 成功后推 `clear_modified()`；file 不存在 → Save As。
- Save As：选路径保存；成功后**不产生新 PackageHandle**（句柄只来自 parse_ofd），下次保存仍全量 write 直到重新打开。

命令全部为 `Fn`。

### 4.5 app_logic

- file_row：New / Open / Save（`!modified` 可禁用）。
- tool_row：手型 / 文本 / 高亮 / 下划线 / 删除线 / 波浪线 / 手写 / 矩形——推 `set_tool`/`apply_markup` 命令；markup 按 `has_selection` 决定可用。创建类工具不回弹。
- 编辑器：

```rust
ofd_with_config(state.commands.clone(), EditorConfig::new(Arc::new(vec![])))
    .on_change(|state| { state.modified = true; })
    .on_text_selection_change(|state, sel| state.has_selection = sel.is_some())
    .on_save_request(|state| { /* do_save 流程 */ })
    .on_context_menu(|state, ev| state.open_context_menu(ev))
    .on_warning(/* 收集 */)
    // 其余 handler 按宿主实际需要接线
```

`on_save_request`/`on_context_menu` 在 message 回调直接执行业务逻辑，删除 flag 轮询与手工 ApplicationHandler。
右键菜单 overlay 用 zstack 叠层：Annotation target 提供"删除批注"（推 `delete_annotation`）；Page/Empty 不显示删除项。

### 4.6 main()

```rust
fn main() -> Result<(), winit::error::EventLoopError> {
    let app_state = AppState::new();
    // 命令行参数：读文件 → load_ofd → 加载、存 package
    Xilem::new_simple(app_state, app_logic,
                      WindowOptions::new().with_title("rofd"))
        .run_in(EventLoop::with_user_event())
}
```

窗口标题随文件名/modified 更新。删除 canvas-origin 簿记、MessageProxy wake task、外部光标同步、手工 MasonryState/AppDriver。预期 600–800 行。

---

## 5. 转换 B 与转换 C

### 5.1 转换 B（A 全部门禁绿后，单独提交）

| 现状 | 目标 |
|---|---|
| `crates/native-view`，`rofd-native-view` | `crates/xilem-view`，`rofd-xilem-view`，lib `rofd_xilem_view` |
| `crates/native-app`，`native-app` | `crates/xilem-app`，`xilem-app` |

文件级 `git mv`（Windows 目录级可能 busy）→ 改两个 manifest + 根 Cargo.toml（members、workspace.dependencies 键、依赖路径）→ sed `rofd_native_view → rofd_xilem_view` → check/test。同步更新引用旧路径的文档与 CI。`rofd-dom/io/editor` 已发布 crate 不动。

### 5.2 转换 C 映射

**Rust 核心**：`EditorComponent`（文件 `editor_component.rs`）→ `OfdComponent`（`ofd_component.rs`）；`EditorConfig` → `OfdConfig`。
**Rust web-view**：`WasmEditor`（`wasm_editor.rs`）→ `WasmOfd`（`wasm_ofd.rs`）；`create_wasm_editor` → `create_wasm_ofd`。
**xilem-view**：A 已用终态名，C 在该 crate 无改名任务；核心改名后 `OfdCommand = Fn(&mut OfdComponent)` 全链自洽。
**SDK TS（`sdk/src/index.ts`，单文件）**：`class Editor` → `class Ofd`；`Editor.init` → `Ofd.init`；`WasmEditor` → `WasmOfd`；`EditorConfig` → `OfdConfig`；`create_wasm_editor` → `create_wasm_ofd`；示例变量 `const ofd = await Ofd.init(...)`。不留别名。`sdk/package.json` 版本 **0.1.6**；README 同步。
**Vue（仅 App.vue + main.ts）**：`import { Editor }` → `import { Ofd }`；类型与 `Editor.init` 同步；变量 `editor`/`ed` → `ofd`（全部调用点）；main.ts 注释同步。tauri-app 复用 web-app 源码，无独立面。

**保留不改名**：`rofd-editor` crate 名与核心 `Editor` 结构体；`command_queue()`；prose 普通名词 "editor"；DOM id/Symbol/e2e 选择器中的 `'editor'`（先 grep e2e，测试载重名保留）；历史文档（`docs/superpowers/`、`tmp/`）不改，需要时加 SUPERSEDED 横幅。

### 5.3 sed 纪律

1. 每模式带 `\b`；复合名先长后短（`OfdCommandQueue` 先于 `OfdCommand`；`WasmEditor`/`create_wasm_editor` 先于裸 `Editor`）。
2. **绝不对 Rust 侧裸 `\bEditor\b` 全局替换**（保留的 `Editor` 结构体）。
3. 文件改名同时检查 mod/use 路径与兄弟文件引用。
4. CJK 全角标点旁 `\b` 失效，sed 后 grep 审计手工补。
5. 每子任务后跑旧名零命中 grep，不信任清单。

### 5.4 C 的提交切分

1. 核心：`OfdComponent`/`OfdConfig` + 文件改名（全部调用点）；
2. web-view Rust：`WasmOfd`/`create_wasm_ofd` + 文件改名；
3. SDK TS + package.json 0.1.6 + README；
4. Vue 宿主；
5. 文档/AGENTS/CI 注释 + 历史横幅；
6. 终验 + 全仓旧名审计。

---

## 6. 测试策略与验证门禁

### 6.1 masonry_events 单测（9 个）

modifiers 映射、named_key 全覆盖、字符键取首字符、mouse_button、scroll_deltas 符号翻转、PixelDelta ÷ scale_factor、行 delta × SCROLL_LINE_PX。差异断言：坐标 f64；ctrl+wheel 产出 ZoomAt factor。

### 6.2 tests/ofd_widget_harness.rs（7 个无头测试）

fixture：`make_doc()` 构造最小 OfdDocument（PageModel + TextBox 批注；另一个变体带一行正文 TextObject），经命令 load_document；800×600。helpers 移植 `probe`/`click`/`wheel`/`key_down`。

| # | 测试 |
|---|---|
| 1 | 点击 TextBox 取得文本光标 → `keyboard_type_chars("hello")` → 批注内容 hello + pop_action 见 Changed |
| 2 | `double_click_selects_word_in_body_text`（替换 rword 的 header 测试）：单击正文无选区，双击（count=2）提交非空 TextSelectionChanged |
| 3 | IME：preedit 不进文档、IME area 高>0、commit 后批注文本更新 |
| 4 | 普通滚轮 Handled::Yes 且 zoom 不变 |
| 5 | ctrl+wheel 走 ZoomAt：×1.1/×0.9 累积，钳制在 `[PX_PER_MM×0.25, PX_PER_MM×3.0]` |
| 6 | 正文拖选 → Ctrl+C：Handled::Yes 且 clipboard_contents 等于所选文本 |
| 7 | 窗口焦点门控：焦聚/失焦 render 快照不同，重新聚焦还原；出生未聚焦 |

### 6.3 组件层单测

见 2.11。无 GPU、无系统字体依赖（空字体构造）。

### 6.4 迁移的 #[ignore] 测试

保持 `#[ignore]`（依赖 gitignored 真实 OFD），路径引用更新；c2_save：`cargo test -p xilem-app --test c2_save -- --ignored`。

### 6.5 阶段门禁

A0/A/B 每个提交：

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

手术刀字节保留测试（`-p rofd-io`）始终绿色。

C 的 SDK/前端提交：

```bash
cd crates/web-view/sdk && npm test && npm run build
cd crates/web-app && npm run build:sdk && npm run type-check
```

C 终态：全仓旧名零命中审计（playbook §5 的 grep，排除历史文档/dist），审计 grep 用一个已知旧名作 sanity 对照。

### 6.6 手动冒烟

`cargo run -p xilem-app`（及带参打开 `test/ru-yuan-ji-lu.ofd`）：工具栏全部按钮、ctrl+滚轮缩放、普通滚轮滚动、右键菜单删除批注、Ctrl+S、CJK 显示、IME 中文输入。web 侧 `npm run dev` 对等流程。

覆盖率沿用 AGENTS §6 的 80% 目标；渲染继续用场景结构断言，不用 GPU 快照。

---

## 7. 文档更新

### 7.1 AGENTS.md（C 终态随改名同步）

- §3 分层图/表格：native-view → xilem-view（依赖 component + xilem）；native-app → xilem-app（依赖含 io）。
- "关键偏离 spec"注：改写为两条历史偏离（component io-free；io 调用上移宿主 host/document_io，web-view 仍持 io）。
- §5：`rofd-native-view` 小节重写为 rofd-xilem-view，参照 rword "Native Word Widget" 写入 as-built 八条（命令队列、action、effective focus、IME、剪贴板、zoom、blink、滚轮）并保留警告 "If you're tempted to reintroduce a winit-layer bridge… don't"；component 小节更新新能力。
- §7 钉版表：xilem/masonry_testing → `271a27a6…`；imaging/imaging_vello → 0.0.1；删 masonry_winit；注明 rust-toolchain 1.98.1。
- §4.4 补一句：单调 Instant 可用于 blink；禁止的是挂钟。
- §1/§11：随 C 更新名称；补充 rword 为 as-built 参照。

### 7.2 README / README.zh-CN / CHANGELOG

运行命令与架构示意更新为终态名；SDK 示例 `Ofd.init` / `const ofd = ...`；native 集成示例改 ofd()/ofd_with_config() + 命令队列；CHANGELOG 增加一条（masonry 适配器、crate 更名、SDK 0.1.6 breaking）。

### 7.3 历史文档

历史 spec/plan 内容不改，需要时顶部加 SUPERSEDED 横幅；playbook 保留原样。

---

## 8. 产出物与后续

- 本 spec 提交至 `docs/superpowers/specs/`；
- 自审（占位符/一致性/范围/歧义）后由 owner 审阅；
- owner 批准后 invoke **writing-plans** skill，按 A0 → A1/A2/A3 → B → C 细化 TDD 实施计划；
- 实现代码在计划批准后开始。
