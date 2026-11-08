# TextBox Run 渲染 + Web IME 桥接 + 文字批注/文本框工具 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 web app 三个缺陷：① 数科式按样式分段（run）的文本框渲染错乱（纵向堆叠/颜色粗斜体丢失/排版丢失）；② web 上中文 IME 无法输入；③ 注释面板缺"文字批注/文本框"创建工具。

**Architecture:** ① 在 dom `AnnotationPayload::TextBox` 增加 `runs: Vec<TextRun>`（解析自 Appearance 的逐段样式+位置）；渲染层有 runs 时按段绝对定位绘制（合成粗体=二次偏移绘制、斜体=切变矩阵），编辑层首次文本编辑把 runs 塌缩为平铺样式（一个 Transaction，undo 可还原）；序列化把 runs 回写为逐段 TextObject（未编辑的外来文件外观保真 round-trip）。② web SDK 用隐藏 `<input>` 承载 IME 组合（compositionstart/update/end → `handleImePreedit/handleImeCommit` → 组件既有 preedit 状态机），keydown 在组合期间门控。③ 组件新增 `CreateKind::Typewriter`（点击放置）/`CreateKind::TextBox`（拖拽画框），wasm 工具串 + App.vue 按钮。

**Tech Stack:** Rust（dom/io/render/editor/component/web-view 六 crate）+ TypeScript SDK（wasm-bindgen 面）+ Vue（App.vue ribbon）+ vitest。

**Spec:** 根因调查结论见会话记录（用户文件 `test/sample.ofd` Annot ID=149 为 run 结构实证）。相关既有 spec：`docs/superpowers/specs/2026-09-10-*-markup-*.md`（§4.1 工具模型）、AGENTS §4.3 手术刀保存、§4.4 不取挂钟。

## Global Constraints

- 提交信息与代码注释一律英文（AGENTS §10）；设计文档中文。
- 严格 5 层单向依赖（AGENTS §3）；render 不依赖 io；component 不依赖 io。
- 库内不取系统时钟（AGENTS §4.4）；`Instant` 仅 blink。
- 手术刀保存核心测试必须保持绿：`cargo test -p rofd-io`（round_trip.rs / save_surgical.rs）。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check` 零告警。
- TS 改动后跑一次 formatter（sdk/web-app 有 prettier 约定，跟随现有风格）。
- serde 兼容：`TextBox.runs` 带 `#[serde(default)]`，旧 JSON/模型可反序列化。

## Review Focus

1. **一 TextObject 多 TextCode**（spec 合法，每 Code 一行）：旧解析只留最后一段——Task 2 测试钉死全部拼接。
2. **编辑 run 富文本后保存再打开**：runs 塌缩后序列化必须可解析回平铺内容——Task 3+5 联测（edit→serialize→parse）。
3. **新工具创建的空 TextBox**（runs 空、content 空）序列化→解析 round-trip——Task 3 测试钉死空内容路径。
4. **组合期间 Chrome 的 Process keydown 不得双插**：Task 8 门控测试（isComposing/keyCode 229）。
5. **组合提交后 preedit 必须清空**（否则组件残留 preedit 覆盖下次输入）——Task 8 断言 `handleImePreedit('')` 在 commit 后被调。
6. **手术刀保存含 runs 批注的文件**：未触碰条目字节保留 + 批注条目重写可解析——Task 3 扩展 save_surgical 断言。

---

### Task 1: dom `TextRun` + `TextBox.runs`

**Files:**
- Modify: `crates/dom/src/annotation.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces（后续所有任务消费）:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct TextRun {
      pub text: String,
      pub font: FontId,
      pub size: f64,
      pub color: Option<Color>,
      pub bold: bool,
      pub italic: bool,
      /// Pen x / baseline y，appearance-local mm（相对 Appearance/@Boundary 原点）。
      pub x: f64,
      pub baseline: f64,
  }
  ```
  `AnnotationPayload::TextBox` 追加 `#[serde(default)] pub runs: Vec<TextRun>`（排在 `border` 之后）。re-export `TextRun`（dom lib.rs 现有 re-export 处）。

- [ ] **Step 1: 失败测试**：`annotation_payload_textbox_runs_round_trips`（带 runs 的 TextBox serde round-trip）+ 旧 JSON（无 runs 字段）反序列化 runs 为空。
- [ ] **Step 2: 跑测试确认编译失败**（`cargo test -p rofd-dom`，expected: no field runs）。
- [ ] **Step 3: 实现** `TextRun` + 字段；修复全仓所有 `TextBox {` 字面量缺字段处（编译器逐一指出，tests 里的构造补 `runs: vec![]`）。
- [ ] **Step 4: `cargo test -p rofd-dom` 全绿。**
- [ ] **Step 5: Commit** `feat(dom): add TextRun and TextBox.runs for rich appearance fidelity`

### Task 2: io 解析——逐 run 收集 + 同行平铺拼接 + 多 TextCode 修复 + Italic/Weight

**Files:**
- Modify: `crates/io/src/parse/annotation.rs`（`AppearanceObject::Text`、`PendingAnnot` 收集循环、`build_payload` TextBox 分支）
- Test: 同文件 tests

**Interfaces:**
- Consumes: Task 1 `TextRun`。
- Produces: TextBox payload 解析产物带 runs（appearance-local 坐标）；`AppearanceObject::Text` 改为：
  ```rust
  Text {
      boundary: Rect,
      font: String,
      size: f64,
      fill: Option<Color>,
      italic: bool,                 // @Italic="true"
      weight: Option<u32>,          // @Weight，>=600 视为 bold
      codes: Vec<(String, f64, f64)>, // 每 TextCode: (text, X, Y)，修复旧“覆盖只留最后一段”
  }
  ```
- 同行判定（flat content 拼接规则）：runs 按 `(baseline, x)` 排序；与前一段 `|baseline 差| > 0.3 * min(size)` → 插入 `'\n'`，否则直接拼接。WPS 两行样本（baseline 6.20 vs 12.84）判为两行；数科四段（全 6.63）判为一行。
- run 坐标：`x = obj.boundary.x + code.X`，`baseline = obj.boundary.y + code.Y`（appearance-local）。
- 平铺回退样式（payload 的 font/size/color）：取**文档序第一个** TextObject 的 font/size/fill（维持既有测试 `freetext_single_textobject_captures_content` 语义）；color 仍 `unwrap_or_default()`。

- [ ] **Step 1: 失败测试**（XML 字符串夹具，取自用户文件 Annot 149 原文，四个 TextObject、含 Italic/Weight/FillColor/Boundary）：断言 content == "发的古典风格"（无 \n）、runs.len()==4、runs[3] 为 `{text:"格", bold:true, color:Some(Rgb(75,172,198)), x≈24.4596, baseline≈6.631}`；runs[2].italic==true；runs[1].size==6.35。
- [ ] **Step 2: 失败测试 2**：一个 TextObject 含两个 TextCode → content 为两段拼接（无丢失）。
- [ ] **Step 3: 失败测试 3**：既有 `freetext_multi_textobject_concatenates_all_lines` 保持绿（WPS 两行 → "first line\nsecond line"），并断言 runs.len()==2、两 run baseline 差 > 5。
- [ ] **Step 4: 跑测试确认新测试失败。**
- [ ] **Step 5: 实现**：TextCode 闭合改为 push 进 `codes`；TextObject 闭合收 `italic/weight`；`build_payload` TextBox 分支按上述规则产 runs + 平铺 content。DeltaX 忽略（渲染自行 shaping，与现状一致）。
- [ ] **Step 6: `cargo test -p rofd-io` 全绿（含手术刀/round-trip 既有套件）。**
- [ ] **Step 7: Commit** `fix(io): parse per-run FreeText appearance (font/size/color/italic/weight/position)`

### Task 3: io 序列化——runs 回写 + round-trip

**Files:**
- Modify: `crates/io/src/serialize/annotation.rs`（TextBox arm + 新 `text_run_object_xml`）
- Test: 同文件 tests + `crates/io/tests/save_surgical.rs`（扩展一例）

**Interfaces:**
- Consumes: Task 1/2。
- Produces: runs 非空时每个 run 一个 TextObject：
  `Boundary="{x} {baseline - size} {w} {size*1.2}"`，`w = max(1, chars) * size`；`Italic="true"` / `Weight="700"` 按需；FillColor 有则写；`<TextCode X="0" Y="{size}">{text}</TextCode>`（文本过既有 XML 转义助手；无则补 `&<>` 转义）。解析回读恒等：`x = boundary.x + 0`、`baseline = (baseline-size) + size`。runs 空时走既有 `text_object_xml` 平铺路径（现状不变）。

- [ ] **Step 1: 失败测试**：Task 2 的 Annot 149 夹具 → parse → serialize → 再 parse：content、runs（坐标 0.001 容差）、bold/italic/color 全等。
- [ ] **Step 2: 失败测试**：空 content 空 runs 的 TextBox（新工具产物）serialize→parse round-trip 仍为空 TextBox。
- [ ] **Step 3: 失败测试（surgical）**：`save_surgical.rs` 加一例——含 runs 批注的包手术后：批注条目重写且可解析出同等 runs，未触碰条目字节不变（沿用该文件既有断言模式）。
- [ ] **Step 4: 实现序列化分支。**
- [ ] **Step 5: `cargo test -p rofd-io` 全绿。**
- [ ] **Step 6: Commit** `feat(io): round-trip TextBox runs through appearance TextObjects`

### Task 4: render——run 绘制 + 合成粗体/斜体

**Files:**
- Modify: `crates/render/src/annotation_scene.rs`（TextBox arm → `draw_textbox`：runs 分派）
- Test: 同文件 tests（`glyph_baselines` 既有助手可复用）

**Interfaces:**
- Consumes: Task 1 `TextRun`；`FontStore::shape`、`draw_glyph_run`（既有 pub(crate)）。
- Produces: `fn draw_text_in_rect` 之前分派：`!runs.is_empty()` → 每 run：`fonts.shape(run.font, run.text, run.size)`，transform = `base * translate((rect.x + run.x, rect.y + run.baseline - glyphs[0].y))`（glyphs 空跳过；`glyphs[0].y` = 行顶→基线距离，shaper y 向下为正）；color = `run.color.unwrap_or(BLACK)`；italic → 额外左乘 `Affine::new([1.0, 0.0, -0.25, 1.0, 0.0, 0.0])`（基线上方向右倾）；bold → 同 brush 再绘制一次，x 偏移 `+0.022 * run.size`。border 绘制不变。

- [ ] **Step 1: 失败测试**：TextBox{content:"发的古典风格", runs:[4 段，含 italic/bold/color]} 渲染 → 至少 4 个 GlyphRun（可 >4：bold 双绘），各 run transform ty 与 `rect.y + baseline` 对应不同值；italic run 的 transform 系数含 skew（非纯平移）。
- [ ] **Step 2: 失败测试**：runs 空时走平铺路径——既有 `textbox_variant_shapes_and_draws_text` / `textbox_multiline_baselines_stay_inside_rect` 保持绿。
- [ ] **Step 3: 实现 `draw_textbox` 分派。**
- [ ] **Step 4: `cargo test -p rofd-render` 全绿。**
- [ ] **Step 5: Commit** `feat(render): draw TextBox appearance runs with synthetic bold/italic`

### Task 5: editor——首次文本编辑塌缩 runs

**Files:**
- Modify: `crates/editor/src/commands/text_commands.rs`
- Test: 同文件 tests

**Interfaces:**
- Consumes: Task 1。
- Produces: 私有 `fn collapse_runs(payload: &mut AnnotationPayload)`——TextBox 且 runs 非空：`font/size` ← runs[0]，`color` ← `runs[0].color.unwrap_or(现值)`，`runs.clear()`。在 `insert_text`/`delete_text`/`set_annotation_text` 对 `after` 做 `text_content_mut` 之前调用（同一 Transaction，一次 undo 完整还原 runs）。

- [ ] **Step 1: 失败测试**：TextBox{content:"发的古典风格", runs:4 段} → `insert_text(offset 6, "！")` → content "发的古典风格！" 且 runs 空、font/size 取自 runs[0]；`undo()` → runs 4 段复原。
- [ ] **Step 2: 失败测试**：runs 空的 TextBox 编辑后仍空（无回归）。
- [ ] **Step 3: 实现塌缩。**
- [ ] **Step 4: `cargo test -p rofd-editor` 全绿。**
- [ ] **Step 5: Commit** `feat(editor): collapse TextBox runs to flat style on first text edit`

### Task 6: component——font_requirements 纳入 run 字体 + 真文件集成测试

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（`font_requirements` 的批注扫描 arm，:495 起）
- Test: 同文件 tests

**Interfaces:**
- Consumes: Task 1。
- Produces: 批注字体扫描在 TextBox arm 上追加 `for r in runs { 收集 r.font }`（与既有 payload.font 同路径归一）。

- [ ] **Step 1: 失败测试**：批注 TextBox{runs:[{font:F9…},…]}，F9 仅出现在 run 里 → `font_requirements().families` 含 F9 的声明族。
- [ ] **Step 2: 实现。**
- [ ] **Step 3: `cargo test -p rofd-component` 全绿；随后 `cargo test --workspace --exclude tauri-app --exclude rofd-web-view` 全绿（Phase A 回归门）。**
- [ ] **Step 4: Commit** `feat(component): collect TextBox run fonts in font requirements`

### Task 7: wasm——IME 事件面 + caretRect getter

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`（Event Handlers 区，:521 起）
- Test: 同文件 tests（纯映射函数模式）

**Interfaces:**
- Consumes: `ViewEvent::ImePreedit/ImeCommit`（既有）、`OfdComponent::caret_rect`（既有 pub）。
- Produces (JS 名即 wasm_bindgen js_name)：
  - `handleImePreedit(text: &str)` → `ViewEvent::ImePreedit { text, caret: None }`（浏览器 CompositionEvent 无 caret range，恒 None）。
  - `handleImeCommit(text: &str)` → `ViewEvent::ImeCommit { text }`。
  - `caretRect() -> Option<Vec<f64>>` → `[x, y, w, h]` 视口 px（web 约定 = 设备 px）。
  - 三者经 Proxy 调用自动 kick 调度器（with-kick 机制，无需额外代码——测试确认命名符合 handle* 约定即可）。

- [ ] **Step 1: 失败测试**（native 可跑的映射函数/薄方法模式，参照既有 parse_key 测试）：JS 名与方法一一对应（字符串断言）。
- [ ] **Step 2: 实现三个方法。**
- [ ] **Step 3: `cargo test -p rofd-web-view`（native 部分）绿；`cargo build -p rofd-web-view --target wasm32-unknown-unknown` 通过。**
- [ ] **Step 4: Commit** `feat(web-view): expose IME preedit/commit and caretRect to the SDK`

### Task 8: SDK——隐藏 input + composition 桥 + keydown 门控

**Files:**
- Modify: `crates/web-view/sdk/src/index.ts`（`init` canvas 创建区 :328、`bindEvents` :642、cursor 双消费者模式 :376）
- Test: `crates/web-view/sdk/src/__tests__/imeWiring.test.ts`（新建，参照既有 wiring 测试的 fake-wasm/happy-dom 模式）

**Interfaces:**
- Consumes: Task 7 三个方法；`WasmOfd` 接口类型加 `handleImePreedit(text: string): void; handleImeCommit(text: string): void; caretRect(): number[] | null;`。
- Produces（SDK 行为）：
  - 隐藏 input：`document.createElement('input')`，样式 `position:absolute; opacity:0; pointer-events:none; border:none; padding:0; width:1px; height:1px; z-index:-1`，append 到 container；`canvas.tabIndex` 移除（改 -1）。
  - 焦点模型迁移：pointerdown 里 `input.focus()`（替换 `canvas.focus()`）；`focus`/`blur` 监听从 canvas 移到 input（仍调 handleFocusGained/Lost）；keydown 监听移到 input，开头加 `if (e.isComposing || e.keyCode === 229) { return; }`（原 preventDefault 仅在非组合时执行）。
  - composition（input 上）：`compositionstart` 置内部标记；`compositionupdate` → `handleImePreedit(e.data || '')`；`compositionend` → `handleImeCommit(e.data || '')` 后紧跟 `handleImePreedit('')` 并 `input.value = ''`。
  - 候选窗锚定：内部订阅 onCursorChange（用户回调存在则包装，仿 :376 onChange 双消费者模式）→ `caretRect()` → 除以 dpr 定位 input（`left = canvas.offsetLeft + x/dpr + 'px'`，top 同理，width/height 同步 caret 尺寸）；pointerdown 后亦刷新一次。
- [ ] **Step 1: 失败测试**：fake canvas+input 上派发 `new CompositionEvent('compositionupdate', {data:'你'})` → wasm.handleImePreedit 收 '你'；`compositionend {data:'你好'}` → handleImeCommit('你好') **且随后 handleImePreedit('')**；keydown `{key:'a', isComposing:true}` / `{keyCode:229}` → handleKeyDown 零调用；非组合 keydown 'a' → handleKeyDown('a',…) 照常。
- [ ] **Step 2: 跑 `cd crates/web-view/sdk && npx vitest run` 确认新测试红。**
- [ ] **Step 3: 实现（含 WasmOfd 接口类型三方法）。**
- [ ] **Step 4: vitest 全绿（含既有 wiring 套件——注意其中绑 canvas keydown 的断言需随焦点模型迁移更新）。**
- [ ] **Step 5: prettier 格式化 touched 文件。**
- [ ] **Step 6: Commit** `feat(sdk): bridge IME composition via hidden input; gate keydown while composing`

### Task 9: component——CreateKind::Typewriter / CreateKind::TextBox

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（`CreateKind` :36、`to_annotation_kind`、pointer 拖拽创建/提交路径 :1425-1870、`build_create_payload` :3182、预览绘制如需）
- Test: 同文件 tests

**Interfaces:**
- Consumes: 既有 DragState::Create 提交路径、`editor.create_annotation` + `set_cursor`。
- Produces:
  - `CreateKind::Typewriter`（点击放置，`to_annotation_kind` → TextBox）与 `CreateKind::TextBox`（拖拽画框 → TextBox）。
  - 常量：`DEFAULT_TEXTBOX_SIZE: f64 = 5.6444`（16pt，对齐 WPS 缺省）、`DEFAULT_TYPEWRITER_W: f64 = 30.0`、`DEFAULT_TYPEWRITER_H: f64 = 8.0`。
  - Typewriter：PointerUp 时位移在既有点击阈值内 → `create_annotation(TextBox { rect: {x, y, W, H}, content: "", font: FontId::default(), size: DEFAULT_TEXTBOX_SIZE, color: BLACK, border: None, runs: vec![] })`，随后 `select` + `set_cursor(id, 0)` + fire cursor/selection/focus 回调（一次 Transaction）。
  - TextBox：`build_create_payload` 新 arm → 同上但 `border: Some(Rgb(0,0,0))`、rect=bbox(start,current)（含既有最小尺寸过滤，过小不创建）；提交后同样 set_cursor(0)。
  - 拖拽预览：Typewriter 无预览（纯点击）；TextBox 复用 `DragPreview::Create`（kind=TextBox 时画预览矩形——预览 arm 若按 kind 枚举需补 match）。

- [ ] **Step 1: 失败测试**：Typewriter 工具下 page 内 pd+pu（位移 0）→ 文档新增 1 条 TextBox 批注（border None、content 空）、selection=Single、text_cursor offset 0；undo 一次移除。
- [ ] **Step 2: 失败测试**：TextBox 工具拖拽 (10,10)→(60,40) → 新增 TextBox border=Some(BLACK)、rect≈bbox、cursor 0；过小拖拽（(10,10)→(12,11)）不创建。
- [ ] **Step 3: 失败测试**：切换工具/拖拽中 Esc 等既有行为无回归（跑既有 create 套件）。
- [ ] **Step 4: 实现。**
- [ ] **Step 5: `cargo test -p rofd-component` 全绿。**
- [ ] **Step 6: Commit** `feat(component): typewriter and textbox create tools`

### Task 10: wasm 工具串 + App.vue 按钮 + 冒烟

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`（`parse_tool_kind` :55）、`crates/web-app/src/App.vue`（annotate 页签 :197-204 组后）、`crates/web-app/src/components/Icons.ts`（新增两图标）
- Test: wasm_ofd.rs tests + `crates/web-app/src/__tests__/`（如有既有 App 级测试模式则加按钮存在性断言；否则以 vitest sdk 侧工具串测试为准）

**Interfaces:**
- Consumes: Task 9 CreateKind。
- Produces: `parse_tool_kind("typewriter") → Tool::Create(CreateKind::Typewriter)`；`parse_tool_kind("textbox") → Tool::Create(CreateKind::TextBox)`；App.vue 注释页签"手绘/图形"组后新增组：`文字批注`（`setTool('typewriter')`，active 判 `activeTool === 'typewriter'`，tooltip "文字批注：点击页面放置并输入（无边框）"）与 `文本框`（`setTool('textbox')`，tooltip "文本框：拖拽画框后输入（带边框）"）；`Icons.ts` 新增 `TypewriterIcon`/`TextBoxIcon`（跟随既有 24px stroke 线型风格）。

- [ ] **Step 1: 失败测试**：parse_tool_kind 两个新映射 + 未知串仍回落 Text。
- [ ] **Step 2: 实现映射；`cargo test -p rofd-web-view` 绿。**
- [ ] **Step 3: App.vue 按钮与图标（TS/Vue 改动跑一次 prettier）。**
- [ ] **Step 4: 端到端冒烟（手动，写成清单贴在 commit body）**：`cd crates/web-app && npm run build:sdk && npm run dev` → 加载 test/sample.ofd（文件打开）→ ① 文本框"发的古典风格"横向四段、青色"格"、斜体"风"、粗体观感"格"；② 双击进入文本框→中文 IME 组合 inline 出现→回车上屏于光标处；③ 文字批注按钮点击放置→直接打字；文本框拖拽→画框→打字；保存后重开文件批注仍在。
- [ ] **Step 5: 全量回归**：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view` + `cd crates/web-view/sdk && npx vitest run` + clippy/fmt。
- [ ] **Step 6: AGENTS.md 微更新**（§5 component 要点补两工具与 runs 行为一行；web-view SDK 段补 IME 桥一句）。
- [ ] **Step 7: Commit** `feat(web): typewriter/textbox ribbon tools and run-faithful textbox rendering`

---

## 自审记录

- 覆盖检查：三问题 → Task 1-6（渲染保真）、7-8（IME）、9-10（工具）；bold=Weight≥600、italic、颜色、位置、多 TextCode、同行判定、塌缩、round-trip、手术刀、门控、候选窗锚定均有对应任务/测试。
- 类型一致：`TextRun` 字段在 Task 1 定义后全计划复用；`handleImePreedit/Commit`、`caretRect`、`"typewriter"/"textbox"` 串跨任务一致。
- 比例：以签名+断言为主，无函数体转写；算法（同行判定、run 坐标、塌缩、双绘偏移、skew 系数）已给精确值。
