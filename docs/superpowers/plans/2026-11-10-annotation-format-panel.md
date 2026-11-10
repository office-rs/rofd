# 批注文字格式面板 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 选中文字批注/文本框时，在框上方浮出字体/字号/颜色格式栏；三个 SDK 适配器各自内建这份 UI。

**Architecture:** 内核（render/editor/component）提供"当前样式 / 锚点 / 可选字体 / 改值命令"；三个适配器（xilem-view、gpui-view、web-view SDK）只做控件渲染与事件回写。字号/颜色进 payload 持久化，字体族 v1 仅会话内生效。

**Tech Stack:** Rust（xilem/masonry、gpui、wasm-bindgen、parley/fontique）、TypeScript（DOM + vitest/jsdom）。

**Spec:** `docs/superpowers/specs/2026-11-10-annotation-format-panel-design.md`

## Global Constraints

- 代码注释、commit message 一律英文；spec/plan/UI 文案用中文（AGENTS §10）。
- 库内不调挂钟（AGENTS §4.4）；单 `main` 分支直接提交。
- 依赖方向不变：适配器只依赖 `rofd-component`，不新增 `rofd-render` 依赖边（AGENTS §3）。
- 每次"选一项" = 一个 Transaction；高度只增不减；富文本框首次改样式塌缩 `runs`。
- 批量验证命令：`cargo test --workspace --exclude tauri-app --exclude rofd-web-view`、
  `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、
  `cargo check -p rofd-web-view --target wasm32-unknown-unknown`。
- 不触碰用户已有的 `test/sample.ofd` 修改。
- 未知/缺失字体必须静默降级：面板照常显示，文字落默认族，不 panic、不丢内容。

## Review Focus

- **选中一个字体后立刻切换工具/点空白**：面板必须消失且不再对已清空的选区发命令（不得 panic 或改错批注）。
- **框贴在视口顶部或底部**：面板必须翻到另一侧且完整可见，不遮挡选区手柄。
- **字体族不在本机**：选中后文字落默认族，面板仍显示所选族名，保存重开不炸。
- **富文本框改字号**：runs 塌缩、一次 undo 全还原、高度只增不减。
- **点击面板本身**：不得清空选区、不得把点击穿透到页面产生新批注/移框。

---

### Task 1: render —— 可选字体列表 + 声明族按名字解析

**Files:**
- Modify: `crates/render/src/text/font.rs`（`from_resources` 的 `declared_families`、`family_list_for`、新增 `FamilyEntry`/`family_entries`）
- Modify: `crates/render/src/lib.rs`（导出 `FamilyEntry`）

**Interfaces:**
- Consumes: `fontique::Collection::family_names()`（0.8）、`normalize_family_key`、`chain_names`。
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct FamilyEntry { pub key: String, pub display: String }
  impl FontStore {
      /// System families (native) + declared families, internal `__rofs_*` filtered.
      pub fn family_entries(&self) -> Vec<FamilyEntry>;
      /// Registered family name -> declared family name -> None.
      pub fn family_display_name(&self, id: &FontId) -> Option<String>;
  }
  ```

- [ ] **Step 1: 写失败测试**（`font.rs` 的 `mod tests`）

```rust
#[test]
fn declared_family_without_slices_resolves_to_the_named_family() {
    let mut res = Resources::default();
    res.fonts.insert(FontId::new("F1"), FontRef { family_name: Some("SimSun".into()) });
    let store = FontStore::from_resources(&res, Arc::new(vec![]));
    match store.family_list_for(&FontId::new("F1")) {
        parley::FontFamily::List(names) => {
            assert!(names.iter().any(|n| matches!(n, parley::FontFamilyName::Named(n) if n.as_ref() == "SimSun")));
        }
        _ => panic!("expected a named family list"),
    }
}

#[test]
fn family_entries_filter_internal_slice_names_and_dedup() {
    let bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
    let mut res = Resources::default();
    res.fonts.insert(FontId::new("F1"), FontRef { family_name: Some("SimSun".into()) });
    let mut store = FontStore::from_resources(&res, Arc::new(bytes.to_vec()));
    store.register_font_slice(Arc::new(bytes.to_vec()), "simsun", "0");
    let entries = store.family_entries();
    assert!(entries.iter().all(|e| !e.display.starts_with("__rofs_")));
    let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    let mut deduped = keys.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(keys.len(), deduped.len(), "keys must be unique: {keys:?}");
    assert!(entries.iter().any(|e| e.display == "SimSun"));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render declared_family_without_slices family_entries_filter -- --nocapture`
Expected: 编译失败 / `family_entries` 不存在，且第一个断言失败（当前落默认族）。

- [ ] **Step 3: 实现**

`from_resources` 的 `declared_families` 改存**原始族名**（去掉 `normalize_family_key` 包装）。
`family_list_for` 的声明族分支改为：先 `normalize_family_key(declared)` 查 `slice_chains`，
命中用链，未命中 push `FontFamilyName::Named(Cow::Owned(declared.clone()))`。
新增 `FamilyEntry` 与 `family_entries(&self)`：遍历 `self.font_cx.borrow_mut().collection.family_names()`，
跳过 `__rofs_` 前缀与空归一化键，按 `normalize_family_key` 去重后 push；再并入声明族
（`display` 用原族名）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-render text::font`
Expected: PASS（既有 render 测试保持绿）。

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/text/font.rs crates/render/src/lib.rs
git commit -m "feat(render): list available families and resolve declared families by name"
```

---

### Task 2: render —— 面板锚点纯函数

**Files:**
- Create: `crates/render/src/format_panel.rs`
- Modify: `crates/render/src/lib.rs`（`pub mod format_panel;` + re-export）
- Modify: `crates/render/src/hit_test.rs:152`（`annotation_viewport_rect` 提升为 `pub`）

**Interfaces:**
- Produces:
  ```rust
  /// Above the annotation rect; flips below when there is no room; clamped into the viewport.
  pub fn format_panel_anchor(
      ann: rofd_dom::Rect,
      panel: (f64, f64),
      viewport: (f64, f64),
      margin: f64,
  ) -> (f64, f64);
  pub fn annotation_viewport_rect(doc: &OfdDocument, ann: &Annotation, vp: &Viewport) -> Option<rofd_dom::Rect>;
  ```

- [ ] **Step 1: 写失败测试**（`format_panel.rs` 内 `mod tests`）

```rust
fn rect(x: f64, y: f64, w: f64, h: f64) -> rofd_dom::Rect { rofd_dom::Rect { x, y, w, h } }

#[test]
fn sits_above_the_box() {
    let (x, y) = format_panel_anchor(rect(100.0, 100.0, 200.0, 50.0), (240.0, 28.0), (800.0, 600.0), 8.0);
    assert_eq!((x, y), (100.0, 64.0));
}

#[test]
fn flips_below_when_the_top_has_no_room() {
    let (x, y) = format_panel_anchor(rect(10.0, 10.0, 200.0, 50.0), (240.0, 28.0), (800.0, 600.0), 8.0);
    assert_eq!((x, y), (10.0, 68.0));
}

#[test]
fn clamps_horizontally_into_the_viewport() {
    let (x, _) = format_panel_anchor(rect(700.0, 300.0, 60.0, 20.0), (240.0, 28.0), (800.0, 600.0), 8.0);
    assert_eq!(x, 552.0);
    let (x, _) = format_panel_anchor(rect(0.0, 300.0, 60.0, 20.0), (240.0, 28.0), (800.0, 600.0), 8.0);
    assert_eq!(x, 8.0);
}

#[test]
fn viewport_smaller_than_panel_still_returns_a_visible_origin() {
    let (x, y) = format_panel_anchor(rect(10.0, 10.0, 20.0, 20.0), (240.0, 28.0), (200.0, 120.0), 8.0);
    assert_eq!((x, y), (8.0, 38.0));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render format_panel`
Expected: 模块/函数不存在。

- [ ] **Step 3: 实现**

规则（顺序固定）：`x = ann.x.clamp(margin, max(margin, vp_w - panel_w - margin))`；
`y = ann.y - panel_h - margin`；若 `y < margin` 则 `y = ann.y + ann.h + margin`；
最后 `y = y.clamp(margin, max(margin, vp_h - panel_h - margin))`。
`hit_test::annotation_viewport_rect` 去掉 `pub(crate)`，lib.rs 导出。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-render format_panel`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/render/src/format_panel.rs crates/render/src/lib.rs crates/render/src/hit_test.rs
git commit -m "feat(render): add format-panel anchor geometry"
```

---

### Task 3: component —— 字号表 / 色板 / 样式类型

**Files:**
- Create: `crates/component/src/text_format.rs`
- Modify: `crates/component/src/lib.rs`（`pub mod text_format;` + re-exports）

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct AnnotationTextStyle {
      pub family: String,
      pub size_mm: f64,
      pub color: rofd_dom::Color,
      pub rich: bool,
  }
  pub struct SizeEntry { pub label: &'static str, pub pt: f64 }
  pub const TEXT_SIZES: &[SizeEntry];
  pub const TEXT_COLOR_STANDARD: [&str; 10];
  pub const TEXT_COLOR_PALETTE: [[&str; 10]; 6];
  pub fn pt_to_mm(pt: f64) -> f64;
  pub fn mm_to_pt(mm: f64) -> f64;
  pub fn size_display(size_mm: f64) -> String;
  pub fn color_hex(c: rofd_dom::Color) -> String;
  pub fn hex_color(hex: &str) -> Option<rofd_dom::Color>;
  ```

- [ ] **Step 1: 写失败测试**（`text_format.rs` 内 `mod tests`）

```rust
#[test]
fn pt_mm_conversion_round_trips() {
    assert!((pt_to_mm(12.0) - 4.233333333333333).abs() < 1e-9);
    assert!((mm_to_pt(4.233333333333333) - 12.0).abs() < 1e-9);
}

#[test]
fn size_display_prefers_the_chinese_label() {
    assert_eq!(size_display(pt_to_mm(26.0)), "一号");
    assert_eq!(size_display(pt_to_mm(10.5)), "五号");
    assert_eq!(size_display(pt_to_mm(13.0)), "13");
}

#[test]
fn palette_entries_are_valid_hex() {
    for row in TEXT_COLOR_PALETTE { for h in row { assert!(hex_color(h).is_some(), "{h}"); } }
    for h in TEXT_COLOR_STANDARD { assert!(hex_color(h).is_some(), "{h}"); }
    assert_eq!(color_hex(Color::Rgb(0x12, 0x34, 0x56)), "#123456");
}

#[test]
fn text_sizes_contains_chinese_and_numeric_entries() {
    assert!(TEXT_SIZES.iter().any(|e| e.label == "初号" && e.pt == 42.0));
    assert!(TEXT_SIZES.iter().any(|e| e.label.is_empty() && e.pt == 72.0));
    assert_eq!(TEXT_SIZES.len(), 16 + 16);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component text_format`
Expected: 模块不存在。

- [ ] **Step 3: 实现**

- `TEXT_SIZES`：16 个中文字号（初号 42 / 小初 36 / 一号 26 / 小一 24 / 二号 22 /
  小二 18 / 三号 16 / 小三 15 / 四号 14 / 小四 12 / 五号 10.5 / 小五 9 / 六号 7.5 /
  小六 6.5 / 七号 5.5 / 八号 5，`label` = 中文名）+ 16 个数字档
  （8,9,10,11,12,14,16,18,20,22,24,26,28,36,48,72，`label` = ""）。
- `size_display`：`mm_to_pt` 后与某个 `SizeEntry` 的 `pt` 差 < 0.05 则返回其 `label`
  （中文或空串），空串时返回格式化的磅值（整数无小数，否则一位小数）。
- `TEXT_COLOR_STANDARD`（rword 同款）：
  `["#000000","#FFFFFF","#FF0000","#FFC000","#FFFF00","#92D050","#00B0F0","#0070C0","#002060","#7030A0"]`。
- `TEXT_COLOR_PALETTE`（6 行 × 10 列，WPS 近似：每行一个明度档，列基色固定）：

```rust
pub const TEXT_COLOR_PALETTE: [[&str; 10]; 6] = [
    ["#FFFFFF","#F2F2F2","#E7E6E6","#FBE5D6","#FFF2CC","#E2EFDA","#DEEBF7","#E4DFEC","#EDEDED","#FDF2F2"],
    ["#F8CBAD","#FFD966","#C6E0B4","#BDD7EE","#B4C6E7","#D9E1F2","#FCE4D6","#E2F0D9","#FFF2CC","#E6E6E6"],
    ["#ED7D31","#FFC000","#70AD47","#5B9BD5","#4472C4","#7030A0","#C00000","#00B0F0","#7F7F7F","#404040"],
    ["#C55A11","#BF9000","#548235","#2E75B6","#2F5597","#5B2C6F","#900000","#0070C0","#595959","#262626"],
    ["#833C0B","#7F6000","#375623","#1F4E79","#1F3864","#3E1D4A","#660000","#002060","#3B3B3B","#0D0D0D"],
    ["#000000","#FFFFFF","#FF0000","#ED7D31","#FFC000","#70AD47","#5B9BD5","#2E75B6","#7030A0","#7F7F7F"],
];
```

- `hex_color` 接受 `#RRGGBB` / `RRGGBB`，非法返回 `None`；`color_hex` 输出 `#RRGGBB`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component text_format`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/text_format.rs crates/component/src/lib.rs
git commit -m "feat(component): add annotation text size table, palette, and style type"
```

---

### Task 4: component —— 只读 API（样式 / 锚点 / 可选字体）

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（新增字段 `available_font_families: Vec<FamilyEntry>` 与四个方法；`load_document`/`new_document` 时清空该字段）
- Test: `crates/component/src/ofd_component.rs` 的 `mod tests`

**Interfaces:**
- Consumes: Task 1 的 `FontStore::family_entries`、Task 2 的 `format_panel_anchor` / `annotation_viewport_rect`、Task 3 的 `AnnotationTextStyle`。
- Produces:
  ```rust
  pub fn annotation_text_style(&mut self, id: &AnnotationId) -> Option<AnnotationTextStyle>;
  pub fn annotation_format_anchor(&mut self, id: &AnnotationId) -> Option<rofd_dom::Rect>;
  pub fn font_families(&mut self) -> Vec<rofd_render::FamilyEntry>;
  pub fn set_available_font_families(&mut self, families: Vec<rofd_render::FamilyEntry>);
  ```

- [ ] **Step 1: 写失败测试**（复用 `component_with_textbox()` 夹具）

```rust
#[test]
fn annotation_text_style_reads_the_flat_textbox() {
    let mut c = component_with_textbox();
    let id = textbox_id(&c);
    let s = c.annotation_text_style(&id).expect("textbox style");
    assert!((s.size_mm - DEFAULT_TEXTBOX_SIZE).abs() < 1e-9);
    assert_eq!(s.color, Color::Rgb(0, 0, 0));
    assert!(!s.rich);
}

#[test]
fn annotation_text_style_is_none_for_other_annotations_and_missing_ids() {
    let mut c = component_with_page();
    assert!(c.annotation_text_style(&AnnotationId::from_int(999)).is_none());
}

#[test]
fn annotation_format_anchor_matches_the_viewport_rect() {
    let mut c = component_with_textbox();
    let id = textbox_id(&c);
    let a = c.annotation_format_anchor(&id).expect("anchor");
    let doc_rect = rofd_render::annotation_viewport_rect(
        c.document(),
        c.document().annotations.find(&id).unwrap(),
        &c.viewport,
    ).unwrap();
    assert_eq!((a.x, a.y, a.w, a.h), (doc_rect.x, doc_rect.y, doc_rect.w, doc_rect.h));
}

#[test]
fn available_font_families_are_merged_into_the_list() {
    let mut c = component_with_page();
    c.set_available_font_families(vec![rofd_render::FamilyEntry {
        key: "simsun".into(), display: "宋体".into(),
    }]);
    let entries = c.font_families();
    assert!(entries.iter().any(|e| e.display == "宋体"));
    c.new_document();
    assert!(!c.font_families().iter().any(|e| e.display == "宋体"));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component annotation_text_style annotation_format_anchor available_font_families`
Expected: 方法不存在。

- [ ] **Step 3: 实现**

- `annotation_text_style`：取批注，`TextBox{font,size,color,runs}` → `Some`，`family` =
  `font_store.family_display_name(font)`（Task 1 已提供）；`rich = !runs.is_empty()`。
- `annotation_format_anchor`：`annotation_viewport_rect(doc, ann, &self.viewport)`。
- `font_families`：`ensure_font_store` 后 `font_store.family_entries()`，再并入
  `self.available_font_families`（按 `key` 去重，已有的保留原 display）。
- `set_available_font_families`：整体替换并按 `key` 去重、排序（key 升序）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component annotation_text_style annotation_format_anchor available_font_families`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/ofd_component.rs crates/component/src/lib.rs
git commit -m "feat(component): expose text style, format anchor, and available font families"
```

---

### Task 5: editor + component —— 修改命令（单 Transaction，只增高度）

**Files:**
- Modify: `crates/editor/src/commands/text_commands.rs`
- Modify: `crates/component/src/ofd_component.rs`
- Test: `crates/editor/src/commands/text_commands.rs` 的 `mod tests`、`ofd_component.rs` 的 `mod tests`

**Interfaces:**
- Consumes: `collapse_runs`、`replace_txn`、`measure_textbox_growth`。
- Produces:
  ```rust
  // editor
  pub fn set_annotation_font_size_measured<F>(&mut self, id: &AnnotationId, size_mm: f64, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  pub fn set_annotation_font_family_measured<F>(&mut self, id: &AnnotationId, family: &str, measure: F)
  where F: FnOnce(&AnnotationPayload) -> Option<f64>;
  // component
  pub fn set_annotation_font_family(&mut self, id: &AnnotationId, family: &str);
  pub fn set_annotation_font_size(&mut self, id: &AnnotationId, size_mm: f64);
  pub fn set_annotation_color(&mut self, id: &AnnotationId, color: rofd_dom::Color);
  ```

- [ ] **Step 1: 写失败测试**（editor）

```rust
#[test]
fn set_font_size_is_one_transaction_and_restores_on_undo() {
    let (mut e, id) = textbox_editor("hello", 100.0, 20.0);
    let before_h = textbox_rect(&e, &id).h;
    e.set_annotation_font_size_measured(&id, 20.0, |_| Some(80.0));
    assert_eq!(textbox_rect(&e, &id).h, 80.0);
    assert_eq!(textbox_size(&e, &id), 20.0);
    e.undo();
    assert_eq!(textbox_size(&e, &id), DEFAULT_SIZE);
    assert_eq!(textbox_rect(&e, &id).h, before_h);
}

#[test]
fn set_font_size_never_shrinks_the_box() {
    let (mut e, id) = textbox_editor("hello", 100.0, 40.0);
    e.set_annotation_font_size_measured(&id, 6.0, |_| None);
    assert_eq!(textbox_rect(&e, &id).h, 40.0);
}

#[test]
fn set_font_family_writes_the_font_id_and_collapses_runs() {
    let (mut e, id) = rich_textbox_editor(); // runs 非空、size = DEFAULT_TEXTBOX_SIZE
    e.set_annotation_font_family_measured(&id, "宋体", |_| Some(24.0));
    let ann = e.document().annotations.find(&id).unwrap();
    let AnnotationPayload::TextBox { font, runs, rect, .. } = &ann.payload else { panic!() };
    assert_eq!(font.0, "宋体");
    assert!(runs.is_empty(), "rich runs collapse on the first style edit");
    assert_eq!(rect.h, 24.0);
}

#[test]
fn unknown_family_still_updates_the_payload() {
    let (mut e, id) = textbox_editor("hi", 100.0, 20.0);
    e.set_annotation_font_family_measured(&id, "No Such Family", |_| None);
    let ann = e.document().annotations.find(&id).unwrap();
    let AnnotationPayload::TextBox { font, .. } = &ann.payload else { panic!() };
    assert_eq!(font.0, "No Such Family");
    // Shaping degrades to the fallback family; the model must stay valid.
    e.undo();
    assert_ne!(
        e.document().annotations.find(&id).unwrap().payload,
        ann.payload
    );
}
```

> `textbox_editor` / `textbox_rect` / `textbox_size` 为测试内小助手（构造一个 100×20 的
> TextBox + `Page`，`size = DEFAULT_TEXTBOX_SIZE`）；若已有同型助手则复用。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-editor set_font_size set_font_family`
Expected: 方法不存在。

- [ ] **Step 3: 实现**

两个 editor 方法都复制 `insert_text_measured` 的骨架：取 before → `collapse_runs` →
写 `size`/`font` → `measure(&after.payload)` → `Some(h)` 时写 `rect.h` →
`after.modified = current_ts` → `execute_transaction(replace_txn(...))`。

component 三个 setter：`ensure_font_store()` → editor 命令（size/family 传
`|p| measure_textbox_growth(store, p)`；color 用既有 `editor.set_annotation_color`）→
`after_annotation_change()`。不得调用 `set_cursor` / 改 selection。

- [ ] **Step 4: 写 component 测试并跑**

```rust
#[test]
fn format_setters_keep_selection_and_cursor_and_fire_on_change() {
    // component_with_textbox + select + set_cursor(0)，统计 on_change 次数
    let mut c = component_with_textbox();
    c.mark_modified_cleared();
    let id = textbox_id(&c);
    c.editor
        .set_selection(rofd_dom::AnnotationSelection::Single(id.clone()));
    c.editor.set_cursor(id.clone(), 0);
    c.set_annotation_font_size(&id, 20.0);
    assert_eq!(
        c.selection(),
        &rofd_dom::AnnotationSelection::Single(id.clone())
    );
    assert_eq!(c.text_cursor().unwrap().offset, 0);
    assert!(c.is_modified(), "a style change dirties the document");
}
```

Run: `cargo test -p rofd-component format_setters`
Expected: PASS。

- [ ] **Step 5: 全量验证并提交**

Run: `cargo test -p rofd-editor && cargo test -p rofd-component`
Expected: PASS。

```bash
git add crates/editor/src/commands/text_commands.rs crates/component/src/ofd_component.rs
git commit -m "feat(editor): add measured font size and family commands; wire component setters"
```

---

### Task 6: component —— 字体需求收集认未知 FontId

**Files:**
- Modify: `crates/component/src/font_req.rs:84-101`
- Test: `crates/component/src/font_req.rs` 的 `mod tests`

**Interfaces:**
- Produces: 无新 API；`collect_family` 行为改变。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn unknown_font_id_contributes_its_normalized_family() {
    let mut doc = OfdDocument::default();
    doc.pages.push(page_with_annotation(AnnotationPayload::TextBox {
        rect: Rect::default(), content: "x".into(), font: FontId::new("宋体"),
        size: 12.0, color: Color::Rgb(0, 0, 0), border: None, runs: vec![],
    }));
    let (families, _) = annotation_requirements(&doc);
    assert_eq!(families, vec!["simsun".to_string()]);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component unknown_font_id_contributes`
Expected: families 为空。

- [ ] **Step 3: 实现**

`collect_family` 里声明名缺席时不再直接 `return`，改用 `normalize_family_key(&font.0)`
作为族键（空键不入列）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component font_req`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/component/src/font_req.rs
git commit -m "feat(component): require the family of font ids that have no declared font"
```

---

### Task 7: web-view —— wasm 导出面

**Files:**
- Modify: `crates/web-view/src/wasm_ofd.rs`
- Modify: `crates/web-view/sdk/src/index.ts`（`WasmOfd` 接口声明，供 Task 9 用）
- Test: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`

**Interfaces:**
- Consumes: Task 4/5 的 component 方法、Task 3 的常量。
- Produces（`#[wasm_bindgen] impl WasmOfd`）：
  ```rust
  pub fn selected_text_annotation(&self) -> Option<String>;
  pub fn annotation_text_style(&self, id: &str) -> Option<js_sys::Array>; // [family, sizeMm, "#rrggbb", rich?1:0]
  pub fn annotation_format_anchor(&self, id: &str) -> Option<Vec<f64>>;  // [x, y, w, h]
  pub fn font_families(&mut self) -> js_sys::Array;                     // [[key, display], ...]
  pub fn set_available_font_families(&mut self, pairs: js_sys::Array);  // [[key, display], ...]
  pub fn text_sizes(&self) -> js_sys::Array;                            // [[label, pt], ...]
  pub fn text_color_standard(&self) -> js_sys::Array;
  pub fn text_color_palette(&self) -> js_sys::Array;                    // 60 strings
  pub fn set_annotation_font_family(&mut self, id: &str, family: &str);
  pub fn set_annotation_font_size(&mut self, id: &str, size_mm: f64);
  pub fn set_text_annotation_color(&mut self, id: &str, hex: &str);
  ```
- `selected_text_annotation` 规则：`selection == Single(id)` 且 payload 是 `TextBox` 时返回
  `Some(id.0)`，否则 `None`。

- [ ] **Step 1: 实现导出（薄封装）**

每个导出只做 `self.component.xxx(...)` 与 JsValue 组装；`set_*` 不需要显式唤醒渲染
——SDK 的 Proxy kick 会在任意 wasm 调用后预约一帧。`set_available_font_families`
解析 `[[String,String]]` 后调用 component。`set_text_annotation_color` 用 `hex_color`
解析，非法参数静默忽略。

- [ ] **Step 2: 更新 TS 接口声明**

`crates/web-view/sdk/src/index.ts` 的 `interface WasmOfd` 增加上述同名方法
（camelCase，wasm-bindgen 自动转换）。

- [ ] **Step 3: 验证编译**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: 通过，无 warning。

- [ ] **Step 4: Commit**

```bash
git add crates/web-view/src/wasm_ofd.rs crates/web-view/sdk/src/index.ts
git commit -m "feat(web-view): expose annotation text format surface to the SDK"
```

---

### Task 8: gpui-view —— 内建格式面板

**Files:**
- Create: `crates/gpui-view/src/format_panel.rs`（状态机 + 渲染片段）
- Modify: `crates/gpui-view/src/view.rs`（`render` 组合面板；`OfdGpuiView` 增加字段）
- Modify: `crates/gpui-view/src/lib.rs`
- Test: `crates/gpui-view/src/format_panel.rs` 的 `mod tests` + `crates/gpui-view/tests/handle_verbs.rs`

**Interfaces:**
- Consumes: Task 4/5 的 component 方法、Task 3 的常量。
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub(crate) struct FormatPanelState {
      pub target: Option<rofd_dom::AnnotationId>,
      pub anchor: (f64, f64),
      pub open: Option<PanelMenu>,          // Family | Size | Color
      pub families: Vec<rofd_render::FamilyEntry>,
      pub style: Option<AnnotationTextStyle>,
  }
  pub(crate) enum PanelMenu { Family, Size, Color }
  impl FormatPanelState {
      pub fn refresh(&mut self, component: &mut OfdComponent);
      pub fn dismiss(&mut self);
      pub fn show(&self) -> bool { self.target.is_some() && self.style.is_some() }
  }
  ```

- [ ] **Step 1: 写失败测试**（纯状态，不渲染）

```rust
#[test]
fn refresh_targets_a_selected_textbox_only() {
    let mut c = component_with_textbox(); // 复用 component 测试夹具思路，在 gpui-view 内新建
    let id = selected_textbox(&mut c);
    let mut s = FormatPanelState::default();
    s.refresh(&mut c);
    assert_eq!(s.target.as_ref(), Some(&id));
    assert!(s.show());
    c.set_selection(AnnotationSelection::None);
    s.refresh(&mut c);
    assert!(!s.show());
}

#[test]
fn anchor_is_above_the_box_and_flips_near_the_top() {
    // 构造 viewport (800,600)、annotation at y=10；断言 s.anchor.1 > 10.0
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-gpui-view format_panel`
Expected: 模块不存在。

- [ ] **Step 3: 实现状态机**

`refresh`：`component.selection()` 为 `Single(id)` 且 `annotation_text_style(id).is_some()`
时设置 target/anchor（`annotation_format_anchor` + `rofd_component::format_panel_anchor`
用面板尺寸 `(240.0, 28.0)`、viewport = `component.viewport_size()`），families 只在
target 变化或为空时刷新；否则清空 target 并把 `open` 复位。

- [ ] **Step 4: 渲染与事件**

`render` 末尾追加面板 `div().absolute().left(px(x)).top(px(y))`：一行三个控件（字体按钮、
字号按钮、颜色按钮），`open` 时在下方显示列表/色板。所有子树在 `on_mouse_down` 里
`cx.stop_propagation()`；外部点击（`on_mouse_down_out`）把 `open` 置空。列表项点击调
`component.set_annotation_font_family/size`；色块调 `set_annotation_color`。
每次 `pressed`/`moved`/滚轮/`render` 前调用 `state.refresh(&mut self.component)`。

- [ ] **Step 5: 验证并提交**

Run: `cargo test -p rofd-gpui-view && cargo clippy -p rofd-gpui-view --all-targets -- -D warnings`
Expected: PASS。

```bash
git add crates/gpui-view/src/format_panel.rs crates/gpui-view/src/view.rs crates/gpui-view/src/lib.rs crates/gpui-view/tests/handle_verbs.rs
git commit -m "feat(gpui-view): float the text format panel above selected text boxes"
```

---

### Task 9: web SDK —— DOM 格式面板

**Files:**
- Create: `crates/web-view/sdk/src/formatPanel.ts`
- Create: `crates/web-view/sdk/src/__tests__/formatPanel.test.ts`
- Modify: `crates/web-view/sdk/src/index.ts`（init 挂载、配置项 `formatPanel?: boolean`、`fontFlow` 推送可选族）

**Interfaces:**
- Consumes: Task 7 的 wasm 方法与 `WasmOfd` 接口。
- Produces:
  ```ts
  export interface FormatPanelHost {
    selectedTextAnnotation(): string | null;
    annotationTextStyle(id: string): string[] | null;   // [family, sizeMm, "#rrggbb", "0"|"1"]
    annotationFormatAnchor(id: string): number[] | null;
    fontFamilies(): string[][];                          // [[key, display]]
    textSizes(): (string | number)[][];                  // [[label, pt]]
    textColorStandard(): string[];
    textColorPalette(): string[];
    setAnnotationFontFamily(id: string, family: string): void;
    setAnnotationFontSize(id: string, sizeMm: number): void;
    setTextAnnotationColor(id: string, hex: string): void;
  }
  export function createFormatPanel(container: HTMLElement, wasm: FormatPanelHost, viewport: () => { w: number; h: number }): { refresh(): void; destroy(): void };
  ```

- [ ] **Step 1: 写失败测试**（vitest + jsdom）

```ts
import { describe, it, expect } from 'vitest';
import { createFormatPanel } from '../formatPanel';

function host(overrides = {}) {
  const calls: string[] = [];
  return {
    calls,
    selectedTextAnnotation: () => '7',
    annotationTextStyle: () => ['宋体', '4.233333333333333', '#000000', '0'],
    annotationFormatAnchor: () => [100, 100, 200, 50],
    fontFamilies: () => [['simsun', '宋体'], ['simhei', '黑体']],
    textSizes: () => [['一号', 26], ['', 12]],
    textColorStandard: () => ['#000000', '#FF0000'],
    textColorPalette: () => ['#FFFFFF', '#000000'],
    setAnnotationFontFamily: (id: string, f: string) => calls.push(`family:${id}:${f}`),
    setAnnotationFontSize: (id: string, s: number) => calls.push(`size:${id}:${s}`),
    setTextAnnotationColor: (id: string, c: string) => calls.push(`color:${id}:${c}`),
    ...overrides,
  };
}

describe('format panel', () => {
  it('shows above the selected text box with the current values', () => {
    const root = document.createElement('div');
    const h = host();
    const panel = createFormatPanel(root, h as any, () => ({ w: 800, h: 600 }));
    panel.refresh();
    const el = root.querySelector('.rofd-format-panel') as HTMLElement;
    expect(el.style.display).not.toBe('none');
    expect(el.style.top).toBe('64px');      // 100 - 28 - 8
    expect(el.textContent).toContain('宋体');
    expect(el.textContent).toContain('12');
  });

  it('hides when nothing is selected', () => {
    const root = document.createElement('div');
    const panel = createFormatPanel(root, host({ selectedTextAnnotation: () => null }) as any, () => ({ w: 800, h: 600 }));
    panel.refresh();
    expect((root.querySelector('.rofd-format-panel') as HTMLElement).style.display).toBe('none');
  });

  it('picking a size applies it and keeps the panel open', () => {
    const root = document.createElement('div');
    const h = host();
    const panel = createFormatPanel(root, h as any, () => ({ w: 800, h: 600 }));
    panel.refresh();
    (root.querySelector('.rofd-fp-size') as HTMLElement).dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
    const item = root.querySelector('.rofd-fp-size-item') as HTMLElement;
    item.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    const applied = h.calls.find((c) => c.startsWith('size:7:'));
    expect(applied).toBeDefined();
    expect(Number(applied!.split(':')[2])).toBeCloseTo((26 * 25.4) / 72, 6);
    expect(h.selectedTextAnnotation()).toBe('7');
  });

  it('pointerdown on the panel does not steal focus from the IME input', () => {
    const root = document.createElement('div');
    const panel = createFormatPanel(root, host() as any, () => ({ w: 800, h: 600 }));
    panel.refresh();
    const ev = new MouseEvent('pointerdown', { bubbles: true, cancelable: true });
    (root.querySelector('.rofd-fp-family') as HTMLElement).dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd crates/web-view/sdk && npx vitest run src/__tests__/formatPanel.test.ts`
Expected: 模块不存在。

- [ ] **Step 3: 实现 `formatPanel.ts`**

- 挂载一个 `div.rofd-format-panel`（`position:absolute; display:none; z-index:10`）。
- `refresh()`：`selectedTextAnnotation()` 为 null → 隐藏；否则读 style/anchor，
  `top = anchor.y - 28 - 8`（不足 8 时 `anchor.y + anchor.h + 8`），`left` clamp 进
  `viewport()`；更新字体/字号显示文本与颜色条。
- 面板根节点 `pointerdown` 上 `preventDefault()`（保隐藏 IME input 的焦点）；
  所有事件 `stopPropagation()`。
- 打开列表 = 再建一个子 div；列表项点击后调用对应 setter，然后再 `refresh()`。
- 字号项应用时把 pt 换算成 mm（`pt * 25.4 / 72`）。

- [ ] **Step 4: 接进 `index.ts`**

`config.formatPanel !== false` 时在 container 内创建面板；在 `setOnSelectionChange`/
`setOnCursorChange`/`setOnChange`/`handleResize` 之后调 `panel.refresh()`；`destroy()` 时移除。
`fontFlow` 就绪后调 `wasm.setAvailableFontFamilies(Object.entries(manifest.families).map(([k,f]) => [k, f.displayName]))`。

- [ ] **Step 5: 跑测试与类型检查**

Run: `cd crates/web-view/sdk && npx vitest run && npx tsc -p tsconfig.json --noEmit`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/web-view/sdk/src/formatPanel.ts crates/web-view/sdk/src/__tests__/formatPanel.test.ts crates/web-view/sdk/src/index.ts
git commit -m "feat(web-sdk): render the annotation text format panel in the DOM"
```

---

### Task 10: xilem-view —— 内建格式面板（先 spike）

**Files:**
- Create: `crates/xilem-view/src/format_panel.rs`
- Modify: `crates/xilem-view/src/ofd_view.rs`（视图改为 `zstack((OfdWidget, panel))`、新增 `format_panel(bool)`）
- Modify: `crates/xilem-view/src/ofd_widget.rs`（共享面板状态单元 + touch 末尾刷新 + 视口变化提交 action）
- Test: `crates/xilem-view/tests/ofd_widget_harness.rs`

**Interfaces:**
- Consumes: Task 4/5、Task 2 的 `format_panel_anchor`（经 component re-export）。
- Produces:
  ```rust
  // ofd_widget.rs
  pub(crate) struct FormatPanelCell {
      pub target: Option<rofd_dom::AnnotationId>,
      pub anchor: (f64, f64),
      pub style: Option<rofd_component::AnnotationTextStyle>,
      pub families: Vec<rofd_render::FamilyEntry>,
      pub open: Option<PanelMenu>,
  }
  pub(crate) type SharedFormatPanel = Arc<Mutex<FormatPanelCell>>;
  // ofd_view.rs
  pub fn ofd_with_config(...).format_panel(bool)   // default true
  // action
  OfdWidgetAction::FormatPanelChanged
  ```

- [ ] **Step 0（spike，人工判定）**

在 `ofd_widget_harness` 里临时搭：`zstack((ofd_widget, transformed(sized_box(label("x"))).translate(Vec2::new(120.0, 40.0)))).alignment(UnitPoint::TOP_LEFT)`，
用 harness 断言：(a) 面板 widget 的布局原点在 (120,40)；(b) 在面板矩形外点击，事件到达
OfdWidget（`component` 收到 PointerDown）；(c) 在面板矩形内点击，OfdWidget 收不到。
三条全绿 → 按下面的步骤实现；任一失败 → 改用 `OfdWidget` 持有 child `WidgetPod`
（在 `layout` 里定位、`on_pointer_event` 先转发子节点），并把结论记录到 spec §5.3。

- [ ] **Step 1: 写失败测试**（在现有 harness 文件里新增夹具与断言）

```rust
// helpers: build a doc with one flat TextBox, drive a press on its center,
// expose the component's size_mm through a command-queue read (Arc<Mutex<f64>>).
#[test]
fn selecting_a_textbox_shows_the_format_panel_in_the_zstack() {
    let mut h = harness_with_textbox();
    h.click_annotation();
    assert!(h.format_panel_visible(), "panel view must be present after selection");
}

#[test]
fn clicking_a_size_item_applies_it_to_the_component() {
    let mut h = harness_with_textbox();
    h.click_annotation();
    h.click_format_size_item("一号");
    assert!((h.component_size_mm() - 26.0 * 25.4 / 72.0).abs() < 1e-6);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-xilem-view format_panel`
Expected: 视图/动作不存在。

- [ ] **Step 3: 实现共享单元与 action**

`OfdWidget` 新增 `panel: SharedFormatPanel`；`after_touch` 末尾调用
`refresh_panel_cell(&mut self.component, &self.panel)`（选区/锚点/样式/字体列表）。
当 `target` 从 None↔Some 或锚点变化时 `ctx.submit_action(OfdWidgetAction::FormatPanelChanged)`，
同时 `ctx.request_paint_only()`。滚动/缩放/resize 分支（`ofd_widget.rs` 的 wheel / zoom /
resize 处理）在面板可见时同样提交该 action。

- [ ] **Step 4: 实现视图**

`OfdView::build` 返回
`zstack((OfdWidget, panel_view)).alignment(UnitPoint::TOP_LEFT)`；
`panel_view` 是 `Option<Box<AnyWidgetView<State>>>`：cell 无 target 时是零尺寸 `sized_box(label(""))`，
否则 `transformed(sized_box(panel_row).background_color(...).border_width(1.0)).translate(Vec2::new(x, y))`。
面板按钮：`button(label(name), move |_state: &mut State| { cell.lock().unwrap().open = Some(PanelMenu::Family); })`
（靠 xilem 的 action 机制触发重跑）；列表项点击后写 `cell.style` 并把命令 push 进
`OfdCommandQueue`。`OfdView::message` 新增 `FormatPanelChanged` 分支返回
`MessageResult::Action(())`。

- [ ] **Step 5: 跑测试与全量验证**

Run: `cargo test -p rofd-xilem-view && cargo test --workspace --exclude tauri-app --exclude rofd-web-view`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/xilem-view/src/format_panel.rs crates/xilem-view/src/ofd_view.rs crates/xilem-view/src/ofd_widget.rs crates/xilem-view/tests/ofd_widget_harness.rs
git commit -m "feat(xilem-view): float the annotation format panel in a zstack overlay"
```

---

### Task 11: 文档与收尾

**Files:**
- Modify: `AGENTS.md` §5（rofd-component、rofd-web-view、Native Ofd Widget 各补一条）
- Modify: `docs/superpowers/specs/2026-11-10-annotation-format-panel-design.md`（状态改为"已实现"，补 spike 结论）

- [ ] **Step 1: 更新 AGENTS.md**

在 rofd-component 段落加入：格式面板 API（`annotation_text_style` /
`annotation_format_anchor` / `font_families` / 三个 setter）、字号表与色板位置、
字体族 v1 不落盘的口径；Native Ofd Widget 段落加入 zstack 面板与命中规则。

- [ ] **Step 2: 更新 spec 状态与 spike 结论**

- [ ] **Step 3: 全量验证**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: 全绿。

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md docs/superpowers/specs/2026-11-10-annotation-format-panel-design.md
git commit -m "docs: record the annotation format panel architecture"
```
