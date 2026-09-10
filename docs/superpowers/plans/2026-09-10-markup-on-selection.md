# markup 批注"选中后按钮应用"重设计 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 高亮/下划线/删除线/波浪线从"创建工具"改为"文字选区上的命令"——先拖选正文，后点工具栏按钮（或选色）应用；旧"先点工具再拖画"路径从类型层面删除。

**Architecture:** component 层把 markup 移出 `Tool`（新 `CreateKind` 枚举 + 编译期排除），泛化 `create_highlight_from_selection` 为 `apply_markup(kind)`（保留选区、per-kind 颜色、一次 undo），新增 `on_text_selection_change` 回调供宿主禁用按钮；web-view SDK 与 native-view/native-app、web-app App.vue 同步换绑。

**Tech Stack:** Rust（rofd-component / rofd-web-view / rofd-native-view / native-app）、wasm-bindgen、Vue 3 + TS（web-app，tauri-app 前端复用）。

**Spec:** [`docs/superpowers/specs/2026-09-10-markup-on-selection-design.md`](../specs/2026-09-10-markup-on-selection-design.md)

## Global Constraints

- 依赖严格向上（AGENTS.md §4.1）：新代码只允许 component 依赖 render/editor/dom；适配器不写状态机。
- body 只读（§4.2）；apply_markup 只新增批注。
- 库不取系统时间（§4.4）。
- render 用 imaging Painter API（§4.5）——本计划不改 render。
- 无裸 unwrap；`apply_markup` 的无操作路径返回 `None`，零副作用（spec §7）。
- 提交信息 conventional commits（feat/fix/refactor/test/chore/docs），单 main 分支直接提交。
- 每个 Task 结束时 `cargo build --workspace` 必须绿（Task 2/3 含跨 crate 签名变更，同 Task 内消化）。
- 行号基于 2026-09-10 HEAD `8e1206e`，执行时以 grep 定位为准。

**测试运行约定**（所有 Task 通用）：

```bash
cargo test -p rofd-component <过滤词>   # 单 crate 按名过滤
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

---

### Task 1: component — text_selection 赋值收口 + on_text_selection_change 回调

**Files:**
- Modify: `crates/component/src/callbacks.rs`（新增回调类型别名 + Callbacks 字段）
- Modify: `crates/component/src/editor_component.rs`（`set_text_selection` helper、公开 setter/getter、替换 10 处赋值）

**Interfaces:**
- Consumes: 现有 `self.text_selection: Option<rofd_render::BodyTextSelection>`（editor_component.rs:144）、`Callbacks`（callbacks.rs:101）。
- Produces（后续 Task 依赖）:
  - `EditorComponent::set_text_selection(&mut self, sel: Option<rofd_render::BodyTextSelection>)`（`pub(crate)`，mod tests 内可直接调）
  - `EditorComponent::has_text_selection(&self) -> bool`
  - `EditorComponent::on_text_selection_change(&mut self, cb: impl Fn(Option<&rofd_render::BodyTextSelection>) + 'static + Send)`（native）/ 同名无 Send 版（wasm，`#[cfg]` 双门控，照抄 `on_selection_change` 的 1559-1566 行模式）
  - `Callbacks.on_text_selection_change: Option<Box<OnTextSelectionChange>>`

- [ ] **Step 1: 写失败测试**（追加到 `editor_component.rs` 底部 `mod tests` 的 P3 段后）

```rust
    // --- 2026-09-10 Task 1: text_selection_change 回调收口 ---

    #[test]
    fn text_selection_change_fires_on_form_same_value_and_clear() {
        let mut c = component_with_body_text();
        let fired = Arc::new(Mutex::new(0u32));
        let f = fired.clone();
        c.on_text_selection_change(move |_sel| {
            *f.lock().unwrap() += 1;
        });
        c.set_tool(Tool::Text);
        // 拖选形成选区：PointerDown 零宽（None→None 不 fire）+ Move 形成 Some（fire 1 次）。
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 16.0,
            y: 45.0,
        });
        assert!(c.text_selection().is_some());
        assert_eq!(*fired.lock().unwrap(), 1, "forming the selection fires once");
        // 同值重复赋值不 fire。
        let before = *fired.lock().unwrap();
        let sel = c.text_selection().cloned();
        c.set_text_selection(sel);
        assert_eq!(*fired.lock().unwrap(), before, "same value must not fire");
        // 空白按下清除 -> fire。
        c.handle_event(&pd(150.0, 150.0));
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 150.0,
            y: 150.0,
        });
        assert!(c.text_selection().is_none());
        assert_eq!(*fired.lock().unwrap(), before + 1, "clearing fires once");
    }

    #[test]
    fn switching_tool_clears_selection_and_fires() {
        let mut c = component_with_body_text();
        let fired = Arc::new(Mutex::new(0u32));
        let f = fired.clone();
        c.on_text_selection_change(move |_sel| {
            *f.lock().unwrap() += 1;
        });
        c.set_tool(Tool::Text);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 16.0,
            y: 45.0,
        });
        let before = *fired.lock().unwrap();
        c.set_tool(Tool::Hand);
        assert!(c.text_selection().is_none());
        assert_eq!(*fired.lock().unwrap(), before + 1);
        assert!(!c.has_text_selection());
    }
```

注意：`set_text_selection` 是 `pub(crate)`，mod tests 在同文件内可直接调用；若编译器报可见性错误，改用 `pub`。

- [ ] **Step 2: 跑测试确认编译失败**

Run: `cargo test -p rofd-component text_selection_change`
Expected: FAIL（编译错误：`on_text_selection_change` / `set_text_selection` / `has_text_selection` 不存在）

- [ ] **Step 3: 实现**

callbacks.rs —— 在 `OnPointerCursor` 别名后追加（native 段与 wasm 段各一份，照抄 §18-39 的 cfg 模式）：

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type OnTextSelectionChange = dyn Fn(Option<&rofd_render::BodyTextSelection>) + Send;
```
```rust
#[cfg(target_arch = "wasm32")]
pub type OnTextSelectionChange = dyn Fn(Option<&rofd_render::BodyTextSelection>);
```

`Callbacks` 结构体（callbacks.rs:101）加字段：

```rust
    pub on_text_selection_change: Option<Box<OnTextSelectionChange>>,
```

editor_component.rs —— 在 `text_selection()` getter（约 254 行）附近加：

```rust
    /// Whether a body-text selection currently exists (markup buttons'
    /// enabled state; spec 2026-09-10 §5.1).
    pub fn has_text_selection(&self) -> bool {
        self.text_selection.is_some()
    }

    /// Central mutator for the body-text selection: assigns only when the
    /// value actually changed, then fires `on_text_selection_change`. This
    /// is the callback's single choke point - direct field writes would miss
    /// or double-fire.
    pub(crate) fn set_text_selection(&mut self, sel: Option<rofd_render::BodyTextSelection>) {
        if self.text_selection.as_ref() == sel.as_ref() {
            return;
        }
        self.text_selection = sel;
        if let Some(cb) = &self.callbacks.on_text_selection_change {
            cb(self.text_selection.as_ref());
        }
    }
```

注册方法（照抄 `on_selection_change` 的 cfg 双门控，editor_component.rs:1559-1566 之后）：

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_text_selection_change(&mut self, cb: impl Fn(Option<&rofd_render::BodyTextSelection>) + 'static + Send) {
        self.callbacks.on_text_selection_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_text_selection_change(&mut self, cb: impl Fn(Option<&rofd_render::BodyTextSelection>) + 'static) {
        self.callbacks.on_text_selection_change = Some(Box::new(cb));
    }
```

替换全部 10 处 `self.text_selection = ...` 直接赋值为 `self.set_text_selection(...)`（先 `grep -n "self.text_selection = " crates/component/src/editor_component.rs` 确认当前行号）：

| 位置 | 原语句 | 改为 |
|---|---|---|
| load_document (~225) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| new_document (~237) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| set_tool (~358) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| Text PointerDown 命中 (~571) | `self.text_selection = (!ranges.is_empty()).then_some(` … `);` | `self.set_text_selection((!ranges.is_empty()).then_some(` … `));`（RHS 整体不变，仅包一层调用） |
| Text PointerDown 未命中 (~586) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| TextSelect 拖回零宽 (~750) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| TextSelect 形成 (~752) | `self.text_selection =\n    Some(rofd_render::BodyTextSelection { … });` | `self.set_text_selection(\n    Some(rofd_render::BodyTextSelection { … }));` |
| pointer_down_annotation 互斥 (~1111) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| pointer_down_markup 互斥 (~1216) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| after_annotation_change (~1415) | `self.text_selection = None;` | `self.set_text_selection(None);` |
| maybe_fire_page_change (~1502) | `self.text_selection = None;` | `self.set_text_selection(None);` |

（上表 11 行含 571/575 与 750/752 两组相邻语句，共 10 个赋值点。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component text_selection_change`
Expected: PASS（2 个新测试）+ 既有 P3 测试全绿：

Run: `cargo test -p rofd-component`
Expected: 0 failed

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/callbacks.rs crates/component/src/editor_component.rs
git commit -m "feat(component): on_text_selection_change callback via set_text_selection choke point"
```

---

### Task 2: component + SDK — apply_markup 泛化（保留选区；undo/redo 保留选区）

**Files:**
- Modify: `crates/component/src/editor_component.rs`（删 `create_highlight_from_selection`，新增 `apply_markup`；`undo`/`redo` 保留选区；改写/新增测试）
- Modify: `crates/web-view/src/wasm_editor.rs`（删 `createHighlightFromSelection` 绑定；新增 `applyMarkup`/`hasTextSelection`/`setOnTextSelectionChange` + 桥接）
- Modify: `crates/web-view/sdk/src/index.ts`（WasmEditor 接口、Editor 类、EditorConfig、init 装配）

**Interfaces:**
- Consumes: Task 1 的 `set_text_selection`；现有 `markup_color(&kind)`（editor_component.rs:390）、`rofd_render::text_selection_rects`、`viewport_to_page_local`、`parse_markup_kind`（wasm_editor.rs:77）、`call_js0`。
- Produces:
  - `EditorComponent::apply_markup(&mut self, kind: rofd_dom::AnnotationKind) -> Option<rofd_dom::AnnotationId>`
  - SDK TS：`applyMarkup(kind: MarkupKind): string | null`、`hasTextSelection(): boolean`、`setOnTextSelectionChange(cb: (() => void) | null): void`、`EditorConfig.onTextSelectionChange?: () => void`、`export type MarkupKind = 'highlight' | 'underline' | 'strikeout' | 'squiggly'`
  - 删除：`EditorComponent::create_highlight_from_selection`、SDK `createHighlightFromSelection`

- [ ] **Step 1: 写失败测试**（替换 editor_component.rs 旧测试 `create_highlight_from_selection_makes_markup_quads_and_undo_removes`（~4850）与 `create_highlight_without_selection_is_none`（~4902），并新增）

```rust
    // --- 2026-09-10 Task 2: apply_markup（选中转 markup，保留选区） ---

    /// 拖选两行（helper 与旧测试一致）：pd(31,25) -> Move(16,45) -> Up。
    fn drag_select_two_lines(c: &mut EditorComponent) {
        c.set_tool(Tool::Text);
        c.handle_event(&pd(31.0, 25.0));
        c.handle_event(&ViewEvent::PointerMove { x: 16.0, y: 45.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 16.0,
            y: 45.0,
        });
    }

    #[test]
    fn apply_markup_makes_quads_keeps_selection_and_undo_keeps_selection() {
        let mut c = component_with_body_text();
        drag_select_two_lines(&mut c);
        let id = c
            .apply_markup(AnnotationKind::Highlight)
            .expect("highlight created");
        // 保留选区（spec §6.1：应用成功后保留）。
        assert!(c.text_selection().is_some(), "selection retained after apply");
        let ann = c.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Markup { quad_points, color } => {
                // per-kind 默认高亮色 = 黄（DEFAULT_HIGHLIGHT_COLOR）。
                assert_eq!(*color, Color::Rgb(255, 255, 0));
                // 两行 -> 两个 quad（4 个点），页局部坐标。
                assert_eq!(quad_points.len(), 4);
                assert_eq!(quad_points[0], Point { x: 30.0, y: 20.0 });
                assert_eq!(quad_points[1], Point { x: 50.0, y: 32.5 });
                assert_eq!(quad_points[2], Point { x: 10.0, y: 40.0 });
                assert_eq!(quad_points[3], Point { x: 20.0, y: 52.5 });
            }
            _ => panic!("expected Markup payload"),
        }
        // undo 移除批注，且选区保留（spec §6.1：undo 不清选区）。
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('z'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        assert!(c.document().annotations.find(&id).is_none(), "undo removes markup");
        assert!(c.text_selection().is_some(), "selection survives undo");
        // redo 恢复，选区仍在。
        c.redo();
        assert!(c.document().annotations.find(&id).is_some(), "redo restores markup");
        assert!(c.text_selection().is_some(), "selection survives redo");
    }

    #[test]
    fn apply_markup_without_selection_is_none_no_history() {
        let mut c = component_with_body_text();
        c.set_tool(Tool::Text);
        assert!(c.apply_markup(AnnotationKind::Highlight).is_none());
        assert!(!c.can_undo(), "no transaction on no-op");
    }

    #[test]
    fn apply_markup_rejects_non_markup_kind() {
        let mut c = component_with_body_text();
        drag_select_two_lines(&mut c);
        assert!(c.apply_markup(AnnotationKind::Note).is_none());
        assert!(!c.can_undo());
        assert!(c.text_selection().is_some(), "no-op must not clear selection");
    }

    #[test]
    fn apply_markup_uses_per_kind_color() {
        let mut c = component_with_body_text();
        drag_select_two_lines(&mut c);
        c.set_markup_color(&AnnotationKind::Squiggly, Color::Rgb(1, 2, 3));
        let id = c.apply_markup(AnnotationKind::Squiggly).unwrap();
        let ann = c.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Markup { color, .. } => assert_eq!(*color, Color::Rgb(1, 2, 3)),
            _ => panic!("expected Markup payload"),
        }
    }

    #[test]
    fn apply_markup_stacks_and_does_not_fire_text_selection_change() {
        let mut c = component_with_body_text();
        let fired = Arc::new(Mutex::new(0u32));
        let f = fired.clone();
        c.on_text_selection_change(move |_sel| {
            *f.lock().unwrap() += 1;
        });
        drag_select_two_lines(&mut c);
        let before = *fired.lock().unwrap();
        let id1 = c.apply_markup(AnnotationKind::Underline).unwrap();
        let id2 = c.apply_markup(AnnotationKind::Underline).unwrap();
        assert_ne!(id1, id2, "stacking creates independent annotations");
        let count = c
            .document()
            .annotations
            .all()
            .iter()
            .filter(|a| a.kind == AnnotationKind::Underline)
            .count();
        assert_eq!(count, 2);
        // undo 两次逐条撤销。
        assert!(c.undo());
        assert!(c.undo());
        assert!(!c.can_undo());
        // 全程选区回调不 fire（选区值未变）。
        assert_eq!(*fired.lock().unwrap(), before);
    }
```

注意：`annotations.all()` 的真实方法名以 `crates/dom/src/annotation.rs` 的 `AnnotationModel` 公开 API 为准（若不是 `all()`，改用 `document().annotations` 上等价的遍历方法；断言意图不变）。`Arc`/`Mutex` 已在 tests 模块 use。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component apply_markup`
Expected: FAIL（编译错误：`apply_markup` 不存在）

- [ ] **Step 3: 实现 component 侧**

用下面的 `apply_markup` **整体替换** `create_highlight_from_selection`（editor_component.rs:443-483）：

```rust
    /// Convert the current body-text selection into a markup annotation
    /// (Highlight/Underline/Strikeout/Squiggly): one Markup quad per selected
    /// line (page-local tl/br pairs), colored by the per-kind default
    /// ([`Self::set_markup_color`]). Walks `create_annotation` (one
    /// Transaction = one undo). Unlike other annotation mutations this KEEPS
    /// the selection - the body text it refers to never changes - so the host
    /// can stack further markups on the same range (spec 2026-09-10 §5.1).
    /// Non-markup kinds and a missing/empty selection return `None` with no
    /// side effects (no annotation, no history, no callbacks).
    pub fn apply_markup(&mut self, kind: AnnotationKind) -> Option<rofd_dom::AnnotationId> {
        if !matches!(
            kind,
            AnnotationKind::Highlight
                | AnnotationKind::Underline
                | AnnotationKind::Strikeout
                | AnnotationKind::Squiggly
        ) {
            return None;
        }
        let sel = self.text_selection.clone()?;
        let page_id = sel.page.clone();
        let rects = rofd_render::text_selection_rects(self.editor.document(), &self.viewport, &sel);
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
            quad_points.push(Point { x: tl.0, y: tl.1 });
            quad_points.push(Point { x: br.0, y: br.1 });
        }
        let color = self.markup_color(&kind);
        let id = self.editor.create_annotation(
            kind,
            page_id,
            AnnotationPayload::Markup { quad_points, color },
        );
        // after_annotation_change clears the selection (文档变更 -> 选区失
        // 效); restore it - markup creation never invalidates body text
        // (spec 2026-09-10 §6.1). Same value -> no callback fire.
        self.after_annotation_change();
        self.set_text_selection(Some(sel));
        self.fire_selection_change();
        Some(id)
    }
```

`undo`/`redo`（editor_component.rs:325-341）保留选区：

```rust
    pub fn undo(&mut self) -> bool {
        let keep = self.text_selection.clone();
        if self.editor.undo() {
            self.after_annotation_change();
            // undo/redo 不触碰正文（body 只读），选区仍然有效
            // （spec 2026-09-10 §6.1）。同值恢复 -> 不 fire 回调。
            self.set_text_selection(keep);
            true
        } else {
            false
        }
    }
    /// Redo the last undone command (same path as Ctrl+Y / Ctrl+Shift+Z).
    pub fn redo(&mut self) -> bool {
        let keep = self.text_selection.clone();
        if self.editor.redo() {
            self.after_annotation_change();
            self.set_text_selection(keep);
            true
        } else {
            false
        }
    }
```

同时检查 Ctrl+Z 的 KeyDown 处理路径：若它不经过 `self.undo()` 而是直接调 `editor.undo()`，改为调 `self.undo()`（grep `editor.undo()` 确认仅此一处语义）。删除旧方法后，同文件旧测试 `create_highlight_from_selection_*` 两个已在 Step 1 替换掉。

- [ ] **Step 4: 跑 component 测试**

Run: `cargo test -p rofd-component`
Expected: 0 failed（新增 5 个 apply_markup 测试 + 既有全绿）

- [ ] **Step 5: 实现 wasm_editor + SDK**

`crates/web-view/src/wasm_editor.rs`：

(a) `parse_color` 的 doc 注释里 `createHighlightFromSelection` 字样改为 `applyMarkup`（~105 行）。

(b) `JsCallbacks`（~147）加字段：

```rust
        pub on_text_selection_change: Rc<RefCell<Option<js_sys::Function>>>,
```

(c) 替换 `create_highlight_from_selection` 绑定（~305-311）为：

```rust
        /// Convert the current body-text selection into a markup annotation
        /// of the given kind ("highlight" | "underline" | "strikeout" |
        /// "squiggly"; other strings return null). Color is the per-kind
        /// default (setMarkupColor). Returns the new annotation's id, or
        /// null when there is no selection. The selection is kept so further
        /// markups can stack on the same range.
        #[wasm_bindgen(js_name = applyMarkup)]
        pub fn apply_markup(&mut self, kind: &str) -> Option<String> {
            let kind = parse_markup_kind(kind)?;
            self.component.apply_markup(kind).map(|id| id.0.clone())
        }

        /// Whether a body-text selection currently exists (markup buttons'
        /// enabled state).
        #[wasm_bindgen(js_name = hasTextSelection)]
        pub fn has_text_selection(&self) -> bool {
            self.component.has_text_selection()
        }

        /// Register the text-selection-change callback (signal-only, no
        /// payload; query hasTextSelection/getSelectedText afterwards).
        #[wasm_bindgen(js_name = setOnTextSelectionChange)]
        pub fn set_on_text_selection_change(&mut self, callback: Option<js_sys::Function>) {
            *self.callbacks.on_text_selection_change.borrow_mut() = callback;
        }
```

(d) `setup_bridge_callbacks`（~621 起，仿 on_selection_change 段 628-632）追加：

```rust
            let on_text_selection_change_js = self.callbacks.on_text_selection_change.clone();
            self.component.on_text_selection_change(Box::new(move |_sel| {
                call_js0(&on_text_selection_change_js);
            }));
```

`crates/web-view/sdk/src/index.ts`：

(a) WasmEditor 接口（~52-78）：删 `createHighlightFromSelection(color: string): string | null;`，加：

```ts
  applyMarkup(kind: 'highlight' | 'underline' | 'strikeout' | 'squiggly'): string | null;
  hasTextSelection(): boolean;
  setOnTextSelectionChange(cb: (() => void) | null): void;
```

(b) 公共类型区（~80 起，`FontSource` 旁）加：

```ts
/** The four text-markup annotation kinds (actions over a body-text selection). */
export type MarkupKind = 'highlight' | 'underline' | 'strikeout' | 'squiggly';
```

(c) `EditorConfig`（~89 起）加：

```ts
  /** Fired when the body-text selection appears/changes/clears (signal-only;
   * query hasTextSelection()/getSelectedText() afterwards). */
  onTextSelectionChange?: () => void;
```

(d) init 回调装配区（~210-216 段）加一行：

```ts
    if (config?.onTextSelectionChange) wasmEditor.setOnTextSelectionChange(config.onTextSelectionChange);
```

(e) Editor 类方法（替换 ~488-493 的 `createHighlightFromSelection`）：

```ts
  /** Convert the current body-text selection into a markup annotation of the
   * given kind. Color is the per-kind default (setMarkupColor). Returns the
   * new annotation's id, or null when there is no selection. The selection is
   * kept so further markups can stack on the same range. */
  applyMarkup(kind: MarkupKind): string | null {
    return this.wasm.applyMarkup(kind);
  }

  /** Whether a body-text selection currently exists (markup buttons' enabled
   * state). */
  hasTextSelection(): boolean {
    return this.wasm.hasTextSelection();
  }

  /** Fired when the body-text selection appears/changes/clears. Signal-only;
   * query hasTextSelection()/getSelectedText() afterwards. */
  setOnTextSelectionChange(cb: (() => void) | null): void {
    this.wasm.setOnTextSelectionChange(cb);
  }
```

- [ ] **Step 6: 全 workspace 编译 + web 测试**

Run: `cargo build --workspace && cargo test -p rofd-web-view`
Expected: 编译通过（`createHighlightFromSelection` 无残留引用——`grep -rn "createHighlightFromSelection\|create_highlight_from_selection" crates/` 应只剩 docs/ 与本 plan）；web-view 测试全绿

- [ ] **Step 7: 提交**

```bash
git add crates/component/src/editor_component.rs crates/web-view/src/wasm_editor.rs crates/web-view/sdk/src/index.ts
git commit -m "feat(component)!: apply_markup generalizes selection-to-markup, keeps selection

- apply_markup(kind) replaces create_highlight_from_selection(color):
  all four markup kinds, per-kind color, one Transaction per apply
- selection retained across apply/undo/redo (body is read-only)
- SDK: applyMarkup / hasTextSelection / setOnTextSelectionChange,
  createHighlightFromSelection removed (breaking, 0.2.0)"
```

---

### Task 3: CreateKind 类型收紧 + parse_tool_kind 删 markup 分支 + native 按钮动作化

**Files:**
- Modify: `crates/component/src/editor_component.rs`（`CreateKind`、`Tool`、`DragState::Create`、`build_create_payload`、相关测试）
- Modify: `crates/component/src/lib.rs`（导出 `CreateKind`）
- Modify: `crates/web-view/src/wasm_editor.rs`（`parse_tool_kind` 收紧 + 新测试）
- Modify: `crates/native-view/src/editor_app.rs`（`apply_markup` pass-through）
- Modify: `crates/native-app/src/main.rs`（markup 按钮 → 动作按钮；shape/freehand 按钮换 `CreateKind`）

**Interfaces:**
- Consumes: Task 2 的 `EditorComponent::apply_markup`。
- Produces:
  - `rofd_component::CreateKind { Shape(ShapeKind), Freehand }` + `CreateKind::to_annotation_kind(&self) -> AnnotationKind`
  - `Tool::Create(CreateKind)`（`DragState::Create { kind: CreateKind, ... }` 同步）
  - `EditorApp::apply_markup(&mut self, kind: rofd_dom::AnnotationKind) -> Option<rofd_dom::AnnotationId>`（native-view）
  - `parse_tool_kind("highlight"|"underline"|"strikeout"|"squiggly") == Tool::Text`

- [ ] **Step 1: 写失败测试**

(a) wasm_editor.rs 的纯 Rust 测试区（`parse_tool_kind` 既有测试旁）加：

```rust
#[test]
fn parse_tool_kind_maps_markup_strings_to_text() {
    // markup 不再是工具（spec 2026-09-10 §3 方案 A）：旧字符串降级为 Text。
    for s in ["highlight", "underline", "strikeout", "squiggly"] {
        assert_eq!(parse_tool_kind(s), Tool::Text, "{s} must not be a create tool");
    }
    assert_eq!(parse_tool_kind("freehand"), Tool::Create(CreateKind::Freehand));
}
```

(b) editor_component.rs 测试区加（放在 Task 2 测试后）：

```rust
    #[test]
    fn create_tool_cannot_be_markup_kind() {
        // 类型即约束：markup 无法构造为 Create 工具（编译期保证，这里仅
        // 验证 Create(CreateKind) 的 kind 映射正确）。
        let tool = Tool::Create(CreateKind::Shape(ShapeKind::Rect));
        assert_eq!(
            tool,
            Tool::Create(CreateKind::Shape(ShapeKind::Rect))
        );
        assert_eq!(
            CreateKind::Freehand.to_annotation_kind(),
            AnnotationKind::Freehand
        );
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-web-view parse_tool_kind_maps_markup && cargo test -p rofd-component create_tool_cannot`
Expected: FAIL（`CreateKind` 不存在；markup 分支仍返回 Create）

- [ ] **Step 3: 实现**

(a) editor_component.rs —— `Tool` 枚举（~23-30）前加：

```rust
/// The annotation kinds that CAN be a drag-create tool. Markup kinds are
/// deliberately absent: markup is a command over the body-text selection
/// ([`EditorComponent::apply_markup`]), never a tool (spec 2026-09-10 §4.1).
#[derive(Debug, Clone, PartialEq)]
pub enum CreateKind {
    Shape(ShapeKind),
    Freehand,
}

impl CreateKind {
    /// The annotation kind a drag of this tool creates.
    pub fn to_annotation_kind(&self) -> AnnotationKind {
        match self {
            CreateKind::Shape(s) => AnnotationKind::Shape(s.clone()),
            CreateKind::Freehand => AnnotationKind::Freehand,
        }
    }
}
```

`Tool::Create(AnnotationKind)` → `Tool::Create(CreateKind)`；`DragState::Create { kind: AnnotationKind, .. }` → `kind: CreateKind`（~32-40 区域，doc 注释同步改"the given [`CreateKind`]"）。

(b) PointerDown Create 分支（~528-536）`kind: kind.clone()` 不变（类型随 DragState 变）。

(c) PointerUp Create 提交（~818-885）：

```rust
                                let payload = build_create_payload(
                                    &kind,
                                    start_l,
                                    current_l,
                                    &path_local,
                                );
                                let id = self
                                    .editor
                                    .create_annotation(kind.to_annotation_kind(), page, payload);
```

（删除 `self.markup_color(&kind)` 实参；后续 `fire_annotation_focus(&id)` 等不变。）

(d) `build_create_payload`（~1904 起）改签名并删 Markup 分支：

```rust
fn build_create_payload(
    kind: &CreateKind,
    start: (f64, f64),
    current: (f64, f64),
    path: &[(f64, f64)],
) -> AnnotationPayload {
    match kind {
        CreateKind::Freehand => { /* 原 AnnotationKind::Freehand 分支体不变 */ }
        CreateKind::Shape(_) => { /* 原 AnnotationKind::Shape(_) 分支体不变 */ }
    }
}
```

（整段删除 `AnnotationKind::Highlight` 与 `AnnotationKind::Underline | Strikeout | Squiggly` 两个 Markup 分支及 `markup_color` 参数；其余分支体逐字保留。若该函数还有 Note/TextBox 等不可达兜底分支，一并删除——Create 工具只产 Shape/Freehand。）

(e) 全文件 grep `Tool::Create(AnnotationKind` 逐处改为 `CreateKind`（已知：c3_smoke 测试 ~4073 `Tool::Create(AnnotationKind::Shape(ShapeKind::Rect))` → `Tool::Create(CreateKind::Shape(ShapeKind::Rect))`；若存在 markup 拖画测试则按新语义改写或删除，删除时在 commit message 注明）。

(f) lib.rs 导出行加 `CreateKind`（grep `pub use editor_component` 找到现有导出列表）。

(g) wasm_editor.rs `parse_tool_kind`（~56-72）：

```rust
pub fn parse_tool_kind(kind: &str) -> Tool {
    match kind {
        "text" | "select" | "textSelect" => Tool::Text,
        "hand" => Tool::Hand,
        // NOTE: markup kinds ("highlight"/"underline"/"strikeout"/"squiggly")
        // are NOT tools (spec 2026-09-10): they fall through to Text. Use
        // applyMarkup over a body-text selection instead.
        "freehand" => Tool::Create(CreateKind::Freehand),
        "rect" => Tool::Create(CreateKind::Shape(ShapeKind::Rect)),
        "ellipse" => Tool::Create(CreateKind::Shape(ShapeKind::Ellipse)),
        "arrow" => Tool::Create(CreateKind::Shape(ShapeKind::Arrow)),
        "line" => Tool::Create(CreateKind::Shape(ShapeKind::Line)),
        "polygon" => Tool::Create(CreateKind::Shape(ShapeKind::Polygon)),
        _ => Tool::Text,
    }
}
```

（import 行补 `CreateKind`。）

(h) native-view/src/editor_app.rs —— 在 `set_clock`（~103）后加：

```rust
    /// Apply a markup annotation over the current body-text selection
    /// (highlight/underline/strikeout/squiggly). Returns the new id, or
    /// None when there is no selection / the kind is not markup.
    pub fn apply_markup(&mut self, kind: rofd_dom::AnnotationKind) -> Option<rofd_dom::AnnotationId> {
        self.component.apply_markup(kind)
    }
```

(i) native-app/src/main.rs —— `tool_button`（~65-72）后加动作按钮 helper：

```rust
/// Markup button: an ACTION over the current body-text selection, not a
/// tool (spec 2026-09-10). Clicking applies the markup kind; without a
/// selection this is a no-op (Task 4 adds the disabled affordance).
fn markup_button(label: &'static str, kind: AnnotationKind) -> impl WidgetView<AppState> + use<> {
    text_button(label, move |app: &mut AppState| {
        app.editor.lock().unwrap().apply_markup(kind.clone());
    })
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0)
}
```

按钮区（~168-171）改：

```rust
    let btn_highlight = markup_button("高亮", AnnotationKind::Highlight);
    let btn_underline = markup_button("下划线", AnnotationKind::Underline);
    let btn_strikeout = markup_button("删除线", AnnotationKind::Strikeout);
    let btn_squiggly = markup_button("波浪线", AnnotationKind::Squiggly);
```

freehand/rect 按钮（~172-173）改 `tool_button("手写", Tool::Create(CreateKind::Freehand))`、`tool_button("矩形", Tool::Create(CreateKind::Shape(ShapeKind::Rect)))`（import 补 `CreateKind`）。

- [ ] **Step 4: 全量验证**

Run: `cargo build --workspace && cargo test -p rofd-component && cargo test -p rofd-web-view`
Expected: 编译通过；component/web-view 测试全绿（含 Step 1 两个新测试）

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 干净（fmt 有 diff 则 `cargo fmt --all` 后复检）

- [ ] **Step 5: 提交**

```bash
git add crates/component/src/editor_component.rs crates/component/src/lib.rs crates/web-view/src/wasm_editor.rs crates/native-view/src/editor_app.rs crates/native-app/src/main.rs
git commit -m "refactor(component)!: markup is never a tool - CreateKind type narrowing

- Tool::Create payload AnnotationKind -> CreateKind { Shape, Freehand };
  markup kinds unrepresentable as tools (compile-time, spec 2026-09-10)
- build_create_payload drops the drag-rect Markup branch
- parse_tool_kind: markup strings fall through to Text (legacy hosts degrade)
- native markup buttons become apply_markup actions via EditorApp pass-through"
```

---

### Task 4: native-app — on_text_selection_change 装配 + markup 按钮禁用态

**Files:**
- Modify: `crates/native-app/src/main.rs`（SharedHasSelection 标志、回调装配（含 wake）、按钮 `.disabled(...)`）

**Interfaces:**
- Consumes: Task 1 `on_text_selection_change`（native 版含 Send）、Task 3 `markup_button`、既有 `SharedWakeProxy` flag-pattern（main.rs:52、pointer_cursor 装配 498-512 同款）、xilem_masonry `Button::disabled(bool)`（checkout 已确认存在）。
- Produces: native 宿主完成 spec §5.3 的禁用态；无新库 API。

- [ ] **Step 1: 实现装配**（本任务无可自动化测试，验收 = 编译 + 手动运行）

(a) 类型别名区（~60 后）加：

```rust
/// Whether a body-text selection exists - markup buttons' enabled state.
/// Pushed by on_text_selection_change (flag-pattern, like pointer_cursor);
/// the callback also wakes the app so app_logic rebuilds and the buttons
/// refresh.
type SharedHasSelection = Arc<Mutex<bool>>;
```

(b) `AppState`（~75-81）加字段 `has_selection: SharedHasSelection,`。

(c) `markup_button` 改造（接收 disabled）：

```rust
/// Markup button: an ACTION over the current body-text selection, not a
/// tool (spec 2026-09-10). Disabled (grayed) without a selection.
fn markup_button(
    label: &'static str,
    kind: AnnotationKind,
    disabled: bool,
) -> impl WidgetView<AppState> + use<> {
    text_button(label, move |app: &mut AppState| {
        app.editor.lock().unwrap().apply_markup(kind.clone());
    })
    .disabled(disabled)
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0)
}
```

(d) app_logic 内按钮区（按钮在 app_logic 闭包里构建，`app: &mut AppState` 可用）：

```rust
    let has_selection = *app.has_selection.lock().unwrap();
    let btn_highlight = markup_button("高亮", AnnotationKind::Highlight, !has_selection);
    let btn_underline = markup_button("下划线", AnnotationKind::Underline, !has_selection);
    let btn_strikeout = markup_button("删除线", AnnotationKind::Strikeout, !has_selection);
    let btn_squiggly = markup_button("波浪线", AnnotationKind::Squiggly, !has_selection);
```

(e) 装配回调（放在 on_pointer_cursor 装配块 ~503-512 之后；`wake_proxy` 已在该作用域）：

```rust
    // Text-selection state: the component fires on_text_selection_change
    // whenever the body-text selection appears/changes/clears. The callback
    // stashes the flag and wakes the app so app_logic rebuilds and the
    // markup buttons' disabled state refreshes.
    let has_selection: SharedHasSelection = Arc::new(Mutex::new(false));
    {
        let hs = has_selection.clone();
        let wp = wake_proxy.clone();
        editor
            .lock()
            .unwrap()
            .component
            .on_text_selection_change(move |sel| {
                *hs.lock().unwrap() = sel.is_some();
                if let Some(proxy) = wp.lock().unwrap().as_ref() {
                    let _ = proxy.message(());
                }
            });
    }
```

（`AppState { ... }` 构造处（~514-519）补 `has_selection: has_selection.clone(),`。若 `wake_proxy` 声明晚于此块，把两者声明顺序调换——以编译器为准。）

- [ ] **Step 2: 编译 + 手动验证**

Run: `cargo build --workspace`
Expected: 编译通过，clippy/fmt 干净

Run: `cargo run -p native-app -- test/ru-yuan-ji-lu.ofd`
手动验收：Text 工具拖选正文 → 四个 markup 按钮由灰变亮 → 点"下划线" → 贴字下划线出现且选区仍在 → 再点"高亮"叠加 → Ctrl+Z 逐步撤销 → 点空白 → 按钮灰显。"手写/矩形"拖画不受影响。

- [ ] **Step 3: 提交**

```bash
git add crates/native-app/src/main.rs
git commit -m "feat(native-app): markup buttons disabled without text selection"
```

---

### Task 5: web-app — App.vue 动作化 + 禁用态 + 选色即应用

**Files:**
- Modify: `crates/web-app/src/components/ToolButton.vue`（新增 `actionDisabled` prop：主键禁用但下拉箭头仍可点）
- Modify: `crates/web-app/src/App.vue`（markup 按钮模板、`applyMarkup`、选色即应用、`onTextSelectionChange` 装配）

**Interfaces:**
- Consumes: Task 2 SDK `applyMarkup`/`hasTextSelection`/`setOnTextSelectionChange`/`EditorConfig.onTextSelectionChange`/`MarkupKind`。
- Produces: 无（纯宿主 UI）。tauri-app 复用 web-app 源码自动继承。

设计说明：ToolButton 已有 `disabled` prop（原生 disabled 属性会连下拉箭头一起封死），而 spec §5.3 要求"无选区时选色仅存色"——颜色面板必须始终可达。故新增 `actionDisabled`：加 `disabled` 样式类 + 拦截主键 click emit，下拉箭头不受影响。这是对 spec §5.3 `:disabled` 的实现细化，语义一致（R1 灰显不可点 + R4 无选区仅存色）。

- [ ] **Step 1: ToolButton.vue 加 actionDisabled**

(a) script 段：

```ts
defineProps<{
  tooltip: string;
  label: string;
  active?: boolean;
  disabled?: boolean;
  /** 主动作禁用（灰显、点击无效），但下拉箭头仍可点（markup 按钮：
   *  无选区时主键禁用、颜色面板仍可预选颜色）。 */
  actionDisabled?: boolean;
  hasDropdown?: boolean;
  /** 按钮内嵌值（如 "100%"），与图标并列显示（大组合控件） */
  value?: string;
}>();
```

(b) template 的 button 元素：`:class="{ active, disabled: disabled, 'has-dropdown': hasDropdown }"` 改为 `:class="{ active, disabled: disabled || actionDisabled, 'has-dropdown': hasDropdown }"`；`@click="$emit('click')"` 改为 `@click="!actionDisabled && $emit('click')"`。（hover 样式 `.tool-btn:not(.disabled):hover` 已按类名生效，无需改 CSS。）

- [ ] **Step 2: App.vue 状态与动作**

(a) 响应式状态区（~483 `squigglyColor` 后）加：

```ts
/** 正文文字选区是否存在（markup 按钮禁用态；spec 2026-09-10 R1）。 */
const hasTextSelection = ref(false);
```

(b) 工具栏动作区（`setTool` 后）加：

```ts
/** markup 四种是选区上的动作而非工具：无选区时按钮禁用（不会走到这里）。
 * 一次点击 = 一条批注 = 一次撤销；选区保留可继续叠加。 */
function applyMarkup(kind: MarkupKind): void {
  editor.value?.applyMarkup(kind);
  refreshHistoryState();
}
```

(c) `Editor.init` 配置（~754-784 的对象里，`onZoomChange` 后）加：

```ts
      // 文字选区出现/变化/清除（信号回调）：驱动 markup 按钮禁用态。
      onTextSelectionChange: () => {
        hasTextSelection.value = editor.value?.hasTextSelection() ?? false;
      },
```

(d) `pickHighlightColor`（~647-652）与 `pickMarkupColor`（~655-662）改为选色即应用：

```ts
function pickHighlightColor(color: string): void {
  highlightColor.value = color;
  editor.value?.setHighlightColor(color);
  if (hasTextSelection.value) applyMarkup('highlight'); // 选色即应用（R4）
  closeDropdown();
}

/** 线类工具选色：颜色独立配置（setMarkupColor），有选区即应用（R4）。 */
function pickMarkupColor(kind: Exclude<MarkupKind, 'highlight'>, color: string): void {
  if (kind === 'underline') underlineColor.value = color;
  else if (kind === 'strikeout') strikeoutColor.value = color;
  else squigglyColor.value = color;
  editor.value?.setMarkupColor(kind, color);
  if (hasTextSelection.value) applyMarkup(kind);
  closeDropdown();
}
```

（`onCustomColorChange`（~678-683）走的就是这两个函数，自动获得同语义，无需改。）

- [ ] **Step 3: App.vue 模板按钮**

四个 markup ToolButton（~140-151）整体替换（以高亮为例，其余三个换 kind/label/tooltip/图标/color ref）：

```vue
            <ToolButton
              label="高亮"
              tooltip="高亮批注：选中正文文字后点击"
              has-dropdown
              :action-disabled="!hasTextSelection"
              @click="applyMarkup('highlight')"
              @dropdown="openDropdown('highlight', $event)"
            >
              <HighlightIcon :color="highlightColor" />
            </ToolButton>
```

其余三个：下划线（`underline`，`UnderlineIcon/underlineColor`）、删除线（`strikeout`，`StrikeoutIcon/strikeoutColor`）、波浪线（`squiggly`，`SquigglyIcon/squigglyColor`）；tooltip 分别为"下划线批注：选中正文文字后点击（下拉选颜色）"等，保持"选中正文文字后点击"措辞。删除原 `:active="activeTool === 'highlight'"` 等绑定（markup 不再是 activeTool 值）。

- [ ] **Step 4: 构建 + 手动验证**

Run: `cd crates/web-app && npm run build`
Expected: vite 构建成功（导入/模板错误会失败）

Run: `cd crates/web-app && npm run dev`
手动验收（对照 spec §8）：拖选正文 → 四按钮亮 → 点下划线 → 贴字下划线 + 选区仍在 → 点高亮叠加 → 颜色面板选红 → 立即以红高亮 → Ctrl+Z 逐步撤销 → 点空白按钮灰显 → 灰显时下拉仍可开、选色只存色不报错 → 保存重开批注保留。tauri 侧 `cd crates/tauri-app && npm run tauri dev` 抽查同样流程。

- [ ] **Step 5: 提交**

```bash
git add crates/web-app/src/components/ToolButton.vue crates/web-app/src/App.vue
git commit -m "feat(web-app): markup buttons act on text selection with disabled state"
```

---

### Task 6: SDK 0.2.0 + 全量回归 + spec 收尾

**Files:**
- Modify: `crates/web-view/sdk/package.json`（version 0.1.5 → 0.2.0）
- Modify: `crates/web-view/Cargo.toml`（version 0.1.0 → 0.2.0）
- Modify: `docs/superpowers/specs/2026-09-10-markup-on-selection-design.md`（状态行）

**Interfaces:**
- Consumes: Task 1-5 全部完成。
- Produces: 发布就绪的 0.2.0（breaking：`createHighlightFromSelection` 移除、`setTool` 不再接受 markup 字符串）。

- [ ] **Step 1: 版本与状态**

`crates/web-view/sdk/package.json` 的 `"version": "0.1.5"` → `"0.2.0"`；`crates/web-view/Cargo.toml` 的 `version = "0.1.0"` → `"0.2.0"`。spec 状态行 `状态：待审阅` → `状态：已实现（2026-09-10）`。

- [ ] **Step 2: 全量回归**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 全绿（含 io 手术刀字节保留 round_trip/save_surgical——本计划未动 io，若红说明越界改动了 io，回查）

Run: `cd crates/web-app && npm run build`
Expected: 构建成功

- [ ] **Step 3: 提交**

```bash
git add crates/web-view/sdk/package.json crates/web-view/Cargo.toml docs/superpowers/specs/2026-09-10-markup-on-selection-design.md
git commit -m "chore(sdk): bump version to 0.2.0 (breaking markup API)"
```

---

## Self-Review 记录

- **Spec 覆盖**：R1（禁用）→ Task 4/5；R2（保留选区）→ Task 2（apply_markup + undo/redo 恢复）；R3（叠加）→ Task 2 测试 `apply_markup_stacks_*`；R4（选色即应用）→ Task 5；§4.1 类型收紧 → Task 3；§4.2 收口 → Task 1；§5.2 SDK → Task 2；§5.3 宿主 → Task 4/5；§7 错误 → Task 2（None 语义 + 测试）；§8 测试 → 各 Task Step 1 + 手动清单；§9 文件清单全覆盖（ToolButton 从"加 disabled"修正为"加 actionDisabled"，因 disabled 会封死下拉，与 R4 冲突——已在 Task 5 注明）。
- **占位符扫描**：无 TBD/TODO；Task 3 (d) 的"分支体不变/逐字保留"指向既存代码的机械搬运，非未写内容。
- **类型一致性**：`apply_markup(AnnotationKind) -> Option<AnnotationId>`（component/EditorApp/wasm 三层一致）；`set_text_selection`/`has_text_selection`/`on_text_selection_change` 命名贯穿 Task 1/2/4/5；`MarkupKind` SDK 导出与 App.vue 本地类型同名同值（App.vue 可改 import SDK 的，保留本地亦可——执行者二选一，保持一致）。
