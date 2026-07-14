# Cluster 3: 交互式批注 UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 批注的创建（WPS 6 拖拽绘制）、选择、移动、缩放（8 手柄）、删除、右键菜单、5 回调；让"批注编辑器"主循环可用。

**Architecture:** render `composite` 接 selection+drag_preview 画手柄+预览；`hit_test` 加 Handle 变体；component 持 `tool: Tool` + `drag: Option<DragState>` 状态机（PointerDown/Move/Up 路由 create/move/resize）；apps 工具栏（6 工具+选择）拖拽创建 + 右键 Delete；补 5 回调。

**Tech Stack:** Rust 2021, imaging/kurbo (render), xilem/masonry (native-app), wasm-bindgen (web-view). C1+C2+C1.5 就绪（io/dom/render 模型完整）。

**Spec:** [`docs/superpowers/specs/2026-07-14-c3-interactive-annotation-ux-design.md`](../specs/2026-07-14-c3-interactive-annotation-ux-design.md)

## Global Constraints

- **render 用 imaging Painter API**（AGENTS.md §4.5），不直接构造 vello::Scene。
- **依赖严格向上**（AGENTS.md §4.1）：component 不依赖 io；render 依赖 dom。
- **错误显式分层**（AGENTS.md §4.6）：创建/移动/缩放在空选区/无效 page 上 no-op，不 panic；无裸 unwrap。
- **commits**：conventional commits，无 attribution 行。单 main 分支。
- **TDD**：先红后绿，每任务 commit。
- **fmt**：baseline clean。用 `cargo fmt -- <files>`（勿 `-p`/`--all`）；stage ONLY 改的文件（`git add <path>`），勿 `git add -A`（test/ + docs PDF untracked）。

---

## File Structure

| 文件 | 责任 | 任务 |
|---|---|---|
| `crates/render/src/hit_test.rs` | HandlePos enum + HitTarget::Handle + hit_test 接 selection | T1 |
| `crates/render/src/composite.rs` | DragPreview enum + composite 接 selection+drag + 画手柄 | T1 |
| `crates/component/src/editor_component.rs` | Tool + DragState + set_tool + handle_event 状态机 + build_scene 传 selection+drag + 5 回调 | T2/T3/T4 |
| `crates/component/src/callbacks.rs` | 5 新回调 slot | T4 |
| `examples/native-app/src/main.rs` | 工具栏 6 工具+选择 + 拖拽创建 + 右键 Delete | T5 |
| `examples/web-app/src/main.ts` | 工具栏 + 拖拽创建 + 右键 Delete | T6 |
| `crates/web-view/src/wasm_editor.rs` | setTool + 右键桥 | T6 |

---

## Task 1: render -- 手柄 + composite 接 selection + handle 命中

**Files:**
- Modify: `crates/render/src/hit_test.rs`
- Modify: `crates/render/src/composite.rs`
- Test: inline

**Interfaces:**
- Consumes: `AnnotationSelection`（dom）, `Rect`/`Point`（dom）。
- Produces: `HandlePos` enum; `HitTarget::Handle(AnnotationId, HandlePos)`; `DragPreview` enum; `composite(doc, vp, fonts, selection, drag)`; `hit_test(doc, vp, selection, point)`。

- [ ] **Step 1: 写失败测试**

`crates/render/src/hit_test.rs` 测试模块加：

```rust
    #[test]
    fn hit_test_handle_when_point_on_selected_corner() {
        // 选中一个 rect 批注，点在其 NW 角手柄上 -> Handle(id, Nw)
        // （构造 doc + viewport + selection + point 在角手柄范围内）
        // ... 用现有 hit_test 测试 helper 风格
    }
```

`crates/render/src/composite.rs` 测试模块加：

```rust
    #[test]
    fn composite_with_selection_draws_handles() {
        // 选中一个批注 -> composite 产出含 8 个手柄填充块 + 1 个选择框的 Scene
        // （场景结构断言：手柄图元数 == 8）
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-render hit_test_handle composite_with_selection`
Expected: FAIL（HandlePos/Handle/DragPreview/composite 新签名未定义）。

- [ ] **Step 3: 加 HandlePos + HitTarget::Handle + hit_test 接 selection**

`crates/render/src/hit_test.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlePos { Nw, Ne, Sw, Se, N, S, E, W }

pub enum HitTarget {
    Annotation(AnnotationId),
    AnnotationText(AnnotationId, usize),
    Handle(AnnotationId, HandlePos),       // 新
    Page(PageId),
    Empty,
}

const HANDLE_SIZE: f64 = 8.0;  // 屏幕像素，不随 zoom
const HIT_PAD: f64 = 4.0;

pub fn hit_test(
    doc: &OfdDocument, vp: &Viewport, selection: &AnnotationSelection, point: (f64, f64),
) -> HitTarget {
    // 1. 先查选中批注的手柄（若 point 在某选中批注的 8 把柄之一内 -> Handle）
    if let AnnotationSelection::Single(id) = selection {
        if let Some(ann) = doc.annotations.find(id) {
            let rect = annotation_viewport_rect(ann, vp);  // 批注在 viewport 的 bounding rect
            if let Some(h) = hit_handle(rect, point) {
                return HitTarget::Handle(id.clone(), h);
            }
        }
    }
    // 2. 现有批注/Page/Empty 查询（不变）
    // ... 现有逻辑
}

fn hit_handle(rect: Rect, point: (f64, f64)) -> Option<HandlePos> {
    // 8 把柄位置（角+边中点），检查 point 是否在某把柄的 HANDLE_SIZE/2+HIT_PAD 范围内
    // ... 返回命中的 HandlePos
}
```

（`annotation_viewport_rect`：把批注的 payload rect 经 viewport 变换到屏幕坐标。读现有 composite 的 rect 变换逻辑复用。）

- [ ] **Step 4: 加 DragPreview + composite 接 selection+drag + 画手柄**

`crates/render/src/composite.rs`：

```rust
pub enum DragPreview {
    Create { kind: AnnotationKind, rect: Rect },
    CreateFreehand { path: Vec<(f64, f64)> },
    Move { id: AnnotationId, rect: Rect },
    Resize { id: AnnotationId, rect: Rect },
}

pub fn composite(
    &self, doc: &OfdDocument, vp: &Viewport, fonts: &FontStore,
    selection: &AnnotationSelection, drag: Option<&DragPreview>,
) -> Scene {
    // 现有 body + annotation 绘制（不变）
    // + 选中批注画选择框（描边 bounding rect）+ 8 把柄（fill 小方块）
    // + drag=Some 时画预览（半透明）
}
```

（手柄用 `Painter::fill` 小方块，固定 HANDLE_SIZE 屏幕像素；选择框用 `Painter::stroke`。读现有 composite 的 Painter 用法。）

- [ ] **Step 5: 更新 composite 调用点（component build_scene）**

`crates/component/src/editor_component.rs` 的 `build_scene`：`composite(doc, vp, fonts)` -> `composite(doc, vp, fonts, &self.editor.selection(), None)`（drag 暂传 None，T2/T3 填）。**临时**：为了让 workspace 编译，T1 先传 `&AnnotationSelection::None, None`（T2 改成真实 selection）。

- [ ] **Step 6: 跑测试确认绿**

Run: `cargo test -p rofd-render && cargo check --workspace`
Expected: render 测试 PASS（含 2 新）；workspace 编译（composite 新签名，所有调用点更新）。

- [ ] **Step 7: clippy + fmt + commit**

```bash
cargo clippy -p rofd-render -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/render/src/hit_test.rs crates/render/src/composite.rs crates/component/src/editor_component.rs
git add crates/render/src/hit_test.rs crates/render/src/composite.rs crates/component/src/editor_component.rs
git commit -m "feat(render): Handle hit-test + composite takes selection/drag for handles"
```

---

## Task 2: component -- Tool + DragState + set_tool + build_scene 传 selection

**Files:**
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: T1（composite 接 selection+drag）。
- Produces: `Tool` enum; `EditorComponent.tool` + `drag` 字段; `set_tool(&mut self, Tool)`; `build_scene` 传真实 selection + drag_preview。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn set_tool_changes_tool_state() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        assert!(matches!(c.tool, Tool::Select));
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        assert!(matches!(c.tool, Tool::Create(_)));
        c.set_tool(Tool::Select);
        assert!(matches!(c.tool, Tool::Select));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component set_tool`
Expected: FAIL（`Tool`/`tool`/`set_tool` 未定义）。

- [ ] **Step 3: 加 Tool + DragState + tool/drag 字段 + set_tool**

`crates/component/src/editor_component.rs`：

```rust
use rofd_render::{DragPreview, HandlePos};

pub enum Tool { Select, Create(rofd_dom::AnnotationKind) }

enum DragState {
    Create { kind: rofd_dom::AnnotationKind, start: (f64, f64), current: (f64, f64), path: Vec<(f64, f64)> },
    Move { id: rofd_dom::AnnotationId, last: (f64, f64) },
    Resize { id: rofd_dom::AnnotationId, handle: HandlePos, anchor: (f64, f64), orig: rofd_dom::Rect },
}

pub struct EditorComponent {
    // ... 现有字段
    pub(crate) tool: Tool,
    pub(crate) drag: Option<DragState>,
}

// new() 加：tool: Tool::Select, drag: None,

pub fn set_tool(&mut self, tool: Tool) {
    self.tool = tool;
    self.drag = None;  // 切工具清拖拽
}
```

- [ ] **Step 4: build_scene 传真实 selection + drag_preview**

```rust
    pub fn build_scene(&mut self) -> Scene {
        // ... 现有 font_store 逻辑
        let drag_preview = self.drag.as_ref().and_then(|d| drag_to_preview(d));
        self.render.composite(
            self.editor.document(), &self.viewport, fonts,
            self.editor.selection(), drag_preview.as_ref(),
        )
    }
```

加 `drag_to_preview`：把 `DragState` 映射成 `DragPreview`（Create{kind,rect=bbox(start,current)} / CreateFreehand{path} / Move{id,rect} / Resize{id,rect}）。

- [ ] **Step 5: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: PASS（set_tool 测试 + 既有）。

- [ ] **Step 6: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/component/src/editor_component.rs
git add crates/component/src/editor_component.rs
git commit -m "feat(component): Tool + DragState + set_tool; build_scene passes selection+drag"
```

---

## Task 3: component -- 拖拽状态机（PointerDown/Move/Up create/move/resize）

**Files:**
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: T1（hit_test Handle）, T2（Tool/DragState）。
- Produces: `handle_event` 实现 PointerDown/Move/Up（create/move/resize）；`compute_resize` helper。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn create_rect_via_drag() {
        let mut c = component_with_note();  // 有页+批注的 helper
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 10.0, y: 10.0, modifiers: Modifiers::default() });
        c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 60.0, modifiers: Modifiers::default() });
        let outcome = c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 50.0, y: 60.0, modifiers: Modifiers::default() });
        assert!(outcome.needs_repaint);
        assert!(matches!(c.editor.selection(), AnnotationSelection::Single(_)), "new rect selected");
        assert!(matches!(c.tool, Tool::Select), "tool back to Select after create");
    }

    #[test]
    fn move_annotation_via_drag() {
        let mut c = component_with_note();
        // 先选中（PointerDown on annotation）
        c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Left, x: 5.0, y: 5.0, modifiers: Modifiers::default() });
        // 拖拽移动
        c.handle_event(&ViewEvent::PointerMove { x: 15.0, y: 15.0, modifiers: Modifiers::default() });
        c.handle_event(&ViewEvent::PointerUp { button: MouseButton::Left, x: 15.0, y: 15.0, modifiers: Modifiers::default() });
        // 批注 rect 应平移了（dx=10, dy=10 相对 last）
        // ... 断言批注 rect 变化
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component create_rect move_annotation`
Expected: FAIL（PointerMove/Up 被丢弃，无创建/移动）。

- [ ] **Step 3: 实现 handle_event PointerDown/Move/Up**

`crates/component/src/editor_component.rs` 的 `handle_event`，扩 PointerDown + 加 PointerMove/PointerUp：

```rust
    pub fn handle_event(&mut self, event: &ViewEvent) -> EventOutcome {
        use crate::event::ViewEvent;
        match event {
            ViewEvent::PointerDown { button: MouseButton::Left, x, y, .. } => {
                let p = (*x, *y);
                match &self.tool {
                    Tool::Create(kind) => {
                        self.drag = Some(DragState::Create { kind: kind.clone(), start: p, current: p, path: vec![p] });
                    }
                    Tool::Select => {
                        let target = rofd_render::hit_test(self.editor.document(), &self.viewport, self.editor.selection(), p);
                        match target {
                            rofd_render::HitTarget::Handle(id, h) => {
                                let was_selected = self.editor.selection().contains(&id);
                                if !was_selected { self.editor.select(id.clone()); }
                                let ann = self.editor.document().annotations.find(&id).expect("selected ann");
                                let orig = annotation_payload_rect(ann);
                                let anchor = opposite_corner(&orig, &h);  // 对角点
                                self.drag = Some(DragState::Resize { id: id.clone(), handle: h, anchor, orig });
                                self.fire_selection_change();
                            }
                            rofd_render::HitTarget::Annotation(id) => {
                                let was_selected = self.editor.selection().contains(&id);
                                self.editor.select(id.clone());
                                if !was_selected { self.fire_annotation_focus(&id); }  // 首次 focus
                                self.fire_annotation_interact(&id);  // interact（首次+重击）
                                self.fire_selection_change();
                                self.drag = Some(DragState::Move { id: id.clone(), last: p });
                            }
                            _ => {
                                self.editor.clear_selection();
                                self.editor.clear_cursor();
                                self.fire_selection_change();
                                self.fire_cursor_change();
                            }
                        }
                    }
                }
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::PointerMove { x, y, .. } => {
                let p = (*x, *y);
                match &mut self.drag {
                    Some(DragState::Create { current, path, .. }) => {
                        *current = p;
                        path.push(p);  // Freehand 追加
                    }
                    Some(DragState::Move { id, last }) => {
                        let dx = p.0 - last.0;
                        let dy = p.1 - last.1;
                        self.editor.move_annotation(id, dx, dy);
                        *last = p;
                    }
                    Some(DragState::Resize { id, handle, anchor, orig }) => {
                        let new_rect = compute_resize(*handle, *anchor, *orig, p);
                        self.editor.resize_annotation(id, new_rect);
                    }
                    None => {}
                }
                if self.drag.is_some() { EventOutcome { needs_repaint: true } } else { EventOutcome { needs_repaint: false } }
            }
            ViewEvent::PointerUp { button: MouseButton::Left, .. } => {
                if let Some(drag) = self.drag.take() {
                    match drag {
                        DragState::Create { kind, start, current, path } => {
                            let payload = build_create_payload(&kind, start, current, &path);
                            let page = current_page_id(self.editor.document(), &self.viewport, current);
                            let id = self.editor.create_annotation(kind.clone(), page, payload);
                            self.editor.select(id);
                            self.set_tool(Tool::Select);  // 创建后回 Select
                            self.fire_annotation_focus(&id);
                            self.fire_annotation_interact(&id);
                            self.fire_selection_change();
                        }
                        DragState::Move { .. } | DragState::Resize { .. } => { /* 已实时 apply */ }
                    }
                    EventOutcome { needs_repaint: true }
                } else { EventOutcome { needs_repaint: false } }
            }
            // 现有 Scroll/Zoom/Resize/KeyDown + 右键（T4）
            // ...
        }
    }
```

加 helper：
- `compute_resize(handle, anchor, orig, p) -> Rect`：角把柄 -> bbox(anchor, p)；边把柄 -> 固定对边移动该边。
- `build_create_payload(kind, start, current, path) -> AnnotationPayload`：Markup{quad_points=[start,current], color=默认} / Freehand{path, color, width} / Shape{Rect, rect=bbox, stroke/fill/width, points:vec![]}。
- `current_page_id(doc, vp, point) -> PageId`：point 所在页。
- `annotation_payload_rect(ann) -> Rect`：批注 payload 的 rect。
- `opposite_corner(rect, handle) -> (f64,f64)`：handle 的对角点。

默认色常量（spec §3.5）：Highlight=`Rgb(255,221,0)`, Underline/Strikeout/Squiggly=`Rgb(0,0,255)`, Freehand=`Rgb(0,0,0)` w=1.5, Rect=`Rgb(255,0,0)` w=2.0。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: PASS（create_rect + move_annotation + 既有）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/component/src/editor_component.rs
git add crates/component/src/editor_component.rs
git commit -m "feat(component): drag state machine (create/move/resize via PointerDown/Move/Up)"
```

---

## Task 4: callbacks -- context_menu + page_change + zoom_change + 右键路由

**Files:**
- Modify: `crates/component/src/callbacks.rs`
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: T3（focus/interact 已在 T3 fire）。
- Produces: `on_context_menu`/`on_page_change`/`on_zoom_change` callback slot + setter + fire；右键 PointerDown 路由。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn right_click_fires_context_menu() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_context_menu(move |point, target| { *f.lock().unwrap() = Some((point, format!("{target:?}"))); });
        c.handle_event(&ViewEvent::PointerDown { button: MouseButton::Right, x: 10.0, y: 20.0, modifiers: Modifiers::default() });
        assert!(fired.lock().unwrap().is_some(), "context_menu fired");
    }

    #[test]
    fn scroll_fires_page_change() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = component_with_note();
        c.on_page_change(move |_idx| { *f.lock().unwrap() = true; });
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 1000.0 });  // 滚到下一页
        // （需多页 doc；若 component_with_note 单页，构造多页或断言不 fire）
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component right_click scroll_fires`
Expected: FAIL（on_context_menu/on_page_change 未定义；右键被丢弃）。

- [ ] **Step 3: 加 3 回调 slot + setter + fire + 右键路由**

`crates/component/src/callbacks.rs` 加 slot（`on_annotation_focus`/`on_annotation_interact` 已在 T3 用，T4 加 slot + setter）：

```rust
pub struct Callbacks {
    // 现有 4
    pub on_annotation_focus: Option<Box<dyn Fn(&AnnotationId) + Send>>,
    pub on_annotation_interact: Option<Box<dyn Fn(&AnnotationId) + Send>>,
    pub on_context_menu: Option<Box<dyn Fn((f64, f64), ContextTarget) + Send>>,
    pub on_page_change: Option<Box<dyn Fn(usize) + Send>>,
    pub on_zoom_change: Option<Box<dyn Fn(f64) + Send>>,
}

pub enum ContextTarget { Annotation(AnnotationId), Page, Empty }
```

（cfg-gated Send，如现有 4 回调。）

`editor_component.rs` 加 setter（`on_annotation_focus(cb)` 等，cfg-gated）+ fire helper（`fire_annotation_focus(id)`/`fire_annotation_interact(id)`/`fire_context_menu(point, target)`/`fire_page_change(idx)`/`fire_zoom_change(z)`）。

右键路由（handle_event 加）：
```rust
            ViewEvent::PointerDown { button: MouseButton::Right, x, y, .. } => {
                let target = rofd_render::hit_test(self.editor.document(), &self.viewport, self.editor.selection(), (*x, *y));
                let ct = match target {
                    rofd_render::HitTarget::Annotation(id) => ContextTarget::Annotation(id),
                    rofd_render::HitTarget::Page(_) => ContextTarget::Page,
                    _ => ContextTarget::Empty,
                };
                self.fire_context_menu((*x, *y), ct);
                EventOutcome { needs_repaint: false }
            }
```

page_change：Scroll/Resize 后算当前可见页（component 加 `current_page: Option<usize>`，变化时 fire）。zoom_change：Zoom 后 fire 新 zoom。

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: PASS（right_click + page_change + 既有）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/component/src/callbacks.rs crates/component/src/editor_component.rs
git add crates/component/src/callbacks.rs crates/component/src/editor_component.rs
git commit -m "feat(component): 5 callbacks (focus/interact/context_menu/page_change/zoom_change) + right-click routing"
```

---

## Task 5: native-app -- 工具栏 6 工具 + 选择 + 拖拽创建 + 右键 Delete

**Files:**
- Modify: `examples/native-app/src/main.rs`
- Test: inline（do_save 风格的集成测试或手动）

**Interfaces:**
- Consumes: T2/T3/T4（component set_tool + drag + callbacks）。
- Produces: native-app 工具栏（7 按钮）+ 右键菜单（Delete）。

- [ ] **Step 1: 加工具栏按钮**

`app_logic` 的 `menu_bar` 加 7 按钮：`选择` + `高亮` + `下划线` + `删除线` + `波浪线` + `手写` + `矩形`。每按钮 onClick -> `app.editor.lock().unwrap().component.set_tool(Tool::Select)` 或 `set_tool(Tool::Create(AnnotationKind::Highlight))` 等。（`EditorApp.component` 是 pub 字段；`Tool`/`AnnotationKind` 需从 rofd_component/rofd_dom import。）

- [ ] **Step 2: 加右键菜单（Delete）**

注册 `on_context_menu` 回调（flag-poll 模式，同 C2 的 on_save_request）-> 弹 masonry 右键菜单（"Delete" 项）-> `editor.delete_selected()`。或用 xilem 的 context menu（若 API 支持）；否则用 flag + 状态栏提示。v1 最简：on_context_menu 回调 eprintln + 可选弹简单菜单。

- [ ] **Step 3: 编译 + 手动验证**

Run: `cargo check -p native-app && cargo clippy -p native-app --all-targets -- -D warnings`
手动（非阻塞）：`cargo run -p native-app -- test/sample.ofd`，选"矩形"工具 -> 拖拽创建 -> 选中 -> 移动 -> 缩放 -> 右键 Delete。

- [ ] **Step 4: fmt + commit**

```bash
cargo fmt -- examples/native-app/src/main.rs
git add examples/native-app/src/main.rs
git commit -m "feat(native-app): toolbar 6 tools + select + drag-create + right-click Delete"
```

---

## Task 6: web-app -- 工具栏 + 拖拽创建 + 右键 Delete

**Files:**
- Modify: `examples/web-app/src/main.ts`
- Modify: `crates/web-view/src/wasm_editor.rs`（setTool + 右键桥）

**Interfaces:**
- Consumes: T2/T3/T4。
- Produces: web 工具栏 + 拖拽创建 + 右键 Delete。

- [ ] **Step 1: WasmEditor 加 setTool + 右键**

`crates/web-view/src/wasm_editor.rs`：加 `#[wasm_bindgen(js_name = setTool)] pub fn set_tool(&mut self, tool_kind: &str)`（字符串 -> Tool）；PointerDown Right 已在 handle_* 路由（component fire on_context_menu）。

- [ ] **Step 2: web-app main.ts 加工具栏 + 右键菜单**

`examples/web-app/src/main.ts`：DOM 工具栏（7 按钮）-> `editor.setTool("rect")` 等；`onContextMenu` 回调 -> JS 弹菜单（"Delete"）-> `editor.deleteSelected()`。

- [ ] **Step 3: wasm 编译 + 手动验证**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
手动（非阻塞）：`cd examples/web-app && npm run build:sdk && npm run dev`，工具栏创建 + 拖拽 + 右键 Delete。

- [ ] **Step 4: fmt + commit**

```bash
cargo fmt -- crates/web-view/src/wasm_editor.rs
git add crates/web-view/src/wasm_editor.rs examples/web-app/src/main.ts
git commit -m "feat(web-app): toolbar 6 tools + drag-create + right-click Delete"
```

---

## Task 7: 集成测试 + 全量绿

**Files:**
- Create: `crates/component/tests/c3_integration.rs`（可选）
- Test: 全量

**Interfaces:**
- Consumes: T1-T6。

- [ ] **Step 1: 组件集成测试**

`crates/component/tests/c3_integration.rs`：创建批注（拖拽）-> 选中 -> 移动 -> 缩放 -> 删除 -> undo（冒烟）。或用 component 的 inline 测试覆盖。

- [ ] **Step 2: 全量验证**

Run: `cargo test -p rofd-dom -p rofd-io -p rofd-render -p rofd-editor -p rofd-component -p rofd-native-view -p native-app`
Expected: PASS（per-crate）。

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check`
Expected: clean。

- [ ] **Step 3: Commit**

```bash
git add <测试文件>
git commit -m "test(component): C3 integration smoke (create/select/move/resize/delete/undo)"
```

---

## Definition of Done

- render: composite 接 selection+drag 画 8 手柄+框+预览；hit_test 加 Handle（接 selection）。
- component: Tool + DragState 状态机（PointerDown/Move/Up create/move/resize）；set_tool；build_scene 传 selection+drag；5 回调（focus/interact/context_menu/page_change/zoom_change）+ 右键路由。
- editor: resize 几何在 component 算（editor.resize_annotation 不变）。
- apps: native + web 工具栏（6 工具+选择）拖拽创建 + 右键 Delete。
- 全量 cargo test（per-crate）绿 + clippy + fmt clean。

## 后续

Cluster 4（次要缺口收尾：ViewEvent 补全/on_warning/错误用例测试/SkippedObject 发出 + 可能含文本光标 + 样式编辑）。
