# 滚动边界封死 + 经典常驻滚动条设计

日期：2026-09-14
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）、[`2026-08-16-ofd-tools-design.md`](2026-08-16-ofd-tools-design.md)（P1 手型工具引入 `clamp_scroll`）

## 1. 背景与问题

用户诉求两条：

1. 编辑器区域要有**滚动条**（垂直 + 水平，可拖拽）。
2. 第一页顶部不能无限往上滚、末页底部不能无限往下滚（x 轴同理不能无限平移）。

代码现状（已核实）：

- **边界 clamp 只做了一半。** `rofd-render` 已有唯一的共享函数
  `clamp_scroll(doc, vp) -> (f64, f64)`（`crates/render/src/viewport.rs`），但
  **只有手型 Pan 拖拽调用它**。下列路径都不 clamp，是"无限滚动"的根源：
  - 滚轮 `ViewEvent::Scroll`（`editor_component.rs:1039`，直接 `+= dx/dy`）；
  - `ViewEvent::ScrollPage`（PageUp/PageDown、工具栏上一页/下一页、自动翻页）；
  - `ViewEvent::Zoom` / `ZoomAt`（缩放入口、Ctrl+滚轮、工具栏缩放、显示比例）；
  - `ViewEvent::Resize`（窗口/容器尺寸变化后不重 clamp）；
  - `load_document` / `new_document` 不重置滚动位置，旧文档的 scroll 残留到新文档。
- **滚动条完全不存在。** web/tauri 是铺满 `.canvas-wrap`（`overflow: hidden`）的
  WebGPU canvas；native 是 masonry canvas。没有滚动位置回调、没有可编程滚动入口。

## 2. 目标与非目标

### 2.1 目标

1. 所有滚动/缩放/尺寸/换文档路径统一经 `clamp_scroll` 收口，视口永远停在合法范围；
   滚轮在边界处的多余 delta 被截掉（不累积、不回弹）。
2. 经典**常驻**滚动条（垂直 + 水平，内容超出才出现），占用视口边缘槽位：
   - 拖拽滑块绝对映射滚动；点击轨道翻一屏；悬停/按下有视觉反馈；
   - 任何工具（手型/文本/批注创建）下滚动条交互优先，事件不下沉到页面；
   - 滑块上方显示双向箭头光标。
3. 功能全部落在核心层（render + component），web/tauri/native 三端由同一个
   `imaging::record::Scene` 自动获得；适配器只加两个光标字符串/图标的映射，
   **SDK 公共 API 面零新增**（AGENTS §4.9：默认开箱即用、宿主零配置）。

### 2.2 非目标（v1 不做，YAGNI）

- 悬浮自动隐藏 / 淡出动画（需要时钟驱动；库不取系统时间，AGENTS §4.4）。
- 轨道按住连发（v1 点一下翻一屏）。
- SDK 的 `scrollTo` / 编程式滚动 API、滚动位置回调。
- 触屏惯性滚动、Home/End/空格键滚动、工具栏"首页/尾页"开放。
- 改变既有上下留白的轻微不对称语义（顶部一个 `page_gap` 留白、末页底边贴边，
  与已上线的 Pan 行为一致）。
- 改变滚轮 delta 换算（LineDelta/PixelDelta 的适配器换算不动，只加 clamp）。
- a11y 语义（场景内绘制的滑块不暴露给平台无障碍树）。

## 3. 设计

### 3.1 内容区尺寸与溢出判定（render 层）

新增 `crates/render/src/scrollbar.rs`（纯函数 + 常量，平台无关、无时钟）。

常量（设备像素，不随 zoom 缩放，与选中手柄同为屏幕空间）：

| 常量 | 值 | 含义 |
|---|---|---|
| `SCROLLBAR_THICKNESS` | 12.0 | 滚动条槽位厚度 |
| `THUMB_INSET` | 2.0 | 滑块在槽内两侧留白 |
| `THUMB_MIN_LEN` | 24.0 | 滑块最短长度 |
| `TRACK_PAGE_RATIO` | 0.9 | 点轨道翻一屏 = 内容区 ×90%（10% 重叠） |

内容尺寸（px，与 `composite::page_origin` 的布局公式同源）：

- `content_w = max(page.physical_box.w) * zoom`（最宽页）。
- `inner_h = sum(page.physical_box.h) * zoom + page_gap * (n-1)`；
  纵向内容总长 `content_h = page_gap + inner_h`（含首页顶部的一个 gap 留白，
  与 `clamp_scroll` 现有 `y_max` 公式一致）。

**两段式溢出判定**（处理"出竖条 → 横条也被挤出来"的经典边界）：迭代到稳定，
至多两轮：

1. 初始内容区 = 视口全尺寸 `(W, H)`，按 `content_w > cw`、`content_h > ch`
   判两轴是否需要条；
2. 扣除已判定条的厚度（竖条占宽、横条占高）后复核两轴；直到判定不再变化。

产出（Copy 小结构）：

```text
struct ScrollbarLayout {
    content_size: (f64, f64),           // 扣除条槽后的内容区尺寸
    vertical: Option<BarGeom>,
    horizontal: Option<BarGeom>,
}
struct BarGeom {
    track: Rect,        // 视口坐标内的槽矩形
    thumb: Rect,        // 当前滑块矩形（已按 scroll 定位）
    axis: Axis,         // Vertical | Horizontal
}
```

槽矩形（视口坐标，原点左上）：

- 竖槽：`(W - T, 0, T, content_h_region)`，即高度扣除横条厚度；
- 横槽：`(0, H - T, content_w_region, T)`；
- 两轴同时出现时右下角 `(W-T, H-T, T, T)` 为角块（槽底色，不响应）。

滑块几何：

- 厚度 = `T - 2*THUMB_INSET`，槽两侧各留 `THUMB_INSET`。
- 长度比例 = `(内容区长 / 内容总长).clamp(THUMB_MIN_LEN/轨道长, 1.0)`。
- 沿条方向（条长 `L`、滑块长 `m`）两端对称留 `THUMB_INSET`，
  行程 `travel = L - m - 2*THUMB_INSET`，
  滑块起端（槽内局部坐标）= `THUMB_INSET + fraction * travel`。
- 纵向 `fraction = scroll.1 / y_max`，
  其中 `y_max = max(0.0, content_h - content_size.1)`；
- 横向：`x_margin = (content_w - content_size.0) / 2`（页窄时为 0、无横条），
  `fraction = (scroll.0 + x_margin) / (2 * x_margin)`。

绘制与拖拽共用上述 `fraction ↔ thumb 起端` 公式（方向互逆），保证"抓住的点
始终贴在指针下"不会有一像素漂移。
- 空文档：两轴均为 `None`（与现有 `clamp_scroll` 空文档钉 0 一致）。

### 3.2 `clamp_scroll` 改为基于内容区

`clamp_scroll(doc, vp)` 的签名不变（所有现有调用点不动），内部改调
`scrollbar.rs` 的布局函数，以**扣除条槽后的内容区尺寸**替换现在的裸
`vp.size`，边界公式不变：

- `y ∈ [0, max(0, content_h - content_size.1)]`；
- `x ∈ [-x_margin, x_margin]`，`x_margin = max(0, (content_w - content_size.0)/2)`；
  内容比内容区窄时钉 0（页面居中，与现状一致）。

几何公式单一来源：`page_origin` 负责布局、`scrollbar.rs` 负责内容尺寸/条几何、
`clamp_scroll` 只做 clamp，三者共享同一组 content_w/content_h 定义，不得各写一份。

### 3.3 所有滚动入口统一收口（component 层）

`EditorComponent::handle_event` 中每一条修改 `scroll/zoom/size` 的路径，
变更后一律 `self.viewport.scroll = clamp_scroll(doc, &self.viewport)`：

| 路径 | 改动 |
|---|---|
| 手型 Pan | 已 clamp，保持 |
| `Scroll`（滚轮） | `+=` 后 clamp（核心修复） |
| `ScrollPage` | 加 delta 后 clamp |
| `Zoom` | zoom 变更后 clamp（边缘为保持视口合法，锚点可能偏差数像素，标准行为） |
| `ZoomAt` | 锚点滚动算完后 clamp（同上） |
| `Resize` | 更新 size 后 clamp（窗口缩小不会停在空白区） |
| `load_document` / `new_document` | scroll 重置为 `(0,0)`，并清除滚动条 hover/active 态；zoom 保持（用户选定的显示比例跨文档保留） |

加一个私有小助手 `fn apply_scroll_delta(&mut self, dx, dy)`（加 delta → clamp →
`maybe_fire_page_change()`）收口滚轮/翻页/轨道翻屏三条路径，避免三处各写一遍。

### 3.4 绘制（render 层，场景最顶层 chrome）

- **不改 `RenderEngine::composite(...)` 的签名**（调用点与测试很多）。
  `EditorComponent::build_scene` 拿到 composite 的 Scene 后，追加一次
  `paint_scrollbars(&mut scene, &layout, visual)`；条永远画在页面、选中手柄、
  拖框预览之上。
- `visual: ScrollbarVisual { hover: Option<Axis>, active: Option<Axis> }`，
  Copy 结构，由 component 的悬停态与当前 `DragState::ScrollThumb` 派生。
- 配色（经典 Windows 风，扁平、无圆角——Painter 只有 `fill_rect`）：

| 元素 | 颜色 |
|---|---|
| desk 底色（现状） | `#E0E0E0` |
| 槽 / 角块 | `#F1F1F1`，靠内容一侧 1px 边线 `#D9D9D9` |
| 滑块（默认） | `#C1C1C1` |
| 滑块（悬停） | `#A8A8A8` |
| 滑块（按下/拖拽） | `#8C8C8C` |

### 3.5 交互（component 层，任何工具下优先）

新增命中函数（render 层纯几何，与 `hit_test`/`handles` 同模式）：

```text
enum ScrollbarHit {
    VerticalThumb, HorizontalThumb,
    VerticalTrack { page_up: bool },     // 点击在滑块上方/下方
    HorizontalTrack { page_left: bool }, // 点击在滑块左方/右方
    Corner,
}
fn hit_scrollbar(layout, point) -> Option<ScrollbarHit>
```

**PointerDown 路由最前面先做 chrome 命中**（先于 `hit_test`、先于工具分派）：

- 命中滑块：进入新的
  `DragState::ScrollThumb { axis, grab_offset }`
  （`grab_offset` = 按下时指针相对滑块起端的距离，沿条方向），
  事件消费（`needs_repaint=true`），不清选区、不起 Pan、不命中批注。
- 命中轨道：立即滚动一屏
  （纵向 ±`TRACK_PAGE_RATIO * 内容区高`，横向 ±`...宽`），经 `apply_scroll_delta`
  收口（自带 clamp + 页码回调）。v1 不连发。
- 命中角块：消费事件，无动作。
- 滚动条区域的事件一律**不下沉**：条下面的批注点不到、拖不起，也不会起文本选区。

**PointerMove**：

- 拖拽中（`ScrollThumb`）：绝对映射——抓住的滑块点始终贴在指针下。
  - 纵向：滑块起端 = `pointer_y_track_local - grab_offset`；
    `fraction = (起端 - THUMB_INSET) / travel`（clamp 0..=1）；
    `scroll.1 = fraction * y_max`，再经 clamp_scroll 收口。
  - 横向：`fraction` 同理映射到 `[-x_margin, x_margin]`。
  - view-only：不改文档、不入 undo 历史；每步 `maybe_fire_page_change()`，
    状态栏页码/`onPageChange` 自动正确。
- 非拖拽：先做 chrome 命中，悬停在滑块上时请求滚动条光标；否则回落现有
  工具光标逻辑（Grab/Text/Default）。
- PointerUp：结束 `ScrollThumb`，清除 active 态，光标按当前悬停重算。
- 滚轮在滚动条上方照常滚文档（wheel 不做命中判定、不被吞）。

### 3.6 光标

`PointerCursor`（`crates/component/src/callbacks.rs`）新增两变体：

```text
ResizeV,  // ↕ 纵向滑块上
ResizeH,  // ↔ 横向滑块上
```

优先级：拖拽中或悬停滑块 > 工具光标。适配器映射（仅有的两处适配器改动）：

- **web**：`crates/web-view/src/wasm_editor.rs` 的 `pointer_cursor_str` 加
  `ResizeV => "ns-resize"`、`ResizeH => "ew-resize"`（字符串直通
  `canvas.style.cursor`，SDK TS 侧零改动），同步其单测。
- **native**：宿主样例 `crates/native-app/src/main.rs` 的光标 match 加
  `ResizeV => CursorIcon::RowResize`、`ResizeH => CursorIcon::ColResize`。
  winit bridge 不感知滚动条，零改动。

### 3.7 绘制脏缓存

滚动条状态（scroll/hover/active）每帧随 Scene 重绘即可：现有 body_scene 稳定缓存、
批注 overlay 按页失效的策略不受影响——滚动条只在 `build_scene` 末尾追加绘制，
不触碰缓存子场景（AGENTS §4.5：CTM/滚动烘焙进各 draw call，不缓存变换子场景）。

## 4. 错误处理

纯 f64 几何，不引入任何 fallible 路径，不产生 `OfdError`/`OfdWarning`：
空文档/内容不溢出时该轴 `None`；所有分母（`y_max`、`2*x_margin`、
`track_len - thumb_len`）使用前判 0，退化到不绘制/钉 0。

## 5. 测试计划（TDD，先红后绿，场景结构断言非像素快照）

**rofd-render（新 `scrollbar.rs` 单测 + composite 结构断言）**

- 不溢出：单页短文档两轴皆 `None`，场景里没有滚动条矩形。
- 仅纵向溢出：出竖条、不出横条；内容宽扣除 12px 后复核的边界用例
  （恰好挤出横条的临界尺寸）。
- 两轴都溢出：槽/角块矩形坐标正确；thumb 长度比例与 `THUMB_MIN_LEN` 夹取；
  thumb 位置随 scroll 的分数映射（顶/中/底三点）。
- 空文档：两轴 `None`，`clamp_scroll` 钉 (0,0)。
- `clamp_scroll` 既有 4 个测试迁移到"内容区尺寸"语义并保持通过。
- 绘制断言：出条时最末若干 fill_rect 的矩形/颜色匹配槽、滑块、角块；
  `hover/active` 改变滑块颜色。

**rofd-component（交互测试）**

- 六个入口（滚轮/ScrollPage/Zoom/ZoomAt/Resize/load_document）越界后 scroll
  被 clamp 到 0 / y_max；`load_document` 后 scroll 归零。
- 竖/横滑块拖拽：PointerDown 命中滑块 → 多次 PointerMove 的绝对映射数值正确、
  边界 clamp、PointerUp 后**无 undo 记录**（`can_undo()==false`）。
- 轨道点击：一次翻 `0.9 * 内容区`，到顶/底 clamp。
- chrome 优先：在滑块/槽位置下放一个批注，PointerDown 不选中、不拖动批注，
  不起 Pan、不起文本选区。
- 光标：悬停滑块 → ResizeV/ResizeH；移出 → 回落工具光标；拖拽中保持；
  松手后重算。
- 拖滑块跨页边界触发 `on_page_change`。
- 既有 `hand_pan_drag_updates_scroll_with_clamp` 等 pan 测试随内容区语义更新。

**rofd-web-view**：`pointer_cursor_str` 两个新字符串的单测。

**手动验收（两端，使用 `test/ru-yuan-ji-lu.ofd`）**：滚轮在第一页顶部/末页底部
封死；滑块拖动与文档 1:1；轨道翻屏；放大到双条出现、角块正确；窗口/容器缩放后
不停在空白区；web（含 tauri）与 native 外观行为一致。

## 6. 受影响文件一览

| 文件 | 改动 |
|---|---|
| `crates/render/src/scrollbar.rs` | **新增**：常量、布局/几何、`hit_scrollbar`、`paint_scrollbars` |
| `crates/render/src/viewport.rs` | `clamp_scroll` 内部改基于内容区（签名不变） |
| `crates/render/src/lib.rs` | 导出新模块/类型 |
| `crates/component/src/editor_component.rs` | 全入口 clamp 收口、`DragState::ScrollThumb`、chrome 路由与悬停态、`build_scene` 末尾追加绘制、load/new 重置 |
| `crates/component/src/callbacks.rs` | `PointerCursor` 加 `ResizeV`/`ResizeH` |
| `crates/web-view/src/wasm_editor.rs` | `pointer_cursor_str` 两个新映射 + 单测 |
| `crates/native-app/src/main.rs` | 光标 match 两个新分支 |
| `crates/web-view/sdk/README.md` | 交互/光标说明补两行（无 API 变更） |

native-view（含 winit bridge）、SDK TS、web-app、tauri-app：无功能改动。
