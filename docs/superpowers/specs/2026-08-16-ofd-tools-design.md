# OFD 编辑器工具集设计：手型工具 / 顶点编辑 / 文本选择

日期：2026-08-16
状态：待审阅
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构，本文档不改变其分层与不变量）

## 1. 背景与目标

对标 WPS 的 OFD/PDF 阅读器交互，为 rofd 增加三个特性：

1. **手型工具**（P1）：点击空白处拖拽平移视图；点选批注后可 Delete 删除。
2. **顶点编辑**（P2）：线段类批注（Line/PolyLine/Polygon/Arrow）通过顶点句柄改形状；
   椭圆用 4 个边中点句柄调整宽高；Freehand 仅显示外包矩形、只可移动。
3. **文本工具**（P3）：拖选正文文字（单页、跨行）、双击选词、三击选段，
   支持复制文本，预留"选中转高亮批注"接口。

**工具模型（2026-08-16 修订，用户裁定）**：对标 WPS，浏览模式只有**两个工具**：
`手型（Hand）` 与 `文本（Text）`。原独立的"选择"工具并入"文本"：`Tool::Select`
删除，`Tool::TextSelect` 更名 `Tool::Text`。`Tool::Text` 是统一入口--
PointerDown 先命中批注（选中/拖动/删除任意类型批注），未命中再走正文文字选区，
两者皆无则清空全部选择。SDK 字符串 `"text"` 为正式名，`"select"`/`"textSelect"`
保留为兼容别名。

### 1.1 WPS 行为的核实情况

公开渠道查不到 WPS 批注交互的官方文档（已检索，无果）。本设计的 WPS 对标行为
来自**用户实测描述**（freehand 仅外包矩形可移动、矩形 8 句柄、椭圆 4 个边中点
句柄、Arrow 与 Line 同为双端点），以及 PDF 阅读器行业惯例（Acrobat/Foxit：
线段类仅顶点句柄、无包围盒缩放）。若后续实测发现偏差，以实测为准修订本节。

## 2. 特性拆分与顺序

| 阶段 | 特性 | 规模 | 依赖 |
|---|---|---|---|
| P1 | 手型工具 | 小 | 无 |
| P2 | 顶点编辑 + 句柄策略调整 | 中 | 无（与 P1 并行可做） |
| P3 | 文本选择工具 | 大 | 建议最后做，单独出实现 plan |

P1/P2/P3 各自独立成 plan，可分别交付。

## 3. P1 手型工具

### 3.1 交互定义

- `Tool` 枚举新增 `Hand` 变体（`crates/component/src/editor_component.rs`）。
- **PointerDown（左键）**：先 `hit_test`。
  - 命中批注/句柄 → 走现有 Text 工具的选中与拖拽逻辑（Move/Resize/Delete
    全部复用，不复制代码）。
  - 命中 `Page`/`Empty` → 清除选区与文字光标，进入新的 `DragState::Pan
    { start: (f64,f64), last: (f64,f64) }`。
- **PointerMove（Pan 中）**：`viewport.scroll -= (pointer - last)`，两轴均允许
  （放大后需要横向平移；WPS 的"上下拖动滚动"是垂直特例）。滚动范围按内容边界
  clamp，clamp 逻辑从现有 `ViewEvent::Scroll` 处理中抽取复用，不得两处各写一份。
- **PointerUp**：结束 Pan，无文档变更（平移不进 undo 历史）。
- 键盘：Delete/Backspace 删除选中批注的逻辑已存在（`handle_key`），手型工具下
  照常生效，无需改动。

### 3.2 光标

component 不直接设系统光标（同"库不取系统时间"原则：平台能力经回调交给宿主）。
新增 `PointerCursor` 枚举（`Default / Grab / Grabbing / Text / …`），经新回调
`on_pointer_cursor(shape)` 上抛。native 宿主映射 winit CursorIcon，web 宿主映射
CSS cursor。悬停光标随命中目标动态切换（PointerMove 无拖拽分支，2026-08-16
修订）：

- **手型工具**：悬停空白 = `Grab`，悬停批注 = 箭头（可点选，WPS 实测），
  拖拽中 = `Grabbing`。
- **文本工具**：批注优先--悬停批注（含覆盖正文文字的批注）= 箭头；纯正文
  文字 = I 型（`Text`）；空白 = 箭头。
- **Markup click-vs-drag**（2026-08-16 二次修订）：文字 markup 批注（高亮/
  下划线/删除线/波浪线）是批注--悬停 = 箭头（可点选），与其它批注一致。
  按下先选中；文本工具下拖动越过阈值（4px）且按下点命中正文文字则转为
  文字拖选（见 §5.2）。
- 创建工具不参与悬停切换。

### 3.3 涉及范围

- `crates/component`：`Tool::Hand`、`DragState::Pan`、`on_pointer_cursor` 回调。
- `crates/render`：clamp 辅助函数（若 Scroll clamp 目前内联在 component，则
  抽到可复用位置）。
- `crates/native-view` / `crates/web-view`：光标回调 → winit/CSS 映射。
- `examples/native-app` / `examples/web-app`：工具栏加手型按钮；SDK 暴露
  `setTool('hand')`。
- **工具栏布局仿 WPS 分组**（demo 层）：`[手型 | 文本] │ [高亮 下划线 删除线
  波浪线] │ [手绘 矩形]`--浏览模式组（手型/文本）与批注创建组用分隔线分开。
  "文本"即 WPS"文本"工具（统一入口：批注选择 + 正文拖选），见 §1 工具模型修订；
  原"选择"按钮已删除。
- **无 spring-back**（已核实 WPS 行为）：画完一个批注后**停留在当前创建工具**，
  继续画下一个；不自动切回选择/手型。

## 4. P2 顶点编辑与句柄策略

### 4.1 句柄策略（对标 WPS，最终版）

| 批注类型 | 选中显示 | 改形方式 | 相对现状 |
|---|---|---|---|
| Rect | 8 句柄包围盒 | 整体缩放 | 不变 |
| Ellipse | 4 个边中点句柄（N/S/E/W） | 拖边中点调宽/高 | 改：8 → 4 |
| Arrow | 2 端点句柄 | 拖端点改方向/长度 | 改：包围盒 → 端点 |
| Line | 2 端点句柄 | 拖端点改方向/长度 | 新增 |
| PolyLine / Polygon | 每顶点一个句柄 | 拖各顶点 | 新增 |
| Freehand | 外包矩形（选中框） | 仅移动，不可改形 | 补画外包矩形（如缺） |
| Markup（高亮/下划线/删除线/波浪线） | 选中态，无句柄 | 点击选中 + Delete；文本工具下拖动转为文字拖选（click-vs-drag，见 §5.2） | 改：拖动不再移动 markup |

要点：**线段类（Line/PolyLine/Polygon/Arrow）没有包围盒缩放句柄**，只有顶点句柄；
外包盒仅作命中/移动用途。Freehand 同 WPS：外包矩形 + 仅移动。

### 4.2 实现设计

- `HandlePos` 新增 `Vertex(usize)` 变体；`hit_handle` 对选中批注按 4.1 的策略
  暴露句柄集合（Ellipse 限 N/S/E/W；线段类由几何点集生成顶点句柄）。
- 顶点来源：
  - Line/Arrow：`Shape` payload 的两个端点（现状存储于 `points`/`rect`，实现
    时核实统一读取路径）。
  - PolyLine/Polygon：`points` 全部顶点。
  - Freehand：不暴露顶点句柄。
- 拖动顶点 → 新 `DragState::VertexMove { id, index, moved }`（PointerMove 仅
  更新预览，不逐帧改文档，与 Move/Resize 的预览模式一致）；PointerUp 一次性
  提交新 editor 命令 `move_annotation_vertex(id, index, new_point)`。
- `move_annotation_vertex` 内部用 before/after 整个 annotation 的
  `ReplaceAnnotationStep`（与现有 `move_annotation` 同模式），天然可 undo。
- Ellipse 4 句柄复用现有 `DragState::Resize` + 边中点 `HandlePos::N/S/E/W`
  （compute_resize 已支持边句柄，仅命中集合收窄）。
- Arrow 端点拖动复用 VertexMove；箭头头部几何随端点重算（渲染侧已有 Arrow
  绘制，核实其输入是 rect 还是 points，保证拖端点后箭头正确）。

### 4.3 涉及范围

- `crates/render/src/hit_test.rs`：`HandlePos::Vertex`、按类型生成句柄集合、
  顶点命中（屏幕空间方块，与现有句柄同尺寸）。
- `crates/component/src/editor_component.rs`：`DragState::VertexMove`、句柄
  策略分派、PointerUp 提交。
- `crates/editor`：`move_annotation_vertex` 命令 + apply→undo 可还原测试。
- 渲染选中态：Freehand/线段类的外包矩形/句柄绘制核实补齐。

## 5. P3 文本选择工具

### 5.1 选区状态归属（关键决策）

**方案 A（采用）**：选区是纯 UI 态，放 `EditorComponent` 内：
`text_selection: Option<BodyTextSelection>`，其中
`BodyTextSelection { page: PageId, ranges: Vec<(ObjectId, start_char, end_char)> }`。
body 保持只读；选区不进 dom、不进 editor 历史、不落盘。渲染时 `composite`
增加选区参数画半透明高亮 overlay。切换工具/换页/文档变更时清空选区。

否决的备选：
- B（选区伪装成临时批注进 `AnnotationModel`）：污染"批注是唯一可变面"语义，
  保存时还需过滤，不可取。
- C（dom 加选区字段）：dom 是纯数据模型，不塞 UI 态。

### 5.2 交互定义

`Tool::Text`（统一工具，见 §1 工具模型修订）：

- **批注优先**：PointerDown 先命中批注（含句柄），命中即走批注选中/拖动逻辑
  （Move/Resize/Delete 全部复用），不产生文字选区。**markup 例外--click-vs-drag**
  （2026-08-16 修订，WPS 实测）：文字 markup 批注（高亮/下划线/删除线/波浪线）
  贴附正文文字，按下先选中 markup；文本工具下按住拖动越过阈值（4px）且按下
  点命中正文文字 -> 清空批注选择、转为文字拖选（`DragState::MarkupPress` ->
  `TextSelect`）；原地释放 -> markup 保持选中（可 Delete）。手型工具下拖动
  markup 与其它批注一致（预览式 Move）。这样既保住"高亮文字上直接拖选"，
  又保住"点击选中高亮批注"。
- **拖选**：PointerDown 于正文文字 → 锚点 char offset；PointerMove 逐帧更新
  选区（单页内跨行，含部分首尾行）；不跨页（拖出页面边界 clamp 到页内最近
  位置）。PointerDown 于空白/图片 → 清空选区与批注选择。
- **悬停光标**：批注优先（含覆盖正文文字的批注，与点击优先级一致）= 箭头；
  纯正文文字 = I 型（`PointerCursor::Text`）；空白 = 箭头
  （`PointerCursor::Default`），PointerMove 无拖拽时动态更新（详见 §3.2）。
- **双击选词**（CJK 按字符邻接分词：连续同类字符为一段）；**三击选段**（同一
  TextObject 的全部内容）。双击/三击需要 component 记录点击时间与次数--
  组件自身无时钟（不变量 4.4），由 ViewEvent 携带或宿主侧判定（实现时定，
  倾向 PointerDown 增加可选 `click_count` 字段，宿主的 winit/DOM 事件天然
  提供该信息）。
- **click_count 语义**（实现裁定）：`PointerDown.click_count` 仅 2（双击选词）
  与 3（三击选段）有特殊语义；0（宿主未提供计数）与 ≥4 一律按单击处理并
  arm 拖选。web 侧的 `pointerdown` 事件 `detail` 恒为 0（Pointer Events
  spec），故 SDK 自行计数（500ms 窗口 + 4px slop，循环 1->2->3，与 winit
  bridge 的 `next_click_count` 一致），不转发 `e.detail`。曾因转发
  `detail=0` 导致纯正文拖选静默失效（批注文字不受影响，因为 markup
  click-vs-drag 不看 click_count），此为该回归的根因。
- **选区与批注选择互斥**：有文字选区时清除批注选择，反之亦然。
- 不做（本期非目标）：跨页选择、Shift+点击扩展、Ctrl+A、键盘 Shift+方向键选字。

### 5.3 命中与几何（render 层）

- 新增 `hit_test_body_text(doc, vp, fonts, point) -> Option<TextHit>`，
  `TextHit { page: PageId, object: ObjectId, code_index: usize, char_offset: usize }`。
- 新增 `text_selection_rects(doc, vp, fonts, sel) -> Vec<Rect>`：每行连续选中
  字符的覆盖矩形（viewport 空间）。
- **几何单一来源**：字形笔位（起点 + 累积 delta）计算从 `body_scene.rs` 的
  `draw_text` 抽成共享函数，命中/选区矩形/绘制三处共用，禁止两套实现漂移。
  字符宽度按 shaping 后的字形 advance；命中判定取最近字符边界（前/后半宽）。
- **CTM（2026-08-16 修订，替代 v1"仅平移近似、缩放/旋转跳过"）**：命中与
  选区矩形使用与绘制完全相同的对象仿射（页原点 × zoom × Boundary × CTM），
  视口点经逆变换映射回局部坐标后做行带/字符格判定--缩放/旋转/剪切文字渲染
  在哪里就在哪里可选。奇异 CTM（det≈0）不可逆，跳过。旋转文本的选区矩形
  取变换后四角的包围盒（近似；平移/缩放精确）。实测依据：sample.ofd 的全部
  22 个 TextObject 均为缩放 CTM（0.0176 单位换算、Size=209），旧实现下整个
  文档的文字均不可选。

### 5.4 复制与转高亮

- component 新增 `selected_text() -> Option<String>`（按 ranges 拼接，行间加
  换行符策略：不同 TextCode 之间加 `\n`，同 Code 内不加）。
- `Ctrl+C`（Text 工具且有选区时）-> 触发新回调 `on_copy(text: String)`。
  **component 层不碰剪贴板**（同"库不取系统时间"原则；且 native/wasm 剪贴板
  API 异构，component 保持平台无关、依赖面不变）。
- **剪贴板由适配器层默认实现**，上层宿主应用开箱即用：
  - `web-view`：`WasmEditor` 默认订阅 `on_copy`，SDK 的 JS 层写
    `navigator.clipboard.writeText`（在 Ctrl+C 的 user activation 窗口内）；
    SDK 配置项可关闭默认实现，宿主自行处理。
  - `native-view`：`EditorApp` 默认订阅 `on_copy`，用 `arboard` 写系统剪贴板
    （依赖加在 native-view，不进 component）；配置项同样可关闭。
  - `examples/native-app` / `examples/web-app`：零代码感知，无需各自接
    `on_copy`。
- 预留（P3 一期可只交付复制）：`create_highlight_from_selection(color) ->
  Option<AnnotationId>`：把 `text_selection_rects` 每行矩形换算成
  `Markup.quad_points`（页局部坐标），走现有 `create_annotation(Highlight,
  Markup)` 命令，可 undo、可保存；成功后清空选区。

### 5.5 涉及范围

- `crates/render`：`hit_test_body_text`、`text_selection_rects`、几何抽取、
  composite 选区 overlay 绘制。
- `crates/component`：`Tool::Text`、`BodyTextSelection`、拖选/双击/三击
  状态机、`selected_text`、`on_copy` 回调、`create_highlight_from_selection`。
- `crates/web-view` + SDK：`setTool('text')`、`onCopy`、`getSelectedText`、
  （P3.5）`createHighlightFromSelection`。
- `crates/native-view`：默认 `on_copy` -> arboard 剪贴板（可配置关闭）。
- `examples`：工具栏文本选择按钮（剪贴板由适配器层默认实现，示例零代码对接）。

## 6. 测试策略

沿用仓库既有约定（场景结构断言、命令 apply→undo、80% 覆盖）：

- **P1**：Pan 拖拽改 scroll 且被 clamp；手型下点批注仍可选中/移动/删除；
  平移不产生 undo 记录；切换工具取消 Pan。
- **P2**：每类批注的句柄集合断言（Rect=8、Ellipse=4、Line/Arrow=2、Polyline=N、
  Freehand/Markup=0）；拖顶点 → `move_annotation_vertex` → undo 还原；
  Arrow 拖端点后箭头几何正确（场景断言）。
- **P3**：`hit_test_body_text` 对已知字形布局的 offset 断言（复用 TestFont
  fixture）；拖选跨行的 ranges 正确；`selected_text` 拼接含换行；
  `text_selection_rects` 行数/坐标断言；转高亮后 quad_points 页局部坐标正确
  且保存往返保留；选区在切工具/文档变更时清空。

## 7. 开放问题（实现 plan 阶段解决）

- Line/Arrow 端点的存储表示（`rect` vs `points`）统一读取路径。
- 双击/三击的 click_count 传递方式（倾向 ViewEvent 字段扩展）。
- Freehand 选中外包矩形当前是否已绘制（P2 实现时核实补齐）。
