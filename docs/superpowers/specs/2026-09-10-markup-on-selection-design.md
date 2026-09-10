# markup 批注交互重设计：选中文字后按钮应用

日期：2026-09-10
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构，本文档不改变其分层与不变量）、[`2026-08-16-ofd-tools-design.md`](2026-08-16-ofd-tools-design.md)（其 §"预留选中转高亮"段落被本文档取代）

## 1. 背景与目标

高亮 / 下划线 / 删除线 / 波浪线（下称 **markup 四种**）当前是"创建工具"：
点工具栏按钮 → `Tool::Create(Markup)` → 回画布拖一个矩形 → 生成 markup 批注。
拖出的只是一个方块，不贴字，交互也不符合阅读器惯例。

**目标交互（用户裁定）**：markup 四种全部改为"**先选字、后点按钮**"——
在正文上拖选文字，然后点击工具栏按钮（或颜色面板选色）应用批注。
**不允许**保留"先点工具再回文本框选"的旧路径。

四条已裁定的交互规则：

| # | 规则 | 裁定 |
|---|---|---|
| R1 | 无选区时点按钮 | 按钮禁用（灰显），需新增 `on_text_selection_change` 回调供宿主跟踪 |
| R2 | 应用成功后选区 | 保留（可继续叠加其他 kind；点空白/切工具才清除） |
| R3 | 同选区重复应用同 kind | 叠加新建独立批注（不做 toggle） |
| R4 | 颜色面板选色 | 有选区时选色即应用；无选区时仅记住颜色 |

## 2. 现状盘点（2026-09-10 代码）

已有能力（本次复用，不重造）：

- **正文文字选区**：Text 工具拖选（单页跨行）、双击选词、三击选段，
  `BodyTextSelection { page, ranges: Vec<BodyTextRange> }`（render，
  纯 UI 状态），Ctrl+C 已接通。
- **选中转批注**：`EditorComponent::create_highlight_from_selection(color)`
  （`editor_component.rs:447`）——选区矩形 → 每行一个 quad →
  `Markup { quad_points, color }`，走 editor 命令、可撤销。仅支持 Highlight，
  成功后清选区。SDK 已暴露 `createHighlightFromSelection(color)`。
- **per-kind 颜色**：`set_markup_color(kind, color)` 四种各自配色。
- **Markup 渲染**：四 kind 按 quad 对（tl/br）画高亮块/底线/中线/波浪线。

废弃路径（本次删除）：

- `parse_tool_kind` 将 `"highlight"/"underline"/"strikeout"/"squiggly"` 映射到
  `Tool::Create(...)`（web SDK + native-app 工具栏同源）。
- `build_create_payload` 的 Markup 分支（拖矩形造 quad）。
- `create_highlight_from_selection`（被泛化方法取代）。

## 3. 方案选择

- **A（采纳）：动作化**——markup 从 `Tool` 中移除，成为文字选区上的命令；
  `Tool::Create` 载荷收紧为新枚举 `CreateKind { Shape(ShapeKind), Freehand }`，
  markup 在类型层面无法成为工具。"markup 不可先点后框选"由编译器保证。
- **B（否决）：保留 `Tool::Create(markup)`，仅工具栏改调命令**——旧路径仍在，
  不变量只靠约定，两套创建路径并存。
- **C（否决）：SDK 层 pending kind，下次选区完成自动应用**——是被禁交互的镜像，
  且状态机藏进适配器层，违反 §4.9 功能内聚。

## 4. 架构与分层变更

```
dom       ── 不变（AnnotationKind、AnnotationPayload::Markup 原样）
render    ── 不变（text_selection_rects、Markup 渲染已有）
editor    ── 不变（create_annotation 命令、Transaction/History 复用）
component ── 核心变更：① Tool/CreateKind 类型收紧；② apply_markup 泛化；
             ③ text_selection 赋值收口 + on_text_selection_change 回调
web-view  ── SDK：applyMarkup / hasTextSelection / setOnTextSelectionChange；
             parse_tool_kind 删 markup 分支
web-app   ── 按钮动作化 + 禁用态 + 选色即应用（tauri-app 前端复用自动继承）
native-view + native-app ── EditorApp 装配回调；动作按钮 + 禁用态
io        ── 不动（手术刀保存零影响：Markup 序列化路径未变）
```

不变量对齐：状态机与命令全部落在 component 及以下（§4.9）；适配器只做绑定；
body 只读（markup 仅新增批注）；库无平台依赖新增。

### 4.1 类型收紧

```rust
/// 可作为"创建工具"拖画的批注种类。markup 故意不在其中：
/// markup 是文字选区上的命令（apply_markup），不是工具。
pub enum CreateKind { Shape(ShapeKind), Freehand }

pub enum Tool { Text, Create(CreateKind), Hand }
```

`DragState::Create { kind: CreateKind, ... }`、`build_create_payload(&CreateKind, ...)`
同步改签名；后者删除 Markup 分支。

### 4.2 text_selection 赋值收口

`text_selection` 现有约 10 处直接赋值，收进私有 helper：

```rust
fn set_text_selection(&mut self, sel: Option<BodyTextSelection>)
```

内部 `PartialEq` 比较，**值变化才**触发 `on_text_selection_change`。这是该回调的
唯一出口，防漏发/重发。

## 5. API 设计

### 5.1 component（Rust）

```rust
/// 把当前文字选区转为 markup 批注（每行一个 quad，贴字）。
/// 仅接受 Highlight/Underline/Strikeout/Squiggly，其余返回 None。
/// 颜色取 per-kind 配置（set_markup_color）。
/// 成功后保留文字选区（可继续叠加）。一个 Transaction = 一次 undo。
pub fn apply_markup(&mut self, kind: AnnotationKind) -> Option<AnnotationId>

pub fn has_text_selection(&self) -> bool
pub fn on_text_selection_change(&mut self, cb: impl Fn(Option<&BodyTextSelection>) + 'static)
```

`apply_markup` 成功后的副作用清单：`after_annotation_change()`（该页
annotation_scene 失效 + `on_change`）、`fire_selection_change()`；
**不**清选区、**不**触发 text_selection_change、**不**选中新批注
（互斥规则：选批注会清文字选区，见 §6）。

删除（breaking）：`create_highlight_from_selection(color)`。

### 5.2 SDK（TS，版本 bump 0.2.0）

```ts
applyMarkup(kind: 'highlight' | 'underline' | 'strikeout' | 'squiggly'): string | null
hasTextSelection(): boolean
setOnTextSelectionChange(cb: () => void): void   // 无参触发，宿主再查询

// 删除：createHighlightFromSelection(color)
// parse_tool_kind：移除 4 个 markup 分支，"highlight" 等字符串落入默认 Tool::Text
```

回调无参、宿主查询的约定与既有 `onSelectionChange` 一致。

### 5.3 宿主（App.vue / native-app）

- 4 个按钮：`:disabled="!hasTextSelection"`，点击 → `applyMarkup(kind)`；
  不再参与 `activeTool`（从切换型变瞬时动作型按钮）。
- 颜色面板选色：`setMarkupColor(kind, color)` → 有选区立即 `applyMarkup(kind)`，
  无选区仅存色；不再 `setTool`。
- tooltip 文案更新为"选中文字后点击"。
- native：native-view `EditorApp` 装配 `on_text_selection_change` → 更新共享
  标志位 + wake proxy 重绘 → xilem 按钮据此 disabled（接线细节在 plan 细化）。
- `ToolButton.vue` 增加 disabled 支持。

## 6. 交互规则与数据流

### 6.1 文字选区生命周期

| 事件 | 选区 | on_text_selection_change |
|---|---|---|
| Text 工具拖选 / 双击选词 / 三击选段 | 形成并实时更新 | ✅ 值变化时 |
| 拖回锚点（零宽） | 清除 | ✅ |
| 点画布空白 | 清除 | ✅ |
| 点任何批注（含 markup，互斥 spec §5.2） | 清除 | ✅ |
| 切工具 / 加载新文档 | 清除 | ✅ |
| **apply_markup 成功** | **保留** | ❌（值未变） |
| undo / redo / zoom / scroll | 保留 | ❌（逻辑区间，矩形每帧现算） |

关键机制：点击工具栏按钮发生在 canvas 之外，pointer 事件不进 component，
选区天然存活——无需"选区粘滞"补丁。

### 6.2 叠加与撤销语义

- 同一选区可连续 apply 多个 kind / 同 kind 多次（R3）→ 每次一条独立批注、
  一条独立 undo；undo 只回滚最后一次 apply，选区不受 undo 影响。
- `apply_markup` 后新批注不处于选中态（Delete 不会误删）。

### 6.3 一次完整操作的数据流

```
拖选正文 → PointerMove → hit_test_body_text → body_text_ranges_between
        → set_text_selection(Some) → 回调 → 宿主启用 4 按钮
点击"下划线" → SDK applyMarkup → component.apply_markup(Underline)
        → text_selection_rects（每行矩形）→ viewport_to_page_local（tl/br 对）
        → editor.create_annotation → Transaction 入 History
        → on_change(page) → 该页 annotation_scene 失效 → 重绘
        → 选区保留 → 按钮仍启用（可继续叠加）
点画布空白 → set_text_selection(None) → 回调 → 按钮禁用
```

## 7. 错误处理

- `apply_markup → None` 三种情况（无选区 / 非 markup kind / 选区矩形为空）：
  **零副作用**——不建批注、不入历史、不触发任何回调。属宿主层误用
  （按钮本应禁用），`Option` 语义足够，不进 `OfdWarning` 降级体系（§4.6：
  那是文档解析问题）。
- SDK 边界：`kind` 字符串非法 → 返回 `null`（复用 `parse_markup_kind`）。
- 老宿主兼容：`setTool("highlight")` 等字符串落入默认 `Tool::Text`，静默降级
  不 panic；CHANGELOG 说明 breaking（SDK 0.2.0）。
- undo/redo 可逆性沿用 Insert/Delete step 既有 apply/revert 测试模式。

## 8. 测试策略（TDD，先红后绿）

component 单测（核心，全部经 `ViewEvent` 驱动的真实拖选构造选区，不手工塞）：

1. 四种 kind 各 apply：quad 数 = 选区行数、颜色 = per-kind 色、kind 正确。
2. apply 成功后 `text_selection` 仍 `Some` 且回调未触发。
3. 无选区 → `None` + `history_len` 不变。
4. 非 markup kind（如 Note）→ `None`。
5. undo 移除批注且选区保留；redo 恢复。
6. 同选区连 apply 两次 → 2 条批注、`history_len == 2`。
7. 一次 apply 恰好 1 条 Transaction（`history_len == 1`）。
8. `set_text_selection` 收口：形成时 fire、赋相同值不 fire、清除 fire。
9. `set_tool` 清选区并 fire。
10. 类型收紧回归：`build_create_payload` 删 Markup 分支后 Shape/Freehand
    创建测试仍绿。

web-view：`parse_tool_kind("highlight") == Tool::Text`（纯 Rust 测试，native 跑）。

宿主手动验收（web + native 各一遍）：拖选 → 按钮变亮 → 点下划线 → 贴字下划线
出现 → 选区仍在 → 再点高亮 → 叠加 → Ctrl+Z 逐步撤销 → 点空白按钮禁用 →
保存重开批注保留。

回归：手术刀字节保留测试（io）自动覆盖；`cargo clippy --workspace --all-targets
-- -D warnings`、`cargo fmt --all -- --check` 干净。

## 9. 变更文件清单

| 文件 | 变更 |
|---|---|
| `crates/component/src/editor_component.rs` | `CreateKind`、`apply_markup`、`set_text_selection` 收口、回调、删 `create_highlight_from_selection`、改写旧测试 |
| `crates/component/src/lib.rs` | 导出 `CreateKind` |
| `crates/web-view/src/wasm_editor.rs` | `applyMarkup` / `hasTextSelection` / `setOnTextSelectionChange`、`parse_tool_kind` 收紧、删 `createHighlightFromSelection` |
| `crates/web-view/sdk/src/index.ts` | TS 接口同步 |
| `crates/web-app/src/App.vue` | 按钮动作化、禁用态、选色即应用 |
| `crates/web-app/src/components/ToolButton.vue` | disabled 支持 |
| native-view `EditorApp`（装配回调） | on_text_selection_change → 标志位 + 重绘 |
| `crates/native-app/src/main.rs` | markup 按钮改动作按钮 + 禁用态 |
| 本 spec | 注明取代 2026-08-16 spec 的"预留选中转高亮"段落 |
