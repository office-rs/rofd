# A1 核心层前置改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不触碰 winit 桥的前提下，把 `rofd-component` 改造为 masonry Widget 可直接驱动的形态：平台化构造入口、配置化初始缩放、钳制的乘法缩放、焦点门控的闪烁光标、IME preedit 状态机与光标处 overlay、脏缓存场景接口，以及 paste/copy 编辑接口。

**Architecture:** 纯核心层改造，全部修改落在 `crates/render` 与 `crates/component`（外加 `crates/web-view` 一个构造调用点跟随）。旧 winit 桥（`WinitEventBridge` + `EditorApp` + `NativeApp`）保持工作：`EditorApp` 的 `build_scene()` 包装器保留其名称，内部改调 `compose_scene()`。新能力（焦点/preedit/缓存）全部是新增内部状态与方法，不改变现有事件语义。核心类型仍叫 `EditorComponent`/`EditorConfig`（C 阶段才改名）；本计划不引入任何 Ofd* 新名。

**Tech Stack:** Rust 1.98.1、xilem/masonry rev `271a27a6…`（A0 已升级，本计划不消费其新 API——那是 A2 的事）、imaging crates.io 0.0.1、parley 0.8（经 render `FontStore` 整形）。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §2（rword 对齐与 zoom 差异）、§3（Widget/组件职责，核心侧方法清单）、§4（宿主 IO 边界，本计划只做组件侧）、§5（IME/preedit）、§6.2（组件相关测试）。

## Global Constraints

- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- **component 保持 io-free**：本计划不给 component 加 io 依赖、不加文件/对话框调用。
- **库内禁止挂钟时间**：不调 `Date::now()`/`SystemTime::now()`；blink 使用单调 `std::time::Instant` 是唯一允许的例外（动画计时器）。
- **body 只读**：preedit/paste 只改批注（TextBox），绝不碰 pages。
- 硬错 `OfdError`、降级 `OfdWarning`；本计划不新增错误路径，但不允许引入裸 `unwrap`（测试代码除外，沿用既有风格）。
- 代码注释/变量/文件名不得出现 "WPS" 字样。
- vello 0.8 / parley 0.8 / winit 0.30 / wgpu 28 依赖不动。

## File Structure

| 文件 | 动作 | 职责 |
| --- | --- | --- |
| `crates/render/src/viewport.rs` | 改 | `MIN_ZOOM`/`MAX_ZOOM` 常量 + 测试 |
| `crates/render/src/lib.rs` | 改 | 导出两个常量、`paint_caret` |
| `crates/render/src/caret_rect.rs` | 改 | 追加 `paint_caret()` 场景绘制 |
| `crates/component/src/config.rs` | 改 | `EditorConfig.zoom` + `with_zoom()` + 单测 |
| `crates/component/src/event.rs` | 改 | `Ime` → `ImePreedit` + `ImeCommit` |
| `crates/component/src/preedit.rs` | 新建 | `PreeditState`（组件自持的合成态） |
| `crates/component/src/preedit_overlay.rs` | 新建 | 光标处简化 overlay 绘制（含 TextBox 裁剪） |
| `crates/component/src/editor_component.rs` | 改 | 全部新字段/新方法/事件臂/脏缓存 |
| `crates/component/src/lib.rs` | 改 | 模块注册 + 公共类型再导出 |
| `crates/component/tests/integration.rs` | 改 | `::new` → `::new_native` |
| `crates/native-view/src/editor_app.rs` | 改 | 构造跟随 `new_native` |
| `crates/native-view/tests/sample_drag_select.rs` | 改 | 构造跟随 `new_native` |
| `crates/web-view/src/wasm_editor.rs` | 改 | 构造按 target 分流 `new_native`/`new_wasm` |

---

### Task 1: render 新增缩放边界常量

**Files:**
- Modify: `crates/render/src/viewport.rs:17`（`PX_PER_MM` 之后追加）
- Modify: `crates/render/src/lib.rs:45`
- Test: `crates/render/src/viewport.rs`（`clamp_tests` 内）

**Interfaces:**
- Consumes: 无。
- Produces: `rofd_render::MIN_ZOOM: f64`（= `PX_PER_MM * 0.25`）、`rofd_render::MAX_ZOOM: f64`（= `PX_PER_MM * 3.0`）。Task 2 起的所有缩放路径都钳制到此区间。

- [ ] **Step 1: 写失败测试**

在 `crates/render/src/viewport.rs` 的 `mod clamp_tests` 末尾（L161 `}` 之前）追加：

```rust
    #[test]
    fn zoom_bounds_framed_around_baseline() {
        assert_eq!(MIN_ZOOM, PX_PER_MM * 0.25);
        assert_eq!(MAX_ZOOM, PX_PER_MM * 3.0);
        assert!(MIN_ZOOM < PX_PER_MM);
        assert!(MAX_ZOOM > PX_PER_MM);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-render zoom_bounds_framed_around_baseline`

Expected: 编译失败 `cannot find value MIN_ZOOM in this scope`。

- [ ] **Step 3: 实现常量**

在 `crates/render/src/viewport.rs` L17（`pub const PX_PER_MM ...` 行）之后追加：

```rust

/// Minimum permitted viewport zoom: 25% of the 96-DPI baseline.
/// Multiplicative zoom always builds on [`PX_PER_MM`], never on 1.0.
pub const MIN_ZOOM: f64 = PX_PER_MM * 0.25;

/// Maximum permitted viewport zoom: 300% of the 96-DPI baseline.
pub const MAX_ZOOM: f64 = PX_PER_MM * 3.0;
```

- [ ] **Step 4: 导出常量**

把 `crates/render/src/lib.rs:45` 的：

```rust
pub use viewport::{clamp_scroll, Viewport, PX_PER_MM};
```

替换为：

```rust
pub use viewport::{clamp_scroll, Viewport, MAX_ZOOM, MIN_ZOOM, PX_PER_MM};
```

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p rofd-render`

Expected: 全部 `test result: ok`。

- [ ] **Step 6: 门禁与提交**

```bash
cargo clippy -p rofd-render --all-targets -- -D warnings
cargo fmt -p rofd-render -- --check
git add crates/render/src/viewport.rs crates/render/src/lib.rs
git commit -m "feat(render): 新增 MIN_ZOOM/MAX_ZOOM 缩放边界常量"
```

---

### Task 2: Zoom/ZoomAt 统一钳制到边界

**Files:**
- Modify: `crates/component/src/editor_component.rs`（rofd_render import 行；Zoom 臂 L1392-1394；ZoomAt 臂 L1436-1438）
- Test: 同文件内联测试模块

**Interfaces:**
- Consumes: Task 1 的 `MIN_ZOOM`/`MAX_ZOOM`。
- Produces: 不变（`Viewport.zoom` 事后保证落在 `[MIN_ZOOM, MAX_ZOOM]`）。

- [ ] **Step 1: 写失败测试**

在 `crates/component/src/editor_component.rs` 内联测试模块（任意 `#[test]` 旁，建议放在现有 zoom 相关测试附近）追加：

```rust
    #[test]
    fn zoom_arm_clamps_to_min_and_max() {
        let mut c = component_with_note();
        c.viewport.zoom = MAX_ZOOM;
        c.handle_event(&ViewEvent::Zoom { factor: 1.1 });
        assert_eq!(c.viewport.zoom, MAX_ZOOM);
        c.viewport.zoom = MIN_ZOOM;
        c.handle_event(&ViewEvent::Zoom { factor: 0.9 });
        assert_eq!(c.viewport.zoom, MIN_ZOOM);
    }

    #[test]
    fn zoomat_arm_clamps_to_bounds() {
        let mut c = component_with_note();
        c.viewport.zoom = MAX_ZOOM;
        c.handle_event(&ViewEvent::ZoomAt {
            factor: 1.1,
            center: (0.0, 0.0),
        });
        assert_eq!(c.viewport.zoom, MAX_ZOOM);
        c.viewport.zoom = MIN_ZOOM;
        c.handle_event(&ViewEvent::ZoomAt {
            factor: 0.9,
            center: (0.0, 0.0),
        });
        assert_eq!(c.viewport.zoom, MIN_ZOOM);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rofd-component zoom_arm_clamps_to_min_and_max`

Expected: FAIL——`assertion left: 12.47… right: 11.33…`（MAX_ZOOM 再乘 1.1 越界）。

- [ ] **Step 3: 跟随导入常量**

把 editor_component.rs 第 1 行的：

```rust
use rofd_render::{DragPreview, FontStore, HandlePos, RenderEngine, Scene, Viewport, PX_PER_MM};
```

替换为：

```rust
use rofd_render::{
    DragPreview, FontStore, HandlePos, RenderEngine, Scene, Viewport, MAX_ZOOM, MIN_ZOOM,
    PX_PER_MM,
};
```

- [ ] **Step 4: 钳制 Zoom 臂**

把 Zoom 臂（L1392-1394）的：

```rust
            ViewEvent::Zoom { factor } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom *= factor;
```

替换为：

```rust
            ViewEvent::Zoom { factor } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom = (self.viewport.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
```

- [ ] **Step 5: 钳制 ZoomAt 臂**

把 ZoomAt 臂（L1436-1438）的：

```rust
            ViewEvent::ZoomAt { factor, center } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom *= factor;
```

替换为：

```rust
            ViewEvent::ZoomAt { factor, center } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom = (self.viewport.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
```

（后续 `ratio = self.viewport.zoom / old_zoom` 自动基于钳后值；钳满时 ratio=1，scroll 不动。）

- [ ] **Step 6: 运行确认通过并提交**

```bash
cargo test -p rofd-component
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -p rofd-component
git add crates/component/src/editor_component.rs
git commit -m "fix(component): Zoom/ZoomAt 缩放统一钳制到 [MIN_ZOOM, MAX_ZOOM]"
```

---

### Task 3: EditorConfig 支持初始缩放

**Files:**
- Modify: `crates/component/src/config.rs`（全文重写）
- Modify: `crates/component/src/editor_component.rs:208-217`（`new()` 读取 config.zoom）
- Test: `config.rs` 新增单测模块；editor_component 新增一个内联测试

**Interfaces:**
- Consumes: Task 1 的 `PX_PER_MM`/`MIN_ZOOM`/`MAX_ZOOM`。
- Produces: `EditorConfig.zoom: f64`（默认 `PX_PER_MM`）、`EditorConfig::with_zoom(f64) -> Self`（钳制）。

- [ ] **Step 1: 重写 config.rs（含测试）**

`crates/component/src/config.rs` 全文替换为：

```rust
use rofd_render::{PX_PER_MM, MAX_ZOOM, MIN_ZOOM};
use std::sync::Arc;

#[derive(Clone)]
pub struct EditorConfig {
    pub default_font_bytes: Arc<Vec<u8>>,
    pub page_gap: f64,
    /// Initial viewport zoom (viewport px per OFD mm). Defaults to
    /// [`PX_PER_MM`]; clamped into the supported range.
    pub zoom: f64,
}

impl EditorConfig {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self {
            default_font_bytes,
            page_gap: 20.0,
            zoom: PX_PER_MM,
        }
    }

    /// Set the initial viewport zoom (builder chainer). Clamped to
    /// `[MIN_ZOOM, MAX_ZOOM]`.
    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_zoom_to_baseline() {
        let c = EditorConfig::new(Arc::new(vec![]));
        assert_eq!(c.zoom, PX_PER_MM);
        assert_eq!(c.page_gap, 20.0);
    }

    #[test]
    fn with_zoom_sets_and_clamps() {
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(PX_PER_MM * 2.0);
        assert_eq!(c.zoom, PX_PER_MM * 2.0);
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(f64::MAX);
        assert_eq!(c.zoom, MAX_ZOOM);
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(0.0);
        assert_eq!(c.zoom, MIN_ZOOM);
    }
}
```

- [ ] **Step 2: new() 读取 config.zoom**

把 editor_component.rs L208-217 的：

```rust
    pub fn new(config: EditorConfig) -> Self {
        let page_gap = config.page_gap;
        Self {
            editor: Editor::new(),
            render: RenderEngine::new(config.default_font_bytes.clone()),
            viewport: Viewport {
                zoom: PX_PER_MM,
                page_gap,
                ..Default::default()
            },
```

替换为：

```rust
    pub fn new(config: EditorConfig) -> Self {
        let page_gap = config.page_gap;
        let zoom = config.zoom;
        Self {
            editor: Editor::new(),
            render: RenderEngine::new(config.default_font_bytes.clone()),
            viewport: Viewport {
                zoom,
                page_gap,
                ..Default::default()
            },
```

- [ ] **Step 3: 加内联测试**

在内联测试模块 `new_constructs_with_defaults` 之后追加：

```rust
    #[test]
    fn new_honors_config_zoom() {
        let c = EditorComponent::new(
            EditorConfig::new(Arc::new(vec![])).with_zoom(PX_PER_MM * 1.5),
        );
        assert_eq!(c.viewport.zoom, PX_PER_MM * 1.5);
    }
```

- [ ] **Step 4: 运行确认通过并提交**

```bash
cargo test -p rofd-component
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -p rofd-component
git add crates/component/src/config.rs crates/component/src/editor_component.rs
git commit -m "feat(component): EditorConfig 支持 zoom 初始缩放配置"
```

---

### Task 4: IME 事件拆分 + insert_at_cursor 共享助手

**Files:**
- Modify: `crates/component/src/event.rs:100-104`（Ime 变体替换 + 两个构造测试）
- Modify: `crates/component/src/editor_component.rs`（新增 `component_with_textbox()` fixture、`insert_at_cursor()`、ImeCommit 臂、3 个测试）
- Test: 同上

**Interfaces:**
- Consumes: 无。
- Produces:
  - `ViewEvent::ImePreedit { text: String, caret: Option<(usize, usize)> }`（本任务无状态机臂，落 catch-all 为 no-op，Task 8 接管）。
  - `ViewEvent::ImeCommit { text: String }`。
  - 组件方法 `fn insert_at_cursor(&mut self, text: &str) -> bool`（Task 8/10 复用）。
  - 测试 fixture `fn component_with_textbox() -> EditorComponent`（Task 6/7/8/10 复用）：单页 200×200，一个 TextBox（rect 0,0,120,40，content "hi"），cursor 在 offset 2，viewport size 200×200、zoom 为默认 `PX_PER_MM`。

- [ ] **Step 1: 写 event.rs 失败测试**

在 `crates/component/src/event.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn ime_preedit_constructs() {
        let e = ViewEvent::ImePreedit {
            text: "ni".into(),
            caret: Some((1, 2)),
        };
        assert!(matches!(e, ViewEvent::ImePreedit { .. }));
    }

    #[test]
    fn ime_commit_constructs() {
        let e = ViewEvent::ImeCommit { text: "你".into() };
        assert!(matches!(e, ViewEvent::ImeCommit { .. }));
    }
```

Run: `cargo test -p rofd-component --lib event::tests`

Expected: 编译失败 `no variant ImePreedit`。

- [ ] **Step 2: 替换事件变体**

把 event.rs L100-104 的：

```rust
    /// IME composition commit: insert `text` at the text cursor (multi-char).
    /// Falls through as a no-op when no text cursor is set.
    Ime {
        text: String,
    },
```

替换为：

```rust
    /// IME preedit update: composition text plus the preedit caret range
    /// (byte offsets into `text`). Empty `text` cancels the composition
    /// without committing. The state machine lands in Task 8; until then
    /// the event is accepted as a no-op.
    ImePreedit {
        text: String,
        caret: Option<(usize, usize)>,
    },
    /// IME composition commit: insert `text` at the text cursor.
    /// No-op when no text cursor is set.
    ImeCommit {
        text: String,
    },
```

Run: `cargo test -p rofd-component --lib event::tests` → PASS。

- [ ] **Step 3: 写组件失败测试 + fixture**

在内联测试模块 `component_with_note()`（L2573-2575）之后，先插入 fixture 与辅助函数：

```rust
    /// Single page 200x200 holding one TextBox (rect 0,0,120,40, content
    /// "hi"), cursor parked at offset 2. Viewport size 200x200; zoom is the
    /// default PX_PER_MM baseline.
    fn component_with_textbox() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![Layer::default()],
            template: None,
        });
        c.load_document(doc);
        let id = c.editor.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 120.0,
                    h: 40.0,
                },
                content: "hi".into(),
                font: FontId::new("F1"),
                size: 10.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
            },
        );
        c.editor.set_cursor(id, 2);
        c.viewport.size = (200.0, 200.0);
        c
    }

    fn textbox_content(c: &EditorComponent) -> String {
        let ann = c
            .document()
            .annotations
            .for_page(&PageId::new("P0"))
            .first()
            .expect("one annotation");
        match &ann.payload {
            AnnotationPayload::TextBox { content, .. } => content.clone(),
            other => panic!("expected textbox, got {other:?}"),
        }
    }
```

再在任意测试旁追加：

```rust
    #[test]
    fn ime_commit_inserts_at_cursor() {
        let mut c = component_with_textbox();
        let outcome = c.handle_event(&ViewEvent::ImeCommit { text: "ab".into() });
        assert!(outcome.needs_repaint);
        assert_eq!(textbox_content(&c), "hiab");
    }

    #[test]
    fn ime_commit_without_cursor_is_noop() {
        let mut c = component_with_textbox();
        c.editor.clear_cursor();
        let outcome = c.handle_event(&ViewEvent::ImeCommit { text: "ab".into() });
        assert!(!outcome.needs_repaint);
        assert_eq!(textbox_content(&c), "hi");
    }
```

Run: `cargo test -p rofd-component ime_commit` → 编译失败 `no variant ImeCommit`? 实际是缺组件方法臂：catch-all 存在所以编译通过、测试 FAIL（content 仍为 "hi"，outcome false）。

- [ ] **Step 4: 实现 insert_at_cursor**

在 `handle_event` 定义（L837）之前插入：

```rust
    /// Insert committed text at the current text cursor, then advance the
    /// cursor and broadcast the change. Shared by `ViewEvent::ImeCommit`
    /// and paste. Returns `false` (no repaint) when no cursor is set.
    fn insert_at_cursor(&mut self, text: &str) -> bool {
        if let Some(cursor) = self.editor.text_cursor().cloned() {
            let new_off = cursor.offset + text.chars().count();
            self.editor
                .insert_text(&cursor.annotation, cursor.offset, text);
            self.editor.set_cursor(cursor.annotation.clone(), new_off);
            self.after_annotation_change();
            self.fire_cursor_change();
            true
        } else {
            false
        }
    }

```

- [ ] **Step 5: 替换 Ime 臂**

把旧 Ime 臂（L1458-1473）整体替换为：

```rust
            ViewEvent::ImeCommit { text } => EventOutcome {
                needs_repaint: self.insert_at_cursor(text),
            },
```

- [ ] **Step 6: 运行确认通过并提交**

```bash
cargo test -p rofd-component
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -p rofd-component
git add crates/component/src/event.rs crates/component/src/editor_component.rs
git commit -m "refactor(component): IME 事件拆分为 ImePreedit/ImeCommit，抽取 insert_at_cursor"
```

---

### Task 5: new 私有化，新增平台构造入口

**Files:**
- Modify: `crates/component/src/editor_component.rs:208`（可见性 + 新增两个构造器）
- Modify: `crates/component/tests/integration.rs:20,77`
- Modify: `crates/native-view/src/editor_app.rs:19`
- Modify: `crates/native-view/tests/sample_drag_select.rs:17`
- Modify: `crates/web-view/src/wasm_editor.rs:637-638`

**Interfaces:**
- Consumes: 无。
- Produces:
  - `#[cfg(not(target_arch = "wasm32"))] EditorComponent::new_native(EditorConfig) -> EditorComponent`
  - `#[cfg(target_arch = "wasm32")] EditorComponent::new_wasm(EditorConfig) -> EditorComponent`
  - `EditorComponent::new` 降为私有（同 crate 内联测试继续使用）。

- [ ] **Step 1: 新增平台构造器**

把 editor_component.rs L208 的：

```rust
    pub fn new(config: EditorConfig) -> Self {
```

替换为：

```rust
    /// Construct a component for a native (desktop, non-WASM) host.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_native(config: EditorConfig) -> Self {
        Self::new(config)
    }

    /// Construct a component for a WASM host.
    #[cfg(target_arch = "wasm32")]
    pub fn new_wasm(config: EditorConfig) -> Self {
        Self::new(config)
    }

    fn new(config: EditorConfig) -> Self {
```

- [ ] **Step 2: 迁移 component 外部集成测试**

把 `crates/component/tests/integration.rs` 中两处：

```rust
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
```

替换为：

```rust
    let mut c = EditorComponent::new_native(EditorConfig::new(Arc::new(vec![])));
```

（两处文本相同，用 replace_all。）

- [ ] **Step 3: 迁移 native-view 两个调用点**

把 `crates/native-view/src/editor_app.rs:19` 的：

```rust
        let mut component = EditorComponent::new(config);
```

替换为：

```rust
        let mut component = EditorComponent::new_native(config);
```

把 `crates/native-view/tests/sample_drag_select.rs:17` 的：

```rust
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
```

替换为：

```rust
    let mut c = EditorComponent::new_native(EditorConfig::new(Arc::new(vec![])));
```

- [ ] **Step 4: 迁移 web-view 构造点（按 target 分流）**

把 `crates/web-view/src/wasm_editor.rs:637-638` 的：

```rust
            let config = EditorConfig::new(std::sync::Arc::new(vec![]));
            let mut component = EditorComponent::new(config);
```

替换为：

```rust
            let config = EditorConfig::new(std::sync::Arc::new(vec![]));
            #[cfg(target_arch = "wasm32")]
            let mut component = EditorComponent::new_wasm(config);
            #[cfg(not(target_arch = "wasm32"))]
            let mut component = EditorComponent::new_native(config);
```

- [ ] **Step 5: 双目标编译确认**

Run:

```bash
cargo build --workspace
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

Expected: 两个目标均 `Finished`，零错误。若 native 构建报 `associated function new is private`，说明还有遗漏的外部调用点（用 `EditorComponent::new(` 全仓搜索定位）。

- [ ] **Step 6: 测试与提交**

```bash
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/editor_component.rs crates/component/tests/integration.rs \
  crates/native-view/src/editor_app.rs crates/native-view/tests/sample_drag_select.rs \
  crates/web-view/src/wasm_editor.rs
git commit -m "refactor(component): new 私有化，新增 new_native/new_wasm 平台构造入口"
```

---

### Task 6: 场景脏缓存（compose_scene/update_scene/scene/set_viewport_size）

**Files:**
- Modify: `crates/component/src/editor_component.rs`（struct 字段、new() 初始化、`build_scene` 改名、render、handle_event 尾部、set_text_selection/register_font_data/load_document/new_document/set_tool 置脏、内联测试调用点改名、3 个新测试）

**Interfaces:**
- Consumes: 无。
- Produces:
  - `EditorComponent::update_scene(&mut self)`：脏时重组。
  - `EditorComponent::scene(&self) -> &Scene`：最近缓存。
  - `EditorComponent::set_viewport_size(&mut self, width: f64, height: f64)`。
  - `fn compose_scene(&mut self) -> Scene`（私有，旧 build_scene 函数体）。
  - `#[doc(hidden)] pub fn build_scene(&mut self) -> Scene` 临时公开垫片（A3 迁移完 sample_drag_select 后删除）。
  - `fn mark_scene_dirty(&mut self)`（pub(crate) 同模块；Task 7/8/9 复用）。

- [ ] **Step 1: 加 struct 字段**

在 editor_component.rs struct 的 `squiggly_color: Color,`（L204）之后、`}`（L205）之前追加：

```rust
    /// Cached composed scene. Rebuilt lazily by `update_scene`.
    pub(crate) scene_cache: Scene,
    /// Whether `scene_cache` is stale (the next `update_scene` recomposes).
    pub(crate) scene_dirty: bool,
```

- [ ] **Step 2: new() 初始化**

在 new() 的 `squiggly_color: DEFAULT_MARKUP_COLOR,`（L232）之后、`}`（L233）之前追加：

```rust
            scene_cache: Scene::default(),
            scene_dirty: true,
```

- [ ] **Step 3: build_scene 改名 compose_scene**

把 L793 的：

```rust
    pub fn build_scene(&mut self) -> Scene {
```

替换为：

```rust
    fn compose_scene(&mut self) -> Scene {
```

并在其文档注释（L786-792）之后、函数之前补一个临时垫片（紧跟 `compose_scene` 的结束 `}` 之后也可以，统一放在 `render()` 之前）。把 L832-835 的：

```rust
    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        let scene = self.build_scene();
        target.draw_scene(&scene);
    }
```

替换为：

```rust
    /// Temporary public alias; removed once external tests migrate in A3.
    #[doc(hidden)]
    pub fn build_scene(&mut self) -> Scene {
        self.compose_scene()
    }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        self.update_scene();
        target.draw_scene(&self.scene_cache);
    }

    /// Mark the cached scene stale. The next `update_scene` recomposes.
    pub(crate) fn mark_scene_dirty(&mut self) {
        self.scene_dirty = true;
    }

    /// Recompose the cached scene when stale. Native paint and wasm
    /// conversion call this before reading `scene()`.
    pub fn update_scene(&mut self) {
        if !self.scene_dirty {
            return;
        }
        self.scene_cache = self.compose_scene();
        self.scene_dirty = false;
    }

    /// The most recently composed scene. Call `update_scene` first.
    pub fn scene(&self) -> &Scene {
        &self.scene_cache
    }

    /// Update the viewport (content region) size. Called by the widget layout.
    pub fn set_viewport_size(&mut self, width: f64, height: f64) {
        if (self.viewport.size.0 - width).abs() < f64::EPSILON
            && (self.viewport.size.1 - height).abs() < f64::EPSILON
        {
            return;
        }
        self.viewport.size = (width, height);
        self.viewport.scroll =
            rofd_render::clamp_scroll(self.editor.document(), &self.viewport);
        self.maybe_fire_page_change();
        self.mark_scene_dirty();
    }
```

- [ ] **Step 4: handle_event 尾部统一置脏**

把 L837-839 的：

```rust
    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::{MouseButton, ScrollDirection, ViewEvent};
        match event {
```

替换为：

```rust
    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::{MouseButton, ScrollDirection, ViewEvent};
        let outcome = match event {
```

把事件 match 尾部（L1474-1479）的：

```rust
            ViewEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
            _ => EventOutcome {
                needs_repaint: false,
            },
        }
    }
```

替换为：

```rust
            ViewEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
            _ => EventOutcome {
                needs_repaint: false,
            },
        };
        if outcome.needs_repaint {
            self.mark_scene_dirty();
        }
        outcome
    }
```

- [ ] **Step 5: 其余置脏点**

(a) `set_text_selection`（L323-331）——把：

```rust
        self.text_selection = sel;
        if let Some(cb) = &self.callbacks.on_text_selection_change {
```

替换为：

```rust
        self.text_selection = sel;
        self.mark_scene_dirty();
        if let Some(cb) = &self.callbacks.on_text_selection_change {
```

(b) `register_font_data`（L255-264）——整体替换为：

```rust
    pub fn register_font_data(&mut self, bytes: Vec<u8>) -> bool {
        let bytes = Arc::new(bytes);
        self.registered_font_bytes.push(bytes.clone());
        let ok = if let Some(store) = self.font_store.as_mut() {
            store.register_font(bytes)
        } else {
            // Will be registered when the FontStore is built.
            true
        };
        self.mark_scene_dirty();
        ok
    }
```

(c) `load_document`——在 L283 `self.current_page = None;` 之后、函数结束 `}` 之前追加：

```rust
        self.mark_scene_dirty();
```

(d) `new_document`——在 L297 `self.current_page = None;` 之后、`}` 之前追加：

```rust
        self.mark_scene_dirty();
```

(e) `set_tool`（L483-490）——把：

```rust
        self.set_text_selection(None);
        self.set_tool_pointer_cursor();
    }
```

替换为：

```rust
        self.set_text_selection(None);
        self.set_tool_pointer_cursor();
        self.mark_scene_dirty();
    }
```

- [ ] **Step 6: 内联测试调用点改名**

在内联测试模块中把所有 `c.build_scene()` 调用改为 `c.compose_scene()`（当前位于约 L2767、L6343、L6355；逐个确认：只有直接对 `EditorComponent` 的调用改；经 `EditorApp` 的调用不受影响）。

- [ ] **Step 7: 写缓存行为测试**

在内联测试模块追加：

```rust
    #[test]
    fn update_scene_reuses_cache_while_clean() {
        let mut c = component_with_note();
        c.update_scene();
        let first = c.scene().commands().len();
        c.update_scene();
        assert_eq!(c.scene().commands().len(), first);
        c.mark_scene_dirty();
        c.update_scene();
        assert_eq!(c.scene().commands().len(), first);
    }

    #[test]
    fn event_marks_scene_dirty_on_repaint() {
        let mut c = component_with_note();
        c.update_scene();
        assert!(!c.scene_dirty);
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 10.0 });
        assert!(c.scene_dirty);
    }

    #[test]
    fn set_viewport_size_updates_and_marks_dirty() {
        let mut c = component_with_textbox();
        c.update_scene();
        assert!(!c.scene_dirty);
        c.set_viewport_size(100.0, 50.0);
        assert_eq!(c.viewport.size, (100.0, 50.0));
        assert!(c.scene_dirty);
        c.update_scene();
        assert!(c.scene().commands().len() > 0);
    }
```

- [ ] **Step 8: 全量验证并提交**

```bash
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/editor_component.rs
git commit -m "refactor(component): 场景改为脏缓存 compose_scene/update_scene/scene"
```

---

### Task 7: 焦点门控的闪烁光标

**Files:**
- Modify: `crates/component/src/editor_component.rs`（3 个新字段、new() 初始化、focus 臂、blink 方法、caret_rect 方法、compose_scene 插入光标绘制、6 个测试）
- Modify: `crates/render/src/caret_rect.rs`（追加 `paint_caret`）
- Modify: `crates/render/src/lib.rs`（导出）

**Interfaces:**
- Consumes: Task 6 的 `mark_scene_dirty()`；render 既有 `rofd_render::caret_rect(...)`。
- Produces:
  - 字段（pub(crate)）：`focused: bool`（出生 false）、`cursor_visible: bool`（出生 true）、`#[cfg(not(target_arch="wasm32"))] blink_deadline: Option<Instant>`、`#[cfg(target_arch="wasm32")] blink_elapsed_ms: u32`。
  - `fn reset_blink(&mut self)`（pub(crate)；Task 8 复用）。
  - `#[cfg(not(target_arch="wasm32"))] pub fn tick_blink(&mut self) -> bool`。
  - `pub fn is_focused(&self) -> bool`。
  - `pub fn caret_rect(&mut self) -> Option<rofd_dom::Rect>`。
  - `rofd_render::paint_caret(&mut Scene, &rofd_dom::Rect)`。

- [ ] **Step 1: 写 render 侧 paint_caret**

在 `crates/render/src/caret_rect.rs` 顶部 import 区加入（与现有 use 合并风格）：

```rust
use imaging::kurbo::{Rect as KurboRect, Shape};
use imaging::{Painter, record::Scene};
```

（若该文件已有同名项的部分导入，合并而非重复。）

文件末尾追加：

```rust
/// Paint the text caret: a vertical bar filled black at `rect` (viewport
/// coordinates). Called by the component after body, annotations and
/// scrollbars composite.
pub fn paint_caret(scene: &mut Scene, rect: &rofd_dom::Rect) {
    let rect = KurboRect::new(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let bez = rect.to_path(0.1);
    let mut painter = Painter::new(scene);
    painter
        .fill(&bez, peniko::Color::from_rgb8(0, 0, 0))
        .draw();
}
```

把 `crates/render/src/lib.rs:30` 的：

```rust
pub use caret_rect::caret_rect;
```

替换为：

```rust
pub use caret_rect::{caret_rect, paint_caret};
```

Run: `cargo build -p rofd-render` → `Finished`（dead-code 暂不出现：同一提交内组件马上消费）。

- [ ] **Step 2: 加组件字段与初始化**

在 struct 的 `scene_dirty: bool,`（Task 6 新增）之后追加：

```rust
    /// Whether the component currently holds effective keyboard focus
    /// (widget focus AND window focus, combined by the adapter). The caret
    /// paints and blinks only while true. Starts false: a freshly opened
    /// document shows no caret until the user clicks (mirrors rword).
    pub(crate) focused: bool,
    /// Current caret visibility phase, toggled by blink.
    pub(crate) cursor_visible: bool,
    /// Monotonic deadline of the next blink toggle (native only; Instant is
    /// an animation timer, allowed by AGENTS §4.4).
    #[cfg(not(target_arch = "wasm32"))]
    blink_deadline: Option<std::time::Instant>,
```

在 new() 的 `scene_dirty: true,` 之后追加：

```rust
            focused: false,
            cursor_visible: true,
            #[cfg(not(target_arch = "wasm32"))]
            blink_deadline: None,
```

- [ ] **Step 3: 写失败测试**

在内联测试模块追加：

```rust
    #[test]
    fn focus_arms_gate_caret_lifecycle() {
        let mut c = component_with_textbox();
        assert!(!c.is_focused());
        c.handle_event(&ViewEvent::FocusGained);
        assert!(c.is_focused() && c.cursor_visible);
        c.handle_event(&ViewEvent::FocusLost);
        assert!(!c.is_focused() && !c.cursor_visible);
    }

    #[test]
    fn caret_rect_none_without_cursor() {
        let mut c = component_with_textbox();
        c.editor.clear_cursor();
        assert!(c.caret_rect().is_none());
    }

    #[test]
    fn caret_rect_tracks_scroll_and_zoom() {
        let mut c = component_with_textbox();
        let r0 = c.caret_rect().expect("caret");
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 20.0 });
        let r1 = c.caret_rect().expect("caret after scroll");
        assert!((r1.y - (r0.y + 20.0)).abs() < 1e-6);
        c.handle_event(&ViewEvent::ZoomAt {
            factor: 2.0,
            center: (0.0, 0.0),
        });
        let r2 = c.caret_rect().expect("caret after zoom");
        assert!((r2.w - r1.w * 2.0).abs() < 1e-6);
    }

    #[test]
    fn caret_paints_only_while_focused() {
        let mut c = component_with_textbox();
        let idle = c.compose_scene().commands().len();
        c.handle_event(&ViewEvent::FocusGained);
        let focused = c.compose_scene().commands().len();
        assert!(focused > idle, "caret adds paint commands on focus");
        c.handle_event(&ViewEvent::FocusLost);
        let blurred = c.compose_scene().commands().len();
        assert_eq!(blurred, idle);
    }

    #[test]
    fn reset_blink_shows_caret() {
        let mut c = component_with_textbox();
        c.cursor_visible = false;
        c.reset_blink();
        assert!(c.cursor_visible);
    }

    #[test]
    fn tick_blink_without_deadline_is_quiet() {
        let mut c = component_with_textbox();
        assert!(!c.tick_blink());
    }
```

Run: `cargo test -p rofd-component caret` → 编译失败（缺方法/臂）。

- [ ] **Step 4: 实现 focus 臂与 blink 方法**

在 handle_event 的 match 中（建议放在 Resize 臂之后、ScrollPage 之前）插入：

```rust
            ViewEvent::FocusGained => {
                self.focused = true;
                self.reset_blink();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::FocusLost => {
                self.focused = false;
                self.cursor_visible = false;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.blink_deadline = None;
                }
                EventOutcome {
                    needs_repaint: true,
                }
            }
```

在 `reset_blink` 的调用生效前，把方法定义放在 `mark_scene_dirty` 附近：

```rust
    /// One caret-blink phase.
    #[cfg(not(target_arch = "wasm32"))]
    const BLINK_PHASE: std::time::Duration = std::time::Duration::from_millis(500);

    /// Restart the blink cycle: caret visible, deadline pushed out. Called
    /// on focus gain and (later) every caret-moving edit/click.
    pub(crate) fn reset_blink(&mut self) {
        self.cursor_visible = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.blink_deadline = Some(std::time::Instant::now() + Self::BLINK_PHASE);
        }
        self.mark_scene_dirty();
    }

    /// Advance the blink phase from the native anim frame. Returns true
    /// when the caret visibility flipped (repaint needed).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn tick_blink(&mut self) -> bool {
        let Some(deadline) = self.blink_deadline else {
            return false;
        };
        if std::time::Instant::now() < deadline {
            return false;
        }
        self.cursor_visible = !self.cursor_visible;
        self.blink_deadline = Some(std::time::Instant::now() + Self::BLINK_PHASE);
        self.mark_scene_dirty();
        true
    }

    /// Whether the component currently holds effective focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }
```

- [ ] **Step 5: 实现 caret_rect 组件方法**

在 `insert_at_cursor` 附近插入：

```rust
    /// Viewport-space caret rectangle (px) for the current text cursor.
    /// Lazily builds the font store. `None` when no cursor is set or the
    /// geometry cannot resolve.
    pub fn caret_rect(&mut self) -> Option<rofd_dom::Rect> {
        if self.font_store.is_none() {
            self.font_store = Some(self.build_font_store());
        }
        let cursor = self.editor.text_cursor()?;
        rofd_render::caret_rect(
            self.editor.document(),
            &self.viewport,
            self.font_store
                .as_ref()
                .expect("font_store initialized"),
            &cursor.annotation,
            cursor.offset,
        )
    }
```

- [ ] **Step 6: compose_scene 中绘制光标**

在 compose_scene（旧 build_scene）函数体内，`paint_scrollbars(...)` 调用块结束之后、tooltip 块（`// Hover tooltip paints last`）之前插入：

```rust
        // Text caret: effective focus only, blink-gated.
        if self.focused && self.cursor_visible {
            if let Some(cursor) = self.editor.text_cursor().cloned() {
                if let Some(rect) = rofd_render::caret_rect(
                    self.editor.document(),
                    &self.viewport,
                    fonts,
                    &cursor.annotation,
                    cursor.offset,
                ) {
                    rofd_render::paint_caret(&mut scene, &rect);
                }
            }
        }
```

- [ ] **Step 7: wasm 侧 update_scene 帧计数 blink（对齐 rword）**

Step 2 的 struct 字段块中，在 `blink_deadline` 字段之后再加一个 wasm-only 字段：

```rust
    #[cfg(target_arch = "wasm32")]
    blink_elapsed_ms: u32,
```

并在 new() 的初始化区对应加入：

```rust
            #[cfg(target_arch = "wasm32")]
            blink_elapsed_ms: 0,
```

Step 4 的 `reset_blink` 实现替换为（加 wasm 计数复位；native 行不变）：

```rust
    pub(crate) fn reset_blink(&mut self) {
        self.cursor_visible = true;
        #[cfg(target_arch = "wasm32")]
        {
            self.blink_elapsed_ms = 0;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.blink_deadline = Some(std::time::Instant::now() + Self::BLINK_PHASE);
        }
        self.mark_scene_dirty();
    }
```

Step 4 的 FocusLost 臂中，在 native `blink_deadline = None` 块之后加：

```rust
                #[cfg(target_arch = "wasm32")]
                {
                    self.blink_elapsed_ms = 0;
                }
```

然后把 Task 6 创建的 `update_scene`：

```rust
    pub fn update_scene(&mut self) {
        if !self.scene_dirty {
            return;
        }
        self.scene_cache = self.compose_scene();
        self.scene_dirty = false;
    }
```

替换为：

```rust
    pub fn update_scene(&mut self) {
        // Blink timing (wasm only): the host's requestAnimationFrame loop
        // calls update_scene at ~60 Hz, so advance by 16ms per call; a
        // completed phase flips caret visibility and marks the scene dirty.
        // Native leaves blink to tick_blink() driven by anim frames.
        #[cfg(target_arch = "wasm32")]
        {
            const BLINK_INTERVAL_MS: u32 = 500;
            const FRAME_MS: u32 = 16;
            if self.focused {
                self.blink_elapsed_ms += FRAME_MS;
                if self.blink_elapsed_ms >= BLINK_INTERVAL_MS {
                    self.blink_elapsed_ms = 0;
                    self.cursor_visible = !self.cursor_visible;
                    self.scene_dirty = true;
                }
            }
        }
        if !self.scene_dirty {
            return;
        }
        self.scene_cache = self.compose_scene();
        self.scene_dirty = false;
    }
```

说明：该 wasm 路径由 Task 10 的 `cargo check --target wasm32` 编译验证（wasm 无测试运行器，不另加运行时测试）。

- [ ] **Step 8: 运行确认通过并提交**

```bash
cargo test -p rofd-render -p rofd-component
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/caret_rect.rs crates/render/src/lib.rs \
  crates/component/src/editor_component.rs
git commit -m "feat(component): 焦点门控的闪烁光标（focus 臂 + native/wasm blink + caret 绘制）"
```

---

### Task 8: IME preedit 状态机

**Files:**
- Create: `crates/component/src/preedit.rs`
- Modify: `crates/component/src/lib.rs`（注册模块）
- Modify: `crates/component/src/editor_component.rs`（字段+初始化、ImePreedit 臂、commit 路径、force-commit 预检查、load/new 清理、FocusLost 联动、9 个测试）

**Interfaces:**
- Consumes: Task 4 的 `insert_at_cursor`；Task 7 的 `reset_blink`（提交后光标可见性复位）；Task 6 的 `mark_scene_dirty`。
- Produces:
  - `preedit::PreeditState { text: String, caret: Option<(usize,usize)>, annotation: AnnotationId, offset: usize }`（pub(crate)）。
  - 组件字段 `preedit: Option<PreeditState>`（pub(crate)；Task 9 消费）。
  - `fn apply_preedit(&mut self, text: &str, caret: Option<(usize,usize)>) -> bool`。
  - `fn force_commit_preedit(&mut self) -> bool`。
  - `fn commit_text(&mut self, text: &str) -> bool`。

- [ ] **Step 1: 新建 preedit.rs 并注册模块**

`crates/component/src/preedit.rs` 全文：

```rust
//! IME preedit state: composition text lives here (not in the dom) until
//! the IME commits. The component owns it independently of any platform
//! widget, so both adapters drive the same state machine.

use rofd_dom::AnnotationId;

/// Active IME composition: text plus the preedit caret range (byte offsets
/// into `text`) and the annotation/offset the composition started at.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreeditState {
    pub text: String,
    pub caret: Option<(usize, usize)>,
    pub annotation: AnnotationId,
    pub offset: usize,
}
```

在 `crates/component/src/lib.rs:5`（`pub mod event;` 之后）插入：

```rust
pub mod preedit;
```

- [ ] **Step 2: 加字段与初始化**

在 struct 的 `blink_deadline` 字段（Task 7）之后追加：

```rust
    /// Active IME preedit, if composing. Text enters the dom only on commit.
    pub(crate) preedit: Option<crate::preedit::PreeditState>,
```

在 new() 的 `blink_deadline: None,` 之后追加：

```rust
            preedit: None,
```

- [ ] **Step 3: 写失败测试**

在内联测试模块追加：

```rust
    #[test]
    fn preedit_starts_updates_and_cancels() {
        let mut c = component_with_textbox();
        assert!(c
            .handle_event(&ViewEvent::ImePreedit {
                text: "n".into(),
                caret: None,
            })
            .needs_repaint);
        assert_eq!(c.preedit.as_ref().unwrap().text, "n");
        assert!(c
            .handle_event(&ViewEvent::ImePreedit {
                text: "ni".into(),
                caret: Some((2, 2)),
            })
            .needs_repaint);
        assert_eq!(c.preedit.as_ref().unwrap().text, "ni");
        assert_eq!(c.preedit.as_ref().unwrap().caret, Some((2, 2)));
        // Empty text cancels.
        assert!(c
            .handle_event(&ViewEvent::ImePreedit {
                text: "".into(),
                caret: None,
            })
            .needs_repaint);
        assert!(c.preedit.is_none());
    }

    #[test]
    fn preedit_text_stays_outside_document_until_commit() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        assert_eq!(textbox_content(&c), "hi");
        c.handle_event(&ViewEvent::ImeCommit {
            text: "你好".into(),
        });
        assert_eq!(textbox_content(&c), "hi你好");
    }

    #[test]
    fn ime_commit_without_preedit_inserts_event_text() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImeCommit { text: "ab".into() });
        assert_eq!(textbox_content(&c), "hiab");
    }

    #[test]
    fn ime_commit_while_preedit_commits_preedit_text() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你".into(),
            caret: None,
        });
        // Event payload differs from preedit; preedit is the source.
        c.handle_event(&ViewEvent::ImeCommit { text: "x".into() });
        assert_eq!(textbox_content(&c), "hi你");
    }

    #[test]
    fn pointer_down_force_commits_before_press() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 90.0,
            y: 5.0,
            modifiers: Modifiers::default(),
            click_count: 1,
        });
        // The composition is committed before the press is dispatched.
        assert_eq!(textbox_content(&c), "hi你好");
        assert!(c.preedit.is_none());
    }

    #[test]
    fn keydown_force_commits_before_key() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
        });
        assert_eq!(textbox_content(&c), "hi你好");
        assert!(c.preedit.is_none());
    }

    #[test]
    fn focus_lost_commits_preedit() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::FocusGained);
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        c.handle_event(&ViewEvent::FocusLost);
        assert_eq!(textbox_content(&c), "hi你好");
        assert!(c.preedit.is_none());
    }

    #[test]
    fn load_document_discards_preedit_without_commit() {
        let mut c = component_with_textbox();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        c.load_document(OfdDocument::default());
        assert!(c.preedit.is_none());
    }

    #[test]
    fn preedit_ignored_without_cursor_anchor() {
        let mut c = component_with_textbox();
        c.editor.clear_cursor();
        let outcome = c.handle_event(&ViewEvent::ImePreedit {
            text: "n".into(),
            caret: None,
        });
        assert!(!outcome.needs_repaint);
        assert!(c.preedit.is_none());
    }
```

Run: `cargo test -p rofd-component preedit` → 编译失败/测试失败（无臂无方法）。

- [ ] **Step 4: 实现状态机方法**

在 `caret_rect()` 方法之后插入：

```rust
    /// Apply an IME preedit update. Empty `text` cancels; otherwise starts
    /// (at the current cursor) or replaces the active composition. Returns
    /// true when the preedit state changed (repaint needed).
    fn apply_preedit(&mut self, text: &str, caret: Option<(usize, usize)>) -> bool {
        if text.is_empty() {
            if self.preedit.take().is_some() {
                self.mark_scene_dirty();
                return true;
            }
            return false;
        }
        if let Some(state) = self.preedit.as_mut() {
            let changed = state.text != text || state.caret != caret;
            if changed {
                state.text = text.to_string();
                state.caret = caret;
                self.mark_scene_dirty();
            }
            return changed;
        }
        if let Some(cursor) = self.editor.text_cursor().cloned() {
            self.preedit = Some(crate::preedit::PreeditState {
                text: text.to_string(),
                caret,
                annotation: cursor.annotation,
                offset: cursor.offset,
            });
            self.mark_scene_dirty();
            true
        } else {
            false
        }
    }

    /// Force-commit the active preedit (focus loss / click elsewhere /
    /// navigation): insert its text at the composition origin and clear it.
    /// Returns true when something was committed.
    fn force_commit_preedit(&mut self) -> bool {
        let Some(state) = self.preedit.take() else {
            return false;
        };
        let new_off = state.offset + state.text.chars().count();
        self.editor
            .insert_text(&state.annotation, state.offset, &state.text);
        self.editor.set_cursor(state.annotation, new_off);
        self.after_annotation_change();
        self.reset_blink();
        self.fire_cursor_change();
        true
    }

    /// Commit path: an active preedit is the source of truth; otherwise the
    /// event's own text is inserted directly.
    fn commit_text(&mut self, text: &str) -> bool {
        if self.preedit.is_some() {
            self.force_commit_preedit()
        } else {
            self.insert_at_cursor(text)
        }
    }
```

- [ ] **Step 5: 接上事件臂**

把 Task 4 的 ImeCommit 臂：

```rust
            ViewEvent::ImeCommit { text } => EventOutcome {
                needs_repaint: self.insert_at_cursor(text),
            },
```

替换为：

```rust
            ViewEvent::ImePreedit { text, caret } => EventOutcome {
                needs_repaint: self.apply_preedit(text, *caret),
            },
            ViewEvent::ImeCommit { text } => EventOutcome {
                needs_repaint: self.commit_text(text),
            },
```

- [ ] **Step 6: 强制提交预检查**

在 handle_event 的 `use crate::event::{...};` 之后、`let outcome = match event {` 之前插入：

```rust
        // An in-progress composition must be committed before the next
        // press/key is dispatched, so the gesture acts on the committed
        // document.
        if matches!(event, ViewEvent::PointerDown { .. } | ViewEvent::KeyDown { .. })
            && self.preedit.is_some()
        {
            self.force_commit_preedit();
        }
```

- [ ] **Step 7: FocusLost 与 load/new 清理**

把 Task 7 的 FocusLost 臂开头：

```rust
            ViewEvent::FocusLost => {
                self.focused = false;
```

替换为：

```rust
            ViewEvent::FocusLost => {
                // Defensive: a composition stranded by focus loss must not
                // be silently dropped.
                self.force_commit_preedit();
                self.focused = false;
```

在 `load_document` 和 `new_document` 中，分别在 `self.drag = None;` 之后追加：

```rust
        self.preedit = None;
```

（两处各一行；文档已整体替换，preedit 不提交、不保留。）

- [ ] **Step 8: 运行确认通过并提交**

```bash
cargo test -p rofd-component
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/component/src/preedit.rs crates/component/src/lib.rs \
  crates/component/src/editor_component.rs
git commit -m "feat(component): IME preedit 状态机（强制提交/取消/导航守卫）"
```

---

### Task 9: preedit 光标处 overlay 渲染

**Files:**
- Modify: `crates/render/src/annotation_scene.rs`（`shape_positioned`/`draw_glyph_run` 升为 `pub(crate)`）
- Create: `crates/component/src/preedit_overlay.rs`
- Modify: `crates/component/src/lib.rs`（注册模块）
- Modify: `crates/component/src/editor_component.rs`（compose_scene 末尾插入 overlay 调用、2 个测试）

**Interfaces:**
- Consumes:
  - `crate::annotation_scene::{shape_positioned, draw_glyph_run}`（render 内可见；本任务把两个助手提权，签名不变）：
    ```rust
    pub(crate) fn shape_positioned(
        content: &str, font_id: &FontId, size: f64, fonts: &FontStore,
    ) -> (Option<FontData>, Vec<Glyph>);
    pub(crate) fn draw_glyph_run(
        painter: &mut Painter<Scene>, font: &FontData, glyphs: &[Glyph],
        affine: Affine, brush: peniko::Color, size: f64,
    );
    ```
  - render 既有 `page_origin(&OfdDocument, &Viewport, usize) -> Option<(f64,f64)>`；`imaging::ClipRef::fill(Rect)` + `Painter::with_clip(clip, |p| ...)`。
- Produces: `preedit_overlay::paint_preedit_overlay(scene, doc, viewport, fonts, &PreeditState)`（pub(crate)）。

- [ ] **Step 1: render 助手提权**

在 `crates/render/src/annotation_scene.rs` 中把：

```rust
fn shape_positioned(
```

改为：

```rust
pub(crate) fn shape_positioned(
```

把：

```rust
fn draw_glyph_run(
```

改为：

```rust
pub(crate) fn draw_glyph_run(
```

（其余签名与函数体不动。）

- [ ] **Step 2: 新建 preedit_overlay.rs**

`crates/component/src/preedit_overlay.rs` 全文：

```rust
//! Simplified preedit overlay: paint the composition string at the caret.
//! No ghost-document reflow (intentional rword difference) - rofd has fixed
//! pages with no reflow, and the text enters the dom only on commit. The run
//! is shaped with parley via the render `FontStore` and clipped to the
//! TextBox boundary.

use imaging::kurbo::{Affine, Rect as KurboRect};
use imaging::record::Scene;
use imaging::{ClipRef, Painter};
use rofd_dom::AnnotationPayload;

use crate::preedit::PreeditState;

/// Paint the preedit overlay when the composition targets a TextBox.
pub(crate) fn paint_preedit_overlay(
    scene: &mut Scene,
    doc: &rofd_dom::OfdDocument,
    viewport: &rofd_render::Viewport,
    fonts: &rofd_render::FontStore,
    state: &PreeditState,
) {
    let Some(ann) = doc.annotations.find(&state.annotation) else {
        return;
    };
    let (rect, font_id, size, dom_color) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            font,
            size,
            color,
            ..
        } => (rect, font, *size, *color),
        _ => return,
    };
    // Locate the owning page to resolve page origin (TextBox geometry is in
    // page-local millimetres).
    let Some(page_idx) = doc.pages.iter().position(|p| ann.page == p.id) else {
        return;
    };
    let Some(origin) = rofd_render::page_origin(doc, viewport, page_idx) else {
        return;
    };
    let base =
        Affine::translate(imaging::kurbo::Vec2::new(origin.0, origin.1))
            * Affine::scale(viewport.zoom);
    // Viewport-space fill-clip rect of the TextBox.
    let clip = KurboRect::new(
        origin.0 + rect.x * viewport.zoom,
        origin.1 + rect.y * viewport.zoom,
        origin.0 + (rect.x + rect.w) * viewport.zoom,
        origin.1 + (rect.y + rect.h) * viewport.zoom,
    );
    let (font_data, glyphs) =
        rofd_render::annotation_scene::shape_positioned(&state.text, font_id, size, fonts);
    let Some(font_data) = font_data else {
        return;
    };
    let brush = match dom_color {
        rofd_dom::Color::Rgb(r, g, b) => peniko::Color::from_rgb8(r, g, b),
    };
    let mut painter = Painter::new(scene);
    painter.with_clip(ClipRef::fill(clip), |p| {
        rofd_render::annotation_scene::draw_glyph_run(
            p,
            &font_data,
            &glyphs,
            base,
            brush,
            size,
        );
    });
}
```

- [ ] **Step 3: 注册模块**

在 `crates/component/src/lib.rs` 的 `pub mod preedit;` 之后插入：

```rust
pub mod preedit_overlay;
```

- [ ] **Step 4: 写失败测试**

在内联测试模块追加：

```rust
    #[test]
    fn preedit_overlay_adds_commands_then_removes_on_commit() {
        let mut c = component_with_textbox();
        let idle = c.compose_scene().commands().len();
        c.handle_event(&ViewEvent::ImePreedit {
            text: "你好".into(),
            caret: None,
        });
        let composing = c.compose_scene().commands().len();
        assert!(composing > idle, "overlay paints during composition");
        c.handle_event(&ViewEvent::ImeCommit {
            text: "你好".into(),
        });
        let committed = c.compose_scene().commands().len();
        // Overlay gone (committed text now draws as a normal annotation:
        // it may add glyph commands, but fewer than overlay+old content).
        assert!(committed < composing);
    }

    #[test]
    fn preedit_overlay_without_target_is_skipped() {
        let mut c = component_with_note();
        // Force a preedit that targets the Note (overlay supports TextBox
        // only): composition must not panic and draws nothing.
        c.preedit = Some(crate::preedit::PreeditState {
            text: "x".into(),
            caret: None,
            annotation: c
                .document()
                .annotations
                .for_page(&PageId::new("P0"))
                .first()
                .unwrap()
                .id
                .clone(),
            offset: 0,
        });
        let commands = c.compose_scene().commands().len();
        c.update_scene();
        assert_eq!(c.scene().commands().len(), commands);
    }
```

Run: `cargo test -p rofd-component preedit_overlay` → 编译失败（模块/函数缺失）。

- [ ] **Step 5: compose_scene 中调用 overlay**

在 compose_scene 函数体内，Task 7 插入的 caret 绘制块之后、tooltip 块之前插入：

```rust
        // Preedit overlay: composition text not yet in the dom.
        if let Some(state) = self.preedit.as_ref() {
            crate::preedit_overlay::paint_preedit_overlay(
                &mut scene,
                self.editor.document(),
                &self.viewport,
                fonts,
                state,
            );
        }
```

- [ ] **Step 6: 运行确认通过并提交**

```bash
cargo test -p rofd-component
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/render/src/annotation_scene.rs crates/component/src/preedit_overlay.rs \
  crates/component/src/lib.rs crates/component/src/editor_component.rs
git commit -m "feat(component): preedit 光标处简化 overlay（parley 整形 + TextBox 裁剪）"
```

---

### Task 10: paste/copy 编辑接口与公共类型导出，A1 总门禁

**Files:**
- Modify: `crates/component/src/editor_component.rs`（`paste_text`、`copy_selection`，Ctrl+C 臂改用 copy_selection、2 个测试）
- Modify: `crates/component/src/lib.rs`（再导出 widget 需要的公共类型）

**Interfaces:**
- Consumes: 既有 `selected_text()`。
- Produces:
  - `pub fn paste_text(&mut self, text: &str) -> bool`。
  - `pub fn copy_selection(&mut self) -> Option<String>`。
  - 公共再导出：`AnnotationId`、`OfdWarning`、`Rect`（来自 rofd_dom），`AnnotationSelection`、`TextCursor`（来自 rofd_editor）。A2 的 OfdWidgetAction/IME 逻辑直接命名这些类型。

- [ ] **Step 1: 写失败测试**

在内联测试模块追加：

```rust
    #[test]
    fn paste_text_inserts_and_reports_false_without_cursor() {
        let mut c = component_with_textbox();
        assert!(c.paste_text("ab"));
        assert_eq!(textbox_content(&c), "hiab");
        let mut c = component_with_textbox();
        c.editor.clear_cursor();
        assert!(!c.paste_text("ab"));
        assert!(!c.paste_text(""));
    }

    #[test]
    fn copy_selection_returns_body_text_in_text_tool() {
        // Body selection requires a drag over body text, which is covered
        // by the migrated sample_drag_select in A3; here we only assert the
        // no-selection / wrong-tool contract.
        let mut c = component_with_textbox();
        assert!(c.copy_selection().is_none());
    }
```

Run: `cargo test -p rofd-component paste_text` → 编译失败（方法缺失）。

- [ ] **Step 2: 实现方法**

在 `caret_rect()` 附近插入：

```rust
    /// Paste text at the current text cursor (Ctrl+V path: the widget
    /// supplies the platform clipboard string). Returns false when no cursor
    /// is set or the text is empty.
    pub fn paste_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.insert_at_cursor(text)
    }

    /// Copy the current selection to a string (body text in the Text tool).
    /// The widget puts it on the platform clipboard; the component never
    /// touches the clipboard itself.
    pub fn copy_selection(&mut self) -> Option<String> {
        if !matches!(self.tool, Tool::Text) {
            return None;
        }
        self.selected_text()
    }
```

- [ ] **Step 3: Ctrl+C 臂改用 copy_selection**

把 handle_key 中 Ctrl+C 块（L1722-1729）的：

```rust
        if modifiers.control && !modifiers.shift && matches!(key, Key::Char('c') | Key::Char('C')) {
            if matches!(self.tool, Tool::Text) {
                if let Some(text) = self.selected_text() {
                    if let Some(cb) = &self.callbacks.on_copy {
                        cb(text);
                    }
                }
            }
```

替换为：

```rust
        if modifiers.control && !modifiers.shift && matches!(key, Key::Char('c') | Key::Char('C')) {
            if let Some(text) = self.copy_selection() {
                if let Some(cb) = &self.callbacks.on_copy {
                    cb(text);
                }
            }
```

- [ ] **Step 4: 补齐公共导出**

把 `crates/component/src/lib.rs` 末尾 L17-20 的：

```rust
// Re-exported so adapters can name the body-text selection type (public API
// surface of `text_selection()`/`on_text_selection_change`) without taking a
// direct rofd-render dependency.
pub use rofd_render::BodyTextSelection;
```

替换为：

```rust
// Re-exported so adapters can name the types in widget actions / callbacks
// without taking direct rofd-render / rofd-dom / rofd-editor dependencies.
pub use rofd_dom::{AnnotationId, OfdWarning, Rect};
pub use rofd_editor::{AnnotationSelection, TextCursor};
pub use rofd_render::BodyTextSelection;
```

- [ ] **Step 5: A1 总门禁**

Run:

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

Expected: 全部通过；wasm 目标下 `new_wasm`/preedit 状态机编译通过、无 Instant 泄漏到 wasm（blink 方法整体 `#[cfg(not(target_arch="wasm32"))]`）。

- [ ] **Step 6: 提交**

```bash
git add crates/component/src/editor_component.rs crates/component/src/lib.rs
git commit -m "feat(component): paste_text/copy_selection 编辑接口，补齐适配器公共类型导出"
```

---

## A1 完成判据（供 A2 开工前核对）

1. `EditorComponent` 可经 `new_native` 构造，`update_scene()`/`scene()` 出 Scene，`set_viewport_size` 驱动尺寸。
2. `FocusGained`/`FocusLost` 切换 `focused`；caret 仅在 focused 且 cursor_visible 时入场景；`tick_blink()` 可由动画帧驱动。
3. `ImePreedit` 文本不进 dom；`ImeCommit`/focus-loss/点击/导航键五条路径均能正确提交或忽略；overlay 在光标处整形绘制并裁剪到 TextBox。
4. `paste_text`/`copy_selection` 就位；widget 所需公共类型（含 12 个 OfdWidgetAction 载荷类型）均可从 rofd-component 顶层命名。
5. 旧 winit 桥仍可编译运行（`cargo run -p native-app`），手术刀字节保留测试未受影响。
