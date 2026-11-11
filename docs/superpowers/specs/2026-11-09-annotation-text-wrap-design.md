# 批注文字自动折行与 WPS 兼容的流式重排设计

日期：2026-11-09
状态：设计已定稿（待实施）
前置：[`2026-07-08-ofd-editor-design.md`](2026-07-08-ofd-editor-design.md)（总体架构与不变量）、[`2026-07-14-c1.5-annotation-fidelity-hardening-design.md`](2026-07-14-c1.5-annotation-fidelity-hardening-design.md)（TextBox 富外观）
范围：文字批注（TypeWriter，无边框 TextBox）与文本框（有边框 TextBox）

## 0. 问题与目标

### 0.1 现状

批注文字（`AnnotationPayload::TextBox`，`runs` 为空时）由 `annotation_scene::draw_text_in_rect` 一次性 shaping 后按原样绘制，只认 content 里的显式 `\n`；`caret_rect` / `caret_offset_at_x` 只按 x 定位，完全不知道行。于是：

- 输入超过 `rect.w` 的文字直接冲出右边框；
- 解析来的多行 TextBox，光标永远停在第一行 y、点第二行也按第一行 x 定位；
- IME 候选窗锚点（`caretRect()`）同样只在第一行；
- 保存后外观里的行断点成为硬折行，重开再拖宽不会重排。

### 0.2 目标

1. **自动折行**：文字批注/文本框输入时按 `rect.w` 折行，CJK 逐字可断，西文按词断，超长单词允许任意点断（`OverflowWrap::Anywhere`）。
2. **框向下扩展**：折行后所需高度超过 `rect.h` 时，`rect.h` 向下增长；只增不减（不把用户手动拉大的框缩掉）。
3. **行感知的光标/命中**：`caret_rect`、点击定位、IME preedit 锚点都按“第几行第几列”计算。
4. **改宽重排**：拖拽改变框宽后按新宽度重新折行（软折行，不记忆旧折点）。
5. **保存 / 重开都不丢流式能力**：保存时同时写
   - `Appearance`（渲染缓存：逐行 TextObject，行几何与字距来自真实排版）；
   - `Remark`（标准字段，纯文本逻辑内容）；
   - `GrowMode`（框体伸缩提示：`FixWidth` / `None`）。
   不写 `AllRichText` / `ParagraphFormat`：数科与 WPS 的编辑器都以 `Remark` + `Appearance` 重排，二者都不是流式重排的来源，写入只会形成重复副本。
   重开后 rofd 从 `Remark` 恢复逻辑文本，拖宽仍能流式重排；WPS 打开 rofd 保存的文本框也能编辑/重排（行为测试 2026-11-09：WPS 的流式能力只依赖 `Remark` + `Appearance`）。
6. **Enter 插入换行**：文本光标激活时按 Enter 在光标处插入显式 `\n`（硬换行），光标落到下一行行首；与普通键入同一条 Transaction，同样触发框高增长。三端适配器已把键盘 Enter 映射为 `Key::Enter`，只需组件处理。
7. **上下方向键按视觉行移动光标**：`ArrowUp` / `ArrowDown` 在当前折行/显式行布局中跨行移动，保留列偏好（`TextCursor.preferred_x`），行内取最近字符边界；到达首行再按上 / 末行再按下时落到文首 / 文末。

### 0.3 非目标

- 自动缩高 / 双击自适应。
- 富文本框（`runs` 非空，外来文件的外观片段）重排：绘制仍按其自带 pen 位置保真，不参与折行/框高增长。
- 水印（`Watermark`）折行：单行语义不变。
- 载入时改写外来文档的几何/内容（编辑与保存快照时生效）。
- 带 Shift 的文字选区（`TextCursor` 无 selection extent，上下移动不扩展选区）。
- 对齐 WPS 的 1mm 内边距（本轮保持 rofd 现有“文字从 rect 原点开始”的绘制；WPS 重排可能比 rofd 早折一个字符，见 §5）。
- 写入/解析 `AllRichText`、`ParagraphFormat`：数科/WPS 的编辑器都用 `Remark` + `Appearance` 重排，rofd 也只写 `Remark` + `Appearance` + `GrowMode`；外来文件的这两个参数被忽略。
- 保留非 FreeText 批注的 WPS 私有 `Parameter`（如 highlight 的 identity 字段；现状即会丢弃）。

## 1. 行为规范

### 1.1 折行算法

- 用 parley greedy line breaking（UAX #14）：CJK 逐字可断，西文按空格断。
- 样式固定 `OverflowWrap::Anywhere`：超长不可断单词也允许 emergency break，保证不溢出。
- 保持现有 `liga=0`（char↔glyph 1:1 的既有假设）。
- 折行宽度 = `rect.w.max(size)`（退化零宽框至少按一个 em 折，避免高度爆炸）。
- 行高与 baseline 取 parley `LineMetrics`（字体相关），不再假设固定 1.2em。

### 1.2 框高增长

- 所需高度 `required_h = 末行 top + 末行 height`（parley 真实行几何）。
- **空内容（`total_chars == 0`）或排版无行时 `required_h = 0`**，不参与增长（新建空框不需要为一行占位）。
- 增长规则：`rect.h = max(rect.h, required_h)`，**只增不减**。
  - 强制点：measure 闭包**只在 `required_h > rect.h` 时返回 `Some(required_h)`**，否则 `None`；编辑器拿到 `Some` 时直接写入。因此删除文字绝不会缩高。
- 触发点：任何文本内容变更（`Key::Char` / `Key::Enter` / Backspace / Delete / IME commit / paste）、TextBox 宽度拖拽结束、保存快照。
- 增长与文字变更落在**同一个** Transaction（一次 undo 同时还原内容和框高）。

### 1.2.1 换行符不变量：批注 content 不含 `\r`

parley 0.8 把 `\r\n` 拆成**两个**换行簇（`a\r\nb` → 三行，中间空行），与 XML/剪贴板常见的 CRLF 语义不符。因此约定：

- 批注文本 content 的模型不变量是**只含 `\n`**。
- 入口归一化（`\r\n` / 孤立 `\r` → `\n`）：
  - `Editor::insert_text(_measured)` / `set_annotation_text`（覆盖键入、paste、IME commit）；
  - `rofd-io` 解析 TextBox 时（content 与每个 `TextRun.text`），防止外来文件带 `\r`。
- 行文本提取仍防御性去掉行尾 `\r`（§4.1）。

### 1.2.2 富文本框（runs 非空）保持现状

`runs` 非空的 TextBox 是外来富外观保真快照（逐 run pen 位置绘制），**不参与折行**：

- 绘制仍走 `draw_text_runs`；
- `caret_rect` / 点击定位 / preedit 仍走旧的单行路径（现状），不做行感知；
- 不参与框高增长与保存快照成形。
首次文本编辑 `collapse_runs` 后 `runs` 清空，此后即按普通 TextBox 折行。

### 1.3 光标与点击

字符 offset →（行，行内列）映射规则：

- 行内字符 `[char_start, char_start + char_len)` 属于该行，列 = `offset - char_start`。
- 软折行边界（`offset == line.char_start + line.char_len == next.char_start`）归**下一行行首**（点击下一行行首、在该处插入都得到直觉结果；代价是点上一行最右端时光标显示在下一行行首，已知取舍）。
- 显式 `\n` 的字符位置（`offset < next.char_start`）归**上一行行尾**。
- 文末 `offset == content.chars().count()` 归末行行尾。
- 行内列超出字形数（行尾）时，x = 该行 advance（含行尾空格）。

`caret_rect`：

- y = `rect.y + line.top`，x = `rect.x + line_relative_x`，宽 `1px * zoom`，高 `line.height * zoom`（视觉行框：ascent + descent + leading）。2026-11-10 修订：此前高 `size * zoom` 且从行框顶起，光标止于 baseline，视觉重心偏上（"光标靠文字上方"）；改为覆盖整行框后与文字光学居中对齐。
- 首行 `line.top == 0`，与现有单行行为逐值一致（仅高度变化）。
- 富 TextBox / Note / Watermark 的 legacy 路径仍用 `size * zoom`。

点击定位：

- 先按 y 选行（`line.top ≤ y < line.top + line.height`，夹紧到首/末行），再在该行内取离 x 最近的字符边界。
- Note 与 `runs` 非空的富 TextBox 无折行，仍走旧 `caret_offset_at_x`。

框内文字拖选（2026-11-10 新增）：

- 编辑态（文本框已持有文字光标）下用 Text 工具在平铺 TextBox 内按下并拖动：锚点 = 按下处 char offset，拖动逐帧按 `caret_offset_at_point` 更新 focus（y 选行、x 选列，出框夹紧到首/末边界），选区 = `[min(anchor,focus), max(...))`。
- 未聚焦框的第一次按下仍走移动拖拽（"先点进框、再拖动选字"）；创建文字工具的框天然处于编辑态，首次拖动即选字。移动框用 Hand 工具。
- 选区是纯 UI 状态（`AnnotTextSelection`，不入 dom/历史/保存）：半透明蓝色逐行矩形（`annotation_text_selection_rects`，行框高）画在文字之上、caret 之下。
- 选区被编辑消费（均为一个 Transaction）：键入/Enter/粘贴/IME 提交替换选区；Backspace/Delete 删除选区；IME 组合开始时先删除选区（独立 undo step）再在塌缩后的光标处组合；←/→ 折叠到选区首/尾；↑/↓ 与普通点击清除选区。
- `selected_text()` / `copy_selection()` 优先返回框内选区文本（其次正文拖选，顺序同 2026-09-22 §2.8）。
- 富 TextBox（`runs` 非空）不参与（不重排、无行几何）。

preedit：

- 平铺 TextBox（`runs` 空）把组合串**内联**插进内容流：按 `content[..offset] + 组合串 + content[offset..]` 走同一份折行排版，组合点之后的文字随之下移/换行。组合串因此出现在它将要提交的位置，且不会叠在既有文字上（2026-11-10 修订：此前的 detached overlay 画在组合点，会与之后的文字重叠）。
- 组合串只用于屏幕预览：不写 dom、不进保存快照、不参与打印/PDF 导出（那些路径 `draw_annotations(..., preedit: None)`）。
- `runs` 非空的富 TextBox 当时保持单行 detached overlay（§1.2.2）。**2026-11-11 修订**：rich-text 改名后（`spans`）所有 TextBox 统一内联预览，overlay 已移除。

### 1.4 改宽

`runs` 为空的 TextBox 在宽度拖拽提交时：

- 新宽先按 `new_rect.w` 重排内容并求 `required_h`；
- `new_rect.h = max(new_rect.h, required_h)`；
- 仍是一次 `editor.resize_annotation` Transaction（高度在调用前算好并入 rect）。
- 两个入口（组件公开的 `OfdComponent::resize_annotation` 与拖拽提交处的直接调用）必须走同一个组件内 helper。

### 1.5 保存与 WPS 互操作

OFD 的 `Appearance` 是版式缓存，不是可编辑源。WPS 的 FreeText 批注把**逻辑文本**存在 `Remark` 里，`Appearance` 只用于显示；WPS 新建的文本框甚至不带任何 `<ofd:Parameters>`，仍能重新打开并流式重排（行为测试 2026-11-09）。数科与 WPS 的编辑器都以 `Remark` + `Appearance` 重排，不依赖 `AllRichText`/`ParagraphFormat`（逆向分析 2026-11-10）。rofd 采用最小约定：

| 消费者 | 读取内容 | 用途 |
|---|---|---|
| 严格阅读器 / WPS 查看态 | `Appearance` 的 PathObject + TextObject | rofd 排版的固定快照（折点、字距、行高一致） |
| WPS 编辑态 | `Remark`（逻辑文本）+ `GrowMode`（增长提示） | 恢复逻辑文本，按框宽流式重排 |
| 数科等编辑态 | `Remark` + `Appearance` | 恢复逻辑文本与逐 run 样式 |
| rofd | `Remark`（逻辑文本）+ 第一个 TextObject 样式 / 富 `runs` | parley 重新排版，重开仍是流式 |

保存产物（`runs` 为空的 TextBox）：

1. **`Appearance`**
   - `Boundary = rect`；文本框先写边框 `PathObject`（现状不变）；
   - 每视觉行一个 `TextObject`：`Boundary="0 {top} {width} {height}"`（`top`/`height`/`width` 来自 parley 行几何），`TextCode X="0" Y="{baseline - top}"`，`DeltaX` 用**真实每字 advance**（不再用字符类估算）；
   - 这样按外观渲染的阅读器看到的折点与字距 = rofd 屏幕。**这是标准渲染载体**；富文本的逐 run 样式也必须写进 `TextObject`（Font/Size/FillColor/Weight/Italic）。
2. **`Remark`**：逻辑文本（只含 Enter 产生的硬 `\n`，不含软折行），XML 转义。
   - **边缘空白保真**：解析器使用 quick-xml `trim_text(true)` 丢弃元素间缩进，会吃掉内容首尾的空格/换行；因此 `Remark` 与 `TextCode` 的首尾空白字符以数字字符引用（`&#x20;`/`&#xA;` 等）写出，trim 不识别、反转义后还原原文。`"ab\n"`、`" abc"`、全空白内容均原样往返。
3. **`GrowMode`**：有边框（文本框）→ `FixWidth`；无边框（文字批注）→ `None`。
4. **不写 `AllRichText` / `ParagraphFormat`**：两者都不是流式重排来源（数科/WPS 编辑器都从 `Remark` + `Appearance` 重排），rofd 也不读；写入只会产生与外观重复的副本。

保存/解析 API：

- `rofd-io` 新增
  ```rust
  pub fn save_ofd_with_text_layout(doc, pkg, hints: &TextLayoutHints) -> Result<Vec<u8>, OfdError>;
  pub fn write_ofd_with_text_layout(doc, hints: &TextLayoutHints) -> Result<Vec<u8>, OfdError>;
  ```
  `hints` 由组件用与渲染相同的 `shape_wrapped` 生成；旧的 `save_ofd` / `write_ofd` 保留，委托为空 hints（外观退化为按 content 的 `\n` 分行 + 1.2em 行距，供测试/兼容）。
- 组件 `save_snapshot() -> SaveSnapshot { document, text_layout }`：文档只克隆并补足外来超宽框的 `rect.h`（`runs` 为空的 TextBox），content 保持逻辑文本**不写入软折行**。
- 宿主保存时改用 `save_snapshot()` + `save_ofd_with_text_layout`。
- 解析：FreeText 批注的**非空 `Remark` 无条件作为 content**（WPS 新建框没有任何 `Parameter`，仍以 Remark 为逻辑文本；行为测试 2026-11-09）；`Remark` 为空时回退外观内容。样式仍取外观第一个 TextObject。
- 重开：content = 逻辑文本 → 拖宽重新排版；`runs` 仍从外观解析（富外观保真）。

已知边界：

- 外来文件的 `AllRichText`/`ParagraphFormat` 被忽略；若文件只有它们没有 `Remark`，rofd 退回按外观内容。
- 非 FreeText 批注的 WPS 私有参数仍会被丢弃（超出本次范围）。
- WPS 重排使用 Qt 引擎且带 1mm 内边距；rofd 用 parley 且从 rect 原点起排，跨引擎断点可能有细微差异（见 §5）。

### 1.6 Enter 插入显式换行

- 文本光标激活时，`Key::Enter` 等价于在光标处插入 `"\n"`：走与普通字符相同的 measured 插入路径，光标 offset +1（落到下一行行首），同一 Transaction 内含框高增长。
- Backspace/Delete 按普通字符语义删除 `\n`（行首 Backspace 即回并两行）。
- 光标未激活时 Enter 无动作（不影响宿主快捷键）。
- 三端适配器已映射：xilem `masonry_events.rs`、gpui `events.rs`（`"enter" | "return"`）、web `wasm_ofd.rs::map_key`（`"Enter"`），本次无适配器改动。
- IME 组合期间的 Enter 由平台组合语义处理（keydown 在 `isComposing`/229 时被 SDK 丢弃），不在组件路径上。

### 1.7 上下方向键按行移动

- 作用对象：`runs` 为空的 TextBox（有软折行/显式行）。Note 与富 TextBox 保持现状（不响应）。
- 当前光标先映射为（行，列，行内 x）；`ArrowUp` 目标行 = 当前行 − 1，`ArrowDown` = 当前行 + 1。
- **列偏好**：一次垂直移动开始时把当前行内 x 存入 `TextCursor.preferred_x`；连续上下移动沿用该 x，不因短行被截断而丢列。任何非垂直移动（左右键、键入、点击、Enter、paste 等）走后清空（现有 `set_cursor` 已置 None）。
- 目标行内取离 preferred x 最近的字符边界；短行自然夹到行尾。
- 边界：首行再按 `ArrowUp` → offset 0（文首）；末行再按 `ArrowDown` → content 字符数（文末）。
- 与 Enter/折行一致：目标 offset 的显示沿用 §1.3 的映射规则（软折行边界归下一行行首）。
- **行亲和（affinity）**：软折行边界的 offset 在 §1.3 下显示于下一行；垂直移动会记录 `(offset, 目标行)`，当光标 offset 仍等于记录值时，`caret_rect` 用该行显示（`caret_rect_on_line`），下一次上下移动也从该行继续。这样短行夹到行尾时不会视觉跳过一行；任何非垂直移动改变 offset 后亲和自动失效。
- 组件在 `handle_key` 的文本光标分支处理；三端适配器已映射 `ArrowUp`/`ArrowDown`，无适配器改动。
- 光标移动只重绘，不改文档、不产生 Transaction。

## 2. 架构与数据流

排版必须用字体度量，因此**唯一排版实现放在 `rofd-render`**（字体事实来源），不新增依赖边：

```
编辑输入 ──► OfdComponent ──► Editor::*_measured(|payload| 高度)
                │  (注入字体感知的 measure 闭包；editor 保持 dom-only)
                ▼
          rofd-render::shape_wrapped(fonts, font, size, max_width)
                │  (parley break_all_lines(Some(width)), OverflowWrap::Anywhere)
                ├── draw_text_in_rect（屏幕/打印/PDF 导出/convert 共用）
                ├── caret_rect / caret_offset_at_point / caret_offset_vertical
                ├── draw_annotations 的 preedit 内联预览（富 TextBox 仍走 preedit_overlay）
                └── save_snapshot（行几何 + 字距 hints）
                        │
                        ▼
                  rofd-io::save_ofd_with_text_layout
                        ├── Appearance: 逐行 TextObject（快照）
                        ├── Remark: 逻辑文本
                        └── GrowMode（框体伸缩提示）
```

- `Editor` 不依赖 render：新增带 `measure: impl FnOnce(&AnnotationPayload) -> Option<f64>` 的文本命令变体，组件注入闭包；旧命令委托为 `|_| None`。
- `rofd-io` 不依赖 render：外观行几何与字距经 `TextLayoutHints`（`rofd-dom` 类型）传入。
- 宿主只改保存取快照的调用点。

## 3. 改动清单

### 3.1 `rofd-dom`

- 新增（`annotation.rs` 或 `layout.rs`）：
  ```rust
  /// OFD 外观文本行距系数（无 hints 的旧序列化路径使用）。
  pub const TEXTBOX_LINE_PITCH_RATIO: f64 = 1.2;

  /// 一视觉行的对外排版提示（保存外观用）。坐标相对 TextBox 原点（mm）。
  pub struct TextLineHint {
      pub text: String,
      pub top: f64,
      pub baseline: f64,
      pub height: f64,
      pub width: f64,           // 行 advance（TextObject Boundary 宽）
      pub advances: Vec<f64>,   // 每字 advance；io 取前 n-1 个写 DeltaX
  }
  pub type TextLayoutHints = std::collections::HashMap<AnnotationId, Vec<TextLineHint>>;
  ```

### 3.2 `rofd-render`

- `text/shape.rs`：新增 wrapped 版 `shape_grouped_with_family`（`max_advance: Option<f32>` + `OverflowWrap::Anywhere`），返回字体分组 glyphs + 每行元数据（byte range / glyph range / advance / top / baseline / height / break reason）。
- `text/wrap.rs`（新）：
  ```rust
  pub struct WrappedLine {
      pub char_start: usize,
      pub char_len: usize,      // 不含行尾 \n / \r
      pub glyph_start: usize,
      pub glyph_end: usize,
      pub width: f64,
      pub top: f64, pub baseline: f64, pub height: f64,
      pub text: String,
  }
  pub struct WrappedText {
      pub font: Option<FontData>,
      pub glyphs: Rc<Vec<ShapedGlyph>>,
      pub lines: Rc<Vec<WrappedLine>>,
      pub total_chars: usize,
  }
  impl WrappedText {
      pub fn height(&self) -> f64;                       // 末行 top + height
      pub fn line_at_offset(&self, offset: usize) -> (usize, usize);
      pub fn to_layout_hints(&self) -> Vec<rofd_dom::TextLineHint>;
  }
  ```
- `text/font.rs`：`FontStore::shape_wrapped(&self, font_id, text, size, max_width) -> Rc<WrappedText>`，缓存键 `(font_id, size_bits, width_bits, text)`；`register_font` / `register_font_slice` 同时清 wrapped 缓存。入参 content 按 §1.2.1 不变量视为 LF-only。
- `annotation_scene.rs`：`draw_text_in_rect` 改用 `shape_wrapped(rect.w.max(size))`；水印路径不变。
- `caret_rect.rs`：`caret_rect` 对 **`runs` 空的 TextBox** 用 wrapped 布局计算 (line, x, y)；富 TextBox / Note / Watermark 保持旧路径。新增
  ```rust
  pub fn caret_offset_at_point(content, font_id, size, max_width, fonts, x_in_rect, y_in_rect) -> usize;
  pub enum VerticalMove { Up, Down }
  pub fn caret_offset_vertical(content, font_id, size, max_width, fonts,
                               offset, direction, preferred_x: Option<f64>,
                               current_line: Option<usize>) -> (usize, f64, usize);
  pub fn caret_rect_on_line(doc, vp, fonts, ann_id, offset,
                            line_hint: Option<usize>) -> Option<Rect>;
  // caret_rect(...) = caret_rect_on_line(..., None)
  ```
- `annotation_scene.rs`：`draw_annotations` 增加 `preedit: Option<&PreeditPreview>`；平铺 TextBox 用组合后的内容走 `draw_text_in_rect`（内联预览）。
- `preedit_overlay.rs`：只服务 `runs` 非空的富 TextBox（单行前缀宽度定位）；平铺 TextBox 直接返回（由内联预览绘制）。
- `layers.rs`：`OverlayInput` 增加 `preedit` 字段，透传给 `draw_annotations`。
- `lib.rs` 导出新类型/函数。

### 3.3 `rofd-editor`

- `text_commands.rs`：
  ```rust
  pub fn insert_text_measured<F>(&mut self, id, offset, chars, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn delete_text_measured<F>(&mut self, id, offset, len, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  ```
  在同一个 `ReplaceAnnotationStep` 里：改 content（含 `collapse_runs`）→ 调 `measure(&after.payload)` → `Some(h)` 时写 `rect.h = h`。
  **契约**：`measure` 只在需要增长时返回 `Some(required_h)`（且必大于当前 `rect.h`）；编辑器不自行取 max，只负责同事务落地。空内容/无行/字体不可用一律 `None`。
  旧 `insert_text` / `delete_text` 委托为 `|_| None`。
- 插入/替换文本时先归一化换行符：`\r\n` / `\r` → `\n`（§1.2.1），`set_annotation_text` 同样处理。
- 新增 `set_cursor_with_preferred_x(annotation, offset, preferred_x: Option<f32>)`（现有 `set_cursor` 委托为 `None`）。

### 3.4 `rofd-component`

- `handle_key`（Char / Enter / Backspace / Delete / ArrowUp / ArrowDown）、`insert_at_cursor`、`force_commit_preedit`：改走 measured 命令；measure 闭包用组件 `FontStore` 对后置 payload 求 `required_h`，仅当 `required_h > rect.h` 且 `total_chars > 0` 时返回 `Some`。
- `Key::Enter` 在文本光标分支内插入 `"\n"`（§1.6）。
- `Key::ArrowUp` / `Key::ArrowDown`：仅对 `runs` 空的 TextBox，调 `caret_offset_vertical` + `set_cursor_with_preferred_x`；只重绘。
- 新增组件内 helper（如 `commit_resize_with_text_height(id, new_rect)`）：按新宽度求 `required_h` 并入 `new_rect.h`，供公开 `resize_annotation` 与拖拽提交（`DragState::Resize` 分支）共用。
- `click_caret_offset`：补 y 参数；`runs` 空的 TextBox 走 `caret_offset_at_point`，Note/富 TextBox 走旧 `caret_offset_at_x`。
- 新增
  ```rust
  pub struct SaveSnapshot {
      pub document: OfdDocument,
      pub text_layout: rofd_dom::TextLayoutHints,
  }
  pub fn save_snapshot(&self) -> SaveSnapshot
  ```
  实现见 §4.3（克隆文档、`runs` 空 TextBox 生成 hints 并补足 `rect.h`；content 不改写）。
- `caret_rect` 无需改签名（render 内部改）。

### 3.5 宿主

- `xilem-app/src/main.rs`：保存改 `let snap = c.save_snapshot(); host::document_io::save_ofd(&snap.document, package.as_ref(), &snap.text_layout, &path)`。
- `gpui-app/src/shell.rs`：两处保存同样改用 `save_snapshot()`。
- `web-view/src/wasm_ofd.rs`：`saveOfd` 用 `save_snapshot()` + `save_ofd_with_text_layout` / `write_ofd_with_text_layout`。
- tauri 前端复用 web SDK，无额外改动。

### 3.6 `rofd-io`

- 序列化：
  - `serialize_page_annot(page, anns, next_id, resources, hints)`：内部逐 annot 传 hints。
  - `serialize_one`：FreeText 批注额外写 `Remark`（逻辑 content）与 `GrowMode`（§1.5、§4.4）；不写 `AllRichText`/`ParagraphFormat`。
  - `text_object_xml`：有 hints 时按 `TextLineHint` 逐行输出（Boundary `0 top width height`、`TextCode Y = baseline - top`、真实 `DeltaX`）；无 hints 时保持现行为（按 `\n` 分行、行距 `TEXTBOX_LINE_PITCH_RATIO`）。
  - 富 `runs` 路径不变（`text_run_object_xml`），同样写 `Remark` 与 `GrowMode`。
- 新增 `save_ofd_with_text_layout(doc, pkg, hints)` / `write_ofd_with_text_layout(doc, hints)`；旧函数委托空 hints。
- 解析：
  - TextBox 的 content 与 `TextRun.text` 归一化 `\r\n` / `\r` → `\n`；
  - FreeText 的非空 `Remark` 无条件作为 content（WPS 新建框无任何参数也依赖 Remark）；空 Remark 回退外观内容。
- `rofd-io` 不新增字体/排版依赖。

## 4. 关键算法

### 4.1 行文本与字符 offset

- `line.text_range()` 是 **byte** range（parley 0.8），含行尾换行符；空文本可能返回 `0..1` 越界，切片必须 clamp。
- 行文本 = `content[range.clamp(len)]` 去掉结尾 `\r` / `\n`；保留行尾空格（软折行断在空格后，该空格属于上一行）。
- 行字符数 `char_len` = 行文本 chars 数（liga off，char↔glyph 1:1 假设沿用现有代码）。
- `total_chars = content.chars().count()`。

### 4.2 `line_at_offset`

```
if lines.is_empty() -> (0, 0)
let last = lines.len()-1
for i in 0..=last:
    let start = lines[i].char_start; let end = start + lines[i].char_len;
    if offset < start { return (i.saturating_sub(1), ...end of prev) }
    if offset < end { return (i, offset - start) }
    if offset == end {
        let next_start = lines.get(i+1).map(|l| l.char_start);
        // 显式换行间隙（offset 落在 \n 字符上）归本行行尾
        if let Some(ns) = next_start { if offset < ns && ns > end { return (i, char_len) } }
        // 软折行边界或文末
        return match next_start { Some(_) => (i+1, 0), None => (i, char_len) };
    }
(last, lines[last].char_len)
```

单测钉死边界（软折行 / 显式 `\n` / 空 content / 末尾空行）。

### 4.3 保存快照

```
save_snapshot():
  doc = document().clone()
  store = self.font_store.clone().unwrap_or_else(|| self.build_font_store())
  hints = TextLayoutHints::new()
  for ann in doc.annotations.by_page.values_mut().flatten():
      if let TextBox { rect, content, font, size, runs, .. } = &mut ann.payload:
          if !runs.is_empty(): continue
          if content.chars().count() == 0: continue
          w = store.shape_wrapped(font, content, *size, rect.w.max(*size))
          if w.lines.is_empty(): continue
          hints.insert(ann.id.clone(), w.to_layout_hints())
          rect.h = rect.h.max(w.height())
  SaveSnapshot { document: doc, text_layout: hints }
```

- content 不改写（逻辑文本原样进 Remark）。
- `rect.h` 用真实行几何补足（外来超宽框在保存文件里也不会被裁）。

### 4.4 WPS 元数据生成

- `Remark` = `xml_escape_preserving_edges(content)`（首尾空白写成数字字符引用）。
- `GrowMode` = `border.is_some() ? "FixWidth" : "None"`。
- **不生成 `AllRichText` / `ParagraphFormat`**：两者都不是流式重排来源，rofd 也不读；外观已经承载逐 run 样式。

### 4.5 外观 TextObject（有 hints）

```
for i, line in hints[ann_id].iter().enumerate():
    TextObject ID=mint()
        Boundary="0 {line.top} {line.width} {line.height}"
        Font={font} Size={size}
        FillColor={color}
        TextCode X="0" Y="{line.baseline - line.top}"
                 DeltaX="{line.advances[0..n-1] 空格分隔}"
```

- 单字符行不写 DeltaX（与现行为一致）。
- `top`/`baseline`/`height`/`width` 全部来自 parley，保存外观即屏幕快照。

## 5. 已定取舍

1. **软折行（live 不改 content）**：删除/改宽自动重排；保存时逻辑文本进 `Remark`，外观进逐行 TextObject。
2. **不引入 rword 的富文本格式**：rword 的是 OOXML 剪贴板 payload（`app:"rword"` JSON），跨仓耦合且 WPS 不识别；rofd 内部模型不变，持久化只用 OFD 标准字段 + WPS 既有约定。
3. **`TextObject` 保留**：`Appearance` 仍是渲染缓存与样式来源；`Remark`/WPS 参数是旁路元数据，不是替代品。
4. **不写不读 Qt 辅助参数**：`AllRichText`/`ParagraphFormat` 不是流式重排来源（数科/WPS 编辑器都从 `Remark` + `Appearance` 重排），rofd 也不读；持久化只保留 `Remark` + `Appearance` + `GrowMode`。
5. **框只增不减**：无法区分用户手动高度与自动增长，缩高会破坏用户意图。
6. **富文本框不重排**：外来富外观是保真快照；首次编辑 collapse runs 后按普通 TextBox 处理。
7. **行几何精确落盘**：hints 带 top/baseline/height/width/advances，外观与屏幕一致，不再用 1.2em 近似（旧 API 保留近似）。
8. **不对齐 WPS 1mm 内边距**：rofd 文字从 rect 原点起排；WPS 编辑态从其 1mm 内边距起排，跨引擎断点可能有细微差异（格式/引擎固有，WPS 打开第三方文件同样存在）。
9. **非空 `Remark` 无条件优先**：WPS 新建的 FreeText 框没有任何 `Parameter`，仍以 Remark 为逻辑文本并支持重开重排（行为测试 2026-11-09）；因此不再要求 WPS 参数存在。`Remark` 为空才回退外观。

## 6. 测试计划（TDD，先红后绿）

### rofd-render

- `shape_wrapped`：西文按空格折、CJK 逐字折、超长单词 emergency break、显式 `\n`、空 content、行文本/字符 offset 正确。
- `line_at_offset`：软折行边界归下一行、`\n` 前归本行行尾、文末、末尾空行。
- `caret_rect`：第二行 offset 的 y = rect.y + line.top；软折行边界；单行行为与现值一致（现有测试保持绿）。
- `caret_offset_at_point`：点第二行返回第二行 offset；点行首/行尾；y 夹紧。
- `caret_offset_vertical`：软折行上/下移；显式 `\n` 上/下移；连续下移保留 preferred_x（长行→短行→长行）；短行夹到行尾；首行 Up → 0、末行 Down → 文末。
- `caret_rect` 高度 = 行框高（`line.height`），光学重心随之下移；legacy 路径仍 `size`。
- `annotation_text_selection_rects`：单行部分区间（x/宽取字形 pen 与行尾）、跨行两矩形（第二矩形 y = 第二行 top）、空区间/反向区间/未知批注/富 TextBox。
- `preedit` 内联预览：组合点在文本中间时组合串之后的字符 x 后移（不与组合串重叠）；折行边界处组合串落到下一行、目标以外批注不受影响；所有 TextBox（含 spans 框）都内联，detached overlay 已移除（2026-11-11 修订）。
- 绘制：折行后各行 baseline 落在 rect 内；`to_layout_hints` 的 top/baseline/width/advances 与 glyph 位置一致。
- 富 TextBox（`runs` 非空）：绘制仍走 `draw_text_runs`；`caret_rect` / preedit 保持旧单行定位。

### rofd-editor

- `insert_text_measured` 返回 `Some(h)` → 同一 Transaction 内 content + rect.h 都变；一次 undo 同时还原。
- `delete_text_measured` 同理；`None` 时行为与旧命令一致。
- `replace_text_measured`：区间替换 + measure 同事务；一次 undo 还原文本与高度；越界/反向区间夹紧；空文本即删除。
- 换行符归一化：插入/替换 `\r\n`、孤立 `\r` 后 content 只含 `\n`。
- `set_cursor_with_preferred_x` 往返：`preferred_x` 保留，旧 `set_cursor` 置 `None`。

### rofd-component

- 框内拖选：编辑态 Text 工具在框内按下→拖动形成选区（anchor/focus 正确）、释放保留、不移动框、不改 modified；首拖未聚焦框仍是移动、Hand 工具仍是移动；普通点击清除；←/→ 折叠到首/尾。
- 选区消费：键入/Enter/粘贴/IME 替换（单 Transaction、光标落在插入末尾）、Backspace/Delete 删除、IME 组合开始先删选区；undo 还原；高亮矩形随选区绘制。
- 窄文本框逐字输入：content 不含 `\n`、rect.h 按折行增长；继续输入到第二行、第三行。
- Enter：插入 `\n`、光标 offset +1、框高按显式行数增长；行首 Backspace 回并。
- ArrowUp/ArrowDown：跨行移动；连续下移保留列偏好，上移回原列；首行/末行边界；点击/左右键/键入后 preferred_x 清空；Note 与富 TextBox 不响应。
- Backspace/Delete 后框不缩；内容正确。
- paste / IME commit 路径同样增长。
- 点击第二行文字设置正确 cursor offset。
- `resize_annotation` 窄化后高度增长；加宽不缩高。
- `save_snapshot`：逻辑 content 不被改写；hints 行数与渲染一致；外来超宽框 rect.h 补足；rich runs / Note / Watermark 无 hints。
- 保存→解析：content 仍为逻辑文本（无软折行 `\n`），重开后改宽行数变化（真实流式）。

### rofd-io

- 序列化：hints 逐行输出 TextObject（top/baseline/DeltaX 精确）；无 hints 走旧分行 + 1.2em。
- FreeText 输出含 `Remark`/`GrowMode`（不含 `AllRichText`/`ParagraphFormat`）；`GrowMode` 按 border 区分；富 runs 的外观 TextObject 保留。
- 解析：非空 `Remark` 无条件作为 content（无参数文件同样如此）；空 Remark 回退外观；`\r\n` 归一化。
- 现有 `annotation_roundtrip` / conformance 用例保持绿（必要时更新结构断言）。

### 集成（可选，真实样例 `#[ignore]`）

- `cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd` 手测：新建文字批注连续输入中文/超长英文，观察折行与框向下扩展；保存重开后再拖宽，观察流式重排。
- 用 WPS 打开 rofd 保存的样例文件，确认文字/样式/增长模式可编辑与重排（人工）。

## 7. 风险

- **每次按键全量排版 O(n)**：批注文本短，可接受；wrapped 缓存按 `(font,size,width,text)` 命中重复渲染。
- **字体缺失**（wasm 字体未加载完就保存）：`shape_wrapped` 无可用字形时不折行；与现有渲染降级一致。
- **跨引擎断点差异**：rofd(parley, 0 内边距) vs WPS(Qt, 1mm 内边距)；外观快照保证“按外观渲染”一致，WPS 编辑态重排可能有细微差异。
- **`Remark` 与外观漂移**：若第三方只改外观不改 `Remark`，rofd 会显示 `Remark` 的旧文本；这是 WPS 约定的固有限制（WPS 自己也会遇到）。
- **私有参数丢失**：非 FreeText 批注的 WPS 私有 Parameter 仍不保留（现状，超出本次范围）。
