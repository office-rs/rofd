# 批注文字流式排版与 WPS 兼容保存 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让文字批注/文本框按框宽自动折行、框高向下扩展、光标/点击/IME 行感知，并把“逻辑文本 + 真实行几何 + WPS 兼容元数据”一起保存，使 rofd 重开后拖宽仍能流式重排、WPS 也能编辑/重排。

**Architecture:** 排版唯一实现在 `rofd-render`（parley `break_all_lines` + `OverflowWrap::Anywhere`），产出 `WrappedText`；组件把字体感知的“所需高度”注入 editor 的 measured 文本命令（editor 保持 dom-only）；保存时组件产出 `TextLayoutHints`（每行 top/baseline/height/width/advances），`rofd-io` 用它写逐行 TextObject，同时写 `Remark` + `AllRichText`/`ParagraphFormat`/`GrowMode`。logical content 永不写入软折行。

**Tech Stack:** Rust 1.98.1、parley 0.8、quick-xml、rofd-dom/render/editor/io/component。

**Spec:** `docs/superpowers/specs/2026-11-09-annotation-text-wrap-design.md`

## Global Constraints

- 代码注释与 commit message 用英文；spec/plan/AGENTS 可用中文；UI 文案可用中文。
- 库内绝不取系统时间（AGENTS §4.4）；本计划不涉及时间。
- 依赖方向：component 不依赖 io；io 不依赖 render；render 不依赖 io。
- 只改 `.annotations`，body 只读。
- 全部命令在仓库根 `D:\code\rofd` 执行：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`。
- 新增测试优先用 `crates/render/tests/fixtures/fonts/TestFont.ttf` 作为默认字体，避免系统字体差异导致跨机器不稳定。
- 提交信息用 conventional commits（`feat:` / `fix:` / `test:` / `docs:`）。
- 保存路径必须保持 `parse(serialize(a)) == a`（annotation roundtrip 测试）。

## Review Focus

1. **多字节文本（CJK/emoji）**：parley 的 `line.text_range()` 是 byte range，offset 映射必须按 char 计算，不能把 byte 当 char。测试在 Task 1、Task 2。
2. **软折行处的行尾空格**：断行常发生在空格后，空格属于上一行；保存/重开不能丢字符或把空格变成换行。测试在 Task 1、Task 6。
3. **CRLF 粘贴**：parley 把 `\r\n` 当两个换行；输入与解析都必须归一化为 `\n`，否则多出空行。测试在 Task 3、Task 7。
4. **富文本框首次编辑**：`collapse_runs` 与框高增长必须在同一个 Transaction；measure 闭包看到的是 collapse 后的 payload。测试在 Task 3、Task 4。
5. **退化/空框**：宽度为 0 时按一个 em 折、空内容不增长高度、保存不产生空行。测试在 Task 1、Task 4、Task 5。

---

### Task 1: 渲染层 wrapped 排版引擎 + dom 持久化提示类型

**Files:**
- Modify: `crates/dom/src/annotation.rs`
- Modify: `crates/render/src/text/shape.rs`
- Create: `crates/render/src/text/wrap.rs`
- Modify: `crates/render/src/text/font.rs`
- Modify: `crates/render/src/text/mod.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/src/text/wrap.rs`（内联 `#[cfg(test)] mod tests`）

**Interfaces:**
- Produces（`rofd-dom`）:
  ```rust
  pub const TEXTBOX_LINE_PITCH_RATIO: f64 = 1.2;
  #[derive(Debug, Clone, PartialEq)]
  pub struct TextLineHint {
      pub text: String,
      pub top: f64,
      pub baseline: f64,
      pub height: f64,
      pub width: f64,
      pub advances: Vec<f64>,
  }
  pub type TextLayoutHints = std::collections::HashMap<AnnotationId, Vec<TextLineHint>>;
  ```
- Produces（`rofd-render`）:
  ```rust
  pub struct WrappedLine {
      pub char_start: usize,
      pub char_len: usize,
      pub glyph_start: usize,
      pub glyph_end: usize,
      pub width: f64,
      pub top: f64,
      pub baseline: f64,
      pub height: f64,
      pub text: String,
  }
  pub struct WrappedText {
      pub font: Option<peniko::FontData>,
      pub glyphs: std::rc::Rc<Vec<ShapedGlyph>>,
      pub lines: std::rc::Rc<Vec<WrappedLine>>,
      pub total_chars: usize,
  }
  impl WrappedText {
      pub fn height(&self) -> f64;                                    // 末行 top + height；无行 -> 0.0
      pub fn line_at_offset(&self, offset: usize) -> (usize, usize);  // (行, 行内列)
      pub fn to_layout_hints(&self) -> Vec<rofd_dom::TextLineHint>;
  }
  impl FontStore {
      pub fn shape_wrapped(&self, font_id: &FontId, text: &str, size: f64,
                           max_width: f64) -> std::rc::Rc<WrappedText>;
  }
  ```

- [ ] **Step 1: Write the failing tests in `crates/render/src/text/wrap.rs`**

```rust
use std::sync::Arc;
use rofd_dom::{FontId, Resources};
use crate::text::FontStore;

fn store() -> FontStore {
    let bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    FontStore::from_resources(&Resources::default(), Arc::new(bytes.to_vec()))
}

#[test]
fn wraps_latin_at_spaces() {
    let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    assert_eq!(w.lines.len(), 2, "two visual lines");
    assert_eq!(w.lines[0].text, "abc def ");   // trailing space stays on line 1
    assert_eq!(w.lines[1].text, "ghi jkl");
    assert_eq!(w.lines[0].char_start, 0);
    assert_eq!(w.lines[1].char_start, 8);
}

#[test]
fn wraps_cjk_per_char() {
    let w = store().shape_wrapped(&FontId::new("F1"), "你好世界你好世界", 12.0, 30.0);
    assert_eq!(w.lines.len(), 3);
    assert!(w.lines.iter().all(|l| l.char_len == 2));
    assert_eq!(w.total_chars, 8);
}

#[test]
fn breaks_long_words() {
    // TestFont @12 is ~7.2mm/char; 45mm forces emergency breaks inside the word.
    let w = store().shape_wrapped(&FontId::new("F1"), "abcdefghijklmnop", 12.0, 45.0);
    assert_eq!(w.lines.len(), 3);
    let joined: String = w.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(joined, "abcdefghijklmnop");
}

#[test]
fn explicit_newline_is_a_line() {
    let w = store().shape_wrapped(&FontId::new("F1"), "ab cd\nef gh", 12.0, 1000.0);
    assert_eq!(w.lines.len(), 2);
    assert_eq!(w.lines[0].text, "ab cd");
    assert_eq!(w.lines[1].text, "ef gh");
    assert_eq!(w.total_chars, 11);
}

#[test]
fn empty_content_has_one_empty_line() {
    let w = store().shape_wrapped(&FontId::new("F1"), "", 12.0, 40.0);
    assert_eq!(w.lines.len(), 1);
    assert_eq!(w.lines[0].text, "");
    assert_eq!(w.total_chars, 0);
    assert_eq!(w.height(), 0.0, "empty content needs no height");
}

#[test]
fn line_at_offset_boundaries() {
    let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    assert_eq!(w.line_at_offset(0), (0, 0));
    assert_eq!(w.line_at_offset(7), (0, 7), "trailing space belongs to line 1");
    assert_eq!(w.line_at_offset(8), (1, 0), "soft break -> next line start");

    let w = store().shape_wrapped(&FontId::new("F1"), "ab cd\nef gh", 12.0, 1000.0);
    assert_eq!(w.line_at_offset(5), (0, 5), "offset on the newline -> line 1 end");
    assert_eq!(w.line_at_offset(6), (1, 0));
    assert_eq!(w.line_at_offset(11), (1, 5), "end of content -> last line end");
}

#[test]
fn to_layout_hints_geometry() {
    let w = store().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    let h = w.to_layout_hints();
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].top, 0.0, "first line top is the text origin");
    assert!((h[0].advances.iter().sum::<f64>() - h[0].width).abs() < 1e-6);
    assert!((h[1].top - h[0].height).abs() < 1e-6, "lines stack by height");
    assert!(h[0].baseline > 0.0);
}

#[test]
fn wrapped_layout_is_cached() {
    let s = store();
    let a = s.shape_wrapped(&FontId::new("F1"), "cache me", 12.0, 40.0);
    let b = s.shape_wrapped(&FontId::new("F1"), "cache me", 12.0, 40.0);
    assert!(std::rc::Rc::ptr_eq(&a, &b), "same key -> same Rc");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-render text::wrap`
Expected: compile error `shape_wrapped` / `WrappedText` not found.

- [ ] **Step 3: Add dom types**

In `crates/dom/src/annotation.rs` add the constant, `TextLineHint`, `TextLayoutHints` exactly as in Interfaces.

- [ ] **Step 4: Implement wrapped shaping in `crates/render/src/text/shape.rs`**

Add:
```rust
pub(crate) struct RawLine {
    pub byte_range: std::ops::Range<usize>,
    pub glyph_count: usize,
    pub width: f64,
    pub top: f64,
    pub baseline: f64,
    pub height: f64,
}

pub(crate) fn shape_grouped_wrapped_with_family(
    fcx: &mut FontContext, text: &str, size: f64,
    family: FontFamily<'_>, max_advance: f32,
) -> (Vec<(FontData, Vec<ShapedGlyph>)>, Vec<RawLine>)
```
Implementation: same builder defaults as `shape_grouped_with_family`, plus
`builder.push_default(StyleProperty::OverflowWrap(parley::style::OverflowWrap::Anywhere));`
then `layout.break_all_lines(Some(max_advance))`. Iterate `layout.lines()`:
`width = metrics().advance`, `top = metrics().min_coord`, `baseline = metrics().baseline`,
`height = metrics().line_height`, `byte_range = line.text_range()`, glyph count from
`positioned_glyphs()`; merge consecutive same-font runs exactly like the existing function.
Refactor the existing `shape_grouped_with_family` to call a shared private
`shape_layout(..., max_advance: Option<f32>)` and keep its `(groups, max line advance)` return.

- [ ] **Step 5: Implement `WrappedText` + `FontStore::shape_wrapped`**

`crates/render/src/text/wrap.rs`: construction from `(groups, raw_lines, text, total_chars)`:
- clamp each byte range to `text.len()`;
- `line_text` = slice, `trim_end_matches(['\n', '\r'])`;
- `char_start` = chars in `text[..range.start.clamp(len)]`;
- `char_len` = `line_text.chars().count()`;
- glyph ranges accumulate across lines (groups are line-ordered);
- `advances[i] = x[i+1]-x[i]`, last = `line.width - x[last]` (clamp `>= 0`);
- `height()` returns `0.0` when `total_chars == 0`, else last `top + height`.
- `line_at_offset` implements the spec §4.2 algorithm.
- `to_layout_hints` maps each line.

`crates/render/src/text/font.rs`: add field
```rust
#[allow(clippy::type_complexity)]
wrapped_cache: RefCell<HashMap<FontId, HashMap<u64, HashMap<u64, HashMap<String, Rc<WrappedText>>>>>>,
```
init in `from_resources`; clear it in `register_font` and `register_font_slice` next to
`glyph_cache`. `shape_wrapped` follows `shape`'s cache pattern keyed by
`(font_id, size.to_bits(), max_width.to_bits(), text)`; call
`family_list_for(font_id)` then `shape_grouped_wrapped_with_family(..., max_width as f32)`.
Export `wrap::{WrappedLine, WrappedText}` from `text/mod.rs` and `lib.rs`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-render text::wrap`
Expected: PASS.

Run: `cargo test -p rofd-render && cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/dom/src/annotation.rs crates/render/src/text crates/render/src/lib.rs
git commit -m "feat(render): wrapped annotation text layout with line hints"
```

---

### Task 2: 渲染、光标、点击、preedit 接入 wrapped 布局

**Files:**
- Modify: `crates/render/src/annotation_scene.rs`
- Modify: `crates/render/src/caret_rect.rs`
- Modify: `crates/render/src/preedit_overlay.rs`
- Modify: `crates/render/src/lib.rs`
- Test: 上述文件内联测试

**Interfaces:**
- Consumes: `FontStore::shape_wrapped`, `WrappedText`, `WrappedLine`（Task 1）。
- Produces:
  ```rust
  pub fn caret_offset_at_point(
      content: &str, font_id: &FontId, size: f64, max_width: f64,
      fonts: &FontStore, x_in_rect: f64, y_in_rect: f64,
  ) -> usize;
  pub enum VerticalMove { Up, Down }
  pub fn caret_offset_vertical(
      content: &str, font_id: &FontId, size: f64, max_width: f64,
      fonts: &FontStore, offset: usize, direction: VerticalMove,
      preferred_x: Option<f64>,
  ) -> (usize, f64);
  ```

- [ ] **Step 1: Write failing tests**

In `crates/render/src/caret_rect.rs` tests (reuse `doc_with_font()`):
```rust
#[test]
fn caret_rect_second_line_uses_line_top() {
    let (mut doc, fonts) = doc_with_font();
    let id = push_ann(&mut doc, AnnotationPayload::TextBox {
        rect: Rect { x: 10.0, y: 20.0, w: 45.0, h: 40.0 },
        content: "abc def ghi jkl".into(),
        font: FontId::new("F1"), size: 12.0,
        color: Color::Rgb(0,0,0), border: None, runs: Vec::new(),
    }, AnnotationKind::TextBox);
    let w = fonts.shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    let line1 = &w.lines[1];
    let r = caret_rect(&doc, &vp(), &fonts, &id, 9).unwrap(); // line 1, local 1
    // viewport y = page_origin_y (20) + rect.y (20) + line.top
    assert!((r.y - (40.0 + line1.top)).abs() < 1e-6, "second line y");
    let expected_x = 300.0 + 10.0 + w.glyphs[line1.glyph_start + 1].x as f64;
    assert!((r.x - expected_x).abs() < 1e-6);
}

#[test]
fn caret_offset_at_point_second_line() {
    let fonts = /* TestFont store */;
    let w = fonts.shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    let off = caret_offset_at_point("abc def ghi jkl", &FontId::new("F1"), 12.0, 45.0,
                                    &fonts, 1.0, w.lines[1].top + 1.0);
    assert_eq!(off, 8, "left edge of line 2 -> its first char");
}

#[test]
fn caret_offset_vertical_preserves_column() {
    let fonts = /* TestFont store */;
    let text = "abc def ghi jkl";
    let (down, pref) = caret_offset_vertical(text, &FontId::new("F1"), 12.0, 45.0,
                                             &fonts, 2, VerticalMove::Down, None);
    assert!(down >= 8 && down <= 11, "lands on line 2 near column 2");
    let (up, _) = caret_offset_vertical(text, &FontId::new("F1"), 12.0, 45.0,
                                        &fonts, down, VerticalMove::Up, Some(pref));
    assert_eq!(up, 2, "returns to the original column");
}

#[test]
fn vertical_edges_go_to_text_bounds() {
    let fonts = /* TestFont store */;
    let text = "abc def ghi jkl";
    assert_eq!(caret_offset_vertical(text, &FontId::new("F1"), 12.0, 45.0,
                                     &fonts, 2, VerticalMove::Up, None).0, 0);
    assert_eq!(caret_offset_vertical(text, &FontId::new("F1"), 12.0, 45.0,
                                     &fonts, 11, VerticalMove::Down, None).0, 15);
}

#[test]
fn caret_offsets_use_chars_not_bytes_for_cjk() {
    // "你好世界" @12, width 30 -> two lines of two chars; each char is 3 UTF-8
    // bytes, so a byte/char mix-up would put offset 2 on line 1.
    let fonts = /* TestFont store */;
    let w = fonts.shape_wrapped(&FontId::new("F1"), "你好世界", 12.0, 30.0);
    assert_eq!(w.lines.len(), 2);
    assert_eq!(w.line_at_offset(2), (1, 0));
    assert_eq!(caret_offset_at_point("你好世界", &FontId::new("F1"), 12.0, 30.0,
                                     &fonts, 1.0, w.lines[1].top + 1.0), 2);
}
```
In `crates/render/src/preedit_overlay.rs` tests add:
```rust
#[test]
fn preedit_positions_on_second_line() {
    // TextBox rect (0,0,45,40), content "abc def ghi jkl", offset 8, preedit "XY".
    // Assert every GlyphRun transform ty == rect.y + wrapped.lines[1].top and
    // tx == rect.x + 0.0 (line 2 start).
}
```
In `crates/render/src/annotation_scene.rs` tests add:
```rust
#[test]
fn textbox_wraps_glyphs_to_multiple_baselines() {
    // TextBox rect (0,0,45,40), content "abc def ghi jkl", size 12.
    // glyph_baselines(scene) must contain at least 2 distinct values.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-render caret rect` / `cargo test -p rofd-render preedit` / `cargo test -p rofd-render annotation_scene`
Expected: new tests FAIL to compile (missing functions) or assert-fail (single-line rendering).

- [ ] **Step 3: Implement**

- `draw_text_in_rect`: call `fonts.shape_wrapped(text.font, text.content, text.size, rect.w.max(text.size))`, then draw `w.font` + `w.glyphs` (map to `imaging::record::Glyph { id, x, y }`) at the rect origin via `draw_glyph_run`.
- `caret_rect`: add a `wrapped` branch only when payload is `TextBox { runs, .. }` with `runs.is_empty()`; map `(line, local)` from `line_at_offset`, x = glyph x (or `line.width` at line end), y = `rect.y + line.top`, h = `size`. Note/Watermark/rich keep the old code path.
- `caret_offset_at_point`: layout, pick line by y (`top <= y < top+height`, clamp), then nearest boundary among that line's glyph x's + `line.width`; return `line.char_start + index`.
- `caret_offset_vertical`: layout, `(line, local)`, current x, `x = preferred_x.unwrap_or(current_x)`; boundary rules per spec §1.7; return `(offset, x)`.
- `preedit_overlay`: layout at `rect.w.max(size)`; locate `(line, local)` for `offset`; pen = `(rect.x + x_in_line, rect.y + line.top)`.
- `rofd-export` / `rofd-convert` / print need **no changes**: they consume `draw_annotations` and inherit the wrapped scene.
- Export the new functions/enums from `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-render`
Expected: PASS, including all pre-existing single-line caret/render tests.

Run: `cargo clippy -p rofd-render --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/render/src
git commit -m "feat(render): wrap annotation text rendering and caret geometry"
```

---

### Task 3: Editor measured 文本命令、换行归一化、preferred_x

**Files:**
- Modify: `crates/editor/src/commands/text_commands.rs`
- Modify: `crates/editor/src/editor.rs`
- Test: 上述文件内联测试

**Interfaces:**
- Produces（`Editor`）:
  ```rust
  pub fn insert_text_measured<F>(&mut self, id: &AnnotationId, offset: usize,
                                 chars: &str, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn delete_text_measured<F>(&mut self, id: &AnnotationId, offset: usize,
                                 len: usize, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn set_cursor_with_preferred_x(&mut self, annotation: AnnotationId,
                                     offset: usize, preferred_x: Option<f32>);
  pub fn normalize_newlines(s: &str) -> String; // 自由函数：rofd_editor::normalize_newlines
  ```
- `insert_text` / `delete_text` delegate with `|_| None`.
- `set_cursor` delegates to `set_cursor_with_preferred_x(.., None)`.

- [ ] **Step 1: Write failing tests**

In `crates/editor/src/commands/text_commands.rs` tests:
```rust
#[test]
fn insert_text_measured_grows_height_in_same_transaction() {
    let (mut e, id) = flat_textbox_editor(); // rect h = 10.0
    e.insert_text_measured(&id, 0, "x", |payload| {
        assert!(matches!(payload, AnnotationPayload::TextBox { .. }));
        Some(77.0)
    });
    match textbox_payload(&e, &id) {
        AnnotationPayload::TextBox { rect, content, .. } => {
            assert_eq!(rect.h, 77.0);
            assert_eq!(content, "xhi");
        }
        other => panic!("expected TextBox, got {other:?}"),
    }
    assert!(e.undo());
    match textbox_payload(&e, &id) {
        AnnotationPayload::TextBox { rect, content, .. } => {
            assert_eq!(rect.h, 10.0, "undo restores height too");
            assert_eq!(content, "hi");
        }
        other => panic!("expected TextBox, got {other:?}"),
    }
}

#[test]
fn insert_text_measured_none_keeps_height() {
    let (mut e, id) = flat_textbox_editor();
    e.insert_text_measured(&id, 0, "x", |_| None);
    assert_eq!(textbox_rect(&e, &id).h, 10.0);
}

#[test]
fn insert_text_normalizes_crlf() {
    let (mut e, id) = flat_textbox_editor();
    e.insert_text(&id, 0, "a\r\nb\rc");
    assert_eq!(textbox_content(&e, &id), "a\nb\nc");
}

#[test]
fn set_annotation_text_normalizes_crlf() {
    let (mut e, id) = flat_textbox_editor();
    e.set_annotation_text(&id, "a\r\nb");
    assert_eq!(textbox_content(&e, &id), "a\nb");
}

#[test]
fn delete_text_measured_applies_growth() {
    let (mut e, id) = flat_textbox_editor_with("hello", 10.0);
    e.delete_text_measured(&id, 0, 1, |_| Some(33.0));
    assert_eq!(textbox_rect(&e, &id).h, 33.0);
}

#[test]
fn rich_textbox_first_edit_collapses_and_grows_atomically() {
    let (mut e, id) = rich_textbox_editor(); // rect h = 10.0, runs non-empty
    e.insert_text_measured(&id, 4, "x", |payload| {
        let AnnotationPayload::TextBox { runs, rect, font, .. } = payload else {
            panic!("expected TextBox");
        };
        assert!(runs.is_empty(), "measure sees the post-collapse payload");
        assert_eq!(font.0, "118", "collapsed to runs[0]");
        Some(rect.h + 5.0)
    });
    match textbox_payload(&e, &id) {
        AnnotationPayload::TextBox { rect, runs, .. } => {
            assert_eq!(rect.h, 15.0);
            assert!(runs.is_empty());
        }
        other => panic!("expected TextBox, got {other:?}"),
    }
    assert!(e.undo());
    assert!(matches!(
        textbox_payload(&e, &id),
        AnnotationPayload::TextBox { runs, rect, .. } if !runs.is_empty() && rect.h == 10.0
    ), "one undo restores runs AND height");
}
```
Add helpers next to the existing `textbox_payload` / `content_of`:
```rust
fn flat_textbox_editor() -> (Editor, AnnotationId) {
    flat_textbox_editor_with("hi", 10.0)
}

fn flat_textbox_editor_with(content: &str, h: f64) -> (Editor, AnnotationId) {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1);
    let id = e.create_annotation(
        AnnotationKind::TextBox,
        PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect { x: 0.0, y: 0.0, w: 30.0, h },
            content: content.into(),
            font: FontId::new("F1"),
            size: 3.5,
            color: Color::Rgb(0, 0, 0),
            border: None,
            runs: vec![],
        },
    );
    (e, id)
}

fn textbox_rect(e: &Editor, id: &AnnotationId) -> Rect {
    match textbox_payload(e, id) {
        AnnotationPayload::TextBox { rect, .. } => *rect,
        other => panic!("expected TextBox, got {other:?}"),
    }
}

fn textbox_content(e: &Editor, id: &AnnotationId) -> String {
    match textbox_payload(e, id) {
        AnnotationPayload::TextBox { content, .. } => content.clone(),
        other => panic!("expected TextBox, got {other:?}"),
    }
}
```
In `crates/editor/src/editor.rs` tests:
```rust
#[test]
fn set_cursor_with_preferred_x_round_trips() {
    let mut e = Editor::new();
    e.set_cursor_with_preferred_x(AnnotationId::from_int(1), 3, Some(4.5));
    let c = e.text_cursor().unwrap();
    assert_eq!((c.offset, c.preferred_x), (3, Some(4.5)));
    e.set_cursor(AnnotationId::from_int(1), 3);
    assert_eq!(e.text_cursor().unwrap().preferred_x, None);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-editor`
Expected: compile errors for the missing methods.

- [ ] **Step 3: Implement**

- Add `pub fn normalize_newlines(s: &str) -> String { s.replace("\r\n", "\n").replace('\r', "\n") }` in `text_commands.rs` and re-export it from `crates/editor/src/lib.rs` (`pub use commands::text_commands::normalize_newlines;` — adjust to the module path in use). Apply it inside `insert_text_measured` before insertion and in `set_annotation_text`.
- `insert_text_measured` / `delete_text_measured`: build `after`, run `collapse_runs`, mutate content, call `measure(&after.payload)`; if `Some(h)` and payload is `TextBox { rect, .. }`, set `rect.h = h`; then bump `modified` and execute the same `replace_txn`.
- `set_cursor_with_preferred_x` in `editor.rs`; `set_cursor` delegates.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-editor`
Expected: PASS.

Run: `cargo clippy -p rofd-editor --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/editor/src
git commit -m "feat(editor): measured text edits, newline normalization, preferred caret column"
```

---

### Task 4: 组件编辑路径接入（增长 / Enter / 上下键 / 点击 / 改宽）

**Files:**
- Modify: `crates/component/src/ofd_component.rs`
- Test: `ofd_component.rs` 内联 `mod tests`

**Interfaces:**
- Consumes: Task 1–3 的 `shape_wrapped`、`caret_offset_at_point`、`caret_offset_vertical`、measured editor 命令。
- Produces（组件内部，`pub(crate)` 或私有）:
  ```rust
  fn measure_textbox_growth(store: &FontStore, payload: &AnnotationPayload) -> Option<f64>;
  fn with_text_height(&mut self, id: &AnnotationId, rect: Rect) -> Rect;
  ```

- [ ] **Step 1: Write failing tests**

Add helper:
```rust
fn component_with_narrow_textbox(content: &str, w: f64, h: f64) -> OfdComponent {
    let bytes = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf").to_vec();
    let mut c = OfdComponent::new(OfdConfig::new(Arc::new(bytes)));
    c.set_clock("t".into(), 1);
    let mut doc = OfdDocument::default();
    doc.pages.push(Page { id: PageId::new("P0"),
        physical_box: Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 },
        layers: vec![Layer::default()], template: None });
    c.load_document(doc);
    let id = c.create_annotation(AnnotationKind::TextBox, PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect { x: 0.0, y: 0.0, w, h },
            content: content.into(), font: FontId::new("F1"), size: 12.0,
            color: Color::Rgb(0,0,0), border: None, runs: Vec::new(),
        });
    c.editor.set_cursor(id, content.chars().count());
    c.viewport = rofd_render::Viewport { scroll: (0.0,0.0), zoom: 1.0,
        size: (200.0,200.0), page_gap: 0.0 };
    c
}
```
Tests:
```rust
#[test]
fn typing_wraps_without_mutating_content_and_grows_height() {
    let mut c = component_with_narrow_textbox("abc def", 45.0, 14.0);
    for ch in [' ', 'g', 'h', 'i', ' ', 'j', 'k', 'l'] {
        c.handle_event(&ViewEvent::KeyDown { key: Key::Char(ch), modifiers: Modifiers::default() });
    }
    let content = textbox_content(&c);
    assert_eq!(content, "abc def ghi jkl", "soft wrap never writes \\n");
    assert!(!content.contains('\n'));
    let snap = c.save_snapshot();
    assert!(snap.text_layout.values().next().unwrap().len() >= 2);
    let ann = c.document().annotations.for_page(&PageId::new("P0")).first().unwrap();
    assert!(matches!(&ann.payload, AnnotationPayload::TextBox { rect, .. } if rect.h > 14.0));
}

#[test]
fn enter_inserts_newline_and_moves_cursor() {
    let mut c = component_with_narrow_textbox("ab", 100.0, 10.0);
    c.handle_event(&ViewEvent::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
    assert_eq!(textbox_content(&c), "ab\n");
    assert_eq!(c.editor.text_cursor().unwrap().offset, 3);
    let ann = c.document().annotations.for_page(&PageId::new("P0")).first().unwrap();
    assert!(matches!(&ann.payload, AnnotationPayload::TextBox { rect, .. } if rect.h > 10.0));
}

#[test]
fn backspace_at_line_start_joins_lines() {
    let mut c = component_with_narrow_textbox("ab\ncd", 100.0, 30.0);
    c.editor.set_cursor(c.editor.document().annotations.for_page(&PageId::new("P0"))[0].id.clone(), 3);
    c.handle_event(&ViewEvent::KeyDown { key: Key::Backspace, modifiers: Modifiers::default() });
    assert_eq!(textbox_content(&c), "abcd");
}

#[test]
fn arrow_down_then_up_keeps_column() {
    let mut c = component_with_narrow_textbox("abc def ghi jkl", 45.0, 40.0);
    let id = c.document().annotations.for_page(&PageId::new("P0"))[0].id.clone();
    c.editor.set_cursor(id.clone(), 2);
    c.handle_event(&ViewEvent::KeyDown { key: Key::ArrowDown, modifiers: Modifiers::default() });
    let down = c.editor.text_cursor().unwrap().offset;
    assert!(down >= 8, "moved to the second visual line");
    assert!(c.editor.text_cursor().unwrap().preferred_x.is_some());
    c.handle_event(&ViewEvent::KeyDown { key: Key::ArrowUp, modifiers: Modifiers::default() });
    assert_eq!(c.editor.text_cursor().unwrap().offset, 2);
}

#[test]
fn arrow_edges_go_to_text_bounds() {
    let mut c = component_with_narrow_textbox("abc def ghi jkl", 45.0, 40.0);
    let id = c.document().annotations.for_page(&PageId::new("P0"))[0].id.clone();
    c.editor.set_cursor(id.clone(), 2);
    c.handle_event(&ViewEvent::KeyDown { key: Key::ArrowUp, modifiers: Modifiers::default() });
    assert_eq!(c.editor.text_cursor().unwrap().offset, 0);
    c.editor.set_cursor(id, 15);
    c.handle_event(&ViewEvent::KeyDown { key: Key::ArrowDown, modifiers: Modifiers::default() });
    assert_eq!(c.editor.text_cursor().unwrap().offset, 15);
}

#[test]
fn click_caret_offset_uses_click_y() {
    let mut c = component_with_narrow_textbox("abc def ghi jkl", 45.0, 40.0);
    let id = c.document().annotations.for_page(&PageId::new("P0"))[0].id.clone();
    let w = c.font_store.as_ref().unwrap().shape_wrapped(&FontId::new("F1"), "abc def ghi jkl", 12.0, 45.0);
    let y = w.lines[1].top + 1.0;
    assert_eq!(c.click_caret_offset(&id, (1.0, y)), Some(8));
}

#[test]
fn narrowing_resize_grows_height_widening_does_not_shrink() {
    let mut c = component_with_narrow_textbox("abc def ghi jkl", 120.0, 14.0);
    let id = c.document().annotations.for_page(&PageId::new("P0"))[0].id.clone();
    c.resize_annotation(&id, Rect { x: 0.0, y: 0.0, w: 45.0, h: 14.0 });
    let h1 = textbox_rect(&c).h;
    assert!(h1 > 14.0, "narrower width needs more lines");
    c.resize_annotation(&id, Rect { x: 0.0, y: 0.0, w: 120.0, h: h1 });
    assert!((textbox_rect(&c).h - h1).abs() < 1e-9, "widening never shrinks");
}

#[test]
fn note_enter_inserts_but_does_not_grow_note_rect() {
    let mut c = component_with_note();
    let rofd_editor::AnnotationSelection::Single(id) = c.editor.selection().clone() else {
        panic!("note is selected after creation");
    };
    c.editor.set_cursor(id.clone(), 2);
    c.handle_event(&ViewEvent::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
    let ann = c.editor.document().annotations.find(&id).unwrap();
    assert!(matches!(&ann.payload, AnnotationPayload::Note { rect, content, .. }
        if content == "hi\n" && rect.h == 100.0));
}

#[test]
fn zero_width_box_wraps_at_one_em_without_panic() {
    let mut c = component_with_narrow_textbox("abc", 0.0, 14.0);
    for ch in ['d', 'e'] {
        c.handle_event(&ViewEvent::KeyDown { key: Key::Char(ch), modifiers: Modifiers::default() });
    }
    assert_eq!(textbox_content(&c), "abcde");
    let snap = c.save_snapshot();
    let lines = snap.text_layout.values().next().unwrap();
    assert!(lines.len() >= 3, "clamped wrap width (one em) still breaks lines");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-component`
Expected: new tests fail (no wrapping / no growth / Enter no-op).

- [ ] **Step 3: Implement**

- Add `measure_textbox_growth(store, payload)`: `TextBox` + `runs.is_empty()` + `total_chars > 0` → `shape_wrapped(font, content, size, rect.w.max(size))`; return `Some(required)` iff `required > rect.h + 1e-9`.
- `insert_at_cursor`: ensure `font_store`; **normalize `text` first** (`let text = rofd_editor::normalize_newlines(text);`) so the cursor advance counts the normalized chars; borrow `store`; call `editor.insert_text_measured(..., |p| measure_textbox_growth(store, p))`; set cursor; fire callbacks.
- `force_commit_preedit`: normalize the preedit text the same way before computing `new_off` and inserting.
- `Key::Char` branch delegates to `insert_at_cursor`; add `Key::Enter => self.insert_at_cursor("\n")`.
- Backspace/Delete use `delete_text_measured` with the same measure closure.
- `with_text_height(id, rect)`: read `(content, font, size, runs.is_empty())` from the annotation, drop the borrow, ensure `font_store`, compute `required`, return `rect` with `h.max(required)`; use it in public `resize_annotation` and the `DragState::Resize` commit before `editor.resize_annotation`.
- `click_caret_offset`: compute `local_y = (p.1 - origin_y) / zoom`; flat TextBox → `caret_offset_at_point(..., local_x - rect.x, local_y - rect.y)`; Note/rich → old `caret_offset_at_x`.
- ArrowUp/Down in the cursor branch: flat TextBox only; `caret_offset_vertical(content, font, size, rect.w.max(size), store, cursor.offset, dir, cursor.preferred_x.map(f64::from))`; `editor.set_cursor_with_preferred_x(...)`; `fire_cursor_change`; repaint.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-component`
Expected: PASS (existing + new).

Run: `cargo clippy -p rofd-component --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/ofd_component.rs
git commit -m "feat(component): wrap-aware text editing, Enter newline, vertical caret moves"
```

---

### Task 5: 组件保存快照 `save_snapshot`

**Files:**
- Modify: `crates/component/src/ofd_component.rs`
- Test: `ofd_component.rs` 内联 `mod tests`

**Interfaces:**
- Consumes: Task 1 `shape_wrapped` / `WrappedText::to_layout_hints` / `TextLayoutHints`。
- Produces:
  ```rust
  pub struct SaveSnapshot {
      pub document: OfdDocument,
      pub text_layout: rofd_dom::TextLayoutHints,
  }
  impl OfdComponent {
      pub fn save_snapshot(&self) -> SaveSnapshot;
  }
  ```

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn save_snapshot_keeps_logical_content_and_fits_height() {
    let c = component_with_narrow_textbox("abc def ghi jkl", 45.0, 14.0);
    let snap = c.save_snapshot();
    let id = snap.document.annotations.for_page(&PageId::new("P0"))[0].id.clone();
    let ann = snap.document.annotations.find(&id).unwrap();
    let AnnotationPayload::TextBox { rect, content, .. } = &ann.payload else { panic!() };
    assert_eq!(content, "abc def ghi jkl", "logical content is not hard-wrapped");
    assert!(!content.contains('\n'));
    let lines = &snap.text_layout[&id];
    assert_eq!(lines.len(), 2);
    assert!(rect.h >= lines.last().unwrap().top + lines.last().unwrap().height - 1e-9);
}

#[test]
fn save_snapshot_grows_foreign_overwide_height() {
    let c = component_with_narrow_textbox("abc def ghi jkl", 45.0, 8.0);
    let snap = c.save_snapshot();
    let ann = &snap.document.annotations.for_page(&PageId::new("P0"))[0];
    assert!(matches!(&ann.payload, AnnotationPayload::TextBox { rect, .. } if rect.h > 8.0));
}

#[test]
fn save_snapshot_skips_rich_note_and_empty_boxes() {
    // rich: runs non-empty -> no hint
    // note -> no hint
    // TextBox with empty content -> no hint, rect.h unchanged
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-component save_snapshot`
Expected: compile error (no `save_snapshot`).

- [ ] **Step 3: Implement**

Add `SaveSnapshot` + `impl OfdComponent::save_snapshot(&self)`:
```
doc = self.editor.document().clone()
store = self.font_store.clone().unwrap_or_else(|| self.build_font_store())
for ann in doc.annotations.by_page.values_mut().flatten():
    if let TextBox { rect, content, font, size, runs, .. } = &mut ann.payload:
        if !runs.is_empty() || content.chars().count() == 0: continue
        w = store.shape_wrapped(font, content, *size, rect.w.max(*size))
        if w.lines.is_empty(): continue
        hints.insert(ann.id.clone(), w.to_layout_hints())
        rect.h = rect.h.max(w.height())
SaveSnapshot { document: doc, text_layout: hints }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-component`
Expected: PASS.

Run: `cargo clippy -p rofd-component --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/ofd_component.rs
git commit -m "feat(component): save snapshot with per-line text layout hints"
```

---

### Task 6: io 序列化（hints + Remark/WPS 参数 + 逐行外观）

**Files:**
- Modify: `crates/io/src/serialize/annotation.rs`
- Modify: `crates/io/src/serialize/package.rs`
- Modify: `crates/io/src/save.rs`
- Modify: `crates/io/src/lib.rs`
- Test: `crates/io/src/serialize/annotation.rs` 内联测试 + `crates/io/tests/annotation_conformance.rs`

**Interfaces:**
- Consumes: `rofd_dom::{TextLineHint, TextLayoutHints, TEXTBOX_LINE_PITCH_RATIO}`。
- Produces:
  ```rust
  pub fn serialize_page_annot(
      page: &PageId, anns: &[Annotation], next_id: &mut u64,
      res: &rofd_dom::Resources, hints: &TextLayoutHints,
  ) -> String;
  pub fn save_ofd_with_text_layout(
      doc: &OfdDocument, pkg: &PackageHandle, hints: &TextLayoutHints,
  ) -> Result<Vec<u8>, OfdError>;
  pub fn write_ofd_with_text_layout(
      doc: &OfdDocument, hints: &TextLayoutHints,
  ) -> Result<Vec<u8>, OfdError>;
  ```
- 旧 `save_ofd` / `write_ofd` 委托空 hints。

- [ ] **Step 1: Write failing tests**

In `crates/io/src/serialize/annotation.rs` tests:
```rust
fn textbox_with_hints() -> (Annotation, TextLayoutHints) {
    let ann = /* TextBox: rect (0,0,45,40), content "abc def ghi jkl", font F1, size 12 */;
    let mut hints = TextLayoutHints::new();
    hints.insert(ann.id.clone(), vec![
        TextLineHint { text: "abc def ".into(), top: 0.0, baseline: 11.14,
                       height: 13.97, width: 48.42, advances: vec![7.4; 8] },
        TextLineHint { text: "ghi jkl".into(), top: 13.97, baseline: 25.11,
                       height: 13.97, width: 35.99, advances: vec![7.6; 7] },
    ]);
    (ann, hints)
}

#[test]
fn textbox_hints_emit_one_textobject_per_line_with_real_geometry() {
    let (ann, hints) = textbox_with_hints();
    let mut next_id = 10;
    let xml = serialize_page_annot(&PageId::new("1"), &[ann], &mut next_id,
                                   &Resources::default(), &hints);
    assert_eq!(xml.matches("<ofd:TextObject").count(), 2);
    assert!(xml.contains("Boundary=\"0 13.97 35.99 13.97\""), "{xml}");
    assert!(xml.contains("Y=\"11.140000000000001\"") || xml.contains("Y=\"11.14\""), "{xml}");
    assert!(xml.contains("DeltaX=\"7.4 7.4"), "{xml}");
}

#[test]
fn textbox_without_hints_keeps_newline_split() {
    // content "a\nb", no hints -> 2 TextObjects, second Boundary y = 1.2*size.
}

#[test]
fn textbox_writes_wps_metadata() {
    let (ann, _) = textbox_with_hints(); // border: Some(black)
    let mut next_id = 10;
    let xml = serialize_page_annot(&PageId::new("1"), &[ann], &mut next_id,
                                   &Resources::default(), &TextLayoutHints::new());
    assert!(xml.contains("<ofd:Remark>abc def ghi jkl</ofd:Remark>"));
    assert!(xml.contains("Name=\"AllRichText\""));
    assert!(xml.contains("Name=\"ParagraphFormat\""));
    assert!(xml.contains("Name=\"GrowMode\">FixWidth"));
    assert!(xml.contains("\"CharSpace\":0.0"));
}

#[test]
fn typewriter_writes_grow_mode_none() {
    // TextBox border: None -> Name="GrowMode">None
}

#[test]
fn rich_textbox_writes_span_per_run() {
    // runs: bold+italic+color text "粗" then plain "体"
    // assert two "<span", "font-weight:700", "font-style:italic", "#4bac c6" lowercase hex
}
```
Update the two existing `serialize_page_annot(...)` call sites in tests with
`&Resources::default(), &TextLayoutHints::new()`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-io serialize::annotation`
Expected: compile error (new parameter) / missing metadata.

- [ ] **Step 3: Implement**

- Thread `res` + `hints` through `serialize_page_annot` → `serialize_one` → `appearance_xml`.
- `serialize_one`: for `AnnotationKind::TextBox`, add `<ofd:Remark>` (always, escaped) and append three Parameters inside the existing `<ofd:Parameters>`:
  `AllRichText`, `ParagraphFormat`, `GrowMode` (§4.4 of the spec).
- Helpers: `html_escape`, `json_escape`, `color_hex`, `paragraph_format_json(font_name, size, color, bold, italic)`, `all_rich_text_html(payload, res)`.
  - `font_name(res, font_id)` = `res.fonts.get(id).and_then(|f| f.family_name.clone())` or the FontId string.
  - pt = `(mm * 72.0 / 25.4 * 100.0).round() / 100.0`.
- `appearance_xml` TextBox branch: when `hints.get(&ann_id)` is present and non-empty and `runs.is_empty()`, emit `text_object_xml_from_hints(rect, font, size, color, lines, next_id)`; otherwise keep the existing `text_object_xml` path.
- `text_object_xml_from_hints`: per line `Boundary="0 {top} {width} {height}"`, `TextCode X="0" Y="{baseline-top}"`, `DeltaX` from `advances[..n-1]` where `n = text.chars().count()` (skip if n < 2).
- `save_ofd` / `write_ofd`: add `save_ofd_with_text_layout` / `write_ofd_with_text_layout`; refactor the existing bodies to take `hints`; old functions pass `&TextLayoutHints::new()`.
- `ensure_annotation_entries` gains a `hints: &TextLayoutHints` parameter and passes it to `serialize_page_annot`.
- Export the new io functions from `crates/io/src/lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-io`
Expected: PASS. Update any exact-XML expectations only if the new metadata changed them (TextObject count/split for no-hints content is unchanged).

Run: `cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/io/src crates/io/tests
git commit -m "feat(io): per-line TextObject hints and WPS-compatible FreeText metadata"
```

---

### Task 7: io 解析（Remark 优先 + CRLF 归一化）

**Files:**
- Modify: `crates/io/src/parse/annotation.rs`
- Test: 同文件内联测试

**Interfaces:**
- Consumes: Task 6 写出的参数名 `AllRichText` / `ParagraphFormat`；`PendingAnnot.params` 已存在。
- Produces: 解析后的 `TextBox.content` 为逻辑文本（仅含硬 `\n`），`TextRun.text` 不含 `\r`。

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn freetext_prefers_remark_when_wps_params_present() {
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
<ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
  <ofd:Remark>逻辑文本</ofd:Remark>
  <ofd:Parameters>
    <ofd:Parameter Name="AllRichText">&lt;html&gt;x&lt;/html&gt;</ofd:Parameter>
    <ofd:Parameter Name="ParagraphFormat">{}</ofd:Parameter>
  </ofd:Parameters>
  <ofd:Appearance Boundary="0 0 45 40">
    <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
      <ofd:TextCode X="0" Y="12">逻辑</ofd:TextCode>
    </ofd:TextObject>
    <ofd:TextObject ID="3" Boundary="0 14 22 14" Font="F1" Size="12">
      <ofd:TextCode X="0" Y="12">文本</ofd:TextCode>
    </ofd:TextObject>
  </ofd:Appearance>
</ofd:Annot></ofd:PageAnnot>"#;
    let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
    let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else { panic!() };
    assert_eq!(content, "逻辑文本", "Remark wins over wrapped appearance lines");
}

#[test]
fn freetext_remark_without_wps_params_keeps_appearance_content() {
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
    <ofd:Remark>备注</ofd:Remark>
    <ofd:Appearance Boundary="0 0 45 40">
      <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">逻辑</ofd:TextCode>
      </ofd:TextObject>
      <ofd:TextObject ID="3" Boundary="0 14 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">文本</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;
    let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
    let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else { panic!() };
    assert_eq!(content, "逻辑\n文本", "no WPS params -> appearance is still the source");
}

#[test]
fn freetext_crlf_is_normalized() {
    let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="FreeText" ID="1" Subtype="FreeText">
    <ofd:Remark>a<CRLF>b</ofd:Remark>
    <ofd:Parameters>
      <ofd:Parameter Name="AllRichText">x</ofd:Parameter>
    </ofd:Parameters>
    <ofd:Appearance Boundary="0 0 45 40">
      <ofd:TextObject ID="2" Boundary="0 0 22 14" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="12">x<CRLF>y</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#
        .replace("<CRLF>", "\r\n");
    let anns = parse_page_annot(&xml, &PageId::new("1")).unwrap();
    let AnnotationPayload::TextBox { content, .. } = &anns[0].payload else { panic!() };
    assert_eq!(content, "a\nb");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-io parse::annotation`
Expected: content is `"逻辑\n文本"` (appearance) instead of `"逻辑文本"`.

- [ ] **Step 3: Implement**

In `build_payload`'s `AnnotationKind::TextBox` arm:
```rust
let has_wps_meta = p.params.iter().any(|(n, _)| n == "AllRichText" || n == "ParagraphFormat");
...
let mut content = flat_content_from_runs(&runs);
if first { content = p.remark.clone(); }
if has_wps_meta && !p.remark.is_empty() { content = p.remark.clone(); }
let content = normalize_newlines(&content);
```
Normalize each `TextRun.text` with the same helper before pushing:
`let text = normalize_newlines(text);`
Add `fn normalize_newlines(s: &str) -> String { s.replace("\r\n", "\n").replace('\r', "\n") }`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-io`
Expected: PASS (existing `freetext_no_textobject_falls_back_to_remark` stays green).

Run: `cargo clippy -p rofd-io --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/parse/annotation.rs
git commit -m "feat(io): restore logical FreeText content from Remark and normalize CRLF"
```

---

### Task 8: 宿主保存接线

**Files:**
- Modify: `crates/xilem-app/src/host/document_io.rs`
- Modify: `crates/xilem-app/src/main.rs`
- Modify: `crates/gpui-app/src/host/document_io.rs`
- Modify: `crates/gpui-app/src/shell.rs`
- Modify: `crates/web-view/src/wasm_ofd.rs`
- Test: host `document_io.rs` 的 `#[cfg(test)]`（xilem/gpui）

**Interfaces:**
- Consumes: `OfdComponent::save_snapshot() -> SaveSnapshot`、`rofd_io::{save_ofd_with_text_layout, write_ofd_with_text_layout}`。
- Produces（host `document_io`）:
  ```rust
  pub fn save_ofd(
      document: &OfdDocument, package: Option<&PackageHandle>,
      text_layout: &rofd_dom::TextLayoutHints, path: &Path,
  ) -> Result<(), String>;
  ```

- [ ] **Step 1: Update host `document_io::save_ofd` and its tests**

xilem and gpui copies are identical:
```rust
use rofd_io::{parse_ofd, save_ofd_with_text_layout, write_ofd_with_text_layout, PackageHandle};

pub fn save_ofd(
    document: &OfdDocument,
    package: Option<&PackageHandle>,
    text_layout: &rofd_dom::TextLayoutHints,
    path: &Path,
) -> Result<(), String> {
    let bytes = match package {
        Some(pkg) => save_ofd_with_text_layout(document, pkg, text_layout),
        None => write_ofd_with_text_layout(document, text_layout),
    }
    .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}
```
Update their tests to pass `&rofd_dom::TextLayoutHints::new()`.

- [ ] **Step 2: Update save call sites**

- xilem `main.rs::save_to`: `let snap = c.save_snapshot();` then
  `host::document_io::save_ofd(&snap.document, package.as_ref(), &snap.text_layout, &path)`.
- gpui `shell.rs` (two places): `let snap = ...component.save_snapshot();` then the same call.
- web-view `wasm_ofd.rs::save_ofd`: `let snap = self.component.save_snapshot();` then
  `save_ofd_with_text_layout(&snap.document, pkg, &snap.text_layout)` /
  `write_ofd_with_text_layout(&snap.document, &snap.text_layout)`.

- [ ] **Step 3: Build/test**

Run: `cargo test -p xilem-app -p gpui-app`
Expected: PASS (tests may need the new argument).

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: success.

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`

- [ ] **Step 4: Commit**

```bash
git add crates/xilem-app crates/gpui-app crates/web-view
git commit -m "feat(hosts): save annotation text layout hints into OFD"
```

---

### Task 9: 端到端回归（保存 → 重开 → 拖宽重排）+ AGENTS 更新

**Files:**
- Create: `crates/component/tests/text_layout_roundtrip.rs`
- Modify: `AGENTS.md`（§5 rofd-component 补一条行为说明）

**Interfaces:**
- Consumes: `OfdComponent::{new, load_document, create_annotation, save_snapshot, resize_annotation}`、`rofd_io::{write_ofd_with_text_layout, parse_ofd}`。

- [ ] **Step 1: Write the failing E2E test**

```rust
use std::sync::Arc;

use rofd_component::{OfdComponent, OfdConfig};
use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, FontId, Layer, OfdDocument, Page, PageId, Rect,
};

const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

fn page_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        id: PageId::new("P0"),
        physical_box: Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 },
        layers: vec![Layer::default()],
        template: None,
    });
    doc
}

fn textbox_component(content: &str, w: f64, h: f64) -> (OfdComponent, rofd_dom::AnnotationId) {
    let mut c = OfdComponent::new(OfdConfig::new(Arc::new(FONT.to_vec())));
    c.load_document(page_doc());
    let id = c.create_annotation(
        AnnotationKind::TextBox,
        PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect { x: 10.0, y: 10.0, w, h },
            content: content.into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
            border: None,
            runs: Vec::new(),
        },
    );
    (c, id)
}

#[test]
fn saved_text_reopens_with_logical_content_and_reflows_on_resize() {
    let (c, id) = textbox_component("abc def ghi jkl", 45.0, 14.0);
    let snap = c.save_snapshot();
    assert_eq!(snap.text_layout[&id].len(), 2, "two wrapped lines");
    let bytes = rofd_io::write_ofd_with_text_layout(&snap.document, &snap.text_layout).unwrap();

    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let (reloaded_id, rect_h) = {
        let ann = &report.document.annotations.for_page(&PageId::new("P0"))[0];
        let AnnotationPayload::TextBox { content, rect, .. } = &ann.payload else {
            panic!("expected TextBox");
        };
        assert_eq!(content, "abc def ghi jkl", "logical content survives save+parse");
        assert!(!content.contains('\n'), "soft wraps stay out of content");
        assert!(rect.h >= 14.0, "height was grown to fit two lines");
        (ann.id.clone(), rect.h)
    };

    let mut c2 = OfdComponent::new(OfdConfig::new(Arc::new(FONT.to_vec())));
    c2.load_document(report.document);
    c2.resize_annotation(&reloaded_id, Rect { x: 10.0, y: 10.0, w: 120.0, h: rect_h });
    let snap2 = c2.save_snapshot();
    assert_eq!(snap2.text_layout[&reloaded_id].len(), 1,
               "widening the box reflows back to one line");
}
```

Also assert the serialized XML contains `Remark`/`AllRichText` by reading the zip entry:
use `zip::ZipArchive` on `bytes` (rofd-component dev-deps has zip? No — it has rofd-io and kurbo.
Use `rofd_io::parse_ofd` for content; to assert metadata, add a small helper via `zip` would need a dev-dep.
Skip the raw-XML assertion here — Task 6 already covers it. Keep the E2E focused on round-trip + reflow.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rofd-component --test text_layout_roundtrip`
Expected: FAIL (`save_snapshot` missing, or content contains `\n` / reflow line count unchanged).

- [ ] **Step 3: Ensure implementation passes**

If the test fails on reflow, verify `resize_annotation` uses `with_text_height` and `save_snapshot` uses `shape_wrapped` for the new width.

- [ ] **Step 4: Update AGENTS.md**

Add one bullet under §5 `rofd-component`:
```
- 批注文字流式排版（spec 2026-11-09）：`runs` 空的 TextBox 按 rect 宽折行、框只向下增高；保存经 `save_snapshot()` 产出 `TextLayoutHints`，io 写逐行 TextObject + `Remark` + WPS `AllRichText`/`ParagraphFormat`/`GrowMode`；重开从 `Remark` 恢复逻辑文本，拖宽仍重排。
```

- [ ] **Step 5: Run the full suite**

Run:
```
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/component/tests/text_layout_roundtrip.rs AGENTS.md
git commit -m "test(component): save-reopen-resize reflow round trip; document text layout"
```

---

## Self-Review Notes

- Spec coverage: §1.1→Task 1；§1.2→Tasks 3/4；§1.2.1→Tasks 3/7；§1.2.2→Tasks 2/4/5；§1.3→Task 2；§1.4→Task 4；§1.5→Tasks 5/6/7/8；§1.6→Task 4；§1.7→Tasks 2/4；§4.3→Task 5；§4.4→Task 6；§4.5→Task 6；§6→各任务测试；§7→Review Focus。
- Type consistency: `TextLineHint`/`TextLayoutHints` 只在 `rofd-dom` 定义一次；`shape_wrapped` 返回 `Rc<WrappedText>`；`SaveSnapshot.text_layout` 类型与 io 参数一致。
- 已知边界：`save_ofd`/`write_ofd` 旧 API 保留（空 hints），宿主全部改用新 API；富文本框不重排；WPS 内边距不对齐。
