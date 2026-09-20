# 批注悬停 tooltip（作者 + 时间）设计

日期：2026-09-18
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）、[`2026-09-14-scrollbar-and-scroll-bounds-design.md`](2026-09-14-scrollbar-and-scroll-bounds-design.md)（场景内绘制 UI chrome 的先例）

## 1. 背景与问题

用户诉求：鼠标悬停在批注上时，光标右下角悬浮显示该批注的**作者**与**创建时间**。

代码现状（已核实）：

- **数据就绪。** dom 的 `Annotation` 已有 `creator: String`、`created / modified: i64`，
  `created` 为 epoch **毫秒**（io 解析测试锚定 `1_783_641_600_000` = 2026-07-10 UTC）。
- **悬停判定就绪。** `EditorComponent::handle_event` 的 `PointerMove` 无拖拽分支
  已在调 `rofd_render::hit_test`（用于切换悬停光标），返回的
  `HitTarget::Annotation(id) / AnnotationText(id, _) / Handle(id, _)` 可直接拿到批注 id。
- **场景内画 UI chrome 有先例。** 滚动条及其悬停高亮就是 component 在
  `build_scene` 末尾追加绘制的视口坐标 chrome。
- **平台信息宿主注入有先例。** `set_clock(author, ts)`（AGENTS §4.4：库不取系统时间）。
- **文本机械就绪。** 批注 TextBox 文本已走 render `text/shape`（parley）shaping 路径。
- **时区能力受限。** workspace 的 chrono 为 `std` features（无 `clock`，刻意不引入
  `SystemTime::now()`），故 native 侧只有 UTC 数学；web 侧有 js-sys 可取本地时区偏移。

## 2. 目标与非目标

### 2.1 目标

1. 悬停任意批注（含其文本区与选中手柄）→ **立即**显示 tooltip：两行
   （第 1 行作者、第 2 行创建时间），锚定光标右下角 `+16px` 偏移、**跟随光标移动**；
   右/下边缘放不下时翻转到光标左上（防遮挡防裁切）。
2. 时间格式 `"YYYY-MM-DD HH:MM"`（分钟粒度）：web 默认**本地时区**（js-sys 偏移），
   native 默认 **UTC**（chrono 无 clock；宿主可注入 formatter 覆盖成本地化实现）。
3. 功能全部落在核心层（render + component），web/tauri/native 三端由同一个
   `imaging::record::Scene` 自动获得；适配器只做 formatter 默认装配
   （Ctrl+C→系统剪贴板的默认装配同款模式，AGENTS §4.9），宿主零配置。
4. 抑制条件：拖拽中、指针在滚动条 chrome 上、正在编辑该批注文本
   （`text_cursor` 落在其中）时不显示。

### 2.2 非目标（v1 不做，YAGNI）

- 修改时间、批注内容预览、回复链（`reply_to`）展示。
- 延迟显示（如 500ms）与淡入淡出动画：需要宿主时钟/定时器驱动，违背库不取
  系统时间的边界（与滚动条 spec"按住连发不做"同一结论）。
- 点击固定（pin）、富文本 tooltip、多批注合并展示。
- 平台原生 tooltip（DOM `title` / masonry tooltip widget）：样式不可控且两端不一致。
- JS 侧自定义 formatter API（只提供 `setTooltipEnabled` 开关；宿主自定义 UI 的
  需求出现再加 `on_annotation_hover` 回调）。
- a11y 暴露（场景内绘制不进平台无障碍树，与滚动条一致）。
- 文本折行/宽度裁剪：作者名与时间串都很短，边缘翻转已兜底。

## 3. 设计

### 3.1 悬停状态机（component 层）

`EditorComponent` 新增字段 `hover: Option<HoverState>`：

```text
struct HoverState {
    ann: AnnotationId,       // 悬停的批注
    pos: (f64, f64),         // 最近一次光标位置（视口逻辑坐标），tooltip 锚点
}
```

维护点：`PointerMove` 的**现有无拖拽悬停分支**（悬停光标逻辑处，那里已调
`hit_test`，顺手复用同一次命中结果）：

- 命中 `Annotation(id) | AnnotationText(id, _) | Handle(id, _)` → `hover = Some{ id, pos }`；
  命中 `Page / Empty` → `hover = None`。
- 每次有效 move 更新 `pos`（tooltip 跟随光标），hover 期间 `needs_repaint = true`
  （web 走 rAF 无感；native 本就按 `needs_repaint` 重画，批注区域小、代价可接受）。
- **清除时机**：
  - `PointerDown`（任意键）即清空——按下意味着交互开始，tooltip 让位；
    拖拽期间的 `PointerMove` 不进悬停分支，不重建 hover；
  - 指针移入滚动条 chrome 的 move（现有提前 return 分支）清空后返回；
  - `set_tool` / `load_document` / `new_document` 顺手清（与滚动条 hover 重置同款）。
- **自愈**：`build_scene` 绘制时 `annotations.find(&hover.ann)` 拿不到（批注被删除、
  undo、换文档）→ 跳过绘制。不设专门的失效钩子，find 语义天然兜底。
- **编辑抑制**：`text_cursor` 的 annotation == 悬停批注（正在编辑其文本）时不画
  ——避免 tooltip 遮挡正在输入的文字。

### 3.2 文本内容：formatter 宿主注入 + 纯函数格式化

- component 新增 API：

  ```text
  set_tooltip_formatter(Option<Box<dyn Fn(&Annotation) -> Vec<String>>>)
  // native 版 + Send bound，wasm 版不加 —— 与 on_selection_change 等回调
  // 同款 #[cfg] 双版本模式
  ```

  返回行数组；`None` 或返回空 vec → 不绘制（干净降级，同时就是关闭开关）。
- 新增**纯函数**（放 component，UI 文案格式化不属几何，不进 render）：

  ```text
  format_tooltip_datetime(epoch_ms: i64, tz_offset_minutes: i32) -> String
  // → "YYYY-MM-DD HH:MM"，纯数学（civil-from-days），零新依赖（不引 chrono），
  // 正/负偏移、闰年、月末正确性由单测锚定

  default_tooltip_lines(ann: &Annotation, tz_offset_minutes: i32) -> Vec<String>
  // → ["作者：{creator}", "创建时间：{format_tooltip_datetime(...)}"]
  // 带标题前缀的默认两行（2026-09-20 增补）；文案单点维护，适配器只注入时区
  ```

- **适配器默认装配**（Ctrl+C→剪贴板默认装配同款）：
  - native-view `EditorApp::new`：装 `|ann| default_tooltip_lines(ann, 0)`（UTC）；
  - web-view `WasmEditor` 构造：同上，但偏移取
    `js_sys::Date::new(&JsValue::NULL).get_timezone_offset()` **取负**
    （JS 返回的是 UTC−local 分钟数，如 UTC+8 返回 −480）→ 本地时间。
- **开关**：`EditorApp::set_tooltip_enabled(bool)` / SDK `setTooltipEnabled(bool)`
  ——`true` 重装默认 formatter，`false` 置 `None`。

### 3.3 绘制（render 层新增 `tooltip.rs`，场景最顶层 chrome）

`build_scene` 末尾（滚动条**之后**，最顶层）追加一次
`paint_tooltip(&mut scene, lines, cursor, viewport_size, …shaping 上下文…)`：

- **视口坐标系（逻辑 px），不随文档 zoom 缩放**——与滚动条、选中手柄同为屏幕空间
  chrome；复用批注文本渲染的 shaping 机械与字体获取路径。
- 常量（逻辑 px）：

  | 常量 | 值 | 含义 |
  |---|---|---|
  | `CURSOR_OFFSET` | 16.0 | 卡片左上角相对光标的偏移 |
  | `PADDING` | 8.0 | 卡片内边距 |
  | `LINE_GAP` | 4.0 | 两行间距 |
  | `FONT_SIZE` | 12.0 | UI 字号（两行同号） |
  | `RADIUS` | 4.0 | 圆角半径 |

- 配色（浅色卡片，扁平）：

  | 元素 | 颜色 |
  |---|---|
  | 背景 | `#FFFFFF`，不透明度 0.94 |
  | 边框 | `#C9CDD4`，1px |
  | 文字 | `#333840`（两行同色同号，作者行不加粗） |

- 几何：shaping 测两行宽高 → 卡片尺寸 = 文本尺寸 + `2*PADDING`；
  锚点 = `cursor + (CURSOR_OFFSET, CURSOR_OFFSET)`；右越界
  （`anchor.x + w > viewport_w`）→ x 翻到 `cursor.x − CURSOR_OFFSET − w`；
  下越界同理翻上。允许同时翻转（光标在右下角时）。
- 圆角矩形用 path fill（批注 Shape 已有 path 机械；滚动条 spec 的"只有
  fill_rect"是当时扁平风格的选择，不构成 API 限制）。
- 文字用已注册 UI 字体走 parley shaping；**未注册任何字体 → 静默跳过整个
  tooltip**（纯 UI 降级，不 fatal、不 `OfdWarning`，AGENTS §4.6 精神）。
- 每帧重画、无缓存（hover 是瞬态，与滚动条 chrome 同策略，不触碰
  body_scene/批注页缓存）。

### 3.4 光标与交互不变量

- 悬停光标逻辑不动（批注上仍是普通箭头）；tooltip **纯展示**：不参与
  `hit_test`/`hit_scrollbar` 命中、不吞事件、不落 undo（view-only）。

### 3.5 数据流

```text
适配器(pointermove → 视口逻辑坐标)
  → EditorComponent::handle_event(PointerMove)
      无拖拽 → hit_test → hover 状态更新 → needs_repaint
  → 宿主重画 → build_scene 末尾:
      hover 有效 且 formatter 已装 且 非编辑抑制 且 批注仍存在
      → formatter(&ann) → render::paint_tooltip → scene 最顶层
```

dom / editor / io 零改动；分层不变量全保住：tooltip 逻辑内聚 component 及以下，
适配器只默认装 formatter（AGENTS §4.9）。

## 4. 错误处理

纯 f64 几何 + 纯函数格式化，不引入任何 fallible 路径，不产生 `OfdError`/`OfdWarning`：

- 悬停批注消失（删除/undo/换文档）→ `find` 不到自愈跳过；
- formatter 未装 / 返回空 vec / 全空行 → 不画；
- 未注册字体 → 跳过整个 tooltip。

## 5. 测试计划（TDD，先红后绿，场景结构断言非像素快照）

**rofd-component**

- `format_tooltip_datetime` 纯函数：UTC 基准、正/负偏移、跨日/跨月/跨年、闰年 2 月。
- hover 进入 / 离开 / 切换批注：状态正确且 `needs_repaint = true`；同批注内移动
  更新 `pos`。
- `PointerDown` 清空；拖拽期间的 move 不建立 hover；滚动条 chrome 上的 move 清空。
- 编辑抑制：`text_cursor` 落在悬停批注内 → 不画。
- 删除 / undo 掉悬停批注后 `build_scene` 不含 tooltip（自愈）。
- formatter 为 `None` / 返回空 vec → 不画；`set_tooltip_enabled(true/false)`
  装卸生效。

**rofd-render**

- `paint_tooltip` 结构断言：背景 path 1 个 + 每行各 1 组 glyph；锚点 =
  `cursor + offset`；右/下越界翻转后的坐标正确；空 `lines` 零 draw。

**手动验收（web + tauri + native）**：悬停各类批注（高亮/手绘/形状/便签/文本框）
显示作者 + 时间；跟随光标；窗口右/下边缘翻转；拖拽与文本编辑时消失；三端一致。

## 6. 受影响文件一览

| 文件 | 改动 |
|---|---|
| `crates/render/src/tooltip.rs` | **新增**：常量、`paint_tooltip` |
| `crates/render/src/lib.rs` | 导出新模块 |
| `crates/component/src/tooltip_text.rs` | **新增**：`format_tooltip_datetime` 纯函数 |
| `crates/component/src/lib.rs` | 导出新模块 |
| `crates/component/src/editor_component.rs` | `hover` 状态机、`set_tooltip_formatter`、`build_scene` 末尾追加绘制、清除时机 |
| `crates/native-view/src/editor_app.rs` | `new` 默认装 UTC formatter；`set_tooltip_enabled` 透传 |
| `crates/web-view/src/wasm_editor.rs` | 构造默认装本地时区 formatter；`setTooltipEnabled` |
| `crates/web-view/sdk/README.md` | 文档补 `setTooltipEnabled` |

web-app / native-app / tauri-app / dom / editor / io：无功能改动（宿主零配置）。
