# A2 Xilem 适配器（OfdWidget/OfdView）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `crates/native-view` 内新增 masonry `Widget` + xilem `View` 三个适配器文件，直接使用最终命名（`OfdWidget`/`OfdView`/`ofd()`/`ofd_with_config()`/`OfdCommand`），把宿主事件经纯翻译表转发给 `EditorComponent`，组件回调经 `Arc<Mutex<Vec<…>>>` 暂存后以 masonry action 上抛。

**Architecture:** 新文件与旧 winit 桥**并存**（旧 `editor_app.rs`/`winit_bridge.rs` 与旧依赖保留，A3 宿主重写后才删）。适配器零功能逻辑：事件映射在 `masonry_events.rs`（纯函数），焦点/IME 会话/指针捕获/剪贴板全由 masonry 承担。缩放差异按 spec——**ctrl+wheel 直接发 `ZoomAt{factor, center}`，widget 不设 zoom 镜像**。A 阶段中间态：`OfdCommand = Arc<dyn Fn(&mut EditorComponent)>`（C 阶段组件改名后自然变为 `&mut OfdComponent`）。

**Tech Stack:** xilem/masonry rev `271a27a6…`（masonry_testing 同 rev）、imaging 0.0.1（`Painter::replay` 组件 Scene）。dpi 类型经 `xilem::masonry::dpi` 取得（ui-events 0.3 同源 dpi 2.x），不另加 dpi 依赖。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §2.2（rofd zoom 差异：乘法基线、ZoomAt、无镜像）、§3.3（OfdWidgetAction 12 变体）、§3.4（widget 字段/触点）、§3.5（View）、§6.2（七个 harness 测试）。

## Global Constraints

- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- **适配器不实现功能**：状态机/几何/业务逻辑一律留在 component；本计划只做事件映射、场景回放、平台装配。
- **旧桥保持可编译可运行**：A2 全程不删旧文件、不改旧宿主行为；旧依赖（winit/arboard/rofd-io）保留，A3 统一删。
- 库内禁止挂钟时间；widget 不直接读时钟（blink 由组件 `tick_blink` 承担）。
- 代码注释/变量/文件名不得出现 "WPS" 字样。
- 无裸 `unwrap`（锁获取 `.unwrap()` 属于"线程永不 panic"约定，允许——与 rword as-built 一致）。

## File Structure

| 文件 | 动作 | 职责 |
| --- | --- | --- |
| `crates/native-view/Cargo.toml` | 改 | 新增 `xilem`；dev 新增 `masonry_testing`（旧依赖保留） |
| `crates/native-view/src/masonry_events.rs` | 新建 | masonry 事件 → rofd `ViewEvent` 纯翻译（f64） |
| `crates/native-view/src/ofd_widget.rs` | 新建 | `OfdWidget` + `OfdWidgetAction` + `OfdCommand` |
| `crates/native-view/src/ofd_view.rs` | 新建 | `OfdView` + `ofd()`/`ofd_with_config()` |
| `crates/native-view/src/lib.rs` | 改 | 分步注册/导出新模块（旧导出保留到 Task 3） |
| `crates/native-view/tests/ofd_widget_harness.rs` | 新建 | spec §6.2 七个 harness 测试 |

---

### Task 1: masonry_events 纯翻译表

**Files:**
- Modify: `crates/native-view/Cargo.toml`
- Create: `crates/native-view/src/masonry_events.rs`
- Modify: `crates/native-view/src/lib.rs`（注册新模块，旧模块保留）

**Interfaces:**
- Consumes: A1 的 rofd-component 公共事件类型（`ViewEvent`/`Key`/`Modifiers`/`MouseButton`）。
- Produces（`rofd_native_view::masonry_events`，经 lib.rs `pub mod` 可达）：
  - `pub const SCROLL_LINE_PX: f64 = 20.0`
  - `pub fn rofd_modifiers(&MasonryModifiers) -> Modifiers`
  - `pub fn named_key(&NamedKey) -> Option<Key>`
  - `pub fn key_down_events(&MasonryKey, &MasonryModifiers) -> Vec<ViewEvent>`
  - `pub fn mouse_button(Option<&PointerButton>) -> Option<MouseButton>`
  - `pub fn scroll_deltas(&ScrollDelta, scale_factor: f64, page_px: f64) -> (f64, f64)`

- [ ] **Step 1: Cargo.toml 新增依赖（旧依赖全部保留）**

把 `crates/native-view/Cargo.toml` 全文替换为：

```toml
[package]
name = "rofd-native-view"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-component = { workspace = true }
rofd-render = { workspace = true }
rofd-io = { workspace = true }
rofd-dom = { workspace = true }
winit = { workspace = true }
arboard = "3"
xilem = { workspace = true }

[dev-dependencies]
kurbo = { workspace = true }
masonry_testing = { workspace = true }
```

说明：旧桥（editor_app/winit_bridge）与旧测试 A3 才删，旧依赖必须保留；masonry_testing 供 Task 4 harness 使用。

- [ ] **Step 2: 新建 masonry_events.rs（含 9 个测试）**

`crates/native-view/src/masonry_events.rs` 全文：

```rust
//! Pure translation from masonry event types (ui-events / keyboard-types)
//! to rofd `ViewEvent`s.
//!
//! This module holds no widget state: every function is a pure mapping,
//! unit-testable without a window or widget tree. It is the masonry
//! successor to the retired winit-bridge translation table.
//!
//! Sign conventions (load-bearing, verified against sources):
//! - ui-events-winit passes winit wheel signs through verbatim
//!   (`MouseScrollDelta::LineDelta(x, y) -> ScrollDelta::LineDelta(x, y)`),
//!   so positive `y` still means "scrolled up" (towards the user).
//! - The component expects web-style deltas: positive `dy` scrolls DOWN,
//!   positive `dx` scrolls RIGHT (`ViewEvent::Scroll`).
//! - Therefore `y` is negated; `x` passes through. Logical pixels.

use rofd_component::{Key, Modifiers, MouseButton, ViewEvent};
use xilem::masonry::core::keyboard::{Key as MasonryKey, NamedKey};
use xilem::masonry::core::{Modifiers as MasonryModifiers, PointerButton, ScrollDelta};

/// Logical pixels scrolled per wheel "line". Matches the legacy
/// winit-bridge constant (20 px) and typical desktop scroll distance.
pub const SCROLL_LINE_PX: f64 = 20.0;

/// Convert masonry (keyboard-types) modifiers to component modifiers.
pub fn rofd_modifiers(m: &MasonryModifiers) -> Modifiers {
    Modifiers {
        shift: m.shift(),
        control: m.ctrl(),
        alt: m.alt(),
        meta: m.meta(),
    }
}

/// Map a keyboard-types logical (named) key to the component `Key`.
///
/// Returns `None` for everything not in the table (dead keys, raw
/// numerics, F-keys); `key_down_events` then emits nothing.
pub fn named_key(key: &NamedKey) -> Option<Key> {
    use NamedKey as N;
    Some(match key {
        N::Enter => Key::Enter,
        N::Backspace => Key::Backspace,
        N::Delete => Key::Delete,
        N::Tab => Key::Tab,
        N::Escape => Key::Escape,
        N::ArrowLeft => Key::ArrowLeft,
        N::ArrowRight => Key::ArrowRight,
        N::ArrowUp => Key::ArrowUp,
        N::ArrowDown => Key::ArrowDown,
        N::Home => Key::Home,
        N::End => Key::End,
        N::PageUp => Key::PageUp,
        N::PageDown => Key::PageDown,
        _ => return None,
    })
}

/// Translate a key-down into the `ViewEvent`s to dispatch.
///
/// - Named keys map through [`named_key`]; unmapped ones produce nothing.
/// - `Character` produces one KeyDown for the string's first char (masonry
///   strings are single graphemes in this revision). The component applies
///   its own ctrl/alt/meta guard, so no filtering happens here.
pub fn key_down_events(key: &MasonryKey, modifiers: &MasonryModifiers) -> Vec<ViewEvent> {
    let mods = rofd_modifiers(modifiers);
    match key {
        MasonryKey::Named(named) => named_key(named).map(|k| ViewEvent::KeyDown {
            key: k,
            modifiers: mods,
        }),
        MasonryKey::Character(s) => s
            .chars()
            .map(|c| ViewEvent::KeyDown {
                key: Key::Char(c),
                modifiers: mods.clone(),
            })
            .next(),
    }
    .into_iter()
    .collect()
}

/// Map a masonry pointer button to the component mouse button.
///
/// `None` (touch contact) and unmapped back/forward buttons produce no
/// event — matching the legacy bridge behaviour.
pub fn mouse_button(button: Option<&PointerButton>) -> Option<MouseButton> {
    match button {
        Some(PointerButton::Primary) => Some(MouseButton::Left),
        Some(PointerButton::Secondary) => Some(MouseButton::Right),
        Some(PointerButton::Auxiliary) => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Wheel deltas as `(dx, dy)` in logical pixels, web convention (positive
/// `y` scrolls down).
///
/// `scale_factor` converts physical-px `PixelDelta`s to logical; `page_px`
/// is the page-scroll policy (the visible height) for `PageDelta`.
pub fn scroll_deltas(delta: &ScrollDelta, scale_factor: f64, page_px: f64) -> (f64, f64) {
    match *delta {
        ScrollDelta::LineDelta(x, y) => (x * SCROLL_LINE_PX, -y * SCROLL_LINE_PX),
        ScrollDelta::PageDelta(x, y) => (x * page_px, -y * page_px),
        // PixelDelta arrives in physical px (trackpads); component viewport
        // is logical. The legacy bridge skipped this division — bug fixed.
        ScrollDelta::PixelDelta(p) => (p.x / scale_factor, -p.y / scale_factor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xilem::masonry::core::keyboard::Key as MK;
    use xilem::masonry::dpi::PhysicalPosition;

    fn mods(ctrl: bool) -> MasonryModifiers {
        let mut m = MasonryModifiers::empty();
        if ctrl {
            m |= MasonryModifiers::CONTROL;
        }
        m
    }

    #[test]
    fn named_key_table() {
        use NamedKey as N;
        assert_eq!(named_key(&N::Enter), Some(Key::Enter));
        assert_eq!(named_key(&N::Backspace), Some(Key::Backspace));
        assert_eq!(named_key(&N::ArrowLeft), Some(Key::ArrowLeft));
        assert_eq!(named_key(&N::PageUp), Some(Key::PageUp));
        assert_eq!(named_key(&N::Home), Some(Key::Home));
        assert!(named_key(&N::F1).is_none());
    }

    #[test]
    fn character_key_maps_to_char() {
        let evs = key_down_events(&MK::Character("a".into()), &mods(false));
        assert_eq!(evs.len(), 1);
        assert!(matches!(
            evs[0],
            ViewEvent::KeyDown {
                key: Key::Char('a'),
                ..
            }
        ));
    }

    #[test]
    fn character_key_passes_ctrl_through() {
        // The component owns the ctrl guard ("skip if control/alt/meta
        // held"); the translator must not pre-filter.
        let evs = key_down_events(&MK::Character("s".into()), &mods(true));
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], ViewEvent::KeyDown { modifiers, .. } if modifiers.control));
    }

    #[test]
    fn unmapped_named_key_maps_to_nothing() {
        assert!(key_down_events(&MK::Named(NamedKey::F1), &mods(false)).is_empty());
        assert!(key_down_events(&MK::Character("".into()), &mods(false)).is_empty());
    }

    #[test]
    fn modifiers_map_fields() {
        let mut m = MasonryModifiers::empty();
        m |= MasonryModifiers::SHIFT | MasonryModifiers::ALT;
        let r = rofd_modifiers(&m);
        assert!(r.shift && r.alt && !r.control && !r.meta);
    }

    #[test]
    fn line_delta_sign_and_scale() {
        // winit: positive y = scroll up → component dy must be negative
        // (web convention: positive scrolls down).
        let (dx, dy) = scroll_deltas(&ScrollDelta::LineDelta(1.0, -3.0), 1.0, 800.0);
        assert_eq!(dx, 20.0);
        assert_eq!(dy, 60.0);
    }

    #[test]
    fn pixel_delta_converts_to_logical() {
        let (dx, dy) = scroll_deltas(
            &ScrollDelta::PixelDelta(PhysicalPosition::new(100.0, -50.0)),
            2.0,
            800.0,
        );
        assert_eq!(dx, 50.0);
        assert_eq!(dy, 25.0);
    }

    #[test]
    fn page_delta_uses_visible_height() {
        let (dx, dy) = scroll_deltas(&ScrollDelta::PageDelta(1.0, 1.0), 1.0, 500.0);
        assert_eq!(dx, 500.0);
        assert_eq!(dy, -500.0);
    }

    #[test]
    fn mouse_button_mapping() {
        assert_eq!(
            mouse_button(Some(&PointerButton::Primary)),
            Some(MouseButton::Left)
        );
        assert_eq!(
            mouse_button(Some(&PointerButton::Secondary)),
            Some(MouseButton::Right)
        );
        assert_eq!(
            mouse_button(Some(&PointerButton::Auxiliary)),
            Some(MouseButton::Middle)
        );
        assert!(mouse_button(Some(&PointerButton::X1)).is_none());
        assert!(mouse_button(None).is_none());
    }
}
```

- [ ] **Step 3: lib.rs 注册新模块（旧模块全部保留）**

把 `crates/native-view/src/lib.rs` 全文替换为：

```rust
//! rofd-native-view - native adapter for rofd.
//!
//! Dual surface during transform A: the new masonry/xilem adapter
//! (`OfdWidget`/`OfdView`, files added incrementally) coexists with the
//! legacy winit bridge (`EditorApp`/`WinitEventBridge`) until the host
//! rewrite (A3).

pub mod editor_app;
pub mod masonry_events;
pub mod winit_bridge;

pub use editor_app::EditorApp;
pub use winit_bridge::WinitEventBridge;
```

- [ ] **Step 4: 验证与提交**

```bash
cargo test -p rofd-native-view masonry_events
cargo clippy -p rofd-native-view --all-targets -- -D warnings
cargo fmt -p rofd-native-view
git add crates/native-view/Cargo.toml crates/native-view/src/masonry_events.rs crates/native-view/src/lib.rs
git commit -m "feat(native-view): masonry 事件纯翻译表（masonry_events）"
```

Expected: 9 个测试全部通过。

---

### Task 2: OfdWidget

**Files:**
- Create: `crates/native-view/src/ofd_widget.rs`
- Modify: `crates/native-view/src/lib.rs`（加注册）

**Interfaces:**
- Consumes: Task 1 翻译表；A1 组件公共面——`new_native`、12 个 on_* 安装器（native 臂接收 `impl Fn … + Send`，**直接传闭包，不包 Box**）、`caret_rect() -> Option<rofd_dom::Rect>`、`copy_selection() -> Option<String>`、`paste_text(&str) -> bool`、`update_scene()`/`scene() -> &Scene`、`set_viewport_size(f64,f64)`、`tick_blink() -> bool`。
- Produces:
  - `pub type OfdCommand = Arc<dyn Fn(&mut EditorComponent) + Send + Sync>`
  - `pub type OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>`；`pub fn command_queue() -> OfdCommandQueue`
  - `pub enum OfdWidgetAction`（12 变体）
  - `pub struct OfdWidget`
  - 静态 API：`OfdWidget::with_component(&mut WidgetMut, &OfdCommand)`、`OfdWidget::copy_selection(&mut WidgetMut) -> bool`

**Ctrl+X 决策（本任务明确锁定）：** rofd 无 `cut_selection`——body 运行时只读、`TextCursor` 无选区端点，"剪切"语义不存在。Ctrl+X **按 Ctrl+C 处理（只复制、不删除）**，不新增组件方法；宿主菜单不提供 Cut 项（A3）。

- [ ] **Step 1: 新建 ofd_widget.rs**

`crates/native-view/src/ofd_widget.rs` 全文：

```rust
//! Masonry `Widget` hosting an `EditorComponent`.
//!
//! The widget owns the component and is the single touch point between
//! masonry's event/layout/paint passes and rofd's platform-agnostic
//! `ViewEvent` surface:
//!
//! - pointer/keyboard/IME events are translated ([`crate::masonry_events`])
//!   and forwarded to the component;
//! - component callbacks (which fire synchronously inside `handle_event`
//!   and cannot reach a widget context) are queued and flushed as masonry
//!   actions at the end of every touch point;
//! - `paint` replays the component's cached `imaging::record::Scene`
//!   (same type masonry paints with — zero conversion);
//! - the caret blink is driven from `on_anim_frame` calling the
//!   component's own `tick_blink`.
//!
//! Hosts never handle winit events: focus, pointer capture, IME sessions
//! and clipboard routing are masonry's.

use std::sync::{Arc, Mutex};

use rofd_component::{
    AnnotationId, AnnotationSelection, BodyTextSelection, ContextTarget, EditorComponent,
    EditorConfig, MouseButton, OfdWarning, PointerCursor, TextCursor, ViewEvent,
};
use xilem::masonry::accesskit::{Node, Role};
use xilem::masonry::core::keyboard::{Key as MasonryKey, KeyState};
use xilem::masonry::core::{
    AccessCtx,
    ChildrenIds,
    LayoutCtx,
    MeasureCtx,
    PaintCtx,
    PropertiesMut,
    PropertiesRef,
    QueryCtx,
    RegisterCtx,
    Widget,
    WidgetMut,
};
use xilem::masonry::core::{CursorIcon, EventCtx, Ime, PointerEvent, TextEvent, Update, UpdateCtx};
use xilem::masonry::imaging::Painter;
use xilem::masonry::kurbo::{Axis, Point, Rect, Size};
use xilem::masonry::layout::{LenReq, Length};

use crate::masonry_events;

/// Multiplicative zoom step per ctrl+wheel tick (the component does the
/// boundary clamp; this widget holds no zoom mirror).
pub const ZOOM_IN_STEP: f64 = 1.1;
pub const ZOOM_OUT_STEP: f64 = 0.9;

/// Fallback preferred length when the parent offers unbounded space.
const DEFAULT_LENGTH: Length = Length::const_px(800.0);

/// A host command: closure run against the embedded `EditorComponent`.
///
/// `Arc<dyn Fn>` (not `Box<dyn FnOnce>`): buttons re-fire, so closures
/// must not move captured data on call — capture by move, use by reference.
///
/// A-stage intermediate: transform C renames the component, and this
/// becomes `Fn(&mut OfdComponent)`.
pub type OfdCommand = Arc<dyn Fn(&mut EditorComponent) + Send + Sync>;
/// Shared queue the host pushes commands into; the `ofd()` view drains it
/// on every rebuild.
pub type OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>;

/// Create an empty command queue.
pub fn command_queue() -> OfdCommandQueue {
    Arc::new(Mutex::new(Vec::new()))
}

/// Actions the widget submits to the xilem layer. Mirrors the component
/// callback surface; produced by draining the pending queue.
///
/// [`OfdWidgetAction::PointerCursorChanged`] is internal: the drain
/// consumes it to update `get_cursor` and never submits it.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum OfdWidgetAction {
    /// `on_change` — document changed (hosts query on demand).
    Changed,
    SelectionChanged(AnnotationSelection),
    CursorChanged(Option<TextCursor>),
    SaveRequested,
    ContextMenu {
        pos: (f64, f64),
        target: ContextTarget,
    },
    AnnotationFocus(AnnotationId),
    AnnotationInteract(AnnotationId),
    PageChanged(usize),
    ZoomChanged(f64),
    TextSelectionChanged(Option<BodyTextSelection>),
    Warnings(Vec<OfdWarning>),
    PointerCursorChanged(PointerCursor),
}

/// Recompute effective focus and notify the component on transitions.
///
/// Effective focus = widget keyboard focus AND window focus. A macro (not
/// a method) because it runs against both `EventCtx` and `UpdateCtx`,
/// which share no trait.
macro_rules! refresh_effective_focus {
    ($self:expr, $ctx:expr) => {{
        let effective = $self.widget_focused && $self.window_focused;
        if effective != $self.component_focused {
            $self.component_focused = effective;
            let event = if effective {
                ViewEvent::FocusGained
            } else {
                ViewEvent::FocusLost
            };
            $self.component.handle_event(&event);
            if effective {
                // (Re)start the blink anim loop.
                $ctx.request_anim_frame();
            }
            $ctx.request_paint_only();
        }
    }};
}

/// Flush queued callback events as masonry actions, refresh the IME area
/// from the caret rect, and request a repaint. End of every touch point
/// (event handlers, focus updates, host commands).
macro_rules! after_touch {
    ($self:expr, $ctx:expr) => {{
        let queued = std::mem::take(&mut *$self.pending.lock().unwrap());
        let mut cursor_changed = false;
        for action in queued {
            if let OfdWidgetAction::PointerCursorChanged(cursor) = action {
                $self.cursor = cursor;
                cursor_changed = true;
            } else {
                // Turbofish: `impl Into<Action>` alone is ambiguous.
                $ctx.submit_action::<OfdWidgetAction>(action);
            }
        }
        if cursor_changed {
            $ctx.request_cursor_icon_change();
        }
        // Keep the IME candidate window glued to the caret. `caret_rect`
        // returns viewport (= content-box) coordinates; None = no cursor.
        if $self.widget_focused {
            if let Some(caret) = $self.component.caret_rect() {
                $ctx.set_ime_area(Rect::new(
                    caret.x,
                    caret.y,
                    caret.x + caret.w,
                    caret.y + caret.h,
                ));
            } else {
                $ctx.clear_ime_area();
            }
        } else {
            $ctx.clear_ime_area();
        }
        $ctx.request_paint_only();
    }};
}

/// Masonry widget embedding an [`EditorComponent`]. Construct via
/// [`OfdWidget::new`]; integrate through the `ofd` xilem view.
pub struct OfdWidget {
    /// The platform-agnostic component — the only core hosts edit.
    component: EditorComponent,
    /// Callback events queued by 'static component callbacks (they cannot
    /// borrow the widget): Arc<Mutex<Vec>>, drained at each touch.
    pending: Arc<Mutex<Vec<OfdWidgetAction>>>,
    /// Desired pointer cursor (mirrored from on_pointer_cursor).
    cursor: PointerCursor,
    /// Widget has keyboard focus (Update::FocusChanged).
    widget_focused: bool,
    /// Window focus (TextEvent::WindowFocusChange). Focused at birth.
    window_focused: bool,
    /// Mirror of the component focus: transitions only.
    component_focused: bool,
    /// Last laid-out size, to detect viewport changes.
    size: Size,
}

impl OfdWidget {
    /// Build a widget hosting a component with the given config and all
    /// component callbacks wired into the internal pending queue.
    ///
    /// The component starts unfocused: no caret until the first click.
    pub fn new(config: EditorConfig) -> Self {
        let mut component = EditorComponent::new_native(config);
        let pending: Arc<Mutex<Vec<OfdWidgetAction>>> = Arc::new(Mutex::new(Vec::new()));

        fn queue(pending: &Arc<Mutex<Vec<OfdWidgetAction>>>, action: OfdWidgetAction) {
            pending.lock().unwrap().push(action);
        }

        let p = pending.clone();
        component.on_change(move |_| queue(&p, OfdWidgetAction::Changed));
        let p = pending.clone();
        component.on_selection_change(move |sel| {
            queue(&p, OfdWidgetAction::SelectionChanged(sel.clone()))
        });
        let p = pending.clone();
        component.on_cursor_change(move |cursor| {
            queue(&p, OfdWidgetAction::CursorChanged(cursor.cloned()))
        });
        let p = pending.clone();
        component.on_save_request(move || queue(&p, OfdWidgetAction::SaveRequested));
        let p = pending.clone();
        component.on_context_menu(move |pos, target| {
            queue(&p, OfdWidgetAction::ContextMenu { pos, target })
        });
        let p = pending.clone();
        component.on_annotation_focus(move |id| {
            queue(&p, OfdWidgetAction::AnnotationFocus(id.clone()))
        });
        let p = pending.clone();
        component.on_annotation_interact(move |id| {
            queue(&p, OfdWidgetAction::AnnotationInteract(id.clone()))
        });
        let p = pending.clone();
        component.on_page_change(move |idx| queue(&p, OfdWidgetAction::PageChanged(idx)));
        let p = pending.clone();
        component.on_zoom_change(move |factor| {
            queue(&p, OfdWidgetAction::ZoomChanged(factor))
        });
        let p = pending.clone();
        component.on_text_selection_change(move |sel| {
            queue(&p, OfdWidgetAction::TextSelectionChanged(sel.cloned()))
        });
        let p = pending.clone();
        component.on_warning(move |warnings| {
            queue(&p, OfdWidgetAction::Warnings(warnings.to_vec()))
        });
        let p = pending.clone();
        component.on_pointer_cursor(move |cursor| {
            queue(&p, OfdWidgetAction::PointerCursorChanged(cursor))
        });

        // Align the component with the widget's actual unfocused state.
        component.handle_event(&ViewEvent::FocusLost);

        Self {
            component,
            pending,
            cursor: PointerCursor::Default,
            widget_focused: false,
            window_focused: true,
            component_focused: false,
            size: Size::ZERO,
        }
    }

    // --- Static WidgetMut API (host-side imperative access) ---

    /// Run a host command against the embedded component, then flush
    /// queued callbacks and refresh IME/paint. The host→widget channel.
    pub fn with_component(this: &mut WidgetMut<'_, Self>, command: &OfdCommand) {
        command(&mut this.widget.component);
        after_touch!(this.widget, this.ctx);
    }

    /// Copy the current selection to the OS clipboard. False when empty.
    pub fn copy_selection(this: &mut WidgetMut<'_, Self>) -> bool {
        if let Some(text) = this.widget.component.copy_selection() {
            this.ctx.set_clipboard(text);
            true
        } else {
            false
        }
    }

    // --- Event plumbing ---

    fn after_event(&mut self, ctx: &mut EventCtx<'_>) {
        after_touch!(self, ctx);
    }

    fn after_update(&mut self, ctx: &mut UpdateCtx<'_>) {
        after_touch!(self, ctx);
    }

    fn handle_ime(&mut self, ctx: &mut EventCtx<'_>, ime: &Ime) {
        match ime {
            Ime::Enabled => {
                // Nothing to do: masonry started the session because we
                // report `accepts_text_input`.
            }
            Ime::Preedit(text, cursor) => {
                self.component.handle_event(&ViewEvent::ImePreedit {
                    text: text.clone(),
                    caret: *cursor,
                });
                ctx.set_handled();
            }
            Ime::Commit(text) => {
                self.component
                    .handle_event(&ViewEvent::ImeCommit { text: text.clone() });
                ctx.set_handled();
            }
            Ime::Disabled => {
                // Disabled maps to FocusLost to trigger the component's
                // preedit force-commit. A real FocusChanged follows; when
                // the platform cancels IME without one, restore focus.
                self.component.handle_event(&ViewEvent::FocusLost);
                self.component_focused = false;
                if self.widget_focused && self.window_focused {
                    self.component.handle_event(&ViewEvent::FocusGained);
                    self.component_focused = true;
                }
                ctx.set_handled();
            }
        }
    }
}

impl Widget for OfdWidget {
    type Action = OfdWidgetAction;

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        match event {
            PointerEvent::Down(btn) => {
                // Click-to-focus (TextArea pattern): applied after the
                // current pass, like every other masonry text widget.
                ctx.request_focus();
                let Some(button) = masonry_events::mouse_button(btn.button.as_ref()) else {
                    return;
                };
                if button == MouseButton::Left {
                    // Keep Move/Up arriving while dragging past the bounds.
                    ctx.capture_pointer();
                }
                let pos = ctx.local_position(btn.state.position);
                self.component.handle_event(&ViewEvent::PointerDown {
                    button,
                    x: pos.x,
                    y: pos.y,
                    modifiers: masonry_events::rofd_modifiers(&btn.state.modifiers),
                    // ui-events count is already u8 (matches rofd ViewEvent).
                    click_count: btn.state.count,
                });
                ctx.set_handled();
                self.after_event(ctx);
            }
            PointerEvent::Move(update) => {
                let pos = ctx.local_position(update.current.position);
                self.component.handle_event(&ViewEvent::PointerMove { x: pos.x, y: pos.y });
                self.after_event(ctx);
            }
            PointerEvent::Up(btn) => {
                let Some(button) = masonry_events::mouse_button(btn.button.as_ref()) else {
                    return;
                };
                let pos = ctx.local_position(btn.state.position);
                self.component.handle_event(&ViewEvent::PointerUp {
                    button,
                    x: pos.x,
                    y: pos.y,
                    modifiers: masonry_events::rofd_modifiers(&btn.state.modifiers),
                });
                ctx.set_handled();
                self.after_event(ctx);
            }
            PointerEvent::Scroll(scroll) => {
                let ctrl = scroll.state.modifiers.ctrl() || scroll.state.modifiers.meta();
                // Page-scroll policy: one page = the visible height.
                let (dx, dy) = masonry_events::scroll_deltas(
                    &scroll.delta,
                    ctx.scale_factor(),
                    self.size.height,
                );
                if ctrl {
                    // Wheel-down (dy > 0 after conversion) zooms out,
                    // wheel-up zooms in. Multiplicative; the component
                    // clamps to its own [MIN_ZOOM, MAX_ZOOM] (no mirror).
                    let factor = if dy > 0.0 {
                        ZOOM_OUT_STEP
                    } else {
                        ZOOM_IN_STEP
                    };
                    let center = ctx.local_position(scroll.state.position);
                    self.component.handle_event(&ViewEvent::ZoomAt {
                        factor,
                        center: (center.x, center.y),
                    });
                } else {
                    self.component.handle_event(&ViewEvent::Scroll { dx, dy });
                }
                // Consume so no outer scroll container pans too.
                ctx.set_handled();
                self.after_event(ctx);
            }
            _ => {}
        }
    }

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        match event {
            TextEvent::Keyboard(key) => {
                if key.state == KeyState::Down && !key.repeat && !key.is_composing {
                    // Clipboard shortcuts: the component cannot reach the
                    // OS clipboard, so Ctrl+C is intercepted here instead
                    // of forwarded (the component's ctrl guard would drop
                    // it). Ctrl+X has no cut semantics on rofd (body is
                    // read-only and TextCursor has no selection extent),
                    // so it behaves as copy-only — never delete.
                    // Ctrl+V arrives as ClipboardPaste via the winit
                    // backend's own interception.
                    let ctrl = key.modifiers.ctrl() || key.modifiers.meta();
                    if ctrl {
                        if let MasonryKey::Character(s) = &key.key {
                            if matches!(s.as_str(), "c" | "C" | "x" | "X") {
                                if let Some(text) = self.component.copy_selection() {
                                    ctx.set_clipboard(text);
                                }
                                ctx.set_handled();
                                self.after_event(ctx);
                                return;
                            }
                        }
                    }
                    let events = masonry_events::key_down_events(&key.key, &key.modifiers);
                    if !events.is_empty() {
                        for view_event in events {
                            self.component.handle_event(&view_event);
                        }
                        ctx.set_handled();
                        self.after_event(ctx);
                    }
                }
            }
            TextEvent::Ime(ime) => {
                self.handle_ime(ctx, ime);
                self.after_event(ctx);
            }
            // The winit backend intercepts Ctrl+V and delivers clipboard.
            TextEvent::ClipboardPaste(text) => {
                self.component.paste_text(text);
                ctx.set_handled();
                self.after_event(ctx);
            }
            TextEvent::WindowFocusChange(focused) => {
                self.window_focused = *focused;
                refresh_effective_focus!(self, ctx);
                self.after_event(ctx);
            }
        }
    }

    fn on_anim_frame(
        &mut self,
        ctx: &mut UpdateCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        _interval: u64,
    ) {
        if !(self.widget_focused && self.window_focused) {
            return;
        }
        // Keep the vsync anim loop alive while focused; `tick_blink`
        // self-guards on monotonic time (500 ms flips) and reports whether
        // the caret visibility changed.
        ctx.request_anim_frame();
        if self.component.tick_blink() {
            ctx.request_paint_only();
        }
        let queued = std::mem::take(&mut *self.pending.lock().unwrap());
        for action in queued {
            if let OfdWidgetAction::PointerCursorChanged(cursor) = action {
                self.cursor = cursor;
                ctx.request_cursor_icon_change();
            } else {
                ctx.submit_action::<OfdWidgetAction>(action);
            }
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>, _props: &mut PropertiesMut<'_>, event: &Update) {
        if let Update::FocusChanged(focused) = event {
            self.widget_focused = *focused;
            refresh_effective_focus!(self, ctx);
            self.after_update(ctx);
        }
    }

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        _axis: Axis,
        len_req: LenReq,
        _cross_length: Option<Length>,
    ) -> Length {
        // Fill whatever space the parent offers (the editor owns its own
        // scrolling; there is no intrinsic content size to report).
        match len_req {
            LenReq::FitContent(space) => space,
            _ => DEFAULT_LENGTH,
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
        if self.size != size {
            self.size = size;
            self.component.set_viewport_size(size.width, size.height);
        }
        ctx.set_clip_path(size.to_rect());
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        painter: &mut Painter<'_>,
    ) {
        // update_scene recomposes only while the dirty flag is set and
        // never fires callbacks, so painting needs no action flush.
        self.component.update_scene();
        painter.replay(self.component.scene());
    }

    fn get_cursor(&self, _ctx: &QueryCtx<'_>, _pos: Point) -> CursorIcon {
        match self.cursor {
            PointerCursor::Default => CursorIcon::Default,
            PointerCursor::Grab => CursorIcon::Grab,
            PointerCursor::Grabbing => CursorIcon::Grabbing,
            PointerCursor::Text => CursorIcon::Text,
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Document
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
        // Full text accessibility is future work.
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_starts_unfocused() {
        // Constructing the widget registers 12 callbacks on the component
        // and sends an initial FocusLost. The invariant that matters is
        // the focus mirror: no caret until the widget is focused.
        let widget = OfdWidget::new(EditorConfig::new(Arc::new(vec![])));
        assert!(!widget.component_focused);
        assert!(!widget.widget_focused);
        assert!(widget.window_focused);
        assert_eq!(widget.cursor, PointerCursor::Default);
    }
}
```

- [ ] **Step 2: lib.rs 加注册**

把 `crates/native-view/src/lib.rs` 全文替换为：

```rust
//! rofd-native-view - native adapter for rofd.
//!
//! Dual surface during transform A: the new masonry/xilem adapter
//! (`OfdWidget`; `OfdView` arrives next) coexists with the legacy winit
//! bridge (`EditorApp`/`WinitEventBridge`) until the host rewrite (A3).

pub mod editor_app;
pub mod masonry_events;
pub mod ofd_widget;
pub mod winit_bridge;

pub use editor_app::EditorApp;
pub use winit_bridge::WinitEventBridge;
```

- [ ] **Step 3: 验证与提交**

```bash
cargo test -p rofd-native-view ofd_widget
cargo clippy -p rofd-native-view --all-targets -- -D warnings
cargo fmt -p rofd-native-view
git add crates/native-view/src/ofd_widget.rs crates/native-view/src/lib.rs
git commit -m "feat(native-view): OfdWidget——masonry Widget 承载 EditorComponent"
```

Expected: widget_starts_unfocused 通过；Task 1 的 9 个测试保持绿色；旧桥照常编译。

---

### Task 3: OfdView

**Files:**
- Create: `crates/native-view/src/ofd_view.rs`
- Modify: `crates/native-view/src/lib.rs`（注册 + 导出新公共面）

**Interfaces:**
- Consumes: `OfdCommandQueue`、`OfdWidget::with_component`、`OfdWidgetAction`。
- Produces:
  - `pub fn ofd<State: 'static>(OfdCommandQueue) -> OfdView<State>`
  - `pub fn ofd_with_config<State: 'static>(OfdCommandQueue, EditorConfig) -> OfdView<State>`
  - `pub struct OfdView<State>` + 11 个 `on_*` chainers
  - `pub type OfdContextMenu = ((f64, f64), ContextTarget)`

- [ ] **Step 1: 新建 ofd_view.rs**

`crates/native-view/src/ofd_view.rs` 全文：

```rust
//! Xilem `View` wrapping [`OfdWidget`].
//!
//! `ofd(queue)` embeds the widget in a xilem tree and exposes the
//! component callback surface as chainable handlers:
//!
//! ```ignore
//! ofd(state.commands.clone())
//!     .on_change(|state| state.mark_modified())
//!     .on_context_menu(|state, payload| state.open_menu(payload))
//! ```
//!
//! Host→widget imperative access (toolbar, file loads) goes through the
//! shared [`OfdCommandQueue`]: push closures from handlers; every rebuild
//! following `MessageResult::Action(())` drains them into
//! [`OfdWidget::with_component`].
//!
//! The `Action` type parameter is fixed to `()`: any handled callback
//! yields `MessageResult::Action(())`, rerunning the app logic.

use std::marker::PhantomData;

use rofd_component::{
    AnnotationId, AnnotationSelection, BodyTextSelection, ContextTarget, EditorConfig, OfdWarning,
    TextCursor,
};
use xilem::core::{MessageCtx, MessageResult, Mut, View, ViewMarker};
use xilem::{Pod, ViewCtx};

use crate::ofd_widget::{OfdCommandQueue, OfdWidget, OfdWidgetAction};

/// Callback handler: app state + event payload.
type Handler<State, Payload> = Box<dyn Fn(&mut State, Payload) + Send + Sync>;

/// Right-click context-menu payload: viewport position + target.
pub type OfdContextMenu = ((f64, f64), ContextTarget);

/// The xilem view for an OFD widget. Create with [`ofd`] / [`ofd_with_config`].
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct OfdView<State> {
    queue: OfdCommandQueue,
    config: EditorConfig,
    on_change: Option<Handler<State, ()>>,
    on_selection_change: Option<Handler<State, AnnotationSelection>>,
    on_cursor_change: Option<Handler<State, Option<TextCursor>>>,
    on_save_request: Option<Handler<State, ()>>,
    on_context_menu: Option<Handler<State, OfdContextMenu>>,
    on_annotation_focus: Option<Handler<State, AnnotationId>>,
    on_annotation_interact: Option<Handler<State, AnnotationId>>,
    on_page_change: Option<Handler<State, usize>>,
    on_zoom_change: Option<Handler<State, f64>>,
    on_text_selection_change: Option<Handler<State, Option<BodyTextSelection>>>,
    on_warnings: Option<Handler<State, Vec<OfdWarning>>>,
    phantom: PhantomData<fn() -> State>,
}

/// Embed an OFD widget with the default config (no registered fonts).
pub fn ofd<State: 'static>(queue: OfdCommandQueue) -> OfdView<State> {
    ofd_with_config(queue, EditorConfig::new(std::sync::Arc::new(vec![])))
}

/// Embed an OFD widget with a custom [`EditorConfig`].
pub fn ofd_with_config<State: 'static>(
    queue: OfdCommandQueue,
    config: EditorConfig,
) -> OfdView<State> {
    OfdView {
        queue,
        config,
        on_change: None,
        on_selection_change: None,
        on_cursor_change: None,
        on_save_request: None,
        on_context_menu: None,
        on_annotation_focus: None,
        on_annotation_interact: None,
        on_page_change: None,
        on_zoom_change: None,
        on_text_selection_change: None,
        on_warnings: None,
        phantom: PhantomData,
    }
}

impl<State: 'static> OfdView<State> {
    /// The document changed (query on demand via the command queue).
    pub fn on_change(mut self, f: impl Fn(&mut State) + Send + Sync + 'static) -> Self {
        self.on_change = Some(Box::new(move |state, ()| f(state)));
        self
    }

    pub fn on_selection_change(
        mut self,
        f: impl Fn(&mut State, AnnotationSelection) + Send + Sync + 'static,
    ) -> Self {
        self.on_selection_change = Some(Box::new(f));
        self
    }

    pub fn on_cursor_change(
        mut self,
        f: impl Fn(&mut State, Option<TextCursor>) + Send + Sync + 'static,
    ) -> Self {
        self.on_cursor_change = Some(Box::new(f));
        self
    }

    /// Ctrl+S save shortcut.
    pub fn on_save_request(mut self, f: impl Fn(&mut State) + Send + Sync + 'static) -> Self {
        self.on_save_request = Some(Box::new(move |state, ()| f(state)));
        self
    }

    /// Right-click context menu with viewport coordinates.
    pub fn on_context_menu(
        mut self,
        f: impl Fn(&mut State, OfdContextMenu) + Send + Sync + 'static,
    ) -> Self {
        self.on_context_menu = Some(Box::new(f));
        self
    }

    pub fn on_annotation_focus(
        mut self,
        f: impl Fn(&mut State, AnnotationId) + Send + Sync + 'static,
    ) -> Self {
        self.on_annotation_focus = Some(Box::new(f));
        self
    }

    pub fn on_annotation_interact(
        mut self,
        f: impl Fn(&mut State, AnnotationId) + Send + Sync + 'static,
    ) -> Self {
        self.on_annotation_interact = Some(Box::new(f));
        self
    }

    pub fn on_page_change(
        mut self,
        f: impl Fn(&mut State, usize) + Send + Sync + 'static,
    ) -> Self {
        self.on_page_change = Some(Box::new(f));
        self
    }

    pub fn on_zoom_change(
        mut self,
        f: impl Fn(&mut State, f64) + Send + Sync + 'static,
    ) -> Self {
        self.on_zoom_change = Some(Box::new(f));
        self
    }

    pub fn on_text_selection_change(
        mut self,
        f: impl Fn(&mut State, Option<BodyTextSelection>) + Send + Sync + 'static,
    ) -> Self {
        self.on_text_selection_change = Some(Box::new(f));
        self
    }

    pub fn on_warnings(
        mut self,
        f: impl Fn(&mut State, Vec<OfdWarning>) + Send + Sync + 'static,
    ) -> Self {
        self.on_warnings = Some(Box::new(f));
        self
    }
}

impl<State: 'static> ViewMarker for OfdView<State> {}

impl<State: 'static> View<State, (), ViewCtx> for OfdView<State> {
    type Element = Pod<OfdWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _state: &mut State) -> (Self::Element, Self::ViewState) {
        let widget = OfdWidget::new(self.config.clone());
        (ctx.with_action_widget(|ctx| ctx.create_pod(widget)), ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        _state: &mut State,
    ) {
        // Drain host commands queued since the last rerun. This is the
        // second half of the host→widget channel.
        let commands: Vec<_> = std::mem::take(&mut *self.queue.lock().unwrap());
        for command in commands {
            OfdWidget::with_component(&mut element, &command);
        }
    }

    fn teardown(&self, (): &mut Self::ViewState, _: &mut ViewCtx, _: Mut<'_, Self::Element>) {}

    fn message(
        &self,
        (): &mut Self::ViewState,
        message: &mut MessageCtx,
        _element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<()> {
        debug_assert!(
            message.remaining_path().is_empty(),
            "id path should be empty in OfdView::message"
        );
        let Some(action) = message.take_message::<OfdWidgetAction>() else {
            return MessageResult::Stale;
        };
        let mut handled = false;
        match *action {
            OfdWidgetAction::Changed => {
                if let Some(f) = &self.on_change {
                    f(app_state, ());
                    handled = true;
                }
            }
            OfdWidgetAction::SelectionChanged(sel) => {
                if let Some(f) = &self.on_selection_change {
                    f(app_state, sel);
                    handled = true;
                }
            }
            OfdWidgetAction::CursorChanged(cursor) => {
                if let Some(f) = &self.on_cursor_change {
                    f(app_state, cursor);
                    handled = true;
                }
            }
            OfdWidgetAction::SaveRequested => {
                if let Some(f) = &self.on_save_request {
                    f(app_state, ());
                    handled = true;
                }
            }
            OfdWidgetAction::ContextMenu { pos, target } => {
                if let Some(f) = &self.on_context_menu {
                    f(app_state, (pos, target));
                    handled = true;
                }
            }
            OfdWidgetAction::AnnotationFocus(id) => {
                if let Some(f) = &self.on_annotation_focus {
                    f(app_state, id);
                    handled = true;
                }
            }
            OfdWidgetAction::AnnotationInteract(id) => {
                if let Some(f) = &self.on_annotation_interact {
                    f(app_state, id);
                    handled = true;
                }
            }
            OfdWidgetAction::PageChanged(idx) => {
                if let Some(f) = &self.on_page_change {
                    f(app_state, idx);
                    handled = true;
                }
            }
            OfdWidgetAction::ZoomChanged(zoom) => {
                if let Some(f) = &self.on_zoom_change {
                    f(app_state, zoom);
                    handled = true;
                }
            }
            OfdWidgetAction::TextSelectionChanged(sel) => {
                if let Some(f) = &self.on_text_selection_change {
                    f(app_state, sel);
                    handled = true;
                }
            }
            OfdWidgetAction::Warnings(warnings) => {
                if let Some(f) = &self.on_warnings {
                    f(app_state, warnings);
                    handled = true;
                }
            }
            // Consumed internally by the widget drain; never submitted.
            OfdWidgetAction::PointerCursorChanged(_) => {}
        }
        if handled {
            MessageResult::Action(())
        } else {
            MessageResult::Nop
        }
    }
}
```

- [ ] **Step 2: lib.rs 最终形态（新公共面导出，旧桥仍保留）**

把 `crates/native-view/src/lib.rs` 全文替换为：

```rust
//! rofd-native-view - native adapter for rofd.
//!
//! Dual surface during transform A: the new masonry/xilem adapter
//! (`OfdWidget`/`OfdView`) coexists with the legacy winit bridge
//! (`EditorApp`/`WinitEventBridge`) until the host rewrite (A3).

pub mod editor_app;
pub mod masonry_events;
pub mod ofd_view;
pub mod ofd_widget;
pub mod winit_bridge;

pub use ofd_view::{ofd, ofd_with_config, OfdContextMenu, OfdView};
pub use ofd_widget::{
    command_queue, OfdCommand, OfdCommandQueue, OfdWidget, OfdWidgetAction,
};

pub use editor_app::EditorApp;
pub use winit_bridge::WinitEventBridge;
```

- [ ] **Step 3: 验证与提交**

```bash
cargo build -p rofd-native-view
cargo test -p rofd-native-view
cargo clippy -p rofd-native-view --all-targets -- -D warnings
cargo fmt -p rofd-native-view
git add crates/native-view/src/ofd_view.rs crates/native-view/src/lib.rs
git commit -m "feat(native-view): OfdView——xilem View 暴露 11 个事件链接口"
```

Expected: 全部测试绿（9 + 2）；新旧两套公共面同时可达。

---

### Task 4: OfdWidget harness 七测试

**Files:**
- Create: `crates/native-view/tests/ofd_widget_harness.rs`

**Interfaces:**
- Consumes: Task 1-3 全部公共面；`masonry_testing::TestHarness`；rofd-dom 构造类型。
- Produces: 无（测试）。

坐标约定（关键，组件出生 zoom = `PX_PER_MM`，page_gap 默认 20，page 200×200，viewport 800×600）：

- body TextObject boundary (10,20,100,40)：第一行 "ABCD"（advance 10，TextCode 原点 (0,10)）字形位于 page x 10..50、y≈20..32.5；第二行 "EF"（TextCode (0,30)）x 10..30、y≈40..52.5。
- TextBox page rect (0,100,120,40)。
- 所有点击坐标按 `BASE = 96/25.4` 放大（页面原点 (0,0)，不滚动）。

- [ ] **Step 1: 新建 harness 文件**

`crates/native-view/tests/ofd_widget_harness.rs` 全文：

```rust
//! Headless integration tests driving `OfdWidget` through
//! `masonry_testing`'s `TestHarness` (spec §6.2, seven contracts):
//! click-to-edit, double-click word select, IME preedit/commit, plain
//! wheel, ctrl+wheel ZoomAt accumulation/clamp, drag-select + Ctrl+C,
//! and window-focus caret gating.

use std::sync::{Arc, Mutex};

use masonry_testing::TestHarness;
use rofd_native_view::{OfdCommand, OfdWidget, OfdWidgetAction};
use rofd_component::EditorConfig;
use xilem::masonry::core::keyboard::{Key as MasonryKey, KeyState, NamedKey};
use xilem::masonry::core::{
    Handled, Ime, KeyboardEvent, Modifiers, PointerButton, PointerButtonEvent, PointerButtons,
    PointerEvent, PointerId, PointerInfo, PointerScrollEvent, PointerState, PointerType, ScrollDelta,
    TextEvent, Widget,
};
use xilem::masonry::dpi::{PhysicalPosition, PhysicalSize};
use xilem::masonry::theme::default_property_set;

use rofd_dom::{
    Color, FontId, Layer, LayerType, ObjectId, OfdDocument, Page, PageId, PageObject, Rect,
    TextCode, TextObject,
};

/// Component birth zoom: 96 px/inch on 25.4 mm/inch.
const BASE: f64 = 96.0 / 25.4;

/// A `PointerInfo` for the primary mouse (mirrors the harness's const).
const MOUSE: PointerInfo = PointerInfo {
    pointer_id: Some(PointerId::PRIMARY),
    persistent_device_id: None,
    pointer_type: PointerType::Mouse,
};

fn create_harness() -> TestHarness<OfdWidget> {
    TestHarness::create_with_size(
        default_property_set(),
        OfdWidget::new(EditorConfig::new(Arc::new(vec![]))).prepare(),
        PhysicalSize::new(800, 600),
    )
}

/// Command loading the shared fixture: one page with two body text lines
/// + an empty TextBox below them. Uses only the component's public API.
fn setup_command() -> OfdCommand {
    Arc::new(|c| {
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
            layers: vec![Layer {
                layer_type: LayerType::Body,
                objects: vec![PageObject::Text(TextObject {
                    id: ObjectId::new("t1"),
                    boundary: Rect {
                        x: 10.0,
                        y: 20.0,
                        w: 100.0,
                        h: 40.0,
                    },
                    ctm: None,
                    font: FontId::new("F1"),
                    size: 10.0,
                    fill: None,
                    codes: vec![
                        TextCode {
                            glyph_ids: vec![1, 2, 3, 4],
                            deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)],
                            text: "ABCD".into(),
                            x: 0.0,
                            y: 10.0,
                        },
                        TextCode {
                            glyph_ids: vec![5, 6],
                            deltas: vec![(10.0, 0.0)],
                            text: "EF".into(),
                            x: 0.0,
                            y: 30.0,
                        },
                    ],
                    draw_param: None,
                })],
            }],
            template: None,
        });
        c.load_document(doc);
        c.create_annotation(
            rofd_dom::AnnotationKind::TextBox,
            PageId::new("P0"),
            rofd_dom::AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 100.0,
                    w: 120.0,
                    h: 40.0,
                },
                content: String::new(),
                font: FontId::new("F1"),
                size: 10.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
            },
        );
    })
}

fn run_setup(harness: &mut TestHarness<OfdWidget>) {
    let command = setup_command();
    harness.edit_root_widget(|mut w| OfdWidget::with_component(&mut w, &command));
}

/// Probe the live component via the host command channel.
fn probe<R: Send + 'static>(
    harness: &mut TestHarness<OfdWidget>,
    f: impl Fn(&mut rofd_component::EditorComponent) -> R + Send + Sync + 'static,
) -> R {
    let out = Arc::new(Mutex::new(None));
    let sink = out.clone();
    let command: OfdCommand = Arc::new(move |c| *sink.lock().unwrap() = Some(f(c)));
    harness.edit_root_widget(|mut w| OfdWidget::with_component(&mut w, &command));
    out.lock().unwrap().take().expect("probe ran")
}

/// Content of the fixture TextBox.
fn textbox_text(harness: &mut TestHarness<OfdWidget>) -> String {
    probe(harness, |c| {
        c.document()
            .annotations
            .for_page(&PageId::new("P0"))
            .iter()
            .find_map(|a| match &a.payload {
                rofd_dom::AnnotationPayload::TextBox { content, .. } => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default()
    })
}

/// Press + release the primary button at (`x`, `y`) with the given click
/// count (harness convenience helpers don't track counts).
fn click(harness: &mut TestHarness<OfdWidget>, x: f64, y: f64, count: u8) {
    let state = || PointerState {
        position: PhysicalPosition::new(x, y),
        count,
        ..PointerState::default()
    };
    harness.process_pointer_event(PointerEvent::Down(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: state(),
    }));
    harness.process_pointer_event(PointerEvent::Up(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: state(),
    }));
}

/// Raw pixel-delta wheel with explicit modifiers.
fn wheel(harness: &mut TestHarness<OfdWidget>, dy: f64, modifiers: Modifiers) -> Handled {
    harness.process_pointer_event(PointerEvent::Scroll(PointerScrollEvent {
        pointer: MOUSE,
        delta: ScrollDelta::PixelDelta(PhysicalPosition::new(0.0, dy)),
        state: PointerState {
            position: PhysicalPosition::new(400.0, 300.0),
            modifiers,
            ..PointerState::default()
        },
    }))
}

/// Drain every submitted action of type T from the harness queue.
fn drain_actions<T: 'static>(harness: &mut TestHarness<OfdWidget>) -> Vec<T> {
    let mut out = Vec::new();
    while let Some((action, _id)) = harness.pop_action::<T>() {
        out.push(action);
    }
    out
}

fn zoom_changes(actions: &[OfdWidgetAction]) -> Vec<f64> {
    actions
        .iter()
        .filter_map(|a| match a {
            OfdWidgetAction::ZoomChanged(z) => Some(*z),
            _ => None,
        })
        .collect()
}

// 1. Click TextBox → type "hello": content + Changed action.
#[test]
fn click_textbox_type_hello() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // TextBox page rect (0,100,120,40) scaled by BASE.
    click(&mut harness, 5.0 * BASE, 105.0 * BASE, 1);
    harness.keyboard_type_chars("hello");

    assert_eq!(textbox_text(&mut harness), "hello");
    let actions = drain_actions::<OfdWidgetAction>(&mut harness);
    assert!(
        actions.iter().any(|a| matches!(a, OfdWidgetAction::Changed)),
        "Changed action expected, got {actions:?}"
    );
}

// 2. Double click on "ABCD" selects the whole word/code.
#[test]
fn double_click_selects_word() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    click(&mut harness, 15.0 * BASE, 25.0 * BASE, 2);

    let selection = probe(&mut harness, |c| c.text_selection().cloned());
    let selection = selection.expect("body text selection on double click");
    assert_eq!(selection.page, PageId::new("P0"));
    assert_eq!(selection.ranges.len(), 1);
    let range = &selection.ranges[0];
    assert_eq!(range.object, ObjectId::new("t1"));
    assert_eq!(range.code_index, 0);
    assert_eq!((range.start, range.end), (0, 4));
}

// 3. IME preedit stays out of the document; IME area h>0; commit enters.
#[test]
fn ime_preedit_area_and_commit() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    assert!(harness.has_ime_session(), "IME session after focus");

    // Anchor a caret first (the component has no caret until clicked).
    click(&mut harness, 5.0 * BASE, 105.0 * BASE, 1);

    harness.process_text_event(TextEvent::Ime(Ime::Enabled));
    harness.process_text_event(TextEvent::Ime(Ime::Preedit("ni".into(), None)));

    assert_eq!(textbox_text(&mut harness), "");
    let (_pos, size) = harness.ime_rect();
    assert!(size.height > 0.0, "IME area must track the caret, got {size:?}");

    harness.process_text_event(TextEvent::Ime(Ime::Commit("你".into())));
    assert_eq!(textbox_text(&mut harness), "你");
}

// 4. Plain wheel consumed; zoom unchanged.
#[test]
fn plain_wheel_consumed_zoom_unchanged() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    let handled = wheel(&mut harness, -120.0, Modifiers::empty());
    assert!(matches!(handled, Handled::Yes), "wheel consumed");

    assert!(
        zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness)).is_empty(),
        "plain wheel must not change zoom"
    );
}

// 5. Ctrl+wheel ZoomAt: accumulates multiplicatively and clamps silent.
#[test]
fn ctrl_wheel_zoom_at_accumulates_and_clamps() {
    let mut harness = create_harness();
    run_setup(&mut harness);

    // Wheel-up physical (dy>0 → flipped dy<0) zooms in ×1.1.
    wheel(&mut harness, 120.0, Modifiers::CONTROL);
    let z1 = zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness))
        .pop()
        .expect("ZoomChanged after first tick");
    assert!((z1 - BASE * 1.1).abs() < 1e-9, "first tick: {z1}");

    // Drive to the clamp: far more in-ticks than needed for MAX.
    for _ in 0..20 {
        wheel(&mut harness, 120.0, Modifiers::CONTROL);
    }
    let last = zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness))
        .pop()
        .expect("zoom ticks");
    assert!(last > 8.0, "accumulated well past baseline: {last}");

    // At the clamp the component stops firing (its guard skips no-change).
    wheel(&mut harness, 120.0, Modifiers::CONTROL);
    assert!(
        zoom_changes(&drain_actions::<OfdWidgetAction>(&mut harness)).is_empty(),
        "no ZoomChanged once clamped"
    );
}

// 6. Drag-select both lines → Ctrl+C clipboard equals "ABCD\nEF".
#[test]
fn drag_select_ctrl_c_copies_text() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // Anchor before 'A' (page x 15 → offset 0) and end after 'F' (x 21 →
    // offset 2 on the second code), line 0 → line 1.
    let anchor = PhysicalPosition::new(15.0 * BASE, 25.0 * BASE);
    let end = PhysicalPosition::new(21.0 * BASE, 45.0 * BASE);
    let end_state = PointerState {
        position: end,
        buttons: PointerButtons::from(PointerButton::Primary),
        ..PointerState::default()
    };
    harness.process_pointer_event(PointerEvent::Down(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: PointerState {
            position: anchor,
            buttons: PointerButtons::from(PointerButton::Primary),
            ..PointerState::default()
        },
    }));
    harness.process_pointer_event(PointerEvent::Move(xilem::masonry::core::PointerUpdate {
        pointer: MOUSE,
        current: end_state.clone(),
        coalesced: vec![],
        predicted: vec![],
    }));
    harness.process_pointer_event(PointerEvent::Up(PointerButtonEvent {
        pointer: MOUSE,
        button: Some(PointerButton::Primary),
        state: end_state,
    }));

    assert_eq!(harness.clipboard_contents(), "");

    let handled = harness.process_text_event(TextEvent::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key: MasonryKey::Character("c".into()),
        modifiers: Modifiers::CONTROL,
        ..KeyboardEvent::default()
    }));
    assert!(matches!(handled, Handled::Yes), "Ctrl+C consumed");
    assert_eq!(harness.clipboard_contents(), "ABCD\nEF");
}

// 7. Window focus gates caret visibility.
#[test]
fn window_focus_gates_caret_visibility() {
    let mut harness = create_harness();
    run_setup(&mut harness);
    harness.focus_on(Some(harness.root_id()));

    // Place a caret (none exists until the first textbox click).
    click(&mut harness, 5.0 * BASE, 105.0 * BASE, 1);

    harness.process_text_event(TextEvent::WindowFocusChange(true));
    let focused = harness.render();

    harness.process_text_event(TextEvent::WindowFocusChange(false));
    let blurred = harness.render();
    assert_ne!(focused, blurred, "caret should hide on window blur");

    harness.process_text_event(TextEvent::WindowFocusChange(true));
    let regained = harness.render();
    assert_eq!(focused, regained, "caret should reappear on refocus");
}
```

Note: `keyboard::{NamedKey}` is imported but only used if a helper needs it; remove that one name from the import list above when writing — the final file imports exactly:

```rust
use xilem::masonry::core::keyboard::{Key as MasonryKey, KeyState};
```

(No `NamedKey`.)

- [ ] **Step 2: 验证与提交**

```bash
cargo test -p rofd-native-view
cargo clippy -p rofd-native-view --all-targets -- -D warnings
cargo fmt -p rofd-native-view
git add crates/native-view/tests/ofd_widget_harness.rs
git commit -m "test(native-view): OfdWidget harness 七项集成契约"
```

Expected: 7 个 harness 测试 + Task 1 的 9 个 + Task 2 的 1 个，全绿。

- [ ] **Step 3: 全工作区门禁**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 零错误零警告；旧宿主 native-app 仍可构建运行（双表面并存）。

---

## A2 完成判据

- [ ] `crates/native-view` 暴露 `ofd()`/`ofd_with_config()`/`OfdView`/`OfdWidget`/`OfdCommand`/`command_queue()` 最终命名公共面。
- [ ] 12 个组件回调全部接线：回调事件经 `Arc<Mutex<Vec<OfdWidgetAction>>>` 在每个触点末端转 masonry action；`PointerCursorChanged` 内部消化不上抛。
- [ ] Ctrl+wheel 走 `ZoomAt{factor, center}`（1.1/0.9，中心=指针位置），widget 无 zoom 镜像；钳制在组件侧，到界后 ZoomChanged 静默。
- [ ] Ctrl+C 经 widget 拦截写 OS 剪贴板；Ctrl+X 复制不删除（决策已锁定）；Ctrl+V 经 ClipboardPaste → `paste_text`。
- [ ] 七项 harness 契约全绿；旧 winit 桥与旧 native-app 宿主保持可编译可运行。
