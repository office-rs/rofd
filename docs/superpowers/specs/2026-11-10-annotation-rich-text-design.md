# 批注文本框的分段样式（选区格式化）设计

- 日期：2026-11-10
- 状态：已实现（内核 + io + web SDK 面板联动；§8 回归门已跑绿）
- 关联：[`2026-11-10-annotation-format-panel-design.md`](2026-11-10-annotation-format-panel-design.md)（本设计**取代**其 §1.6 的"富 runs 一律塌缩"行为）、[`2026-11-09-annotation-text-wrap-design.md`](2026-11-09-annotation-text-wrap-design.md)（流式排版的基线）

---

## 1. 为什么

格式面板（字族/字号/颜色）目前只作用于**整框**：`TextBox` 只有一个平坦 `size`/`color`/`font`，富文本框的 `runs` 是外来文件的只读保真快照，任何编辑都把它塌缩掉。用户要求：**框内有选中文字时，只改选中的那段**。

要做到这一点，文本框必须从"单一样式"升级为"可编辑的分段样式"（rich text）。这是既定的产品方向，不是补丁：

| 用户决策 | 取值 |
| --- | --- |
| 外来富文本框（`runs` 带绝对 x/baseline） | **转成流式**：丢弃原绝对位置，按框宽重排（与原外观可能有细微差异，接受） |
| 选区覆盖全框时的归一 | **合并**：全选改样式后归并回平坦字段，`spans` 清空 |

### 非目标（v1）

- 面板不加粗体/斜体/下划线按钮（`bold`/`italic` 仅作为外来样式的携带与保真回合，不可由 UI 新设）。
- 不做段落级属性（对齐、行距、缩进）。
- 不做 body（正文）格式化——§4.2 不变。
- xilem-view 面板仍按旧 spec §5.3 挂账。
- 富文本框的 IME 组合串**内联**进流式行（2026-11-11 修订，取代早先的 detached overlay）：组合串作为插入点样式的一段参与折行，组合点之后的文字随之后移，不再与既有文字重叠。

---

## 2. 数据模型

### 2.1 `TextRun` → `TextSpan`

```rust
/// One styled fragment of a TextBox's content. Spans are ordered and their
/// texts concatenate to the box content (see the invariant below).
pub struct TextSpan {
    pub text: String,
    pub font: FontId,
    pub size: f64,
    /// None = inherit the box's flat `color`.
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}
```

- 绝对位置字段 `x` / `baseline` **删除**。位置一律由流式排版计算。
- `AnnotationPayload::TextBox { runs: Vec<TextRun> }` 改名为 `{ spans: Vec<TextSpan> }`。
- 这是内部数据模型（dom 的 serde JSON 只用于测试/调试，磁盘格式是 OFD XML），改名不涉及磁盘兼容。

### 2.2 不变量

1. `spans.is_empty()` ⇔ 平坦框：整框用 `font`/`size`/`color` 单一样式。
2. `spans` 非空时：`spans.iter().map(|s| s.text.chars().count()).sum() == content.chars().count()`，且 `spans` 顺序拼接**逐字等于** `content`。
3. `spans` 中不出现空的 span；相邻 span 的样式（font/size/color/bold/italic）不得完全相等（随时合并）。
4. `content` 仍是唯一逻辑文本（LF-only、软折行不入 `content`），`Remark` 继续承载它。

不变量由编辑器命令层维护（§4），dom 只存数据。解析路径负责把外来外观规范化成满足不变量的 `spans`（§6）。

### 2.3 样式语义

- 平坦字段（`font`/`size`/`color`）是**基线样式**：`spans` 为空时是整框样式；`spans` 非空时是"没有选中文字时面板显示/整框操作"的样式值。
- `span.color = None` 渲染时回落 `color`。解析产生的 span 保留 `None` 的原始语义（源 TextObject 没有 `FillColor`）。
- `span.size` 是 span 自己的字号（mm）；行高由排版取行内最大值，caret 高度随行高。

---

## 3. 排版（rofd-render）

### 3.1 新的排版输入

```rust
/// Style run over byte ranges of the content, in reading order.
pub struct LayoutRun {
    pub byte_range: Range<usize>,
    pub font: FontId,
    pub size: f64,
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}
```

构造：`spans` 为空 → 一个覆盖全内容的平坦 run（用 `font`/`size`/`color`）；否则按 `spans` 的字符数累计换算成字节区间。这个转换函数放 `crates/render/src/text/spans.rs`（`layout_runs(content, spans, flat…) -> Vec<LayoutRun>`），render/component 共用。

### 3.2 `FontStore::shape_wrapped`

- 改为接受 `&[LayoutRun]`（连同 `content` 与 `max_width`）：
  `shape_wrapped_runs(&self, content: &str, runs: &[LayoutRun], max_width: f64) -> Rc<WrappedText>`。
- parley 的 `RangedBuilder` 按 run 的字节区间 `push` 每段 `FontSize` / `FontFamily`（各自的 `family_list_for`）/ `FontWeight` / `FontStyle`；`liga=0`、`OverflowWrap::Anywhere`、`push_default` 的兜底样式不变。
- 平坦框是"单 run"特例，`draw_text_in_rect`、`caret_rect`、`measure`、`save_snapshot` 全部走同一入口——**删除"富框走 legacy 几何"的分叉**。
- 缓存键从 `(font_id, size, width, text)` 变为 `(width, text, style_signature)`，`style_signature` 是 `runs` 的 `(byte_range, font, size, bold, italic)` 紧凑串；字形缓存仍按 `(font_id, size, text)`（span 不改变逐段 shaping 键）。字体集变化时两者一起清空（现状）。

### 3.3 `WrappedText` 的扩展

- 现有 `glyphs` 仍是**按行序展平的** `ShapedGlyph` 列表（caret/hit/advances 继续用它）。
- 新增 `pub spans: Rc<Vec<ShapedSpan>>`：
  ```rust
  pub struct ShapedSpan {
      pub style_index: usize, // index into the LayoutRun slice passed to shape
      pub font: FontData,     // parley 选中的实际字体（含回退）
      pub size: f64,
      pub bold: bool,
      pub italic: bool,
      pub glyph_start: usize, // index into WrappedText::glyphs
      pub glyph_end: usize,
  }
  ```
  由 parley 的 `GlyphRun` 顺序产生；相邻且样式相同的 group 合并。跨行时同一 span 拆成多个 `ShapedSpan`（绘制互不影响）。
- **`color` 不进 `ShapedSpan`、也不进缓存键**：颜色不影响 parley shaping，绘制时用 `style_index` 回查 `LayoutRun.color`（`None` 回落平坦 `color`）。否则纯换色会命中旧缓存、两个同字同宽不同色的框会串色。
- 删除单值 `font` 字段及 `draw_text_in_rect` 里"整段一个字体"的假设。
- `WrappedLine` 的 `height`/`baseline` 已来自 parley（行内最大字号），无需改。

### 3.4 绘制

`draw_annotations` 的 `TextBox` 分支统一为：

```text
layout_runs -> shape_wrapped_runs -> for span in wrapped.spans:
    affine = base * translate(rect.x, rect.y)
    italic 时左乘 skew（沿用 ITALIC_SKEW）
    color = runs[span.style_index].color.unwrap_or(flat color)
    用该 span 的 font/size/color 画一次；bold 时再偏移 BOLD_EMBOLDEN_RATIO 画第二次
```

即把 `draw_text_runs` 的合成粗体/斜体/颜色逻辑搬进按 `ShapedSpan` 的循环，删除按 `TextRun.x/baseline` 定位的旧实现。边界框（border）绘制不变。

### 3.5 caret / 选区 / 点击落点

- `caret_rect_on_line`：删除"runs 非空走 legacy 单行几何"的分支——所有 TextBox 都按 `shape_wrapped_runs` 求 `(line, local)`，caret 高度 = `line.height`（行内最大字号自然生效）。
- `annotation_text_selection_rects`、`click_caret_offset`、`line_at_offset`、上下键的视觉行移动全部自动继承（它们都消费 `WrappedText`）。
- `WrappedText::font` 删除后，这些调用点不需要字体，编译期会指出残留。

---

## 4. 编辑器命令（rofd-editor）

### 4.1 纯函数辅助（新文件 `crates/editor/src/text_spans.rs`）

对 `AnnotationPayload::TextBox` 操作，全部维护 §2.2 不变量：

```rust
fn ensure_spans(payload)                      // 空 spans + 非空 content -> 用平坦样式生成一个 span
fn spans_insert(payload, offset, text)        // 在 offset 处插入，继承 offset-1（或 0 处下一个）字符的样式
fn spans_delete(payload, offset, len)         // 删字符，删除后合并相邻同样式、丢弃空 span
fn spans_apply(payload, range, f: impl Fn(&mut TextSpan))
                                              // 在 range 两端切开，对覆盖的 span 应用样式，再归一（合并/清空）
```

`spans_apply` 的归一规则（用户决策 2）：

1. `range` 为 `None` 或空区间（无选中文字）→ 直接写平坦字段并 `spans.clear()`（整框操作，语义明确）。
2. `range` 非空 → `ensure_spans` → 两端切开 → 对覆盖的每个 span 应用该属性 → 丢弃空 span → 合并相邻同样式。
3. 应用后**只有结果可被平坦模型无损表达时才合并**：`spans.len() == 1`，且该 span 的 `bold == false && italic == false`。此时 `flat.color = span.color.unwrap_or(flat.color)`、`flat.font/size = span.font/size`，`spans.clear()`；否则保留 spans。

这样"全选 → 改字号"在样式本来就统一的框上归并回平坦框（用户要的"合并"），但在多色/含粗斜体的框上只改字号、保留其余差异——不会因为一次全选操作把颜色和粗斜体全部抹平。面板不提供粗/斜按钮，`bold`/`italic` 只可能来自外来文件，故"保留 spans"是正确归宿。

`color` 的写法：span 设 `color = Some(c)`；整框合并时写 `payload.color`。字号/字族的整框合并写 `size`/`font`。

### 4.2 文本编辑保留 spans

`insert_text_measured` / `delete_text_measured` / `replace_text_measured` / paste / IME 提交：把现在的 `collapse_runs` 调用替换为 `spans_insert` / `spans_delete`；`set_annotation_text`（整段替换）仍清空 `spans`（新文本无样式来源）。

- 插入的字符继承插入点前一个字符的样式；`offset == 0` 时继承后一个字符；空框则用平坦样式。
- 一个编辑仍是一个 `Transaction`，undo 一次完整还原 span 与文字。

### 4.3 样式命令签名

```rust
set_annotation_font_size_measured(id, range: Option<(usize, usize)>, size_mm, measure)
set_annotation_font_family_measured(id, range: Option<(usize, usize)>, family, measure)
set_annotation_color(id, range: Option<(usize, usize)>, color)
```

- `range == None` 或空区间 → 整框（§4.1 规则 1 的直接写平坦字段）。
- `range == Some((s, e))` 非空 → clamp 到内容长度后交给 `spans_apply`（§4.1 规则 2/3）。覆盖全内容时**仍走规则 3 的无损合并**，而不是绕过检查直接抹平。
- `measure` 闭包契约不变（返回需要增高的新高度；只增不减），只是它的排版输入现在是 span-aware。

字体族仍是 v1 规则：`span.font = FontId(显示族名)`（面板给归一化键，组件映射到该项 display），不建 Font 资源、不落盘字体字节。渲染对未知 FontId 依次尝试：归一化键 → 切片链 → 已注册族名（fontique 实际名，避开 `times` 这类别名键）→ 原 id。

---

## 5. 组件层（rofd-component）

### 5.1 选区感知

组件持有 `annot_text_selection`（`AnnotTextSelection`，仅 UI 预览态，不入 dom/history）。三个 setter 把 `self.annot_selection_range(id)` 作为 `range` 传给编辑器：

```text
有选中文字 -> Some(range)  -> 只改选区
无选中文字 -> None         -> 整框（合并）
```

适配器 API 不变：面板仍调用 `set_annotation_font_size(id, mm)` 等，无需知道选区。这正是"SDK 自带能力"的边界。

### 5.2 样式快照

`annotation_text_style(id)` 变为选区感知：

- 有选中文字：取选区**首字符**所在 span 的样式（`spans` 为空则平坦样式）；`rich = !spans.is_empty()`。
- 无选中文字：平坦字段（现状）。

混合样式选区 v1 直接显示首字符样式（不做"多值/—"指示；记入 §9 已决项）。字族解析（`family_display_name` → `available_font_families` → `FontId`）对 span 的 `font` 同样适用。

### 5.3 增高测量

`measure_textbox_growth` 不再因 `runs` 非空而返回 `None`：所有 TextBox 都按 span-aware 排版求 `required = wrapped.height()`，只在 `required > rect.h` 时返回（只向下增高不变）。

### 5.4 保存快照

`save_snapshot` 不再跳过富框：所有非空 TextBox 都算 `TextLayoutHints` 并写入；高度不足时同样增高 `rect.h`。

### 5.5 移除 `runs` 门控（否则功能只能生效一次）

"富框"曾是不可编辑的外来快照，组件里有五处 `runs` 早退；span 化后必须全部删除，否则用户改过一次样式、框里有了 spans，就再也无法选中/编辑它：

| 位置 | 现状 | 改为 |
| --- | --- | --- |
| `ofd_component.rs` `is_flat_textbox`（约 :810，用在 :2811） | 只有平坦框可用 Text 工具拖选 | 删除该判断（所有 TextBox 可拖选） |
| `ofd_component.rs` `with_text_height`（:1261） | 富框 resize 不增高 | span-aware 排版增高 |
| `ofd_component.rs` 上下键视觉行移动（:1717） | 富框返回 `None` | span-aware 排版 |
| `ofd_component.rs` `click_caret_offset`（:2931） | 富框走 `caret_offset_at_x` 单行近似 | 统一 `caret_offset_at_point` |
| `render/caret_rect.rs` `annotation_text_selection_rects`（:272） | 富框返回空（无高亮） | 统一 span-aware 行矩形 |

同类的 `save_snapshot`（:608）与 `measure_textbox_growth`（:3956）已由 §5.3/§5.4 覆盖。IME 组合串对 span 框也已内联（与平坦框同一条 `layout_runs_with_preedit` 路径），不再使用 detached overlay。

---

## 6. IO（rofd-io）

### 6.1 解析（`parse/annotation.rs`）

TextBox 解析改为"规范化成流式 spans"。关键点：外观里只有**视觉行**，`Remark` 里有**硬换行**；软折行在两边都不产生字符，因此重开时必须按字符对齐，而不是按行拼 `\n`。

1. 收集外观 `TextObject` → 按 `(baseline, x)` 排序 → 逐段文本（v1 的 reading-order 规则）。
2. `appearance_text` = 各段文本直接拼接（不含任何换行；解析器不插入软换行）。
3. 选 `content` 并按需对齐：
   - `Remark` 非空：`content = Remark`（现状）。双指针遍历 `content`（跳过 `\n`）与 `appearance_text`；每段外观文本映射为 `content` 的一段连续字符。**夹在两段之间的 `\n` 归前一段**（使其 text 覆盖这些字符）。全部字符合上 → `spans` 成立且满足 §2.2 不变量；任一字符对不上 → `spans.clear()`（降级平坦，首段样式当平坦样式）并发警告（`OfdWarning::SkippedObject`，reason 注明样式对齐失败）。为避免改动 `parse_page_annot` 的公开签名：保留原函数（丢警告），新增 `parse_page_annot_with_warnings(xml, page, &mut Vec<OfdWarning>)` 供 `parse/mod.rs` 收集。
   - `Remark` 为空（无逻辑文本可依）：`content = flatten`，即 baseline 跳变 > 0.3×较小字号时在**前一段文本尾部**追加 `\n`（v1 规则）；这样构造出的段文本天然拼接 === `content`，直接作为 `spans`。
4. 样式完全一致（无 bold/italic、相邻段 font/size/color 相等）→ `spans.clear()`（v1 的 flat 优化保留）。
5. 否则 `spans` = 规范化后的段列表。

软折行回环的关键：序列化每行一个 TextObject，但 `Remark` 不含软换行；重开时第 3 步的对齐把行文本首尾相接，**只有 `Remark` 里的真实 `\n` 会落进 span 文本**，软换行不产生字符。因此"长段文字里给两个词上色 → 保存 → 重开"样式仍在。

空行：`Remark` 里的连续 `\n` 由第 3 步归前一段覆盖，不依赖外观里是否存在空 TextObject。

### 6.2 布局提示（`TextLineHint`）

```rust
pub struct TextLineFragmentHint {
    pub char_start: usize,   // 行内字符偏移
    pub char_len: usize,
    pub x: f64,              // 行内 pen 起点（mm）
    pub advances: Vec<f64>,  // 每个字符的步进
}

pub struct TextLineHint {
    pub char_start: usize,   // 本行在 content 中的起始字符偏移（新增）
    pub text: String,
    pub top: f64,
    pub baseline: f64,
    pub height: f64,
    pub width: f64,
    pub advances: Vec<f64>,           // 平坦路径沿用
    pub fragments: Vec<TextLineFragmentHint>, // 每行按样式切段；平坦框也有（序列化平坦分支不消费）
}
```

`WrappedText::to_layout_hints` 生成 `fragments`：对每行，把 `spans × line.glyph_range` 求交，得到每个片段的 `x`（首个 glyph 的 x）、`advances`（相邻 glyph x 差，末字用 `span`/行的 pen 位置）。字形与字符 1:1（`liga=0`），片段字符数 = glyph 数。

### 6.3 序列化（`serialize/annotation.rs`）

TextBox 分支：

- `spans` 非空且 hints 存在（按行）：
  - 每行 → 每个 fragment 一个 `<TextObject>`（**坐标一律 appearance-local**，与现有 `text_object_xml_from_hints` 的 `Boundary="0 {line.top} …"` 同系；严格阅读器会自行叠加 `Appearance/@Boundary` 原点）：
    `Boundary = (frag.x, line.top, frag_width, line.height)`，
    `Font/Size` 取覆盖该 fragment 全局字符区间（`line.char_start + frag.char_start`）的 span，
    `Italic/Weight` 同 span，
    `FillColor` **只在 `span.color == Some(c)` 时写**（`None` 不写，保持 §2.3 的"无 FillColor"语义，重开仍是 `None`），
    `TextCode X=0 Y=(baseline - line.top)` + `DeltaX`（fragment.advances 的前 n-1 个）。
  - 空行仍写一个空 `<TextObject>`（宽 0，保留空行；解析第 3 步不依赖它）。
  - 同一行的多个 fragment 共享 `Y`；解析按 `(baseline, x)` 重排 + 跳换行对齐 → 重开还原样式与换行。
- `spans` 非空但 hints 缺失（无 `SaveSnapshot` 的裸 `save_ofd`）：降级为按 `content` 的平坦 TextObject（样式丢失，文档化）。宿主路径永远带 hints。
- `spans` 为空：现有 hints / legacy 两分支不变。

### 6.4 位图/ID/字节保留

- 每个 TextObject 仍从 `mint_object_id` 取号，`MaxUnitID` 补丁逻辑不变。
- 手术刀保存的核心测试（`round_trip.rs`、`save_surgical.rs`）必须保持绿。

---

## 7. 适配器

- **web SDK**：无 API 变化。`annotationTextStyle` 在选区存在时返回首字符样式，面板标签随之更新；选区变化本身已触发重绘（`set_annot_text_selection` → `mark_scene_dirty`），调度器的 `refresh()` 会重读样式。补一条 vitest：选区样式变化后面板标签跟随。
- **gpui-view**：同上，无 API 变化；`FormatPanelState::refresh` 已每帧重读。
- **xilem-view**：仍无面板（挂账），本设计不增加其工作量。

---

## 8. 测试计划（TDD，先红后绿）

| 层 | 用例（核心） |
| --- | --- |
| dom | `TextSpan` serde 回环；`spans` 默认空（旧 JSON）；不变量的构造帮助函数 |
| editor | `spans_insert/delete/apply` 表驱动（开头/中间/结尾/跨 span/全删/全选合并）；全选改字号在统一样式框上归并、在多色框上保留其余差异；文本编辑保留 span 且 undo 一次还原；`set_*_measured` 带 range 时只改选区、一 Transaction |
| render | 双字号/双颜色 span 的折行：行高 = 行内最大字号；`ShapedSpan` 分组正确；**同文本同宽不同颜色的两个框共用缓存也不串色**；纯换色后重排取到新颜色；caret 在选区首/尾的 x/y；`to_layout_hints` 的 fragments 覆盖每行且 x/advances 单调 |
| component | 有 `annot_text_selection` 时 setter 只改选区、无选区改整框；`annotation_text_style` 选区感知；含 spans 的框可拖选/可上下键移光标/可 resize 增高（§5.5 五处门控回归）；`save_snapshot` 为富框产出 hints 并增高；IME/paste 保留 span |
| io | 解析外来富框 → spans 满足不变量；**软折行长段：两处上色 → serialize → parse → 样式仍在**；序列化按 fragment 写 TextObject（appearance-local Boundary、样式属性、`None` 颜色不写 FillColor 逐个断言）；serialize→parse 回环保留每段样式与换行；Remark 与外观无法对齐 → 降级平坦 + 警告；空行回环 |
| web SDK | 面板标签随选区样式刷新（vitest，mock host 返回值切换） |

回归门：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、SDK `vitest` + `tsc --noEmit`。

---

## 9. 风险与已决项

| 项 | 结论 |
| --- | --- |
| 外来富框转流式后与原外观的差异 | 接受（用户决策 1）；原 x/baseline 不保留 |
| Remark 与外观文本不一致 | 降级为平坦样式 + `OfdWarning`；不猜测映射 |
| 混合样式选区显示 | 显示首字符样式（不做"多值"指示） |
| 空行在逐段序列化中丢失 | 每行至少一个 TextObject；回环测试钉死 |
| 缓存键膨胀 | span 签名只含区间/字体/字号/权重/斜体，短串；字体集变化清空（现状） |
| 富框 IME 预览 | 2026-11-11 起与平坦框同路：内联进流式行（组合串继承插入点 span 样式） |
| 性能 | 每次样式变更重排一次（与现在的字号变更同量级）；选区编辑只重排受影响框 |

---

## 10. 实施顺序（供 plan 拆分）

1. dom：`TextSpan`/`spans` 改名 + serde 测试（全仓构造点同步改）。
2. render：`LayoutRun` + `shape_wrapped_runs` + `ShapedSpan` 绘制；caret/hit 统一。
3. editor：`text_spans` 纯函数 + 文本编辑保留 spans + range setter。
4. component：选区感知的 style/setter、measure、`save_snapshot`，以及移除 §5.5 的 `runs` 门控（含拖选/上下键/resize）。
5. io：解析规范化、`TextLineHint.fragments`、序列化逐 fragment。
6. 适配器回归（web/gpui 面板标签随选区刷新）。
7. 文档：更新 AGENTS §rofd-component 条目、旧面板 spec §1.6 状态。
