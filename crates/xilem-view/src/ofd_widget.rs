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
    AccessCtx, ChildrenIds, LayoutCtx, MeasureCtx, PaintCtx, PropertiesMut, PropertiesRef,
    QueryCtx, RegisterCtx, Widget, WidgetMut,
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
        component
            .on_annotation_focus(move |id| queue(&p, OfdWidgetAction::AnnotationFocus(id.clone())));
        let p = pending.clone();
        component.on_annotation_interact(move |id| {
            queue(&p, OfdWidgetAction::AnnotationInteract(id.clone()))
        });
        let p = pending.clone();
        component.on_page_change(move |idx| queue(&p, OfdWidgetAction::PageChanged(idx)));
        let p = pending.clone();
        component.on_zoom_change(move |factor| queue(&p, OfdWidgetAction::ZoomChanged(factor)));
        let p = pending.clone();
        component.on_text_selection_change(move |sel| {
            queue(&p, OfdWidgetAction::TextSelectionChanged(sel.cloned()))
        });
        let p = pending.clone();
        component
            .on_warning(move |warnings| queue(&p, OfdWidgetAction::Warnings(warnings.to_vec())));
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
                self.component
                    .handle_event(&ViewEvent::PointerMove { x: pos.x, y: pos.y });
                self.after_event(ctx);
            }
            PointerEvent::Up(btn) => {
                let Some(button) = masonry_events::mouse_button(btn.button.as_ref()) else {
                    return;
                };
                let pos = ctx.local_position(btn.state.position);
                // PointerUp carries no modifiers in rofd's ViewEvent (the
                // component's up-path does not consume them).
                self.component.handle_event(&ViewEvent::PointerUp {
                    button,
                    x: pos.x,
                    y: pos.y,
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
