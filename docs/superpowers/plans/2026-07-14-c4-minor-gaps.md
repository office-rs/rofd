# Cluster 4: 次要缺口收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 收尾 v1 的次要缺口：ViewEvent 补全、on_warning 回调、io warning 发出、错误/warning 用例测试、C3 deferred Minors。

**Architecture:** io parse emit SkippedObject/FontSubstituted/ResourceNotFound + 错误/warning 测试；component 加 ViewEvent(ScrollPage/ZoomAt/Ime) + on_warning 回调 + C3 Minors 修复（page-stacking 重构/resize guard/zoom_change 守卫）；适配器 wire on_warning。

**Tech Stack:** Rust 2021, quick-xml, xilem/winit (native), wasm-bindgen (web). C1-C3 就绪。

**Spec:** [`docs/superpowers/specs/2026-07-14-c4-minor-gaps-design.md`](../specs/2026-07-14-c4-minor-gaps-design.md)

## Global Constraints

- 错误显式分层（AGENTS.md §4.6）：硬错 OfdError；可降级 OfdWarning 不 fatal；无裸 unwrap。
- 依赖严格向上（AGENTS.md §4.1）。
- commits：conventional commits，无 attribution 行。单 main 分支。
- TDD：先红后绿，每任务 commit。
- fmt：baseline clean。用 `cargo fmt -- <files>`；stage ONLY 改的文件（`git add <path>`），勿 `git add -A`（test/ + docs PDF untracked）。

---

## Task 1: io -- SkippedObject/FontSubstituted/ResourceNotFound 发出 + 错误/warning 用例测试

**Files:**
- Modify: `crates/io/src/parse/page.rs`（未知元素 -> SkippedObject）
- Modify: `crates/io/src/parse/mod.rs`（缺字体/图片 -> FontSubstituted/ResourceNotFound）
- Create/Modify: `crates/io/tests/error_cases.rs`（新，错误 + warning 用例）
- Test: `crates/io/tests/error_cases.rs`

**Interfaces:**
- Consumes: `OfdWarning::{SkippedObject, FontSubstituted, ResourceNotFound}`（C1 定义）。
- Produces: io parse emits these warnings；错误/warning 测试夹具。

- [ ] **Step 1: 写失败测试**

`crates/io/tests/error_cases.rs`：

```rust
mod fixtures;
use rofd_io::{parse_ofd, OfdError, OfdWarning};

#[test]
fn bad_zip_returns_zip_error() {
    let bytes = b"not a zip file";
    let err = parse_ofd(bytes).unwrap_err();
    assert!(matches!(err, OfdError::Zip { .. }), "bad zip -> OfdError::Zip, got {err:?}");
}

#[test]
fn bad_xml_returns_xml_error() {
    // 用 fixtures 构造一个 XML 畸形的 .ofd（Document.xml 是坏 XML）
    // ... 构造 zip with malformed Document.xml
    let err = parse_ofd(&bytes).unwrap_err();
    assert!(matches!(err, OfdError::Xml { .. }), "bad xml -> OfdError::Xml");
}

#[test]
fn template_annotation_emits_missing_feature_warning() {
    let bytes = fixtures::build_minimal_ofd(); // 含 template（若 fixture 无 template，构造一个）
    let report = parse_ofd(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| matches!(w, OfdWarning::MissingFeature { feature, .. } if feature == "Template")));
}

#[test]
fn unknown_page_element_emits_skipped_object_warning() {
    // 构造一个 Page.xml 含未知元素 <ofd:UnknownObject/>
    // ... 构造 zip
    let report = parse_ofd(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| matches!(w, OfdWarning::SkippedObject { .. })), "unknown element -> SkippedObject");
}
```

（构造坏 ZIP/XML/未知元素夹具：用 `zip::ZipWriter` 手搓，参考 `fixtures.rs` 的 `build_minimal_ofd` 模式。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-io --test error_cases`
Expected: FAIL（SkippedObject 从不 emit；错误用例可能 pass 或 fail 取决于现有行为）。

- [ ] **Step 3: io parse emit warnings**

`crates/io/src/parse/page.rs`：在 `handle_element_start` 的 `_ => {}` catch-all 改为 emit SkippedObject：

```rust
_ => {
    // 未知元素 -> 跳过 + warning（不 fatal）
    // 需把 warnings 传入或用 channel；最简：返回 Vec<OfdWarning> 或 parse_page 接 &mut warnings
}
```

（`parse_page` 需接 `&mut Vec<OfdWarning>` 或返回 warnings；读现有 parse_page 签名适配。）

`crates/io/src/parse/mod.rs`：缺字体文件 -> `FontSubstituted`；缺图片 -> `ResourceNotFound`：

```rust
// 现有：if let Some(fe) = entries.iter().find(|x| x.name == path) { ... }
// 改：else { warnings.push(OfdWarning::FontSubstituted { requested: font_name, used: "default".into() }); }
// 图片同理：else { warnings.push(OfdWarning::ResourceNotFound { kind: ResourceKind::Image, id: id.0.clone() }); }
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-io`
Expected: PASS（error_cases + 既有 io 测试）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-io --all-targets -- -D warnings
cargo fmt -- crates/io/src/parse/page.rs crates/io/src/parse/mod.rs crates/io/tests/error_cases.rs
git add crates/io/src/parse/page.rs crates/io/src/parse/mod.rs crates/io/tests/error_cases.rs
git commit -m "feat(io): emit SkippedObject/FontSubstituted/ResourceNotFound warnings + error/warning test cases"
```

---

## Task 2: component -- ViewEvent 补全 (ScrollPage/ZoomAt/Ime)

**Files:**
- Modify: `crates/component/src/event.rs`（加 3 变体）
- Modify: `crates/component/src/editor_component.rs`（handle_event 路由）
- Test: inline

**Interfaces:**
- Consumes: 无
- Produces: `ViewEvent::ScrollPage/ZoomAt/Ime` + handle_event 路由。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn scroll_page_moves_by_page_height() {
        let mut c = component_with_note();
        c.viewport.size = (800.0, 600.0);
        let page_h = c.editor.document().pages[0].physical_box.h * c.viewport.zoom;
        let outcome = c.handle_event(&ViewEvent::ScrollPage { direction: ScrollDirection::Down });
        assert!(outcome.needs_repaint);
        assert!((c.viewport.scroll.1 - page_h - c.viewport.page_gap).abs() < 0.01, "scrolled down one page");
    }

    #[test]
    fn ime_inserts_text_at_cursor() {
        let mut c = component_with_note();
        // 选中 note（设 cursor）
        let id = /* 选中 note 的 id */;
        c.editor.set_cursor(id.clone(), 2);
        c.handle_event(&ViewEvent::Ime { text: "你好".into() });
        // 断言 note content 含 "你好"
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component scroll_page ime_inserts`
Expected: FAIL（ScrollPage/ZoomAt/Ime/ScrollDirection 未定义）。

- [ ] **Step 3: 加 ViewEvent 变体 + handle_event 路由**

`crates/component/src/event.rs`：

```rust
pub enum ScrollDirection { Up, Down }

pub enum ViewEvent {
    // ... 现有
    ScrollPage { direction: ScrollDirection },
    ZoomAt { factor: f64, center: (f64, f64) },
    Ime { text: String },
}
```

`crates/component/src/editor_component.rs` handle_event 加：

```rust
            ViewEvent::ScrollPage { direction } => {
                let page_h = self.editor.document().pages.first().map(|p| p.physical_box.h * self.viewport.zoom).unwrap_or(0.0);
                let delta = page_h + self.viewport.page_gap;
                self.viewport.scroll.1 += match direction { ScrollDirection::Down => delta, ScrollDirection::Up => -delta };
                self.maybe_fire_page_change();
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::ZoomAt { factor, center } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom *= factor;
                // 调整 scroll 使 center 点保持不动
                self.viewport.scroll.0 = center.0 - (center.0 - self.viewport.scroll.0) * (self.viewport.zoom / old_zoom);
                self.viewport.scroll.1 = center.1 - (center.1 - self.viewport.scroll.1) * (self.viewport.zoom / old_zoom);
                if (self.viewport.zoom - old_zoom).abs() > f64::EPSILON { self.fire_zoom_change(self.viewport.zoom); }
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::Ime { text } => {
                if let Some(cursor) = self.editor.text_cursor().cloned() {
                    let new_off = cursor.offset + text.chars().count();
                    self.editor.insert_text(&cursor.annotation, cursor.offset, &text);
                    self.editor.set_cursor(cursor.annotation.clone(), new_off);
                    self.after_annotation_change();
                    self.fire_cursor_change();
                    return EventOutcome { needs_repaint: true };
                }
                EventOutcome { needs_repaint: false }
            }
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: PASS。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/component/src/event.rs crates/component/src/editor_component.rs
git add crates/component/src/event.rs crates/component/src/editor_component.rs
git commit -m "feat(component): ViewEvent ScrollPage/ZoomAt/Ime + handle_event routing"
```

---

## Task 3: component -- on_warning 回调 + 适配器 wire

**Files:**
- Modify: `crates/component/src/callbacks.rs`（on_warning slot）
- Modify: `crates/component/src/editor_component.rs`（setter + fire_warnings）
- Modify: `crates/native-view/src/editor_app.rs`（load_ofd 后 fire）
- Modify: `crates/web-view/src/wasm_editor.rs`（load_ofd 后 fire）
- Test: inline

**Interfaces:**
- Consumes: `OfdWarning`（io），`LoadReport.warnings`。
- Produces: `on_warning` callback + `fire_warnings(&[OfdWarning])`；适配器 wire。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn on_warning_fires_with_load_warnings() {
        // 构造一个带 warning 的 .ofd（如 template）
        // load -> fire on_warning with the warnings
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_warning(move |_warnings| { *f.lock().unwrap() = true; });
        // 需经适配器层 load_ofd（component 不调 parse_ofd）
        // 或直接 fire_warnings 测：
        c.fire_warnings(&[OfdWarning::MissingFeature { feature: "test".into(), entry: "test".into() }]);
        assert!(*fired.lock().unwrap());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component on_warning`
Expected: FAIL（on_warning/fire_warnings 未定义）。

- [ ] **Step 3: 加 on_warning slot + setter + fire + 适配器 wire**

`callbacks.rs`：加 `on_warning: Option<Box<dyn Fn(&[OfdWarning]) + Send>>` slot。
`editor_component.rs`：加 `on_warning(cb)` setter（cfg-gated）+ `fire_warnings(&self, warnings: &[OfdWarning])`。
`editor_app.rs`（EditorApp）：`load_ofd` 后 `self.component.fire_warnings(&report.warnings);`（report.warnings 需从 parse_ofd 返回 -> load_ofd 接 bytes -> parse -> warnings）。
`wasm_editor.rs`：同样 `load_ofd` 后 fire。

注意：EditorApp.load_ofd 当前 `parse_ofd` 后丢弃 warnings（只取 document + package）。改：保留 warnings -> fire。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component -p rofd-native-view`
Expected: PASS。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component -p rofd-native-view -p rofd-web-view --all-targets -- -D warnings
cargo fmt -- crates/component/src/callbacks.rs crates/component/src/editor_component.rs crates/native-view/src/editor_app.rs crates/web-view/src/wasm_editor.rs
git add <上述文件>
git commit -m "feat(component): on_warning callback + adapters wire LoadReport.warnings"
```

---

## Task 4: C3 deferred Minors（page-stacking 重构 + resize guard + zoom_change 守卫）

**Files:**
- Modify: `crates/render/src/composite.rs`（提取 page_origin pub fn）
- Modify: `crates/render/src/hit_test.rs` + `caret_rect.rs`（复用 page_origin）
- Modify: `crates/component/src/editor_component.rs`（resize guard + zoom_change 守卫）
- Test: inline

**Interfaces:**
- Consumes: C3 代码。
- Produces: `page_origin` 共享 helper；resize guard；zoom_change change-check。

- [ ] **Step 1: 提取 page_origin helper**

`crates/render/src/composite.rs`（或新 `page_geom.rs`）：

```rust
/// 计算第 page_idx 页在 viewport 中的原点（page_x, page_y）。
/// 与 composite 的页堆叠一致：y = page_gap - scroll.1, 每页 += page_h + page_gap。
pub fn page_origin(doc: &OfdDocument, vp: &Viewport, page_idx: usize) -> Option<(f64, f64)> {
    let page = doc.pages.get(page_idx)?;
    let page_w = page.physical_box.w * vp.zoom;
    let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0) + vp.scroll.0;
    let mut y = vp.page_gap - vp.scroll.1;
    for i in 0..page_idx {
        let h = doc.pages.get(i)?.physical_box.h * vp.zoom;
        y += h + vp.page_gap;
    }
    Some((page_x, y))
}
```

替换 hit_test.rs / caret_rect.rs / composite.rs / annotation_viewport_rect / draw_drag_preview 中的页堆叠循环 -> 调 `page_origin`。component 的 `current_page_id`/`viewport_to_page_local`/`visible_page_index` 也可复用（需 render pub export）。

- [ ] **Step 2: resize guard**

`editor_component.rs` PointerDown Handle 分支：加检查若 payload 是 Markup/Freehand -> 不进入 Resize（no-op）：

```rust
rofd_render::HitTarget::Handle(id, h) => {
    let ann = self.editor.document().annotations.find(&id);
    if let Some(ann) = ann {
        if matches!(&ann.payload, AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. }) {
            // Markup/Freehand 不可缩放 -> 不进入 Resize，仅 select
            self.editor.select(id.clone());
            self.fire_selection_change();
            return EventOutcome { needs_repaint: true };
        }
        // ... 现有 Resize 逻辑
    }
}
```

- [ ] **Step 3: zoom_change 守卫**

`editor_component.rs` Zoom 分支：加 `if (new_zoom - old_zoom).abs() > f64::EPSILON` 才 fire（ZoomAt 已在 T2 加了；补 Zoom 分支）。

- [ ] **Step 4: 跑测试 + clippy + fmt + commit**

```bash
cargo test -p rofd-render -p rofd-component
cargo clippy -p rofd-render -p rofd-component --all-targets -- -D warnings
cargo fmt -- <改的文件>
git add <改的文件>
git commit -m "refactor: page_origin helper DRY + resize guard + zoom_change guard"
```

---

## Task 5: 全量绿 + V1 收尾

**Files:**
- Test: 全量

- [ ] **Step 1: 全量验证**

Run: `cargo test -p rofd-dom -p rofd-io -p rofd-render -p rofd-editor -p rofd-component -p rofd-native-view -p native-app`
Expected: PASS（per-crate）。

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check`
Expected: clean。

- [ ] **Step 2: 真实样本验证**

Run: `cargo test -p rofd-io --test sample_ofd -- --ignored` + `cargo test -p rofd-io --test real_sample -- --ignored` + `cargo test -p rofd-native-view --test c2_save -- --ignored`
Expected: PASS（sample.ofd + ru-yuan-ji-lu.ofd + app-layer surgical save）。

- [ ] **Step 3: V1 收尾 commit**

```bash
git add <任何测试文件>
git commit -m "test: C4 final green + V1 completion verification"
```

---

## Definition of Done

- ViewEvent: ScrollPage/ZoomAt/Ime 补全 + handle_event 路由。
- on_warning 回调 + 适配器 wire（LoadReport.warnings -> 宿主）。
- SkippedObject/FontSubstituted/ResourceNotFound 真正 emit + 错误/warning 用例测试。
- C3 deferred Minors: page_origin DRY + resize guard + zoom_change 守卫。
- 全量 cargo test（per-crate）绿 + clippy + fmt + wasm clean。
- 真实样本 #[ignore] 测试通过。
- **V1 收尾完成**（C1-C4）。
