# Annotation Rich-Text (Selection Formatting) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make font family/size/color edits apply to the selected text inside an annotation TextBox, backed by editable, flowed style spans (`TextSpan`) that survive editing and OFD save/reload.

**Architecture:** `TextBox.runs` (foreign, position-bearing, read-only) becomes `TextBox.spans` (ordered, position-free style segments whose texts concatenate to `content`). Parley lays out all runs (flat = one run) through a single `shape_wrapped_runs` entry; render/caret/save consume that layout. Editor commands gain a range parameter and maintain the spans invariants; the component passes its `AnnotTextSelection` range so adapters stay unchanged.

**Tech Stack:** Rust, parley 0.8 (ranged styles), imaging Scene, wasm-bindgen adapters, vitest SDK tests.

**Spec:** `docs/superpowers/specs/2026-11-10-annotation-rich-text-design.md`

## Global Constraints

- Commit messages and code comments in **English**; Chinese only in `docs/superpowers/` and user-facing UI copy.
- Never modify body content (`pages`); annotations are the only mutable surface (AGENTS §4.2).
- Dependency direction stays one-way: adapters depend only on `rofd-component`; editor depends on dom; render depends on dom.
- No new third-party dependencies.
- Library never reads wall-clock time; use only the injected `current_ts`.
- Each style/edit operation is **one `Transaction`** (one undo).
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` must be clean.
- Never touch the user's pre-existing `M test/sample.ofd`.
- Preservation gates: `cargo test -p rofd-io` surgical tests (`round_trip.rs`, `save_surgical.rs`) must stay green.

## Review Focus

1. A long soft-wrapped paragraph with two words colored must keep both colors after save → reload (parse aligns appearance text to `Remark`, skipping `\n`).
2. Two boxes with identical text/font/size/width but different colors must never share a cached color (color is not in the layout cache key or `ShapedSpan`).
3. Once a box has spans it must remain fully selectable/editable (component's five `runs` gates removed).
4. Select-all + change size on a mixed-color box must keep the color mix (merge only when the result is expressible flat).
5. Serialized fragment `TextObject` boundaries stay appearance-local, so repeated save/reload does not accumulate offsets; `span.color == None` writes no `FillColor` and reloads as `None`.

---

### Task 1: DOM model — `TextRun` → `TextSpan`

**Files:**
- Modify: `crates/dom/src/annotation.rs:48-114` (`TextBox` field, `TextSpan`)
- Modify (mechanical compile fixes): `crates/io/src/parse/annotation.rs`, `crates/io/src/serialize/annotation.rs`, `crates/render/src/annotation_scene.rs`, `crates/render/src/caret_rect.rs` (tests only), `crates/component/src/{ofd_component.rs,font_req.rs}`, `crates/editor/src/commands/text_commands.rs`, `crates/gpui-view/tests/view_input.rs`, `crates/xilem-view/tests/ofd_widget_harness.rs`, `crates/io/tests/{annotation_roundtrip.rs,save_surgical.rs,annotation_conformance.rs}`, `crates/render/tests/hit_test.rs`, `crates/component/tests/text_layout_roundtrip.rs`
- Test: `crates/dom/src/annotation.rs` (existing `mod tests`)

**Interfaces:**
- Produces:
  ```rust
  pub struct TextSpan {
      pub text: String,
      pub font: FontId,
      pub size: f64,
      pub color: Option<Color>,
      pub bold: bool,
      pub italic: bool,
  }
  ```
  `AnnotationPayload::TextBox { rect, content, font, size, color, border, spans: Vec<TextSpan> }` (`#[serde(default)]` on `spans`).
- Note: `x`/`baseline` are deleted; every construction site that used `TextRun { x, baseline }` drops those fields.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn textbox_spans_round_trip_without_positions() {
    let ann = base_ann(
        AnnotationPayload::TextBox {
            rect: Rect { x: 1.0, y: 2.0, w: 46.0, h: 10.0 },
            content: "发的古格".into(),
            font: FontId::new("118"),
            size: 4.2333,
            color: Color::Rgb(13, 13, 13),
            border: None,
            spans: vec![
                TextSpan { text: "发的古".into(), font: FontId::new("118"), size: 4.2333,
                           color: None, bold: false, italic: false },
                TextSpan { text: "格".into(), font: FontId::new("157"), size: 4.2333,
                           color: Some(Color::Rgb(75, 172, 198)), bold: true, italic: false },
            ],
        },
        AnnotationKind::TextBox,
    );
    assert_round_trips(&ann);
}

#[test]
fn textbox_spans_default_empty_for_legacy_json() {
    let json = r#"{"rect":{"x":0,"y":0,"w":10,"h":10},"content":"a","font":"F1","size":12,
                   "color":{"Rgb":[0,0,0]},"border":null}"#;
    let payload: AnnotationPayload = serde_json::from_str(json).unwrap();
    assert!(matches!(payload, AnnotationPayload::TextBox { spans, .. } if spans.is_empty()));
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-dom annotation`
Expected: FAIL — `TextSpan` not found / field `spans` missing.

- [ ] **Step 3: Implement the model rename**

Replace `TextRun` with `TextSpan` (above), rename the `TextBox` field to `spans`, keep `#[serde(default)]`. Then `cargo build --workspace` and fix every compile error by renaming `.runs`/`runs:` to `.spans`/`spans:` and deleting `x`/`baseline` literals.

Three call sites need a **temporary** adaptation so the workspace stays green until later tasks replace them (mark each with a `// TEMP(rich-text)` comment and remove in Tasks 7/8/2):

- `crates/io/src/parse/annotation.rs`: keep a local `ParsedRun { text, font, size, color, bold, italic, x, baseline }` inside the TextBox arm for ordering/flattening (unchanged `flat_content_from_runs` logic), and convert the ordered result to `Vec<TextSpan>`. Behavior (Remark, uniform-clear) stays as today; Task 7 rewrites the normalization.
- `crates/io/src/serialize/annotation.rs`: rename `text_run_object_xml` to `text_span_object_xml`; lay spans left-to-right with `x = running sum of chars * size`, `baseline = size` (count/attribute assertions in the existing test still hold). Task 8 replaces it with hint-driven geometry.
- `crates/render/src/annotation_scene.rs`: `draw_text_runs` draws spans sequentially at an accumulating pen x from `(rect.x, rect.y)`, keeping the italic/bold/color logic. Task 2 deletes it.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-dom annotation && cargo build --workspace`
Expected: PASS, workspace compiles.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "refactor(dom): replace positional text runs with editable TextSpan"
```

---

### Task 2: Render — ranged layout, `ShapedSpan`, single draw path

**Files:**
- Create: `crates/render/src/text/spans.rs`
- Modify: `crates/render/src/text/mod.rs`, `crates/render/src/text/font.rs`, `crates/render/src/text/shape.rs`, `crates/render/src/text/wrap.rs`, `crates/dom/src/annotation.rs` (`TextLineHint`), `crates/render/src/annotation_scene.rs:98-177,565-656`
- Test: `crates/render/src/text/wrap.rs` (unit tests), `crates/render/src/annotation_scene.rs` (scene tests)

**Interfaces:**
- Produces:
  ```rust
  pub struct LayoutRun {
      pub byte_range: std::ops::Range<usize>,
      pub font: FontId,
      pub size: f64,
      pub color: Option<Color>,
      pub bold: bool,
      pub italic: bool,
  }
  pub fn layout_runs(content: &str, spans: &[TextSpan], flat_font: &FontId,
                     flat_size: f64, flat_color: Color) -> Vec<LayoutRun>;
  pub struct ShapedSpan {
      pub style_index: usize, pub font: FontData, pub size: f64,
      pub bold: bool, pub italic: bool,
      pub glyph_start: usize, pub glyph_end: usize,
  }
  // FontStore:
  pub fn shape_wrapped_runs(&self, content: &str, runs: &[LayoutRun],
                            max_width: f64) -> Rc<WrappedText>;
  ```
  `WrappedText { spans: Rc<Vec<ShapedSpan>>, glyphs, lines, total_chars }`; the single-value `font` field is removed. `FontStore::shape_wrapped(font_id, text, size, max_width)` remains as a thin wrapper that builds one `LayoutRun` and delegates (keeps ~30 existing call sites green).
- Consumes: `TextSpan` from Task 1.

- [ ] **Step 1: Write the failing tests**

In `crates/render/src/text/wrap.rs`:

```rust
#[test]
fn wraps_two_sizes_and_uses_the_tallest_line_height() {
    let s = store();
    let runs = [
        LayoutRun { byte_range: 0..2, font: FontId::new("F1"), size: 12.0,
                    color: Some(Color::Rgb(255, 0, 0)), bold: false, italic: false },
        LayoutRun { byte_range: 2..4, font: FontId::new("F1"), size: 24.0,
                    color: Some(Color::Rgb(0, 0, 255)), bold: false, italic: false },
    ];
    let w = s.shape_wrapped_runs("abcd", &runs, 1000.0);
    assert_eq!(w.spans.len(), 2);
    assert_eq!(w.spans[0].style_index, 0);
    assert_eq!(w.spans[1].style_index, 1);
    assert!(w.lines[0].height >= 24.0);
}

#[test]
fn layout_cache_is_color_agnostic() {
    let s = store();
    let a = [LayoutRun { byte_range: 0..4, font: FontId::new("F1"), size: 12.0,
                         color: Some(Color::Rgb(255, 0, 0)), bold: false, italic: false }];
    let mut b = a.clone();
    b[0].color = Some(Color::Rgb(0, 0, 255));
    let wa = s.shape_wrapped_runs("abcd", &a, 1000.0);
    let wb = s.shape_wrapped_runs("abcd", &b, 1000.0);
    assert!(Rc::ptr_eq(&wa, &wb), "color must not split the layout cache");
}
```

In `crates/render/src/annotation_scene.rs` (fixture builder already exists):

```rust
#[test]
fn textbox_with_spans_draws_each_span_in_its_own_color() {
    let runs = vec![
        TextSpan { text: "ab".into(), font: FontId::new("F1"), size: 12.0,
                   color: Some(Color::Rgb(255, 0, 0)), bold: false, italic: false },
        TextSpan { text: "cd".into(), font: FontId::new("F1"), size: 12.0,
                   color: Some(Color::Rgb(0, 0, 255)), bold: false, italic: false },
    ];
    let scene = build(&[ann(AnnotationPayload::TextBox { /* rect 200x50, content "abcd" */
        /* font F1, size 12, color black, border None */ spans: runs }, AnnotationKind::TextBox)]);
    let brushes = glyph_brushes(&scene); // add a small helper next to glyph_runs
    assert!(brushes.iter().any(|c| c == &peniko::Color::from_rgb8(255, 0, 0)));
    assert!(brushes.iter().any(|c| c == &peniko::Color::from_rgb8(0, 0, 255)));
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-render wraps_two_sizes layout_cache_is_color_agnostic textbox_with_spans_draws`
Expected: FAIL — `shape_wrapped_runs` / `spans` / `LayoutRun` not found.

- [ ] **Step 3: Implement**

1. `text/spans.rs`: `layout_runs` walks `spans` accumulating `chars().count()` per span and converts char counts to byte ranges over `content`; when `spans` is empty return one run `0..content.len()` with the flat style. Add unit test asserting byte ranges for a CJK + ASCII mix.
2. `shape.rs`: add `shape_layout_runs(fcx, content, runs: &[LayoutRun], families: &[FontFamily<'_>], max_advance)` beside the existing `shape_layout` (`families[i]` corresponds to `runs[i]`; `style_index = i`); **do not change `shape_with_family` / body shaping**. Precompute each run's `family_list_for` **before** borrowing `font_cx`, then push per-range `FontSize`, `FontFamily`, `FontWeight` (`BOLD` if `bold`), `FontStyle` (`Italic` if italic). Keep `liga=0`, `OverflowWrap::Anywhere`, and `push_default` fallbacks. Resolve each parley glyph run's style via `run.run().text_range()` → first run whose byte range contains the range start. Emit `Vec<(ShapedSpanMeta, Vec<ShapedGlyph>)>` (`ShapedSpanMeta { style_index, font, size, bold, italic }`); keep merging consecutive glyphs with the same style index + font.
3. `font.rs`: replace the `wrapped_cache` key with a private `WrappedKey { width_bits: u64, text: String, sig: String }` (`Hash + Eq`); `sig` is built from `(byte_range, font.0, size.to_bits(), bold, italic)` **excluding color**; `shape_wrapped_runs` builds the families and calls the shape function, then caches. `shape_wrapped` delegates with one run.
4. `wrap.rs`: `WrappedText` stores `spans` (assign `glyph_start/end` in `from_layout`); delete `font`. Change `from_layout` input to `Vec<(ShapedSpanMeta, Vec<ShapedGlyph>)>`.
5. `dom::annotation`: add `TextLineFragmentHint { char_start, char_len, x, advances }`; `TextLineHint` gains `char_start: usize` and `fragments: Vec<TextLineFragmentHint>`. Update the two `TextLineHint` literals in `crates/io/src/serialize/annotation.rs` tests with `char_start: 0, fragments: vec![]`.
6. `wrap.rs::to_layout_hints`: for each line, intersect each `spans[i].glyph_start..glyph_end` with the line's glyph range; emit one fragment (`char_start = global_glyph_start - line.glyph_start`, `char_len = len`, `x = glyphs[start].x`, advances = neighbor deltas, last uses `line.width`).
7. `annotation_scene.rs`: delete `draw_text_runs`; rewrite `draw_text_in_rect(painter, rect, content, runs, fonts, base)` to shape via `shape_wrapped_runs`, then loop `wrapped.spans`, drawing each with its font/size and `runs[style_index].color.unwrap_or(flat_color)`, applying `ITALIC_SKEW` and the bold second pass. Inline IME preedit composition applies only when `payload.spans.is_empty()` (spans boxes keep today's detached-overlay behavior).

- [ ] **Step 4: Run to verify GREEN + no regression**

Run: `cargo test -p rofd-render`
Expected: PASS (new tests plus all existing scene/wrap tests).

- [ ] **Step 5: Commit**

```bash
git add crates/render crates/dom/src/annotation.rs crates/io/src/serialize/annotation.rs
git commit -m "feat(render): lay out annotation text as styled spans"
```

---

### Task 3: Render — caret, selection rects and offset mapping on span layout

**Files:**
- Modify: `crates/render/src/caret_rect.rs:43-152,212-400`
- Test: `crates/render/src/caret_rect.rs` (`mod tests`), `crates/render/tests/body_position.rs` (must stay green)

**Interfaces:**
- Produces:
  ```rust
  pub fn caret_offset_at_point_runs(content: &str, runs: &[LayoutRun], max_width: f64,
                                    store: &FontStore, x: f64, y: f64) -> usize;
  pub fn caret_offset_vertical_runs(content: &str, runs: &[LayoutRun], max_width: f64,
                                    store: &FontStore, offset: usize,
                                    direction: VerticalMove, preferred_x: Option<f64>,
                                    line_hint: Option<usize>)
                                    -> (usize, Option<f32>, usize);
  ```
  Existing `caret_offset_at_point` / `caret_offset_vertical` stay as flat wrappers (one run) so Note/Watermark and existing tests compile.
- Consumes: `layout_runs`, `shape_wrapped_runs` from Task 2.

- [ ] **Step 1: Write the failing tests**

Add the fixtures `textbox_with_spans(spans) -> AnnotationPayload` and `base_span() -> TextSpan` next to the existing `push_ann` / `doc_with_font` helpers.

```rust
#[test]
fn caret_rect_uses_span_layout_height_for_a_mixed_size_line() {
    let (mut doc, fonts) = doc_with_font();
    let id = push_ann(&mut doc, textbox_with_spans(vec![
        TextSpan { text: "ab".into(), font: FontId::new("F1"), size: 6.0, ..base_span() },
        TextSpan { text: "cd".into(), font: FontId::new("F1"), size: 24.0, ..base_span() },
    ]), AnnotationKind::TextBox);
    let r = caret_rect(&doc, &vp(), &fonts, &id, 0).unwrap();
    assert!(r.h >= 24.0, "caret spans the tallest run's line box: {r:?}");
}

#[test]
fn selection_rects_are_non_empty_for_a_span_box() {
    let (mut doc, fonts) = doc_with_font();  // existing helper
    let id = push_ann(&mut doc, /* TextBox content "abcd", spans split 2+2 */, AnnotationKind::TextBox);
    let rects = annotation_text_selection_rects(&doc, &vp(), &fonts, &id, 0, 3);
    assert!(!rects.is_empty(), "span boxes must highlight their selection");
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-render caret_rect_uses_span_layout selection_rects_are_non_empty`
Expected: FAIL (height = flat size / empty rects due to the `spans` gate).

- [ ] **Step 3: Implement**

- `caret_rect_on_line`: for `TextBox` build `layout_runs(content, spans, font, size, color)`, shape with `rect.w.max(max_run_size)`, then the existing line/local/caret math. Delete the `if runs.is_empty()` / legacy fallback fork for TextBox; keep the legacy branch only for `Note`/`Watermark`.
- `annotation_text_selection_rects`: same runs-based shape and remove the `!spans.is_empty() → empty` early return.
- Add `caret_offset_at_point_runs` / `caret_offset_vertical_runs` by parameterizing the current bodies on a shaped `WrappedText`; keep the public flat functions as wrappers.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-render`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/caret_rect.rs
git commit -m "feat(render): span-aware caret and selection geometry"
```

---

### Task 4: Editor — span helpers and range-aware style commands

**Files:**
- Create: `crates/editor/src/text_spans.rs` (module registration in `crates/editor/src/lib.rs`)
- Modify: `crates/editor/src/commands/text_commands.rs:12-33,48-130,142-269`
- Test: `crates/editor/src/text_spans.rs` (new `mod tests`), `crates/editor/src/commands/text_commands.rs` (existing `mod tests`)

**Interfaces:**
- Produces (all pure over `AnnotationPayload`):
  ```rust
  pub struct SpanStyle { pub font: FontId, pub size: f64, pub color: Color,
                         pub bold: bool, pub italic: bool }
  pub fn ensure_spans(payload: &mut AnnotationPayload);
  pub fn spans_insert(payload: &mut AnnotationPayload, offset: usize, text: &str);
  pub fn spans_delete(payload: &mut AnnotationPayload, offset: usize, len: usize);
  pub fn spans_apply(payload: &mut AnnotationPayload, range: Option<(usize, usize)>,
                     f: &mut dyn FnMut(&mut TextSpan));
  pub fn style_at(payload: &AnnotationPayload, offset: usize) -> Option<SpanStyle>;
  ```
- Editor signatures become:
  ```rust
  pub fn set_annotation_font_size_measured<F>(&mut self, id: &AnnotationId,
      range: Option<(usize, usize)>, size_mm: f64, measure: F) where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn set_annotation_font_family_measured<F>(&mut self, id: &AnnotationId,
      range: Option<(usize, usize)>, family: &str, measure: F) where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn set_annotation_color(&mut self, id: &AnnotationId,
      range: Option<(usize, usize)>, color: Color);
  ```
  `insert_text_measured` / `delete_text_measured` / `replace_text_measured` keep their signatures and stop collapsing.
- Consumes: `TextSpan` (Task 1).

- [ ] **Step 1: Write the failing tests**

In `crates/editor/src/text_spans.rs`:

```rust
#[test]
fn insert_at_a_span_boundary_inherits_the_preceding_style() {
    let mut p = textbox("abcd", vec![span("ab", 12.0, RED), span("cd", 12.0, BLUE)]);
    spans_insert(&mut p, 2, "X");
    assert_eq!(content(&p), "abXcd");
    assert_eq!(texts(&p), vec!["abX", "cd"]);
    assert_eq!(spans(&p)[1].color, Some(BLUE));
}

#[test]
fn insert_in_the_middle_of_one_span_keeps_a_single_span() {
    let mut p = textbox("ab", vec![span("ab", 12.0, RED)]);
    spans_insert(&mut p, 1, "X");
    assert_eq!(texts(&p), vec!["aXb"]);
}
```

Table-driven apply test (one `#[test]`, rows as `(input, range, mutation, expected)`):
- range `None` on a uniform box → flat fields updated, `spans` empty;
- range `Some((0, len))` on a uniform box → merged to flat;
- range `Some((0, len))` on a red/blue box changing size → still 2 spans, colors kept;
- range `Some((1, 3))` on a single span → 3 spans with the middle changed;
- range covering everything including a bold span → spans retained (bold not expressible flat).

In `text_commands.rs`:

```rust
#[test]
fn set_font_size_on_a_selection_changes_only_that_range() {
    let (mut e, id) = flat_textbox_editor_with("hello", 20.0);
    e.set_annotation_font_size_measured(&id, Some((1, 3)), 30.0, |_| None);
    assert_eq!(style_sizes(&e, &id), vec![(0, 1, 20.0), (1, 2, 30.0), (3, 2, 20.0)]);
}

#[test]
fn text_edit_preserves_spans_and_undo_restores_once() {
    let (mut e, id) = span_textbox_editor(); // "ab" red, "cd" blue
    let before = textbox_payload(&e, &id).clone();
    e.insert_text(&id, 2, "XY");
    assert_eq!(spans(&e, &id).len(), 2, "insert does not collapse");
    assert!(e.undo());
    assert_eq!(textbox_payload(&e, &id), &before);
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-editor text_spans && cargo test -p rofd-editor set_font_size_on_a_selection text_edit_preserves_spans`
Expected: FAIL (`text_spans` module missing; signatures mismatch).

- [ ] **Step 3: Implement**

- `text_spans.rs`: implement the helpers per spec §4.1. `ensure_spans` creates one span from the flat style when content is non-empty. `spans_apply` implements the three rules: `None`/empty range → flat write + clear; otherwise split at both ends, mutate covered spans, drop empties, merge equal neighbors, then merge to flat only when the result is a single non-bold/non-italic span (color = `span.color.unwrap_or(flat.color)`).
- `style_at`: `spans` empty → flat style; else walk char counts, clamp to content end.
- `text_commands.rs`: delete `collapse_runs`; call `spans_insert`/`spans_delete` in insert/delete/replace; `set_annotation_text` clears `spans`; the three style commands take `range` and call `spans_apply`. All other payload kinds ignore `range`.
- Update all call sites in this crate to pass `None`; note the component call sites are updated in Task 5.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-editor`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/editor
git commit -m "feat(editor): apply text styles to selection ranges and keep spans on edits"
```

---

### Task 5: Component — selection-aware style APIs, measurement and save snapshot

**Files:**
- Modify: `crates/component/src/ofd_component.rs:589-746,3944-3962`
- Test: `crates/component/src/ofd_component.rs` (`mod tests`), `crates/component/tests/text_layout_roundtrip.rs`

**Interfaces:**
- Consumes: `style_at`, range-aware setters, `layout_runs`, `shape_wrapped_runs`, `SpanStyle` (Tasks 2-4).
- Produces: unchanged adapter-facing API (`annotation_text_style`, `set_annotation_font_family`, `set_annotation_font_size`, `set_annotation_color`); `annotation_text_style` is now selection-aware and `rich = !spans.is_empty()`.

- [ ] **Step 1: Write the failing tests**

Add the named fixture helpers (`textbox_payload`) next to the existing `textbox_content` / `textbox_rect` helpers in the same module.

```rust
#[test]
fn style_setter_targets_only_the_text_selection() {
    let mut c = component_with_textbox();            // content "hello", flat size 10
    let id = textbox_id(&c);
    c.set_annot_text_selection(Some(AnnotTextSelection {
        annotation: id.clone(), anchor: 1, focus: 3,
    }));
    c.set_annotation_font_size(&id, 20.0);
    let sizes: Vec<f64> = match &textbox_payload(&c, &id) {
        AnnotationPayload::TextBox { spans, .. } => spans.iter().map(|s| s.size).collect(),
        _ => panic!("expected TextBox"),
    };
    assert_eq!(sizes, vec![10.0, 20.0, 10.0], "only chars 1..3 changed");
    let s = c.annotation_text_style(&id).unwrap();
    assert!((s.size_mm - 20.0).abs() < 1e-9, "panel shows the selection's first-char style");
}

#[test]
fn save_snapshot_emits_fragments_for_span_boxes_and_grows() {
    let mut c = component_with_textbox();            // "hello", size 10, short box
    let id = textbox_id(&c);
    c.set_annot_text_selection(Some(AnnotTextSelection {
        annotation: id.clone(), anchor: 1, focus: 3,
    }));
    c.set_annotation_font_size(&id, 24.0);
    let snap = c.save_snapshot();
    let hints = snap.text_layout.get(&id).expect("hints for a span box");
    assert!(hints.iter().any(|h| h.fragments.len() == 3), "three styled fragments on one line");
    let h = match &snap.document.annotations.find(&id).unwrap().payload {
        AnnotationPayload::TextBox { rect, .. } => rect.h,
        _ => panic!("expected TextBox"),
    };
    assert!(h > 10.0, "grows to fit the taller run");
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-component style_setter_targets_only_the_text_selection save_snapshot_emits_hints_for_span_boxes`
Expected: FAIL (whole box changes / no hints).

- [ ] **Step 3: Implement**

- Setters: compute `let range = self.annot_selection_range(id);` and pass it to the editor. Clamp inside `spans_apply`.
- `annotation_text_style`: when `annot_selection_range(id)` is `Some((start, _))`, resolve `style_at(&ann.payload, start)` (family via `family_display_name` → `available_font_families` → `font.0`); otherwise read the flat fields. `rich = !spans.is_empty()`.
- `measure_textbox_growth`: build `layout_runs` (spans or flat) and shape; return `Some(required)` only when `required > rect.h + 1e-9`. Remove the `spans.is_empty()` guard.
- `save_snapshot`: for every non-empty TextBox build runs and shape at `rect.w.max(max_run_size)`; insert `to_layout_hints()`; grow `rect.h` when needed. Remove the spans skip.
- Update editor call sites in this file to pass `range`/`None` as appropriate (delete-text and paste call sites keep their existing signatures).

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-component && cargo test -p rofd-component --test text_layout_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "feat(component): make annotation formatting selection-aware"
```

---

### Task 6: Component — remove the remaining `runs` gates

**Files:**
- Modify: `crates/component/src/ofd_component.rs:810-816,1250-1274,1717-1720,2922-2950,2808-2816`
- Test: `crates/component/src/ofd_component.rs` (`mod tests`), `crates/component/tests/sample_drag_select.rs`

**Interfaces:**
- Consumes: `caret_offset_at_point_runs`, `caret_offset_vertical_runs` (Task 3), `measure_textbox_growth` (Task 5).
- Produces: span-bearing boxes behave exactly like flat boxes for drag-select, vertical caret movement, click placement and resize growth.

- [ ] **Step 1: Write the failing tests**

Add the fixtures `component_with_span_textbox(content)` / `component_with_narrow_span_textbox(content, w, h)` next to `component_with_textbox` / `component_with_narrow_textbox` (same page/zoom setup, spans split the content in two styled halves).

```rust
#[test]
fn span_textbox_supports_drag_selection() {
    let mut c = component_with_span_textbox("hello");   // two styled spans
    drag_hello(&mut c, 0, 3);                            // existing helper
    assert!(c.annot_selection_range(&textbox_id(&c)).is_some());
}

#[test]
fn span_textbox_arrow_down_moves_to_the_next_visual_line() {
    let mut c = component_with_narrow_span_textbox("abcdef ghijkl mn", 45.0, 60.0);
    let id = textbox_id(&c);
    c.editor.set_cursor(id, 6);
    c.handle_event(&ViewEvent::KeyDown { key: Key::ArrowDown, modifiers: Modifiers::default() });
    assert_eq!(c.caret_line.unwrap().1, 1);
}

#[test]
fn resizing_a_span_textbox_grows_it_to_fit() {
    let mut c = component_with_span_textbox("abcdef ghijkl mn");
    let id = textbox_id(&c);
    c.resize_annotation(&id, rofd_dom::Rect { x: 0.0, y: 0.0, w: 20.0, h: 10.0 });
    assert!(/* rect.h */ > 10.0);
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-component span_textbox_supports_drag_selection span_textbox_arrow_down resizing_a_span_textbox`
Expected: FAIL at the `runs`/`spans` early returns.

- [ ] **Step 3: Implement**

- Delete `is_flat_textbox` and its use at the Text-tool press (drag-select every TextBox while editing).
- Vertical caret movement: build runs and call `caret_offset_vertical_runs`; delete the `spans` early return.
- `click_caret_offset`: always use `caret_offset_at_point_runs` (delete the `caret_offset_at_x` rich branch).
- `with_text_height`: use the span-aware measure (delete the early return).
- Update any test that encoded the old gates (e.g. "rich box cannot be selected") to the new behavior, and state in the commit body why the expectation changed.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-component`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "fix(component): let span-bearing text boxes stay selectable and editable"
```

---

### Task 7: IO parse — normalize appearance into spans

**Files:**
- Modify: `crates/io/src/parse/annotation.rs:311-343,486-578`
- Modify: `crates/io/src/parse/mod.rs:211,240` (use the warning-collecting entry point)
- Test: `crates/io/src/parse/annotation.rs` (`mod tests`), `crates/io/tests/parse.rs`, `crates/io/tests/annotation_roundtrip.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError>; // unchanged, drops warnings
  pub fn parse_page_annot_with_warnings(xml: &str, page: &PageId,
      warnings: &mut Vec<OfdWarning>) -> Result<Vec<Annotation>, OfdError>;
  ```
  Round-trip invariant: `spans` non-empty ⇒ concatenated `spans[i].text` equals `content` char-for-char.
- Consumes: `TextSpan` (Task 1).

- [ ] **Step 1: Write the failing tests**

Add the test helpers `spans_of(&Annotation) -> &[TextSpan]` and `concat(&[TextSpan]) -> String` next to the existing parse fixtures.

```rust
#[test]
fn remark_text_aligns_to_appearance_without_soft_breaks() {
    // appearance: "abc def " on line 1, "ghi" on line 2; Remark: "abc def ghi"
    let anns = parse_page_annot(SOFT_WRAP_XML, &PageId::new("1")).unwrap();
    let TextBox { content, spans, .. } = &anns[0].payload else { panic!() };
    assert_eq!(content, "abc def ghi");
    assert_eq!(concat(spans), *content, "span texts cover content exactly");
    assert_eq!(spans.len(), 2);
}

#[test]
fn hard_breaks_in_remark_are_attributed_to_the_preceding_span() {
    // appearance lines "ab", "cd"; Remark "ab\ncd" -> spans ["ab\n", "cd"]
    let anns = parse_page_annot(HARD_BREAK_XML, &PageId::new("1")).unwrap();
    assert_eq!(concat(spans_of(&anns[0])), "ab\ncd");
}

#[test]
fn unalignable_remark_degrades_to_flat_with_a_warning() {
    let mut warnings = Vec::new();
    let anns = parse_page_annot_with_warnings(MISALIGNED_XML, &PageId::new("1"), &mut warnings).unwrap();
    assert!(spans_of(&anns[0]).is_empty());
    assert!(warnings.iter().any(|w| matches!(w, OfdWarning::SkippedObject { reason, .. }
        if reason.contains("style"))));
}

#[test]
fn uniform_appearance_stays_flat() {
    let anns = parse_page_annot(UNIFORM_TWO_LINES_XML, &PageId::new("1")).unwrap();
    assert!(spans_of(&anns[0]).is_empty());
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-io remark_text_aligns hard_breaks_in_remark unalignable_remark uniform_appearance`
Expected: FAIL (old code inserts `\n` for every visual line / no warning entry point).

- [ ] **Step 3: Implement**

- Rewrite the TextBox parse arm: collect ordered fragments (text + style, no positions). `appearance_text` = fragment texts with `\n` removed.
- If `remark` non-empty: `content = normalize_newlines(remark)`. Walk `content` chars, skipping `\n` and assigning them to the current fragment; consume non-newline chars in fragment order. If counts/text diverge → `spans.clear()` and push `OfdWarning::SkippedObject { page, reason: "annotation text style could not be aligned with Remark (spans dropped)" }`.
- If `remark` empty: `content` = fragments joined with `\n` inserted at baseline jumps > `0.3 * min(size)`; each inserted newline is appended to the preceding fragment's text.
- Apply the uniform-style optimization (no bold/italic and all adjacent styles equal → clear spans).
- Drop the old `flat_content_from_runs` (or reduce it to the no-Remark join used above).
- `PendingAnnot::finish` gains a `&mut Vec<OfdWarning>` parameter; the public `parse_page_annot` delegates with a scratch vec; `parse/mod.rs` calls `parse_page_annot_with_warnings` and extends the report warnings.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-io`
Expected: PASS (parse, conformance, roundtrip, surgical suites).

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/parse
git commit -m "feat(io): normalize foreign rich text into flowed spans on parse"
```

---

### Task 8: IO serialize — per-fragment TextObjects with real geometry

**Files:**
- Modify: `crates/io/src/serialize/annotation.rs:346-395,538-637`
- Test: `crates/io/src/serialize/annotation.rs` (`mod tests`), `crates/io/tests/annotation_roundtrip.rs`

**Interfaces:**
- Consumes: `TextLineHint { char_start, fragments, .. }` (Task 2), `TextBox.spans` (Task 1).
- Produces: XML round-trip that preserves each span's font/size/color/bold/italic and hard newlines, with appearance-local boundaries.

- [ ] **Step 1: Write the failing tests**

Add fixtures `span_textbox_with_hints()`, `span_textbox_with_hints_none_color()`, `soft_wrapped_span_textbox()` and the helper `colors_of(&Annotation) -> Vec<Option<Color>>` next to the existing `textbox_with_hints`.

```rust
#[test]
fn span_hints_emit_one_textobject_per_fragment_with_local_boundaries() {
    let (ann, hints) = span_textbox_with_hints(); // content "ab\ncd", spans red "ab\n" + blue "cd"
    let xml = serialize_page_annot(&PageId::new("1"), &[ann], &mut 10, &hints);
    assert_eq!(xml.matches("<ofd:TextObject").count(), 2, "{xml}");
    // appearance-local: first object starts at x 0, second at the fragment x
    assert!(xml.contains("Boundary=\"0 0 "), "{xml}");
    assert!(xml.contains("FillColor Value=\"#FF0000\"") || xml.contains("Value=\"255 0 0\""), "{xml}");
    assert!(xml.contains("FillColor Value=\"#0000FF\"") || xml.contains("Value=\"0 0 255\""), "{xml}");
}

#[test]
fn none_color_fragment_writes_no_fillcolor_and_reloads_as_none() {
    let (ann, hints) = span_textbox_with_hints_none_color();
    let xml = serialize_page_annot(&PageId::new("1"), &[ann], &mut 10, &hints);
    let parsed = parse_page_annot(&xml, &PageId::new("1")).unwrap();
    assert!(spans_of(&parsed[0]).iter().all(|s| s.color.is_none()));
}

#[test]
fn wrapped_span_round_trip_keeps_both_colors() {
    // content is longer than the width -> per-line TextObjects, Remark without soft breaks
    let (ann, hints) = soft_wrapped_span_textbox();
    let xml = serialize_page_annot(&PageId::new("1"), &[ann], &mut 10, &hints);
    let parsed = parse_page_annot(&xml, &PageId::new("1")).unwrap();
    assert_eq!(colors_of(&parsed[0]), vec![RED, RED, BLUE, BLUE]);
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p rofd-io span_hints_emit none_color_fragment wrapped_span_round_trip`
Expected: FAIL (runs path writes per-stored-run geometry / no fragments).

- [ ] **Step 3: Implement**

- Delete `text_run_object_xml`.
- TextBox arm: when `spans` non-empty and `line_hint` non-empty, for each line and each fragment emit a `<TextObject>`:
  `Boundary = (frag.x, line.top, frag_width, line.height)` where `frag_width = sum(frag.advances)`;
  style = span covering `line.char_start + frag.char_start` (walk cumulative span char counts);
  `FillColor` only when `span.color == Some`;
  `TextCode X=0 Y=baseline-top` + `DeltaX` from advances (first `n-1`).
  Empty fragment (blank line) → one `<TextObject>` with width 0 and empty `TextCode`.
- When `spans` non-empty but no hints: fall back to the existing flat `text_object_xml` of `content` (documented degradation).
- Keep the flat hints/legacy paths untouched for `spans.is_empty()`.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p rofd-io && cargo test -p rofd-io --test save_surgical`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/io/src/serialize
git commit -m "feat(io): serialize styled annotation spans as per-fragment text objects"
```

---

### Task 9: Adapters + whole-workspace verification

**Files:**
- Test: `crates/web-view/sdk/src/__tests__/formatPanel.test.ts`
- Modify: `AGENTS.md` (rofd-component bullet)
- Modify: `docs/superpowers/specs/2026-11-10-annotation-format-panel-design.md` (status note)

**Interfaces:**
- Consumes: everything above; adapter APIs unchanged (no wasm/TS signature changes).

- [ ] **Step 1: Add the web SDK test**

```ts
it("shows the style at the start of the text selection", () => {
  const root = document.createElement("div");
  let styled = ["宋体", "4.23", "#000000", "1"];
  const h = host({
    annotationTextStyle: () => styled,
  });
  const panel = createFormatPanel(root, h as never, vp);
  expect((root.querySelector(".rofd-fp-color") as HTMLElement).style.borderBottom)
    .toContain("#000000");
  styled = ["黑体", "7.06", "#FF0000", "1"]; // selection moved inside the box
  panel.refresh();
  const el = root.querySelector(".rofd-format-panel") as HTMLElement;
  expect(el.textContent).toContain("黑体");
  expect((root.querySelector(".rofd-fp-color") as HTMLElement).style.borderBottom)
    .toContain("#FF0000");
});
```

- [ ] **Step 2: Run the SDK tests**

Run: `cd crates/web-view/sdk && npm test`
Expected: PASS (new test plus the existing 86).

- [ ] **Step 3: Update docs**

- `AGENTS.md` rofd-component bullet: replace the "富 runs 塌缩" wording with the span model (selection-aware setters, `spans` invariant, no collapse; format panel applies to selection).
- `2026-11-10-annotation-format-panel-design.md`: change the status line to note that §1.6 is superseded and implemented by the rich-text design.

- [ ] **Step 4: Full verification**

Run:
```bash
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd crates/web-view/sdk && npx tsc --noEmit
```
Expected: all green. If the wasm package needs refreshing for a browser check, run `npm run build:ts` in `crates/web-view/sdk`.

- [ ] **Step 5: Commit**

```bash
git add crates/web-view/sdk/src/__tests__/formatPanel.test.ts AGENTS.md docs/superpowers/specs
git commit -m "test(web-sdk): cover selection-aware panel labels and document spans"
```
