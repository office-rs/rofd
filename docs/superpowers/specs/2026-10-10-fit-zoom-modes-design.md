# 适应宽度 / 适应高度 / 适应页面（fit 缩放模式）设计

日期：2026-10-10
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）、[`2026-09-14-scrollbar-and-scroll-bounds-design.md`](2026-09-14-scrollbar-and-scroll-bounds-design.md)（clamp_scroll 单一实现、两遍溢出分析）、[`2026-09-22-rofd-xilem-refactor-and-rename-design.md`](2026-09-22-rofd-xilem-refactor-and-rename-design.md)（AGENTS "Native Ofd Widget" 契约 6：zoom）

## 1. 背景与问题

用户诉求：实现「适应宽度 / 适应高度 / 适应页面」三个缩放模式。web-app 工具栏已有
三个**禁用占位按钮**（`App.vue`，图标齐备，tooltip "v1 暂未开放"）等着启用。

代码现状（已核实）：

- **缩放机制就绪。** `Viewport { scroll, zoom, size, page_gap }`（rofd-render）；
  `Zoom` / `ZoomAt` 事件做乘法缩放，钳制 `[MIN_ZOOM, MAX_ZOOM] = [25%, 300%]`
  （基数 `PX_PER_MM`，96 DPI），缩放后过 `clamp_scroll` 单一实现，`on_zoom_change`
  仅在 zoom 实变时 fire，并复查 `maybe_fire_page_change`。
- **滚动条布局是两遍溢出分析。** `scrollbar_layout` 求解「条出现 ⇄ 内容区缩小」
  耦合；内容区 = 视口减可见条（`SCROLLBAR_THICKNESS = 12px`）。内容度量
  `content_metrics = (最宽页宽 × zoom, gap + Σ(页高 × zoom + gap) + gap)`。
  x 轴滚动/居中以**最宽页**为准。
- **页尺寸逐页可不同。** `doc.pages[i].physical_box`（mm）。
- **组件已追踪当前页。** `current_page: Option<usize>`（视口中心页语义，文档
  初始 None）；`set_viewport_size` 防御性（NaN/非正数直接忽略，尺寸不变 no-op）。
- **native 宿主无任何缩放 UI**（xilem-app 工具栏连放大/缩小都没有）；web 侧
  `handleZoomAt` / `onZoomChange` 已通，无 fit API。

### 决策记录（2026-10-10 与用户确认）

| # | 决策 |
| --- | --- |
| D1 | 三种 fit 均以**当前页**为基准（WPS 阅读器行为；页尺寸不一时较宽页仍可出横向滚动条） |
| D2 | fit 结果**可突破 25% 下限**：新 `FIT_MIN_ZOOM = PX_PER_MM × 0.05`（5%），上限仍 `MAX_ZOOM`；大图幅文档（A0 图纸）真的装得下 |
| D3 | **双侧 UI**：web-app 启用占位按钮；xilem-app 新增完整缩放按钮组 |
| D4 | FitPage 激活时**滚动到当前页顶部**；FitWidth / FitHeight 视口**中心锚定** |
| D5 | `OfdConfig` 同时给 `with_zoom` 与 `with_zoom_mode` 时，**zoom_mode 优先** |

## 2. 目标与非目标

### 2.1 目标

1. 三种 fit 模式 + 完整状态机：进入（`set_zoom_mode`）、退出（手动缩放回
   Custom）、重适配（resize / 换文档自动 re-fit，**翻页不 re-fit**——阅读稳定性，
   Adobe/WPS 行为）。
2. `on_zoom_mode_change(ZoomMode)` 新回调：ctrl+滚轮静默退出 fit 时宿主 UI
   高亮必须知情；`zoom_mode()` getter。
3. `OfdConfig::with_zoom_mode`（嵌入者默认 fit-width 场景）+ SDK init
   `zoomMode` 选项。
4. 双侧 UI：web-app 启用三个按钮 + 显示比例下拉追加适应项；xilem-app 新增
   缩小｜显示比例｜放大｜实际大小｜适应宽度｜适应高度｜适应页面按钮组。
5. 核心逻辑全部落在 render + component（AGENTS §4.9 平台边界）；fit 几何是
   render 纯函数（与 `clamp_scroll` 同级同风格，可结构性测试）。

### 2.2 非目标（v1 不做，YAGNI）

- fit 偏好持久化（localStorage / 配置文件记忆上次模式）。
- 第四模式「适应可见」/ 单页-双页视图切换（排版仍是纵向连滚多页栈）。
- fit 模式下翻页重适配（刻意不做，见 2.1-1）。
- marquee zoom（框选区域放大）、非等比缩放。
- 触屏双指 pinch 缩放（ViewEvent 面尚无 pinch 事件，另立议题）。

## 3. 设计

### 3.1 render 层：`fit_zoom` 纯函数（`crates/render/src/fit.rs` 新文件）

```rust
/// 适应目标轴。基准页由调用方解析（component 解析当前页）。
pub enum FitTarget { Width, Height, Page }

/// 计算把 `page`（mm）适应进视口内容区的 zoom。
/// None = 无法计算（doc 无页 / page 退化 / 视口未布局）。
pub fn fit_zoom(doc: &OfdDocument, vp: &Viewport, page: &Rect, target: FitTarget) -> Option<f64>
```

- **联合不动点迭代**（与 `scrollbar_layout` 同型，判据严格一致）：

  ```text
  content_w(z) = max页宽 × z                      （最宽页，全文档）
  content_h(z) = gap + Σ(页高 × z + gap) + gap     （与 content_metrics 同源）
  need_v = content_h(z) > region_h(vp.h − need_h 条宽)
  need_h = content_w(z) > region_w(vp.w − need_v 条宽)
  region 确定后：Width → region_w / page.w
                Height → region_h / page.h
                Page → min(两者)
  ```

  以当前 `vp.zoom` 估初值，迭代至 `(need_v, need_h)` 稳定（≤3 轮）。
- **与 scrollbar_layout 的一致性**：判据同源保证应用算出的 z 后，实际滚动条
  出现性与预估一致——fit-width 基准页恰为最宽页时 `content_w == region_w`，
  严格 `>` 判据下不出横条，x 由 clamp_scroll 居中钉 0。
- **钳制**：收敛后 `z.clamp(FIT_MIN_ZOOM, MAX_ZOOM)`。
- **None 条件**：`vp.size` 任一维非有限或 ≤ 0；`page.w/h` 非有限或 ≤ 0；
  `doc.pages` 为空。
- **常量**：`FIT_MIN_ZOOM = PX_PER_MM × 0.05` 放 `viewport.rs`（与
  MIN_ZOOM / MAX_ZOOM 并列，注释说明只用于 fit 与低比例下的动态下限，见 3.2.4）。

### 3.2 component 层：`ZoomMode` 状态机

#### 3.2.1 枚举与字段

- 新文件 `crates/component/src/zoom_mode.rs`：
  `pub enum ZoomMode { Custom, FitWidth, FitHeight, FitPage }`
  （`Clone + Copy + PartialEq + Eq + Debug`），`lib.rs` 导出。
- `OfdComponent` 新字段 `zoom_mode: ZoomMode`（默认 `Custom`）。

#### 3.2.2 进入：`set_zoom_mode(&mut self, mode: ZoomMode)`

命令式 API（`set_tool` 同风格，返回 `()`）：

1. 与当前模式同值 → no-op（不 fire、不重算）。
2. 记录模式；解析基准页 = `current_page.unwrap_or(0)` 的 `physical_box`（D1）。
3. `fit_zoom` → `Some(z)`：
   - 更新 `viewport.zoom = z`；
   - **滚动锚定**（D4）：FitWidth / FitHeight 用视口中心锚定（同 ZoomAt 数学：
     `scroll = center − (center − scroll) × ratio`）；FitPage 把 `scroll.1` 对齐
     当前页顶（按 `page_origin` 叠加公式：`scroll.1 = gap + Σ_{j<i}(页高_j × z + gap)`，
     使该页顶贴内容区顶）；
   - `clamp_scroll` 收尾（x 居中 / 边界钳制）；
   - fire `on_zoom_change`（zoom 实变才 fire，既有守卫）→
     `maybe_fire_page_change` → `mark_scene_dirty`。
4. `fit_zoom` → `None`：**仅记录模式**，不动 zoom / scroll、不 fire——视口尚未
   布局（native 构造早于 layout 是常态），待 `set_viewport_size` 到达时兜底。
5. fire `on_zoom_mode_change`（模式实变才 fire；第 4 步 None 挂起也 fire——
   模式确实切换了，UI 高亮需要反映）。

#### 3.2.3 退出与 re-fit 触发点

- **退出**：`Zoom` / `ZoomAt` 事件处理器开头，若 `zoom_mode != Custom` → 置
  `Custom` + fire `on_zoom_mode_change`（无论 zoom 是否实变——例如已顶在
  MAX_ZOOM 再 ×1.1，zoom 不变但模式必须退出）。
- **re-fit**（均走 3.2.2 的 3-5 步，但锚定统一用视口中心，不滚页顶）：
  - `set_viewport_size`：尺寸实变后，若 mode != Custom → re-fit；
  - `new_document`：若 mode != Custom → re-fit（`current_page` 已重置 None →
    基准页 0，页尺寸全变）。
- **不 re-fit**：翻页（`maybe_fire_page_change` 路径）——比例保持，防阅读中
  页宽跳变。

#### 3.2.4 钳制交互（防「上拽」）

fit 可把 zoom 置于 25% 以下（如 A0 fit-page 到 17%）。若手动缩放仍用
`(old × factor).clamp(MIN_ZOOM, MAX_ZOOM)`，17% × 1.1 = 18.7% 会被**上拽**到
25%——跳变。规则：

```text
floor = if old_zoom < MIN_ZOOM { FIT_MIN_ZOOM } else { MIN_ZOOM }
new   = (old × factor).clamp(floor, MAX_ZOOM)
```

- 从 ≥25% 手动缩小：仍在 25% 停（既有契约 6 语义不变）；
- 从 fit 的 17% 放大：18.7% → 20.6% → … 平滑爬回，绝不跳 25%；
- 从 fit 的 17% 再缩小：可降至 5% 为止。

`Zoom` / `ZoomAt` 两个处理器同改；fit 自身与 re-fit 用 3.1 的钳制。

#### 3.2.5 回调与配置

- `callbacks.rs`：`OnZoomModeChange` 别名（native `+ Send` / wasm 无，双 cfg
  门控同既有）+ `Callbacks.on_zoom_mode_change` 字段。
- `pub fn on_zoom_mode_change(cb)`（双 cfg 门控样板，同 `on_zoom_change`）、
  `pub fn zoom_mode(&self) -> ZoomMode`。
- `config.rs`：`OfdConfig::with_zoom_mode(mode)`（builder 链式，同 `with_zoom`
  风格）。构造时记录模式并立即尝试 fit（通常因视口未布局而 None 挂起，首次
  layout 后 `set_viewport_size` 兜底生效）；`with_zoom` 的显式值仍写入
  `viewport.zoom` 作为挂起期的兜底初值；两者同给时 zoom_mode 最终生效（D5）。

### 3.3 xilem-view 与 xilem-app（native 链路）

- `ofd_widget.rs`：`OfdWidgetAction` 增 `ZoomModeChanged(ZoomMode)`——镜像组件
  新回调面（契约 2）。widget 内组件回调装配进 pending queue → `ctx.submit_action`
  （照搬 `on_zoom_change` 的装配样式；不同于 `PointerCursorChanged` 内部消费，
  本 action 必须上抛，宿主按钮高亮依赖它）。
- `ofd_view.rs`：`.on_zoom_mode_change(handler)` 挂接（同 `on_zoom_change` 装配）。
- `xilem-app/src/main.rs`：
  - `AppState` 增 `zoom_mode: ZoomMode` 与缩放百分比显示；
  - 工具栏新增一组：**缩小｜显示比例（点击回 100%）｜放大｜实际大小｜适应宽度｜
    适应高度｜适应页面**；
  - 缩放命令经 `OfdCommand` 闭包调 `component.set_zoom_mode` / `ZoomAt`；
  - `ZoomModeChanged` action → 更新 `AppState.zoom_mode` → 按钮高亮。

### 3.4 web-view / SDK / web-app（web 链路，tauri-app 复用前端零改动）

- `wasm_ofd.rs`：
  - `parse_zoom_mode(s) -> ZoomMode`（`"custom" | "fit-width" | "fit-height" |
    "fit-page"`；无效串回退 `Custom`，与 `set_tool` 宽松语义一致）+ 反向
    `zoom_mode_str(mode) -> &'static str`；
  - `#[wasm_bindgen]`：`setZoomMode(mode: &str)` / `zoomMode() -> String` /
    `setOnZoomModeChange(cb)`（样板同 `set_on_zoom_change`）。
- `sdk/src/index.ts`：
  - `OfdInitConfig.zoomMode?: 'fit-width' | 'fit-height' | 'fit-page'`；
  - `Ofd` 类：`setZoomMode(mode: 'custom' | 'fit-width' | 'fit-height' | 'fit-page')`
    （含 `'custom'`——宿主程序化退出 fit）/ `zoomMode()` /
    `onZoomModeChange?` 回调（deferCb 装配同 `onZoomChange`）；
  - init 时 `config.zoomMode` → `wasm.setZoomMode(...)`（组件挂起到首次
    `handle_resize`，SDK 无需等布局时序）。
- `web-app/src/App.vue`：
  - 三个占位按钮去 disabled，`@click` 绑 `setZoomMode('fit-width' | …)`；
  - `zoomMode` ref 随 `onZoomModeChange` 更新，驱动按钮 active 高亮；
  - 显示比例下拉追加 适应宽度 / 适应高度 / 适应页面 三项；
  - fit 激活后百分比文本仍由 `onZoomChange` 驱动（如 "73%"），无需特判。

## 4. 错误处理

- `fit_zoom` 返回 None 是「未就绪」不是错误：静默挂起（模式已记录），不 fire
  zoom 类回调、不 panic；layout 后 `set_viewport_size` 兜底（AGENTS §4.6 无
  静默吞错——这里没有错误被吞，是明确的延迟语义）。
- `setZoomMode` 无效字符串 → 回退 `Custom`（`set_tool` 同语义，宽进宽出）。
- 非有限值防御沿用：`fit_zoom` 的 None 分支拦截 NaN/inf；缩放钳制天然拦截
  手动路径。
- 极小视口（宽 < 12px + ε）：region 收敛到 0 附近 → z 落到 `FIT_MIN_ZOOM`
  钳制，不产生 0/NaN。

## 5. 测试计划（TDD，先红后绿，数值/结构断言非像素）

### render（`fit.rs` 内联单测）

- `fit_width_no_scrollbar`：单页小文档大视口，数值断言 `region_w / page.w`。
- `fit_width_reserves_vertical_bar`：多页栈高出视口 → v 条出现，
  `z = (vp.w − 12) / page.w`。
- `fit_height_reserves_horizontal_bar`：场上有更宽异尺寸页 → h 条参与迭代。
- `fit_page_takes_min`：宽矮视口下取宽轴比例。
- `fit_breaks_min_zoom_floor`：A0 大页小视口 → z < MIN_ZOOM 且
  `= max(FIT_MIN_ZOOM, 理论值)`。
- `fit_clamps_to_max`：小页（如印章页）大视口 → z = MAX_ZOOM。
- `fit_returns_none`：空文档 / 退化页（w 或 h ≤ 0）/ 零视口。
- `fit_iteration_converges`：构造 v/h 条互相触发翻转的用例，断言稳定与
  scrollbar_layout 判据一致。

### component（`ofd_component.rs` 测试区）

- `set_zoom_mode_fit_width_updates_zoom_fires_callbacks`
  （zoom + `on_zoom_change` + `on_zoom_mode_change` 各自断言）。
- `set_zoom_mode_same_value_is_noop`。
- `zoomat_exits_fit_mode_fires_mode_change`（含「zoom 顶格不变但模式仍退出」
  分支：MAX_ZOOM 下再 ×1.1）。
- `manual_zoom_dynamic_floor`：fit 至 17% 后 ×1.1 → ≈18.7%（不上拽 25%）；
  从 100% ×0.9 连缩在 25% 停（既有语义回归）。
- `resize_refits_fit_mode`：改 `set_viewport_size` → zoom 随之变化。
- `new_document_refits_fit_mode`。
- `page_change_does_not_refit`：翻页后 zoom 不变。
- `fit_before_layout_defers`：先 `set_zoom_mode` 后 `set_viewport_size` →
  生效且 fire。
- `fit_page_scrolls_current_page_to_top`（滚动锚定断言）。
- `fit_width_center_anchor`（ZoomAt 同款锚定数学断言）。
- `fit_pending_no_zoom_fire`：空文档挂起期不 fire `on_zoom_change`。

### xilem-view（无头 harness 增 1 个）

- 宿主命令 → `set_zoom_mode` 生效 + `ZoomModeChanged` action 上抛。

### 人工 GUI 验证（plan 收尾步骤）

- native：`cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd`（按钮组 + resize
  重适配 + ctrl+滚轮退出高亮）；
- web：`npm run dev`（三个按钮 + 下拉项 + resize）；tauri 复用前端一并目检。

## 6. 受影响文件一览

| 文件 | 变更 |
| --- | --- |
| `crates/render/src/fit.rs` | 新增：`FitTarget` + `fit_zoom` + 单测 |
| `crates/render/src/viewport.rs` | `FIT_MIN_ZOOM` 常量 |
| `crates/render/src/lib.rs` | `mod fit` + 导出 |
| `crates/component/src/zoom_mode.rs` | 新增：`ZoomMode` 枚举 |
| `crates/component/src/lib.rs` | 导出 `ZoomMode` |
| `crates/component/src/ofd_component.rs` | `zoom_mode` 字段、`set_zoom_mode` / `zoom_mode()` / `on_zoom_mode_change`、事件退出、动态下限、re-fit 触发点、测试 |
| `crates/component/src/callbacks.rs` | `OnZoomModeChange` + 字段 |
| `crates/component/src/config.rs` | `with_zoom_mode` |
| `crates/xilem-view/src/ofd_widget.rs` | `ZoomModeChanged` action + 回调装配 |
| `crates/xilem-view/src/ofd_view.rs` | `.on_zoom_mode_change` 挂接 |
| `crates/xilem-app/src/main.rs` | AppState + 缩放按钮组 + 高亮 |
| `crates/web-view/src/wasm_ofd.rs` | `parse_zoom_mode` / `setZoomMode` / `zoomMode` / `setOnZoomModeChange` |
| `crates/web-view/sdk/src/index.ts` | init `zoomMode` + `setZoomMode` / `zoomMode` / `onZoomModeChange` |
| `crates/web-app/src/App.vue` | 启用三按钮 + 下拉项 + 高亮 |
