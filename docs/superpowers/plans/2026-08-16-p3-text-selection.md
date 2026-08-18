# P3 文本选择工具 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 WPS 对齐的文本选择工具（`Tool::TextSelect`）：拖选正文（单页跨行）、双击选词、三击选段、Ctrl+C 复制（适配器默认接剪贴板）、`create_highlight_from_selection` 选中转高亮。

**Architecture:** 选区是纯 UI 态（spec §5.1 方案 A）：字段挂 `EditorComponent`，类型与几何全部住在 render 新模块 `body_text.rs`（绘制/命中/选区矩形三处共用同一笔位计算，单一来源）；body 只读不破坏，选区不进 dom/editor 历史/落盘。剪贴板按 §4.9 由适配器默认装配（web 用 `navigator.clipboard`、native 用 arboard），component 只出 `on_copy` 回调。

**Tech Stack:** Rust（rofd workspace）+ TypeScript SDK（wasm-pack --target web）。

**Spec:** [`docs/superpowers/specs/2026-08-16-ofd-tools-design.md`](../specs/2026-08-16-ofd-tools-design.md) §5（P3 文本选择工具）+ §6 测试策略。

## Global Constraints

- 依赖只向上（AGENTS §4.1）；component 不依赖 io/平台 crate；render 不依赖 component/editor。
- body 只读（§4.2）；库不取系统时间（§4.4）——见下方 Ruling 1 关于 `Instant` 的例外边界。
- 选区方案 A：不进 dom、不进 editor 历史、不落盘；切工具/换页/文档变更时清空（spec §5.1）。
- 选区与批注选择互斥（spec §5.2）。
- 错误显式（§4.6）；无新抽象 trait；conventional commits，无 attribution 尾注。
- **几何单一来源**（spec §5.3）：字形笔位（起点 + 累积 delta）计算从 `body_scene.rs::draw_text` 抽成共享函数，命中/选区矩形/绘制三处共用。
- 已核实的代码事实（不要重新探索）：
  - `TextCode { glyph_ids: Vec<u32>, deltas: Vec<(f32,f32)>, text: String, x: f64, y: f64 }`；`TextObject { id: ObjectId, boundary: Rect, ctm: Option<Ctm>, font, size, fill, codes: Vec<TextCode>, .. }`（`crates/dom/src/object.rs:28-54`）。deltas[i] 是第 i 个字形后到下一字形的推进量（GB/T 33190 DeltaX）。
  - `draw_text`（`crates/render/src/body_scene.rs:68-162`）两条路径（glyph_ids 非空按字形 ID；空则 `fonts.shape` 取字形 ID），**位置一律由 (code.x, code.y) + 累积 deltas 决定**，shaper 的自然 x/y 被忽略。
  - `compose_object_transform(page_origin, zoom, boundary, ctm)` = `translate(origin) * scale(zoom) * translate(boundary.xy) * ctm`（`crates/render/src/ctm.rs:36-47`）。ctm=None 时逆变换：`local = (viewport - origin)/zoom - boundary.xy`。
  - `page_origin(doc, vp, page_idx)` 在 `composite.rs:59`；component 用自己的私有辅助 `viewport_to_page_local`（`editor_component.rs:1576`）做视口->页局部换算。
  - `composite` 调用链：`build_scene` -> `self.render.composite(doc, vp, fonts, selection, drag_preview)`（`editor_component.rs:301-307`）；`Tool` 枚举在 `editor_component.rs:21-28`（Select/Create/Hand）；`Callbacks` 在 `component/src/callbacks.rs`（cfg 门控 Send 型别名 + `on_pointer_cursor` 即 P1 模式）。
  - wasm 侧：`parse_tool_kind`（`wasm_editor.rs:55-67`，现有 "select"/"hand"/... 字符串），`JsCallbacks`（`wasm_editor.rs:104-116`）；SDK `crates/web-view/sdk/src/index.ts`（`WasmEditor` interface + `EditorConfig` + `setOnPointerCursor` 接线在 L199-202）。
  - native 侧：`EditorApp`（`native-view/src/editor_app.rs:10-14`）；`WinitEventBridge` 在 `native-view/src/winit_bridge.rs`（构造 PointerDown）。
  - **`PointerCursor` 现只有 Default/Grab/Grabbing**（`callbacks.rs:83-90`）——Task 3 需新增 `Text` 变体并同步 `pointer_cursor_str`（`wasm_editor.rs:74-76` + 测试 747-749）与 native-app 宿主映射（`examples/native-app/src/main.rs:229-233`，`CursorIcon::Text`）。
  - `viewport_to_page_local` 是 `editor_component.rs:1576` 的**私有辅助**（`viewport_to_page_local(doc, vp, &page, point) -> Option<(f64,f64)>`），非 rofd_render 导出；Task 5 在同文件内直接调用。
  - `clear_selection_and_cursor`（:822）、`after_annotation_change`（:974）、`fire_selection_change`（:981）三个私有辅助已存在，Task 3/5 直接用。
  - P2 后 `MIN_CREATE_DRAG_PX = 3.0` 存在（`editor_component.rs:1379`）；测试约定：页 200x200、vp `size (0,0) gap 0 zoom 1 scroll (0,0)`、页原点 (0,0)。
- **Rulings（计划阶段裁定，执行时不必再问）**：
  1. **`Instant` 用于双击计数**：`WinitEventBridge` 用 `std::time::Instant`（单调钟）做双击/三击检测。§4.4 禁的是 wall-clock（SystemTime/Date::now，影响文档时间戳可测性）；Instant 是交互节奏检测且只出现在平台适配器。若用户不认可，回退方案是宿主（examples）自行计数。
  2. **`BodyTextSelection` 类型放 render**：spec §5.1 说选区状态放 EditorComponent——指**字段归属**；类型定义放 `rofd-render`（render 的 API 需要它作参数，render 不能依赖 component），component 直接复用。语义不变。
  3. **`hit_test_body_text` 不带 fonts 参数**：spec §5.3 签名含 fonts，但笔位由 deltas 决定、字形数由 glyph_ids/text 长度决定，命中根本不需要字体（shaping 只影响绘制用的字形 ID）。按 YAGNI 去掉。
  4. **range 带 code_index**：spec §5.1 的 `Vec<(ObjectId, start, end)>` 无法表达同一 TextObject 内跨 TextCode（跨行）的部分选区，细化为 `BodyTextRange { object, code_index, start, end }`（spec §7 开放问题授权实现阶段定）。
  5. **CTM 旋转体 v1 不可选**：命中/选区矩形只处理 ctm 平移近似--线性部分为单位阵（epsilon 内）时把平移 `(e,f)`（对象局部 mm，随 zoom 缩放）加到格子原点，与 `draw_text` 的 `compose_object_transform` 顺序一致；`ctm` 有缩放/旋转（含 0.0176 单位换算）的 TextObject 在命中与选区矩形中整体跳过（不做逆矩阵）。已知限制，记录于 body_text.rs 文档注释。
  6. **拖到无字区域**：PointerMove 未命中文字时保持选区不变（等效“钳制到最近已过字符”的 v1 简化），不强行找最近行。
  7. **demo 工具串**：`setTool("textSelect")`（spec §5.5 原文）；现有"文本"按钮从 Select 切到 TextSelect。

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/render/src/body_text.rs` | 新建 | 正文文本几何单一来源：类型、笔位、命中、选区 range/矩形、选词/选段 |
| `crates/render/src/body_scene.rs` | 修改 | `draw_text` 改用共享笔位（行为不变） |
| `crates/render/src/composite.rs` | 修改 | `composite` 增选区参数，画半透明高亮 overlay |
| `crates/render/src/lib.rs` | 修改 | 导出 body_text |
| `crates/component/src/event.rs` | 修改 | `PointerDown` 增 `click_count: u8` |
| `crates/component/src/editor_component.rs` | 修改 | `Tool::TextSelect`、`text_selection` 字段、拖选状态机、`selected_text`、`on_copy` 触发、`create_highlight_from_selection` |
| `crates/component/src/callbacks.rs` | 修改 | `OnCopy` 型别名 + `on_copy` 槽 |
| `crates/web-view/src/wasm_editor.rs` | 修改 | `parse_tool_kind("textSelect")`、`handleMouseDown` 增 count、`setOnCopy`、`getSelectedText`、`createHighlightFromSelection`、默认 on_copy 装配 |
| `crates/web-view/sdk/src/index.ts` | 修改 | 接口/配置/默认剪贴板/事件 detail 传递 |
| `crates/native-view/src/winit_bridge.rs` | 修改 | 双击/三击计数（Instant） |
| `crates/native-view/src/editor_app.rs` | 修改 | 默认 `on_copy` -> arboard 剪贴板（可关） |
| `crates/native-view/Cargo.toml` | 修改 | 加 `arboard` 依赖 |
| `examples/native-app/src/main.rs` | 修改 | "文本"按钮 -> TextSelect |
| `examples/web-app/src/App.vue` | 修改 | "文本"按钮 -> 'textSelect' |

---

### Task 1: render `body_text.rs` —— 类型 + 共享笔位 + `hit_test_body_text`

**Files:**
- Create: `crates/render/src/body_text.rs`
- Modify: `crates/render/src/body_scene.rs`（`draw_text` 改用共享笔位，行为不变）
- Modify: `crates/render/src/lib.rs`（`pub mod body_text;` + re-export）

**Interfaces:**
- Produces: `BodyTextRange { object: ObjectId, code_index: usize, start: usize, end: usize }`、`BodyTextSelection { page: PageId, ranges: Vec<BodyTextRange> }`、`TextHit { page: PageId, object: ObjectId, code_index: usize, char_offset: usize }`（均 `#[derive(Debug, Clone, PartialEq, Eq)]`，pub）、`pub fn hit_test_body_text(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> Option<TextHit>`、`pub(crate) struct CharCell { x, y, advance: f64 }` + `pub(crate) fn code_char_cells(t: &TextObject, code: &TextCode, n: usize) -> Vec<CharCell>`。

- [ ] **Step 1: 写失败测试**

新建 `body_text.rs`，先写测试（deltas 路径，无需字体 fixture）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{Layer, ObjectId, Page, PageId, PageObject, Rect, TextCode, TextObject, Viewport};

    fn text_obj(codes: Vec<TextCode>) -> TextObject {
        TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect { x: 10.0, y: 20.0, w: 100.0, h: 20.0 },
            ctm: None,
            font: rofd_dom::FontId::new("F1"),
            size: 10.0,
            fill: None,
            codes,
            draw_param: None,
        }
    }

    fn code4() -> TextCode {
        // 4 个字形，每个推进 10mm：笔位 x = 0,10,20,30（对象局部，code.x=0）。
        TextCode {
            glyph_ids: vec![1, 2, 3, 4],
            deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)],
            text: "ABCD".into(),
            x: 0.0,
            y: 10.0,
        }
    }

    fn doc_with(obj: TextObject) -> (OfdDocument, Viewport) {
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 },
            layers: vec![Layer { objects: vec![PageObject::Text(obj)] }],
            template: None,
        });
        let vp = Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (0.0, 0.0), page_gap: 0.0 };
        (doc, vp)
    }

    #[test]
    fn char_cells_pen_positions_and_fallback_advance() {
        let t = text_obj(vec![code4()]);
        let cells = code_char_cells(&t, &t.codes[0], 4);
        assert_eq!(cells.len(), 4);
        assert_eq!((cells[0].x, cells[0].y), (0.0, 10.0));
        assert_eq!(cells[1].x, 10.0);
        // 前 3 个 advance 来自 deltas；最后一个无 delta -> 回退最后一个非零 delta (10)。
        assert_eq!(cells[2].advance, 10.0);
        assert_eq!(cells[3].x, 30.0);
        assert_eq!(cells[3].advance, 10.0, "fallback = last non-zero delta");
    }

    #[test]
    fn char_cells_empty_deltas_fallback_to_size() {
        let t = text_obj(vec![TextCode {
            glyph_ids: vec![1, 2],
            deltas: vec![],
            text: "AB".into(),
            x: 0.0,
            y: 0.0,
        }]);
        let cells = code_char_cells(&t, &t.codes[0], 2);
        // 无任何 delta -> advance 回退字号。
        assert_eq!(cells[0].advance, 10.0);
        assert_eq!(cells[1].x, 0.0, "zero deltas -> pen never advances");
    }

    // 对象 boundary (10,20)，code.y=10 -> 行带 viewport y ∈ [20+(10-10), 20+(10+2.5)] = [20, 32.5]。
    // 字符 i 的格子的 viewport x 起点 = 10 + 10i，中线边界在 x = 10 + 10i + 5。
    #[test]
    fn hit_mid_first_char_is_offset_zero() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        let h = hit_test_body_text(&doc, &vp, (12.0, 25.0)).expect("hit");
        assert_eq!(h.char_offset, 0);
        assert_eq!(h.code_index, 0);
        assert_eq!(h.object, ObjectId::new("t1"));
    }

    #[test]
    fn hit_second_half_of_char_advances_offset() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        // x=16 落在字符 0 格子 (10..20) 的后半 -> offset 1。
        let h = hit_test_body_text(&doc, &vp, (16.0, 25.0)).expect("hit");
        assert_eq!(h.char_offset, 1);
        // x=36 在字符 2 (30..40) 后半 -> offset 3。
        assert_eq!(hit_test_body_text(&doc, &vp, (36.0, 25.0)).unwrap().char_offset, 3);
        // x=50（最后一格 30..40 右缘的 1.5 格余量内）-> offset = n。
        assert_eq!(hit_test_body_text(&doc, &vp, (50.0, 25.0)).unwrap().char_offset, 4);
    }

    #[test]
    fn hit_misses_blank_area_and_wrong_band() {
        let (doc, vp) = doc_with(text_obj(vec![code4()]));
        assert!(hit_test_body_text(&doc, &vp, (100.0, 100.0)).is_none(), "blank desk");
        assert!(hit_test_body_text(&doc, &vp, (15.0, 60.0)).is_none(), "below the line band");
        // x 远超行尾（最后一个 advance 的 1.5 倍余量之外）不算命中。
        assert!(hit_test_body_text(&doc, &vp, (200.0, 25.0)).is_none(), "far right of line");
    }

    #[test]
    fn hit_picks_nearest_line_among_codes() {
        let obj = text_obj(vec![
            TextCode { glyph_ids: vec![1, 2], deltas: vec![(10.0, 0.0)], text: "AB".into(), x: 0.0, y: 10.0 },
            TextCode { glyph_ids: vec![3, 4], deltas: vec![(10.0, 0.0)], text: "CD".into(), x: 0.0, y: 30.0 },
        ]);
        let (doc, vp) = doc_with(obj);
        // 靠近第二行 (viewport y 中心 50) -> code_index 1。
        assert_eq!(hit_test_body_text(&doc, &vp, (15.0, 51.0)).unwrap().code_index, 1);
        assert_eq!(hit_test_body_text(&doc, &vp, (15.0, 25.0)).unwrap().code_index, 0);
    }
}
```

（`Layer`/`Page` 字段名以 `crates/dom` 实际为准——`PageObject` 在 `layer.objects`；如 fixture 结构体字段有出入按实际调整，测试语义不变。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render body_text`
Expected: FAIL，模块函数未定义（先在 `lib.rs` 挂 `pub mod body_text;` 使编译入口存在）。

- [ ] **Step 3: 实现 `body_text.rs`**

```rust
//! Body-text geometry shared by drawing, hit-testing, and selection rects
//! (spec P3 §5.3: single source of truth - never a second implementation).
//!
//! v1 limitation: TextObjects with a scaling/rotating CTM are not selectable
//! (only translation is accounted for); Freehand-style shaped text is drawn
//! by glyph ids but positioned by the same deltas used here.

use rofd_dom::{ObjectId, OfdDocument, Page, PageId, Rect, TextCode, TextObject, Viewport};

/// A selected character range within one TextCode: chars `[start, end)` of
/// `code_index` inside `object`. Offsets are char (not byte) offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyTextRange {
    pub object: ObjectId,
    pub code_index: usize,
    pub start: usize,
    pub end: usize,
}

/// The text-selection UI state (spec §5.1 plan A: pure UI state - never in
/// dom, editor history, or saved output). One page only (v1: no cross-page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyTextSelection {
    pub page: PageId,
    pub ranges: Vec<BodyTextRange>,
}

/// Where a pointer hit landed in the body text (char offsets; `char_offset`
/// may equal the code's char count = past-the-end boundary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHit {
    pub page: PageId,
    pub object: ObjectId,
    pub code_index: usize,
    pub char_offset: usize,
}

/// Pen position + advance of one character in a TextCode (object-local:
/// relative to the TextObject's Boundary origin, CTM not applied).
pub(crate) struct CharCell {
    pub x: f64,
    pub y: f64,
    pub advance: f64,
}

/// Character cells for a TextCode: the pen starts at `(code.x, code.y)`;
/// each character sits at the pen and advances by its document delta
/// (GB/T 33190 DeltaX semantics - the same math `draw_text` uses to place
/// glyphs, extracted so hit/selection/draw cannot drift apart).
///
/// `n` is the cell count to produce: the glyph count for glyph-id codes,
/// the char count for shaped text (the drawer passes its own glyph count).
/// The last character's advance (no following delta) falls back to the last
/// non-zero delta, then to the font `size`.
pub(crate) fn code_char_cells(t: &TextObject, code: &TextCode, n: usize) -> Vec<CharCell> {
    let fallback = code
        .deltas
        .last()
        .map(|d| d.0 as f64)
        .filter(|a| *a > 0.0)
        .unwrap_or(t.size);
    let mut cells = Vec::with_capacity(n);
    let mut pen_x = code.x;
    let mut pen_y = code.y;
    for i in 0..n {
        let (dx, dy) = code.deltas.get(i).copied().unwrap_or((0.0, 0.0));
        let advance = if i < code.deltas.len() { dx as f64 } else { fallback };
        cells.push(CharCell { x: pen_x, y: pen_y, advance });
        pen_x += dx as f64;
        pen_y += dy as f64;
    }
    cells
}

/// Character count of a code as hit-testing sees it: glyph count when the
/// code carries glyph ids, else the text's char count.
pub(crate) fn code_char_count(code: &TextCode) -> usize {
    if code.glyph_ids.is_empty() {
        code.text.chars().count()
    } else {
        code.glyph_ids.len()
    }
}

/// Hit-test a viewport-space point against body text (all pages, document
/// order). Within a code's line band the nearest character boundary wins
/// (前/后半宽). Among bands, the vertically nearest wins. CTM rotation is
/// ignored (v1 - see module docs).
pub fn hit_test_body_text(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> Option<TextHit> {
    use rofd_dom::PageObject;
    let mut best: Option<(f64, TextHit)> = None;
    for (page_idx, page) in doc.pages.iter().enumerate() {
        let Some((ox, oy)) = crate::composite::page_origin(doc, vp, page_idx) else {
            continue;
        };
        for layer in &page.layers {
            for obj in &layer.objects {
                let PageObject::Text(t) = obj else { continue };
                let base_x = ox + t.boundary.x * vp.zoom;
                let base_y = oy + t.boundary.y * vp.zoom;
                for (ci, code) in t.codes.iter().enumerate() {
                    let n = code_char_count(code);
                    if n == 0 {
                        continue;
                    }
                    let cells = code_char_cells(t, code, n);
                    let last = &cells[n - 1];
                    // Line band: ascent (one em) above the pen, a quarter em
                    // descender below (same band `text_selection_rects` uses).
                    let band_top = base_y + (last.y - t.size) * vp.zoom;
                    let band_bot = base_y + (last.y + t.size * 0.25) * vp.zoom;
                    if point.1 < band_top || point.1 > band_bot {
                        continue;
                    }
                    let x_local = (point.0 - base_x) / vp.zoom;
                    // x extent: half a char of slack left, 1.5 advances right.
                    let x0 = cells[0].x - cells[0].advance / 2.0;
                    let x1 = last.x + last.advance * 1.5;
                    if x_local < x0 || x_local > x1 {
                        continue;
                    }
                    let dist = (point.1 - (band_top + band_bot) / 2.0).abs();
                    if best.as_ref().is_some_and(|(d, _)| dist >= *d) {
                        continue;
                    }
                    let mut offset = n;
                    for (i, c) in cells.iter().enumerate() {
                        if x_local < c.x + c.advance / 2.0 {
                            offset = i;
                            break;
                        }
                    }
                    best = Some((
                        dist,
                        TextHit {
                            page: page.id.clone(),
                            object: t.id.clone(),
                            code_index: ci,
                            char_offset: offset,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, h)| h)
}
```

`lib.rs`：`pub mod body_text;` + `pub use body_text::{hit_test_body_text, BodyTextRange, BodyTextSelection, TextHit};`（跟随现有 re-export 风格）。

- [ ] **Step 4: 重构 `draw_text` 用共享笔位（行为不变）**

`body_scene.rs::draw_text` 的两条定位循环替换为（保留 glyph_ids/shape 分支取字形 ID 与字体，位置统一走 `code_char_cells`）：

```rust
        let cells_len = if code.glyph_ids.is_empty() {
            0 // shaped path: cell count comes from the glyph run below
        } else {
            code.glyph_ids.len()
        };
        let (font, ids): (Option<peniko::FontData>, Vec<u32>) = if !code.glyph_ids.is_empty() {
            let font = fonts.resolve_or_default(&t.font).cloned();
            (font, code.glyph_ids.clone())
        } else {
            let (font, glyphs) = fonts.shape(&t.font, &code.text, t.size);
            (font, glyphs.iter().map(|g| g.glyph_id).collect())
        };
        // Shared pen geometry (hit-testing and selection rects use the same
        // cells - spec §5.3 single source of truth).
        let cells = crate::body_text::code_char_cells(t, code, ids.len().max(cells_len));
        let positioned: Vec<Glyph> = ids
            .iter()
            .zip(cells.iter())
            .map(|(&id, c)| Glyph { id, x: c.x as f32, y: c.y as f32 })
            .collect();
```

（外层 `let font = match font`、`positioned.is_empty()` 提前返回、`painter.glyphs(...)` 保持不变。`cells_len` 仅为了在 glyph_ids 为空但 shape 产出 0 字形时保底；如实现中发现冗余可简化为 `code_char_cells(t, code, ids.len())`，行为等价。既有 `render_smoke`/`body_position` 测试必须全绿。）

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p rofd-render`
Expected: PASS（新增 6 个测试 + 既有全绿）。

- [ ] **Step 6: 提交**

```bash
git add crates/render/src/body_text.rs crates/render/src/body_scene.rs crates/render/src/lib.rs
git commit -m "feat(render): shared body-text pen geometry and body-text hit test"
```

---

### Task 2: render 选区 range 计算 + 选区矩形 + composite overlay

**Files:**
- Modify: `crates/render/src/body_text.rs`
- Modify: `crates/render/src/composite.rs`（`composite` 增 `text_selection: Option<&BodyTextSelection>` 参数 + overlay 绘制）
- Modify: `crates/component/src/editor_component.rs`（`build_scene` 调用点加一个参数，保持编译绿）
- Test: `crates/render/src/body_text.rs` 测试模块；`crates/render/tests/render_smoke.rs`（composite 调用点补 `None`）

**Interfaces:**
- Consumes: Task 1 的类型与 `code_char_cells`/`code_char_count`。
- Produces: `pub fn body_text_ranges_between(page: &Page, a: &TextHit, b: &TextHit) -> Vec<BodyTextRange>`、`pub fn word_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange>`、`pub fn paragraph_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange>`、`pub fn text_selection_rects(doc: &OfdDocument, vp: &Viewport, sel: &BodyTextSelection) -> Vec<Rect>`（viewport 空间）；`composite` 新签名（`text_selection` 在 `selection` 之后、`drag_preview` 之前）。

- [ ] **Step 1: 写失败测试**

`body_text.rs` 测试模块追加（复用 Task 1 的 `text_obj`/`doc_with`/`code4`）：

```rust
    #[test]
    fn ranges_between_same_code_clamps_and_orders() {
        let obj = text_obj(vec![code4()]);
        let page = &doc_with(obj).0.pages[0];
        let hit = |o: usize| TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: 0,
            char_offset: o,
        };
        // 反向拖（b 在 a 前）-> 归一化为 [1,3)。
        let r = body_text_ranges_between(page, &hit(3), &hit(1));
        assert_eq!(r, vec![BodyTextRange { object: ObjectId::new("t1"), code_index: 0, start: 1, end: 3 }]);
        // 同 offset -> 空（零宽选区不产出 range）。
        assert!(body_text_ranges_between(page, &hit(2), &hit(2)).is_empty());
        // 越界 clamp 到 n。
        let r = body_text_ranges_between(page, &hit(9), &hit(0));
        assert_eq!(r[0].end, 4);
    }

    #[test]
    fn ranges_between_two_codes_partial_ends_full_middle() {
        let obj = text_obj(vec![
            TextCode { glyph_ids: vec![1, 2, 3], deltas: vec![(10.0, 0.0), (10.0, 0.0)], text: "ABC".into(), x: 0.0, y: 10.0 },
            TextCode { glyph_ids: vec![4, 5, 6], deltas: vec![(10.0, 0.0), (10.0, 0.0)], text: "DEF".into(), x: 0.0, y: 30.0 },
            TextCode { glyph_ids: vec![7, 8], deltas: vec![(10.0, 0.0)], text: "GH".into(), x: 0.0, y: 50.0 },
        ]);
        let page = &doc_with(obj).0.pages[0];
        let hit = |ci: usize, o: usize| TextHit {
            page: PageId::new("P0"),
            object: ObjectId::new("t1"),
            code_index: ci,
            char_offset: o,
        };
        let rs = body_text_ranges_between(page, &hit(0, 2), &hit(2, 1));
        // code0 [2,3) + code1 全部 + code2 [0,1)。
        assert_eq!(rs.len(), 3);
        assert_eq!((rs[0].code_index, rs[0].start, rs[0].end), (0, 2, 3));
        assert_eq!((rs[1].code_index, rs[1].start, rs[1].end), (1, 0, 3));
        assert_eq!((rs[2].code_index, rs[2].start, rs[2].end), (2, 0, 1));
    }

    #[test]
    fn word_range_at_same_char_class_run() {
        // "AB12中" (glyph 计数 5)：点在 'B'(offset 1) -> 词 = [0,2)（连续字母段）。
        let code = TextCode {
            glyph_ids: vec![1, 2, 3, 4, 5],
            deltas: vec![(10.0, 0.0); 4],
            text: "AB12中".into(),
            x: 0.0,
            y: 10.0,
        };
        let obj = text_obj(vec![code]);
        let page = &doc_with(obj).0.pages[0];
        let hit_at = |o: usize| TextHit {
            page: PageId::new("P0"), object: ObjectId::new("t1"), code_index: 0, char_offset: o,
        };
        let w = word_range_at(page, &hit_at(1));
        assert_eq!(w, vec![BodyTextRange { object: ObjectId::new("t1"), code_index: 0, start: 0, end: 2 }]);
        // 点在 '1'(offset 2) -> [2,4)（数字段）。
        assert_eq!(word_range_at(page, &hit_at(2))[0].start, 2);
        assert_eq!(word_range_at(page, &hit_at(2))[0].end, 4);
        // CJK 每字一类段：点在 '中'(offset 4) -> [4,5)。
        assert_eq!((word_range_at(page, &hit_at(4))[0].start, word_range_at(page, &hit_at(4))[0].end), (4, 5));
        // 空白类：命中在空格上 -> 只选空格本身。
    }

    #[test]
    fn paragraph_range_covers_whole_object() {
        let obj = text_obj(vec![
            TextCode { glyph_ids: vec![1], deltas: vec![], text: "A".into(), x: 0.0, y: 10.0 },
            TextCode { glyph_ids: vec![2], deltas: vec![], text: "B".into(), x: 0.0, y: 30.0 },
        ]);
        let page = &doc_with(obj).0.pages[0];
        let hit = TextHit { page: PageId::new("P0"), object: ObjectId::new("t1"), code_index: 1, char_offset: 0 };
        let rs = paragraph_range_at(page, &hit);
        assert_eq!(rs.len(), 2, "三击 = 同一 TextObject 全部 codes");
        assert_eq!((rs[0].code_index, rs[0].start, rs[0].end), (0, 0, 1));
        assert_eq!((rs[1].code_index, rs[1].start, rs[1].end), (1, 0, 1));
    }

    #[test]
    fn selection_rects_one_per_code_line() {
        // boundary (10,20)；code0 y=10 行带 y ∈ [20, 32.5]；选 [1,3) -> x ∈ [20, 40]。
        let obj = text_obj(vec![code4()]);
        let (doc, vp) = doc_with(obj);
        let sel = BodyTextSelection {
            page: PageId::new("P0"),
            ranges: vec![BodyTextRange { object: ObjectId::new("t1"), code_index: 0, start: 1, end: 3 }],
        };
        let rects = text_selection_rects(&doc, &vp, &sel);
        assert_eq!(rects.len(), 1);
        let r = &rects[0];
        assert_eq!(r.x, 10.0 + 10.0, "start cell x");
        assert_eq!(r.x + r.w, 10.0 + 20.0 + 10.0, "last cell x + advance");
        assert_eq!(r.y, 20.0, "band top");
        assert_eq!(r.y + r.h, 20.0 + 12.5, "band bottom (size + 0.25*size)");
        // 未知页 -> 空。
        let mut bad = sel.clone();
        bad.page = PageId::new("P9");
        assert!(text_selection_rects(&doc, &vp, &bad).is_empty());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render body_text`
Expected: FAIL，四个新函数未定义。

- [ ] **Step 3: 实现选区逻辑**

`body_text.rs` 追加：

```rust
/// All body-text segments on a page in reading order (layer order, object
/// order, code order): `(object, code_index, char_count)`.
fn page_text_segments(page: &Page) -> Vec<(ObjectId, usize, usize)> {
    let mut segs = Vec::new();
    for layer in &page.layers {
        for obj in &layer.objects {
            if let rofd_dom::PageObject::Text(t) = obj {
                for (ci, code) in t.codes.iter().enumerate() {
                    segs.push((t.id.clone(), ci, code_char_count(code)));
                }
            }
        }
    }
    segs
}

fn find_object<'a>(page: &'a Page, object: &ObjectId) -> Option<&'a TextObject> {
    page.layers.iter().flat_map(|l| l.objects.iter()).find_map(|o| match o {
        rofd_dom::PageObject::Text(t) if &t.id == object => Some(t),
        _ => None,
    })
}

fn hit_to_segment(page: &Page, h: &TextHit) -> Option<usize> {
    page_text_segments(page)
        .iter()
        .position(|(o, c, _)| o == &h.object && *c == h.code_index)
}

/// Character ranges covering the span from `a` to `b` (either order) within
/// the page's reading order. Endpoint segments are partial (clamped to their
/// char count); segments between are full. Zero-width ranges are dropped.
pub fn body_text_ranges_between(page: &Page, a: &TextHit, b: &TextHit) -> Vec<BodyTextRange> {
    let segs = page_text_segments(page);
    let (Some(ia), Some(ib)) = (hit_to_segment(page, a), hit_to_segment(page, b)) else {
        return Vec::new();
    };
    let (lo, hi, lo_off, hi_off) = if ia <= ib {
        (ia, ib, a.char_offset, b.char_offset)
    } else {
        (ib, ia, b.char_offset, a.char_offset)
    };
    let span = hi - lo;
    segs[lo..=hi]
        .iter()
        .enumerate()
        .map(|(k, (o, c, n))| {
            let start = if k == 0 { lo_off.min(*n) } else { 0 };
            let end = if k == span { hi_off.min(*n) } else { *n };
            BodyTextRange { object: o.clone(), code_index: *c, start, end }
        })
        .filter(|r| r.end > r.start)
        .collect()
}

/// Character class for double-click word segmentation (CJK: 连续同类字符为
/// 一段). 0 = alphabetic, 1 = numeric, 2 = CJK ideograph, 3 = other.
fn char_class(c: char) -> u8 {
    if c.is_alphabetic() && !is_cjk(c) {
        0
    } else if c.is_numeric() {
        1
    } else if is_cjk(c) {
        2
    } else {
        3
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{3000}'..='\u{303F}'
        | '\u{FF00}'..='\u{FFEF}')
}

/// Double-click word: the maximal run of one char class around the hit
/// character, inside the hit's TextCode.
pub fn word_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange> {
    let Some(t) = find_object(page, &hit.object) else { return Vec::new() };
    let Some(code) = t.codes.get(hit.code_index) else { return Vec::new() };
    let chars: Vec<char> = code.text.chars().collect();
    // Codes with no chars to classify (glyph-only, empty text) fall back to
    // the whole code; glyph-id codes WITH text still segment by chars
    // (char offsets == glyph indices when lengths agree).
    if chars.is_empty() {
        let n = code_char_count(code);
        return vec![BodyTextRange { object: hit.object.clone(), code_index: hit.code_index, start: 0, end: n }];
    }
    let n = chars.len();
    // The clicked character: at the boundary, take the char before the caret
    // when past the end, else the char at the offset.
    let idx = hit.char_offset.min(n - 1);
    let class = char_class(chars[idx]);
    let mut start = idx;
    let mut end = idx + 1;
    while start > 0 && char_class(chars[start - 1]) == class {
        start -= 1;
    }
    while end < n && char_class(chars[end]) == class {
        end += 1;
    }
    vec![BodyTextRange { object: hit.object.clone(), code_index: hit.code_index, start, end }]
}

/// Triple-click paragraph: every TextCode of the hit's TextObject, full
/// content (spec: 同一 TextObject 的全部内容).
pub fn paragraph_range_at(page: &Page, hit: &TextHit) -> Vec<BodyTextRange> {
    let Some(t) = find_object(page, &hit.object) else { return Vec::new() };
    t.codes
        .iter()
        .enumerate()
        .filter(|(_, code)| code_char_count(code) > 0)
        .map(|(ci, code)| BodyTextRange {
            object: hit.object.clone(),
            code_index: ci,
            start: 0,
            end: code_char_count(code),
        })
        .collect()
}

/// One covering rect per selected code line, viewport space. Uses the same
/// shared cells + line band as `hit_test_body_text`.
pub fn text_selection_rects(doc: &OfdDocument, vp: &Viewport, sel: &BodyTextSelection) -> Vec<Rect> {
    let Some(page_idx) = doc.pages.iter().position(|p| p.id == sel.page) else {
        return Vec::new();
    };
    let Some((ox, oy)) = crate::composite::page_origin(doc, vp, page_idx) else {
        return Vec::new();
    };
    let page = &doc.pages[page_idx];
    let mut out = Vec::new();
    for range in &sel.ranges {
        if range.start >= range.end {
            continue;
        }
        let Some(t) = find_object(page, &range.object) else { continue };
        let Some(code) = t.codes.get(range.code_index) else { continue };
        let n = code_char_count(code);
        let cells = code_char_cells(t, code, n);
        let (Some(first), Some(last)) = (cells.get(range.start), cells.get(range.end - 1)) else {
            continue;
        };
        let x0 = ox + (t.boundary.x + first.x) * vp.zoom;
        let x1 = ox + (t.boundary.x + last.x + last.advance) * vp.zoom;
        let y0 = oy + (t.boundary.y + last.y - t.size) * vp.zoom;
        let y1 = oy + (t.boundary.y + last.y + t.size * 0.25) * vp.zoom;
        out.push(Rect { x: x0, y: y0, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0) });
    }
    out
}
```

`lib.rs` re-export 补 `body_text_ranges_between, text_selection_rects, word_range_at, paragraph_range_at`。

- [ ] **Step 4: composite 增选区参数与 overlay**

`composite.rs`：`composite` 签名在 `selection` 之后插入 `text_selection: Option<&BodyTextSelection>`（import 自 `crate::body_text`）。绘制位置：body 之后、批注 overlay 之前（高亮垫在文字上、批注下）。绘制代码：

```rust
    if let Some(sel) = text_selection {
        for r in crate::body_text::text_selection_rects(doc, vp, sel) {
            let rect = kurbo::Rect::new(r.x, r.y, r.x + r.w, r.y + r.h);
            painter
                .fill(&rect, peniko::Color::from_rgba8(68, 132, 255, 77))
                .draw();
        }
    }
```

（fill 的具体 Painter 调用形态对齐文件内既有 `fill_rect`/`fill` 用法；颜色常量语义 = 半透明选区蓝 rgba(68,132,255,0.3)。）

调用点更新：`editor_component.rs::build_scene` 传 `self.text_selection.as_ref()`（Task 3 才有该字段——**本任务先传 `None`**，Task 3 改回真实字段）；`render_smoke.rs` 等 render 内部 composite 调用补 `None`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p rofd-render && cargo test -p rofd-component && cargo build --workspace`
Expected: PASS（render 新增 5 测试 + 既有全绿；component 因传 None 不变）。

- [ ] **Step 6: 提交**

```bash
git add crates/render/src/body_text.rs crates/render/src/composite.rs crates/render/src/lib.rs crates/component/src/editor_component.rs crates/render/tests/render_smoke.rs
git commit -m "feat(render): body-text selection ranges, rects, and composite overlay"
```

---

### Task 3: component `Tool::TextSelect` 状态机 + `click_count`

> 原子说明：`ViewEvent::PointerDown` 加字段会破坏全部构造点（component 测试、`wasm_editor::handleMouseDown`、`winit_bridge`、native-app）；本任务把这些构造点统一补 `click_count: 1`（真实计数由 Task 6/7 的适配器接上）。

**Files:**
- Modify: `crates/component/src/event.rs`
- Modify: `crates/component/src/editor_component.rs`
- Modify: `crates/web-view/src/wasm_editor.rs`（仅 `handleMouseDown` 构造点补字段）
- Modify: `crates/native-view/src/winit_bridge.rs`（仅 PointerDown 构造点补字段）

**Interfaces:**
- Consumes: Task 1/2 的 `hit_test_body_text`/`body_text_ranges_between`/`word_range_at`/`paragraph_range_at`/`BodyTextSelection`/`TextHit`；Task 2 的 composite 签名。
- Produces: `ViewEvent::PointerDown { button, x, y, modifiers, click_count: u8 }`；`Tool::TextSelect`；`DragState::TextSelect { anchor: TextHit }`（pub(crate)）；`EditorComponent::text_selection(&self) -> Option<&BodyTextSelection>`。

- [ ] **Step 1: 写失败测试**

`editor_component.rs` 测试模块追加（沿用 `component_with_page` 风格构造含正文文字的 doc；vp 同测试约定）：

```rust
    /// 一页 P0 (200x200) + 一个 TextObject（boundary (10,20)，两行 code：
    /// y=10 四字符 "ABCD"（advance 10）、y=30 两字符 "EF"）。viewport zoom=1
    /// -> 行带 0: y∈[20,32.5] 字符格 x=10+10i；行带 1: y∈[40,52.5] x=10,20。
    fn component_with_body_text() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 },
            layers: vec![Layer {
                objects: vec![PageObject::Text(TextObject {
                    id: ObjectId::new("t1"),
                    boundary: Rect { x: 10.0, y: 20.0, w: 100.0, h: 40.0 },
                    ctm: None,
                    font: FontId::new("F1"),
                    size: 10.0,
                    fill: None,
                    codes: vec![
                        TextCode { glyph_ids: vec![1, 2, 3, 4], deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)], text: "ABCD".into(), x: 0.0, y: 10.0 },
                        TextCode { glyph_ids: vec![5, 6], deltas: vec![(10.0, 0.0)], text: "EF".into(), x: 0.0, y: 30.0 },
                    ],
                    draw_param: None,
                })],
            }],
            template: None,
        });
        c.load_document(doc);
        c.viewport = rofd_render::Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (0.0, 0.0), page_gap: 0.0 };
        c
    }

    fn pd(x: f64, y: f64) -> ViewEvent {
        ViewEvent::PointerDown { button: MouseButton::Left, x, y, modifiers: Modifiers::default(), click_count: 1 }
    }

    #[test]
    fn text_select_drag_across_lines_produces_ranges() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        // 锚点：行 0 的 'C' 前半 (x=31 -> offset 2)。
        c.handle_event(&pd(31.0, 25.0));
        // 拖到行 1 的 'F' 后半 (x=16 -> offset 1)。
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        let sel = c.text_selection().expect("selection exists");
        assert_eq!(sel.page, PageId::new("P0"));
        assert_eq!(sel.ranges.len(), 2, "partial first line + partial second line");
        assert_eq!((sel.ranges[0].code_index, sel.ranges[0].start, sel.ranges[0].end), (0, 2, 4));
        assert_eq!((sel.ranges[1].code_index, sel.ranges[1].start, sel.ranges[1].end), (1, 0, 1));
    }

    #[test]
    fn text_select_click_blank_clears_selection() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        assert!(c.text_selection().is_some());
        c.handle_event(&pd(150.0, 150.0));
        assert!(c.text_selection().is_none(), "blank press clears");
    }

    #[test]
    fn text_select_double_click_selects_word_triple_selects_object() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left, x: 31.0, y: 25.0,
            modifiers: Modifiers::default(), click_count: 2,
        });
        // "ABCD" 全部同类字母 -> 词 = 整个 code。
        let sel = c.text_selection().unwrap();
        assert_eq!((sel.ranges[0].code_index, sel.ranges[0].start, sel.ranges[0].end), (0, 0, 4));
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left, x: 31.0, y: 25.0,
            modifiers: Modifiers::default(), click_count: 3,
        });
        let sel = c.text_selection().unwrap();
        assert_eq!(sel.ranges.len(), 2, "triple click = whole TextObject");
    }

    #[test]
    fn text_and_annotation_selection_are_mutually_exclusive() {
        let mut c = component_with_shape(ShapeKind::Rect,
            Rect { x: 0.0, y: 100.0, w: 50.0, h: 50.0 }, vec![]);
        // 先选中批注（Select 工具）。
        c.handle_event(&pd(25.0, 125.0));
        assert!(matches!(c.selection(), AnnotationSelection::Single(_)));
        // 切 TextSelect 并拖选文字 -> 批注选择被清除。
        c.set_tool(Tool::TextSelect);
        assert!(c.text_selection().is_none(), "switching tools clears");
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        assert!(c.text_selection().is_some());
        // set_tool 已清批注选择；反向：Select 点批注 -> 文字选区清空。
        c.set_tool(Tool::Select);
        c.handle_event(&pd(25.0, 125.0));
        assert!(matches!(c.selection(), AnnotationSelection::Single(_)));
        assert!(c.text_selection().is_none(), "selecting an annotation clears text selection");
    }

    #[test]
    fn text_selection_clears_on_document_change() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        assert!(c.text_selection().is_some());
        // 文档变更（创建批注）-> 清空。
        c.set_clock("t".into(), 1);
        c.create_annotation(AnnotationKind::Highlight, PageId::new("P0"), AnnotationPayload::Markup {
            quad_points: vec![], color: Color::Rgb(255, 255, 0),
        });
        assert!(c.text_selection().is_none());
    }

    #[test]
    fn text_select_drag_without_move_yields_no_selection() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        // 单击（不拖）-> 零宽选区即无选区。
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 31.0, y: 25.0 });
        assert!(c.text_selection().is_none());
    }
```

（`component_with_shape` 为 P2 已有测试辅助；如名不同用实际名。既有全部 `ViewEvent::PointerDown` 构造点（component 测试、wasm_editor、winit_bridge、native-app 若有）统一补 `click_count: 1`——这些机械改动也计入本任务。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component text_select`
Expected: FAIL（`Tool::TextSelect` 不存在 + click_count 字段编译错——先加字段让全 workspace 编译过、再看到测试红）。

- [ ] **Step 3: 实现**

3a. `event.rs`：`PointerDown` 增字段 + 文档注释：

```rust
    PointerDown {
        button: MouseButton,
        x: f64,
        y: f64,
        modifiers: Modifiers,
        /// Click count for multi-click gestures (1 = single, 2 = double,
        /// 3 = triple). The host supplies it (winit/DOM `detail`); the
        /// component has no clock (AGENTS §4.4) and never derives it.
        click_count: u8,
    },
```

3b. `editor_component.rs`：

- `Tool` 增变体（doc 注释：WPS 文本工具——拖选正文/双击选词/三击选段；批注选择交给 Select）：

```rust
    /// WPS-style text tool: drag over body text to select it (single page,
    /// across lines), double-click selects a word, triple-click a TextObject.
    /// Pure UI state - never enters dom/history/saves (spec §5.1).
    TextSelect,
```

- `set_tool` 光标映射：`Tool::TextSelect => PointerCursor::Text,`。**同时新增 `PointerCursor::Text` 变体**（`callbacks.rs`，doc 注释 "Text tool hovering (WPS 文本，I-beam)"）并补齐两处映射：`wasm_editor.rs::pointer_cursor_str` 增 `PointerCursor::Text => "text",`（含测试 747-749 区追加断言）；`examples/native-app/src/main.rs:229-233` 的 match 增 `PointerCursor::Text => CursorIcon::Text,`（native 宿主映射在 demo 侧，属 Task 3 的机械改动以保证编译）。
- `set_tool` 体内：`self.text_selection = None;`（切工具清空）。
- 字段 `text_selection: Option<rofd_render::BodyTextSelection>`（init None；`load_document` 置 None）。
- `DragState` 增：

```rust
    /// Selecting body text: `anchor` is the press-time hit; PointerMove
    /// recomputes ranges from anchor to the current hit (preview-only UI
    /// state - no document change, no history).
    TextSelect {
        anchor: rofd_render::TextHit,
    },
```

- `build_scene`：Task 2 的 `None` 改为 `self.text_selection.as_ref()`。
- `pointer_down` 的 `match &self.tool` 增 `Tool::TextSelect` arm（在 Hand arm 之后）：

```rust
                    Tool::TextSelect => {
                        match rofd_render::hit_test_body_text(
                            self.editor.document(),
                            &self.viewport,
                            p,
                        ) {
                            Some(hit) => {
                                // 互斥（spec §5.2）：文字选区取代批注选择。
                                self.clear_selection_and_cursor();
                                let page_id = hit.page.clone();
                                let page = self
                                    .editor
                                    .document()
                                    .pages
                                    .iter()
                                    .find(|pg| pg.id == page_id);
                                let mut ranges = page.map(|page| match click_count {
                                    2 => rofd_render::word_range_at(page, &hit),
                                    3 => rofd_render::paragraph_range_at(page, &hit),
                                    _ => vec![rofd_render::BodyTextRange {
                                        object: hit.object.clone(),
                                        code_index: hit.code_index,
                                        start: hit.char_offset,
                                        end: hit.char_offset,
                                    }],
                                }).unwrap_or_default();
                                // 单击不拖 -> 零宽 caret range 不算选区。
                                ranges.retain(|r| r.end > r.start);
                                self.text_selection =
                                    (!ranges.is_empty()).then(|| rofd_render::BodyTextSelection {
                                        page: page_id,
                                        ranges,
                                    });
                                if click_count == 1 {
                                    self.drag = Some(DragState::TextSelect { anchor: hit });
                                }
                                outcome.needs_repaint = true;
                            }
                            None => {
                                if self.text_selection.take().is_some() {
                                    outcome.needs_repaint = true;
                                }
                            }
                        }
                    }
```

（`clear_selection_and_cursor` 为既有辅助；若其内部还会 fire 回调则照用——互斥语义要触发 selection_change。）

- `pointer_down_annotation` 开头（选中批注的两个分支前）：`self.text_selection = None;`（反向互斥）。
- PointerMove drag match 增 arm：

```rust
                    Some(DragState::TextSelect { anchor }) => {
                        // Ruling 6：未命中文字时保持选区不变。
                        if let Some(hit) = rofd_render::hit_test_body_text(
                            self.editor.document(),
                            &self.viewport,
                            *p,
                        ) {
                            if hit.page == anchor.page {
                                if let Some(page) = self
                                    .editor
                                    .document()
                                    .pages
                                    .iter()
                                    .find(|pg| pg.id == hit.page)
                                {
                                    let ranges = rofd_render::body_text_ranges_between(
                                        page, anchor, &hit,
                                    );
                                    let sel = rofd_render::BodyTextSelection {
                                        page: hit.page.clone(),
                                        ranges,
                                    };
                                    if self.text_selection.as_ref() != Some(&sel) {
                                        self.text_selection = Some(sel);
                                        needs_repaint = true;
                                    }
                                }
                            }
                        }
                    }
```

（局部变量名/needs_repaint 传递方式对齐该 match 现有 arm 的写法。）

- PointerUp drag match 增 arm：`DragState::TextSelect { .. } => {}`（仅消费掉，drag 由 `take()` 清除）。
- 清空时机补齐：`after_annotation_change`（文档变更）与 `current_page` 变化处（Scroll/ScrollPage/Zoom 中更新 `current_page` 的位置）加“页变了才清”：

```rust
        if self.current_page != new_page {
            self.text_selection = None;
        }
```

（找到实际更新 current_page 的代码点，把清空插在页变化分支里；若多处更新则抽小辅助。）

- getter：

```rust
    /// The current body-text selection (TextSelect tool), if any (spec §5.1).
    pub fn text_selection(&self) -> Option<&rofd_render::BodyTextSelection> {
        self.text_selection.as_ref()
    }
```

3c. 构造点机械补 `click_count: 1`：`wasm_editor.rs::handleMouseDown`、`winit_bridge.rs` PointerDown 构造、component 测试全部 PointerDown。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component && cargo test -p rofd-render && cargo build --workspace`
Expected: PASS（新增 6 测试 + 既有全绿；wasm/native 编译过）。

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/event.rs crates/component/src/editor_component.rs crates/web-view/src/wasm_editor.rs crates/native-view/src/winit_bridge.rs
git commit -m "feat(component): TextSelect tool with drag/word/paragraph body-text selection"
```

---

### Task 4: component `selected_text` + `on_copy` 回调 + Ctrl+C

**Files:**
- Modify: `crates/component/src/callbacks.rs`
- Modify: `crates/component/src/editor_component.rs`
- Modify: `crates/component/src/lib.rs`（如需 re-export）

**Interfaces:**
- Consumes: Task 3 的 `text_selection` 状态。
- Produces: `EditorComponent::selected_text(&self) -> Option<String>`；`Callbacks.on_copy: Option<Box<OnCopy>>` + cfg 门控 setter `pub fn on_copy(&mut self, cb: impl Fn(String) + 'static [+ Send])`。

- [ ] **Step 1: 写失败测试**

`callbacks.rs` 测试（镜像 `on_pointer_cursor_fires`）：

```rust
    #[test]
    fn on_copy_fires() {
        let fired = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let f = fired.clone();
        let cbs = Callbacks {
            on_copy: Some(Box::new(move |text: String| {
                *f.lock().unwrap() = text;
            })),
            ..Default::default()
        };
        (cbs.on_copy.as_ref().unwrap())("hello".into());
        assert_eq!(*fired.lock().unwrap(), "hello");
    }
```

`editor_component.rs` 测试追加：

```rust
    #[test]
    fn selected_text_joins_codes_with_newline() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        // 跨两行：code0 [2,4) = "CD"，code1 [0,1) = "E"；不同 code 间加 \n。
        assert_eq!(c.selected_text().as_deref(), Some("CD\nE"));
        // 无选区 -> None。
        c.handle_event(&pd(150.0, 150.0));
        assert_eq!(c.selected_text(), None);
    }

    #[test]
    fn ctrl_c_with_selection_fires_on_copy() {
        use std::sync::{Arc, Mutex};
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        let got = Arc::new(Mutex::new(String::new()));
        let g = got.clone();
        c.on_copy(move |text| *g.lock().unwrap() = text);
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('c'),
            modifiers: Modifiers { control: true, ..Default::default() },
        });
        assert_eq!(*got.lock().unwrap(), "CD\nE");
        // 无选区时 Ctrl+C 不触发。
        c.handle_event(&pd(150.0, 150.0));
        *got.lock().unwrap() = String::new();
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('c'),
            modifiers: Modifiers { control: true, ..Default::default() },
        });
        assert_eq!(*got.lock().unwrap(), "");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component on_copy && cargo test -p rofd-component selected_text`
Expected: FAIL，`on_copy`/`selected_text` 未定义。

- [ ] **Step 3: 实现**

`callbacks.rs`：型别名（cfg 两组，同 `OnPointerCursor` 模式）+ `Callbacks` 字段 `pub on_copy: Option<Box<OnCopy>>,` +（setter 在 EditorComponent 上）：

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type OnCopy = dyn Fn(String) + Send;
#[cfg(target_arch = "wasm32")]
pub type OnCopy = dyn Fn(String);
```

`editor_component.rs`：setter（cfg 两组，同 `on_warning` 模式）：

```rust
    /// Fired on Ctrl+C while body text is selected (TextSelect tool). The
    /// component never touches the clipboard (AGENTS §4.9) - adapters wire
    /// the default platform clipboard behind this callback.
```

实现：

```rust
    /// The selected body text, joined with `\n` between TextCodes (same code
    /// never spans a newline; different codes are different lines).
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.text_selection.as_ref()?;
        let page = self.editor.document().pages.iter().find(|p| p.id == sel.page)?;
        let mut out = String::new();
        for (i, range) in sel.ranges.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if let Some(t) = page
                .layers
                .iter()
                .flat_map(|l| l.objects.iter())
                .find_map(|o| match o {
                    PageObject::Text(t) if t.id == range.object => Some(t),
                    _ => None,
                })
            {
                if let Some(code) = t.codes.get(range.code_index) {
                    // glyph-id codes: fall back to a per-glyph '?'-free slice of
                    // `text` when lengths agree; else skip (v1).
                    if code.glyph_ids.is_empty() {
                        out.extend(code.text.chars().skip(range.start).take(range.end - range.start));
                    } else {
                        let n = code.text.chars().count();
                        if n == code.glyph_ids.len() {
                            out.extend(code.text.chars().skip(range.start).take(range.end - range.start));
                        }
                    }
                }
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }
```

（glyph-id code 且 text 与 glyph 数一致时才可按偏移切 text；不一致跳过该段——v1 已知限制，注释说明。）

KeyDown 处理：在现有 `Key::Char(c)` 处理分支前加：

```rust
            ViewEvent::KeyDown { key: Key::Char('c'), modifiers } if modifiers.control => {
                if matches!(self.tool, Tool::TextSelect) {
                    if let Some(text) = self.selected_text() {
                        if let Some(cb) = &self.callbacks.on_copy {
                            cb(text.clone());
                        }
                    }
                }
                EventOutcome { needs_repaint: false }
            }
```

（若现有 KeyDown 已有 Ctrl 组合处理（如 Ctrl+Z/S），插在同一层并保证不影响它们。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component && cargo clippy -p rofd-component --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/callbacks.rs crates/component/src/editor_component.rs crates/component/src/lib.rs
git commit -m "feat(component): selected_text and on_copy callback for text selection"
```

---

### Task 5: component `create_highlight_from_selection`（选中转高亮）

**Files:**
- Modify: `crates/component/src/editor_component.rs`

**Interfaces:**
- Consumes: Task 2 的 `text_selection_rects`；既有 `editor.create_annotation(Highlight, Markup)`；`editor_component.rs:1576` 的私有辅助 `viewport_to_page_local(doc, vp, &page, point) -> Option<(f64,f64)>`（同文件直接调用）。
- Produces: `pub fn create_highlight_from_selection(&mut self, color: rofd_dom::Color) -> Option<rofd_dom::AnnotationId>`。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn create_highlight_from_selection_makes_markup_quads_and_undo_removes() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 16.0, y: 45.0 });
        let id = c.create_highlight_from_selection(Color::Rgb(255, 255, 0))
            .expect("highlight created");
        // 选区已清空（成功后清空，spec §5.4）。
        assert!(c.text_selection().is_none());
        let ann = c.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Markup { quad_points, color } => {
                assert_eq!(*color, Color::Rgb(255, 255, 0));
                // 两行 -> 两个 quad（4 个点），页局部坐标（viewport=页局部因 zoom1/origin0）。
                assert_eq!(quad_points.len(), 4);
                assert_eq!(quad_points[0], Point { x: 30.0, y: 20.0 }, "line0 tl: cell2 x=20 + boundary 10");
                assert_eq!(quad_points[1], Point { x: 50.0, y: 32.5 }, "line0 br: cell3 x+adv=40 + boundary 10");
                assert_eq!(quad_points[2], Point { x: 10.0, y: 40.0 }, "line1 tl");
                assert_eq!(quad_points[3], Point { x: 20.0, y: 52.5 }, "line1 br");
            }
            _ => panic!("expected Markup payload"),
        }
        // 可 undo（走 create_annotation 命令）。
        assert!(c.can_undo());
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('z'),
            modifiers: Modifiers { control: true, ..Default::default() },
        });
        assert!(c.document().annotations.find(&id).is_none(), "undo removes highlight");
    }

    #[test]
    fn create_highlight_without_selection_is_none() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::TextSelect);
        assert!(c.create_highlight_from_selection(Color::Rgb(255, 255, 0)).is_none());
    }
```

（quad 坐标由 Task 2 `selection_rects_one_per_code_line` 的同一几何推得：行 0 选 [2,4) -> 首格 x=20、末格 x+adv=40，加 boundary.x=10 -> viewport x∈[30,50]、y∈[20,32.5]；行 1 选 [0,1) -> x∈[10,20]、y∈[40,52.5]；zoom=1、页原点 (0,0) 时 viewport==页局部。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component create_highlight`
Expected: FAIL，方法未定义。

- [ ] **Step 3: 实现**

```rust
    /// Convert the current body-text selection into a Highlight annotation
    /// (spec §5.4): one Markup quad per selected line (page-local tl/br
    /// pairs), via the existing `create_annotation` command - undoable and
    /// saved like any annotation. Clears the selection on success.
    pub fn create_highlight_from_selection(
        &mut self,
        color: rofd_dom::Color,
    ) -> Option<rofd_dom::AnnotationId> {
        let sel = self.text_selection.clone()?;
        let page_id = sel.page.clone();
        let rects =
            rofd_render::text_selection_rects(self.editor.document(), &self.viewport, &sel);
        if rects.is_empty() {
            return None;
        }
        let mut quad_points = Vec::with_capacity(rects.len() * 2);
        for r in &rects {
            // viewport -> page-local (same conversion hit-testing uses).
            let tl = viewport_to_page_local(
                self.editor.document(),
                &self.viewport,
                &page_id,
                (r.x, r.y),
            )?;
            let br = viewport_to_page_local(
                self.editor.document(),
                &self.viewport,
                &page_id,
                (r.x + r.w, r.y + r.h),
            )?;
            quad_points.push(rofd_dom::Point { x: tl.0, y: tl.1 });
            quad_points.push(rofd_dom::Point { x: br.0, y: br.1 });
        }
        let id = self.editor.create_annotation(
            AnnotationKind::Highlight,
            page_id,
            AnnotationPayload::Markup { quad_points, color },
        );
        self.text_selection = None;
        self.after_annotation_change();
        self.fire_selection_change();
        Some(id)
    }
```

（`viewport_to_page_local` 为本文件既有私有辅助（:1576），它返回——它返回 `(f64, f64)` 页局部；若参数顺序不同按实际调。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component`
Expected: PASS（注意 Task 3 的 `text_selection_clears_on_document_change` 测试期望创建后清空——本方法行为一致）。

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/editor_component.rs
git commit -m "feat(component): create_highlight_from_selection converts selection to markup"
```

---

### Task 6: native 适配 —— 双击计数 + arboard 剪贴板默认装配

**Files:**
- Modify: `crates/native-view/src/winit_bridge.rs`
- Modify: `crates/native-view/src/editor_app.rs`
- Modify: `crates/native-view/Cargo.toml`（`arboard = "3"`）
- Modify: `examples/native-app/src/main.rs`（"文本"按钮 -> `Tool::TextSelect`）

**Interfaces:**
- Consumes: Task 3 的 `click_count`、Task 4 的 `on_copy`。
- Produces: `WinitEventBridge` 内部双击/三击计数（500ms 窗口 + 位置容差）；`EditorApp::set_default_clipboard(&mut self, enabled: bool)`（默认开启）。

- [ ] **Step 1: 写失败测试**

`winit_bridge.rs` 测试模块（构造 bridge 的现有测试风格；如 bridge 不可无窗口构造，则把计数逻辑抽成纯函数 `next_click_count(state: &mut ClickState, pos: (f64,f64), now: Instant) -> u8` 并对纯函数写测试）：

```rust
    #[test]
    fn click_count_sequences_within_window() {
        let mut s = ClickState::default();
        let t0 = std::time::Instant::now();
        assert_eq!(next_click_count(&mut s, (10.0, 10.0), t0), 1);
        assert_eq!(next_click_count(&mut s, (11.0, 10.0), t0 + Duration::from_millis(200)), 2);
        assert_eq!(next_click_count(&mut s, (10.0, 11.0), t0 + Duration::from_millis(400)), 3);
        // 超窗 -> 重新从 1 计。
        assert_eq!(next_click_count(&mut s, (10.0, 10.0), t0 + Duration::from_millis(1200)), 1);
        // 位置跳变（>4px）-> 重新计。
        assert_eq!(next_click_count(&mut s, (50.0, 50.0), t0 + Duration::from_millis(1400)), 1);
        // 连续第 4 击 -> 回绕到 1。
        let mut s2 = ClickState::default();
        assert_eq!(next_click_count(&mut s2, (0.0, 0.0), t0), 1);
        assert_eq!(next_click_count(&mut s2, (0.0, 0.0), t0 + Duration::from_millis(100)), 2);
        assert_eq!(next_click_count(&mut s2, (0.0, 0.0), t0 + Duration::from_millis(200)), 3);
        assert_eq!(next_click_count(&mut s2, (0.0, 0.0), t0 + Duration::from_millis(300)), 1);
    }
```

`editor_app.rs` 测试（native 可跑：on_copy 回调直接验证文本透传；剪贴板本体在无头环境可能失败——不断言 arboard 结果，只断言回调链路）：

```rust
    #[test]
    fn default_on_copy_wired_and_disable_stops_firing() {
        // 无法在无头环境断言系统剪贴板内容；此处验证 EditorApp 构造后
        // component.on_copy 槽非空（默认装配）且 set_default_clipboard(false)
        // 后不再默认订阅（槽位被清）。component 需暴露查询或用行为验证：
        // 借助 selected_text 路径不可行（无文档），改为直接检查 EditorApp
        // 的 clipboard_enabled 标志 + 手动订阅覆盖默认。
        let config = EditorConfig::new(Arc::new(vec![]));
        let mut app = EditorApp::new(config);
        assert!(app.default_clipboard_enabled());
        app.set_default_clipboard(false);
        assert!(!app.default_clipboard_enabled());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-native-view`
Expected: FAIL，`ClickState`/`next_click_count`/`set_default_clipboard` 未定义。

- [ ] **Step 3: 实现**

3a. `winit_bridge.rs`（纯函数 + bridge 字段）：

```rust
/// Double/triple-click tracking for the bridge. Monotonic `Instant` only
/// (Ruling 1): the §4.4 clock ban targets wall-clock document timestamps;
/// interaction rhythm detection in a platform adapter is not that.
#[derive(Default)]
pub(crate) struct ClickState {
    last: Option<(std::time::Instant, (f64, f64), u8)>,
}

const DOUBLE_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);
const CLICK_SLOP_PX: f64 = 4.0;

fn next_click_count(s: &mut ClickState, pos: (f64, f64), now: std::time::Instant) -> u8 {
    let count = match s.last {
        Some((t, p, n))
            if now.duration_since(t) <= DOUBLE_CLICK_WINDOW
                && (p.0 - pos.0).abs() <= CLICK_SLOP_PX
                && (p.1 - pos.1).abs() <= CLICK_SLOP_PX =>
        {
            (n % 3) + 1
        }
        _ => 1,
    };
    s.last = Some((now, pos, count));
    count
}
```

bridge 构造 `ViewEvent::PointerDown` 处改为传 `click_count: next_click_count(&mut self.clicks, (x, y), std::time::Instant::now())`（bridge 已持有指针位置与逻辑坐标换算；变量名按实际）。

3b. `editor_app.rs`：

```rust
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub package: Option<PackageHandle>,
    clipboard_enabled: std::rc::Rc<std::cell::Cell<bool>>,
}

impl EditorApp {
    pub fn new(config: EditorConfig) -> Self {
        let mut component = EditorComponent::new(config);
        // Default platform-clipboard assembly (AGENTS §4.9): Ctrl+C text
        // lands on the system clipboard with zero host code. Disable via
        // set_default_clipboard(false), then subscribe on_copy yourself.
        let enabled = std::rc::Rc::new(std::cell::Cell::new(true));
        let flag = enabled.clone();
        component.on_copy(move |text| {
            if flag.get() {
                // Clipboard unavailable (headless/locked) is non-fatal UI
                // degradation - nothing actionable to surface to the host.
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
        });
        Self { component, current_file: None, package: None, clipboard_enabled: enabled }
    }

    /// Toggle the default Ctrl+C -> system-clipboard wiring (on by default).
    pub fn set_default_clipboard(&mut self, enabled: bool) {
        self.clipboard_enabled.set(enabled);
    }
    pub fn default_clipboard_enabled(&self) -> bool {
        self.clipboard_enabled.get()
    }
```

（`component.on_copy` 要求闭包 `Send`（native 型别名）：捕获 `Rc<Cell<bool>>` 非 Send——native 侧 EditorApp 本就单线程非 Send（AGENTS §5 native-view 要点），若编译器拒绝则 native `OnCopy` 不带 `Send`（与 wasm 一致），调整型别名并全库检索该别名使用点确认无 Send 依赖。以编译器为准，报告记录选择。）

3c. `Cargo.toml`：`arboard = "3"`。

3d. `examples/native-app/src/main.rs`：P1 的 "文本" 按钮（原 `Tool::Select`）改绑 `Tool::TextSelect`；保留选择语义的入口如无单独按钮则新增一个"选择"按钮或保持工具栏两组（手型|文本）不变仅改目标。以 WPS 对标：文本 = TextSelect。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-native-view && cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/native-view/src/winit_bridge.rs crates/native-view/src/editor_app.rs crates/native-view/Cargo.toml examples/native-app/src/main.rs
git commit -m "feat(native-view): click-count tracking and default arboard clipboard"
```

---

### Task 7: web 适配 —— SDK 字符串/回调/剪贴板 + demo

**Files:**
- Modify: `crates/web-view/src/wasm_editor.rs`
- Modify: `crates/web-view/sdk/src/index.ts`
- Modify: `examples/web-app/src/App.vue`

**Interfaces:**
- Consumes: Task 3 的 `click_count` 与 `Tool::TextSelect`、Task 4 的 `on_copy`/`selected_text`、Task 5 的 `create_highlight_from_selection`。
- Produces: `setTool("textSelect")`；`handleMouseDown(button, x, y, shift, ctrl, alt, meta, clickCount)`（参数追加在尾）；`setOnCopy(cb)`；`getSelectedText(): string | null`；`createHighlightFromSelection(color: string): string | null`；SDK `EditorConfig.onCopy?: (text: string) => void`、`clipboard?: boolean`（默认 true）。

- [ ] **Step 1: 写失败测试**

`wasm_editor.rs` 测试（native 侧 `cargo test` 可跑的纯函数部分）：

```rust
    #[test]
    fn parse_tool_kind_text_select() {
        assert_eq!(parse_tool_kind("textSelect"), Tool::TextSelect);
        assert_eq!(parse_tool_kind("select"), Tool::Select);
    }
```

`index.ts` 无单测设施——以 `npm run build`（含 `tsc`）+ 手动冒烟为准；类型层错误由构建拦截。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-web-view parse_tool_kind`
Expected: FAIL，`Tool::TextSelect` 无匹配分支（编译错或断言失败——先写测试再加分支保证红）。

- [ ] **Step 3: 实现 wasm 侧**

- `parse_tool_kind` 增 `"textSelect" => Tool::TextSelect,`。
- `handleMouseDown` 尾参 `click_count: u32` -> 构造 `ViewEvent::PointerDown { .., click_count: click_count as u8 }`（Task 3 已补的字段位把 `1` 换成实参）。
- `JsCallbacks` 增 `pub on_copy: Rc<RefCell<Option<js_sys::Function>>>`；`WasmEditor` 构造处默认装配：

```rust
        let on_copy = callbacks.on_copy.clone();
        self.component.on_copy(move |text| {
            if let Some(cb) = on_copy.borrow().as_ref() {
                let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(&text));
            }
        });
```

- 新方法：

```rust
        #[wasm_bindgen(js_name = setOnCopy)]
        pub fn set_on_copy(&mut self, cb: Option<js_sys::Function>) {
            *self.callbacks.on_copy.borrow_mut() = cb;
        }

        /// 当前文字选区文本；无选区返回 null。
        #[wasm_bindgen(js_name = getSelectedText)]
        pub fn get_selected_text(&self) -> Option<String> {
            self.component.selected_text()
        }

        /// 选区转高亮批注；成功返回批注 id 字符串，无选区返回 null。
        /// 颜色格式 "#RRGGBB"（复用现有颜色解析辅助；如无则按现有
        /// set_annotation_color 类方法的解析路径）。
        #[wasm_bindgen(js_name = createHighlightFromSelection)]
        pub fn create_highlight_from_selection(&mut self, color: &str) -> Option<String> {
            let parsed = parse_color(color); // 现有辅助；若无则新增 hex 解析
            self.component.create_highlight_from_selection(parsed).map(|id| id.to_string())
        }
```

（`parse_color`：检索 wasm_editor.rs 现有颜色字符串解析（上下文菜单/属性面板如有）；没有就新增 `fn parse_color(s: &str) -> rofd_dom::Color` 支持 `#RRGGBB`，含单测。）

- [ ] **Step 4: 实现 SDK + demo**

`index.ts`：

- `WasmEditor` interface：`handleMouseDown` 尾加 `clickCount: number`；增 `setOnCopy(cb: ((text: string) => void) | null): void;`、`getSelectedText(): string | null;`、`createHighlightFromSelection(color: string): string | null;`。
- `EditorConfig` 增：

```ts
  /** Fired on Ctrl+C with body text selected (TextSelect tool). Defaults to
   * writing the text to the system clipboard via navigator.clipboard
   * (inside the user-activation window); pass `clipboard: false` and use
   * this to handle copying yourself. */
  onCopy?: (text: string) => void;
  /** Set false to disable the default Ctrl+C -> clipboard wiring. */
  clipboard?: boolean;
```

- 接线（注册回调区，`setOnPointerCursor` 之后）：默认订阅 = `config.onCopy ?? (clipboard === false ? null : (text) => { void navigator.clipboard.writeText(text); })`；非 null 则 `wasmEditor.setOnCopy(...)`。
- 事件绑定区 `pointerdown` 处理：`wasmEditor.handleMouseDown(button, x, y, ..., e.detail)`（DOM 的 `MouseEvent.detail` 就是连击计数）。

`App.vue`：工具栏 "文本" 按钮的 `setTool('select')` 改为 `setTool('textSelect')`（activeTool 字符串同步改；其余 markup/形状按钮不动）。

- [ ] **Step 5: 验证 + 提交**

Run: `cargo test -p rofd-web-view && cargo clippy -p rofd-web-view -- -D warnings && cd examples/web-app && npm run build:sdk && npm run build`
Expected: 全绿（web-app 生产构建过 = TS 类型/接口对齐）。

```bash
git add crates/web-view/src/wasm_editor.rs crates/web-view/sdk/src/index.ts examples/web-app/src/App.vue
git commit -m "feat(web-view): textSelect tool, onCopy/getSelectedText SDK surface, default clipboard"
```

---

### Task 8: 全量回归

**Files:** 无新改动（只验证）。

- [ ] **Step 1: 全量检查**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p rofd-component --target wasm32-unknown-unknown
```

Expected: 四条全绿；手术刀测试（round_trip/save_surgical）PASS（本计划未触 io）。小修正以 `chore: fmt/clippy fixes for text selection` 提交。

- [ ] **Step 2: demo 冒烟**

`cargo run -p native-app -- test/ru-yuan-ji-lu.ofd`：切"文本"工具——光标变 I 型；拖选正文跨行出现蓝色半透明选区；双击选词、三击选段；Ctrl+C 后在别处粘贴验证；切手型/创建工具选区消失。web 同验（`npm run dev`）+ DevTools 确认 `getSelectedText()` 返回选中文本、`createHighlightFromSelection('#FFFF00')` 生成可保存的高亮。

---

## Self-Review 记录

- **Spec 覆盖**：§5.1 方案 A（UI 态、不进 dom/历史/落盘、切工具/换页/文档变更清空）-> Task 3；§5.2 拖选跨行/空白清除/双击 CJK 分词/三击选段/click_count 字段/互斥/非目标不做 -> Task 2+3（click_count Ruling 1 落 bridge）；§5.3 hit_test_body_text/几何单一来源/text_selection_rects/最近字符边界 -> Task 1+2（fonts 参数与 tuple 细化见 Ruling 3/4）；§5.4 selected_text 换行策略/Ctrl+C on_copy/适配器默认剪贴板可关/转高亮走 create_annotation 可 undo 成功清空 -> Task 4+5+6+7；§5.5 涉及范围逐文件 -> File Structure 表全覆盖；§6 P3 测试项逐条 -> 各任务测试（offset 断言 Task 1、跨行 ranges Task 2/3、拼接换行 Task 4、rects 行数坐标 Task 2、转高亮 quad 页局部 + undo Task 5、切工具清空 Task 3；保存往返由 io 既有 annotation_roundtrip 的 Markup 覆盖）。
- **占位符**：Task 6 Step 1 的 arboard 无头测试降级为标志位断言（附理由）；Task 7 的 parse_color 指向"现有辅助或新增+单测"（带明确规格）；其余步骤均含完整代码。
- **类型一致性**：`code_char_cells(t, code, n)`、`code_char_count(code)`、`hit_test_body_text(doc, vp, point)`、`body_text_ranges_between(page, a, b)`、`word_range_at(page, hit)`、`paragraph_range_at(page, hit)`、`text_selection_rects(doc, vp, sel)`、`BodyTextRange/BodyTextSelection/TextHit`、`ViewEvent::PointerDown{click_count: u8}`、`Tool::TextSelect`、`DragState::TextSelect{anchor}`、`selected_text()`、`on_copy(Fn(String))`、`create_highlight_from_selection(Color)` 各任务间一致；Task 2 composite 调用点先传 `None`、Task 3 改真实字段，编译每任务收口。
