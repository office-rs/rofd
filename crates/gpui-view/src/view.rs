// Ported from rword@4e0b25b6 (crates/gpui-view/src/view.rs) — rofd
// divergences: no rich clipboard port (gpui's text clipboard only), the
// shortcut set is rofd's (no cut — the body is read-only; ctrl+s stays
// kernel-owned), scroll mapping takes the pointer position for the
// anchored ZoomAt, and out-of-bounds releases synthesize the bare rofd
// `PointerUp` shape (no modifiers).

//! OfdGpuiView: the gpui Element/Entity hosting an [`OfdComponent`].
//!
//! - `OfdElement::paint` pushes the element size into the kernel as the
//!   viewport (`set_viewport_size`), recomposes the scene, records it
//!   through [`GpuiSink`], and replays the commands into the window.
//! - Input arrives as div listeners translated by [`crate::events`]:
//!   mouse positions are translated window -> viewport by the element
//!   origin (the inverse of the paint offset); ctrl+wheel zooms anchored
//!   at the pointer; clipboard shortcuts (ctrl+c/v) are adapter-owned
//!   because the kernel cannot reach the OS clipboard.
//! - IME rides gpui's pull-model [`gpui::EntityInputHandler`] (impl in
//!   [`crate::ime`]), (re)declared every paint while focused.
//! - Kernel callbacks queue into [`crate::events_surface::EventQueue`]
//!   and drain into `cx.emit(OfdGpuiEvent)` at the top of each paint.
//! - The blink loop only requests frames while the element holds focus —
//!   a hidden or unfocused window stops painting entirely (on-demand
//!   spirit; the kernel's `tick_blink` owns the 500ms timing).

use std::cell::RefCell;
use std::sync::Arc;

use gpui::{
    div, prelude::*, relative, size, App, Bounds, ClipboardItem, Context, Element, ElementId,
    Entity, FocusHandle, Focusable, GlobalElementId, InspectorElementId, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, Style, Window,
};
use rofd_component::{OfdComponent, OfdConfig, ViewEvent};

use crate::events::{self, ScrollOutcome};
use crate::events_surface::{EventQueue, OfdGpuiEvent};
use crate::font_cache::FontCache;
use crate::gpui_sink::{self, GpuiSink};
use crate::ime::ImeBridge;

/// The host-facing editor view. Construct via [`OfdGpuiView::new`] inside
/// `cx.new`, then drive it like any gpui entity.
pub struct OfdGpuiView {
    pub component: OfdComponent,
    pub focus_handle: FocusHandle,
    /// Interior-mutability cache: replay needs `(&mut Window, &mut cache)`
    /// at once, which entity-update scoping cannot express.
    pub(crate) fonts: RefCell<FontCache>,
    /// Pull->push IME bridge (composition-only shadow store).
    pub(crate) ime: ImeBridge,
    /// Kernel-callback event queue, drained into cx.emit at paint start.
    pub(crate) events: Arc<EventQueue>,
    /// Element origin in WINDOW coordinates (updated each paint) — used
    /// to translate incoming mouse events from window coords to the
    /// kernel's viewport coords (the inverse of the paint offset).
    pub(crate) element_origin: std::cell::Cell<(f32, f32)>,
    /// Last mouse position in viewport coords (feed for out-of-bounds
    /// synthetic PointerUp).
    pub(crate) last_mouse: std::cell::Cell<(f32, f32)>,
    /// Previous effective focus / element size — change detection only.
    pub(crate) had_focus: std::cell::Cell<bool>,
    pub(crate) last_size: std::cell::Cell<(f32, f32)>,
}

impl OfdGpuiView {
    pub fn new(config: OfdConfig, cx: &mut Context<Self>) -> Self {
        let mut component = OfdComponent::new_native(config);
        let events = Arc::new(EventQueue::new());
        crate::events_surface::install(&mut component, &events);
        Self {
            component,
            focus_handle: cx.focus_handle(),
            fonts: RefCell::new(FontCache::new()),
            ime: ImeBridge::new(),
            events,
            element_origin: std::cell::Cell::new((0.0, 0.0)),
            last_mouse: std::cell::Cell::new((0.0, 0.0)),
            had_focus: std::cell::Cell::new(false),
            last_size: std::cell::Cell::new((0.0, 0.0)),
        }
    }

    /// Translate a window-space mouse position into the kernel's
    /// viewport space (subtract the element origin — the inverse of the
    /// paint-time offset).
    fn to_viewport(&self, pos: gpui::Point<gpui::Pixels>) -> (f32, f32) {
        let (ox, oy) = self.element_origin.get();
        (f32::from(pos.x) - ox, f32::from(pos.y) - oy)
    }

    /// Feed one translated platform event to the kernel and repaint.
    pub(crate) fn handle_view_event(&mut self, event: ViewEvent, cx: &mut Context<Self>) {
        self.component.handle_event(&event);
        cx.notify();
    }

    /// on_mouse_down body: click-to-focus first, then translate +
    /// forward.
    fn pressed(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        let (x, y) = self.to_viewport(event.position);
        self.handle_view_event(events::mouse_down_at(event, x, y), cx);
    }

    /// on_mouse_move body: remember the viewport position, then forward.
    fn moved(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = self.to_viewport(event.position);
        self.last_mouse.set((x, y));
        self.handle_view_event(events::mouse_move_at(event, x, y), cx);
    }

    /// on_mouse_up body: translate + forward.
    fn released(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = self.to_viewport(event.position);
        self.handle_view_event(events::mouse_up_at(event, x, y), cx);
    }

    /// on_mouse_up_out body: a drag ending OUTSIDE the editor still
    /// delivers the up from the last known viewport position (without
    /// pointer capture the in-bounds on_mouse_up never fires; a stranded
    /// kernel drag gesture is worse than an approximated release
    /// position).
    fn released_outside(&mut self, button: rofd_component::MouseButton, cx: &mut Context<Self>) {
        let (x, y) = self.last_mouse.get();
        self.handle_view_event(
            ViewEvent::PointerUp {
                button,
                x: x as f64,
                y: y as f64,
            },
            cx,
        );
    }

    /// Copy the selection to the OS clipboard through gpui's text
    /// clipboard; a no-op when nothing is selected (Text tool only).
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.component.copy_selection() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.notify();
        }
    }

    /// Paste clipboard text into the annotation at the text cursor.
    pub fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            if self.component.paste_text(&text) {
                cx.notify();
            }
        }
    }
}

impl gpui::EventEmitter<OfdGpuiEvent> for OfdGpuiView {}

impl Focusable for OfdGpuiView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for OfdGpuiView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only the three kernel-known buttons get listeners: Navigate /
        // Back / Forward (and any future button) are dropped entirely
        // instead of coercing to Left through the translator.
        div()
            .id("rofd-editor")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::pressed))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::pressed))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::pressed))
            .on_mouse_move(cx.listener(Self::moved))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::released))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::released))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::released))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.released_outside(rofd_component::MouseButton::Left, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.released_outside(rofd_component::MouseButton::Right, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.released_outside(rofd_component::MouseButton::Middle, cx);
                }),
            )
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _window, cx| {
                let zoom = this.component.zoom();
                let pointer = this.to_viewport(event.position);
                match events::scroll(event, zoom, pointer) {
                    ScrollOutcome::Scroll(ev) | ScrollOutcome::Zoom(ev) => {
                        this.handle_view_event(ev, cx);
                    }
                    ScrollOutcome::None => {}
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                // Auto-repeat never re-fires shortcuts (the interception
                // sits before the is_held filter in the passthrough).
                if event.is_held {
                    return;
                }
                // Adapter-owned shortcuts intercept BEFORE the kernel
                // forward: the kernel cannot reach the OS clipboard
                // (ctrl+c/v), and undo/redo are first-class view commands.
                // ctrl only (the platform key is not ctrl here); ctrl+s
                // is NOT intercepted — the kernel owns it (SaveRequested
                // fires from the KeyDown passthrough).
                let ks = &event.keystroke;
                let ctrl = ks.modifiers.control;
                if ctrl && !ks.modifiers.shift {
                    match ks.key.as_str() {
                        "z" => {
                            if this.component.undo() {
                                cx.notify();
                            }
                            return;
                        }
                        "y" => {
                            if this.component.redo() {
                                cx.notify();
                            }
                            return;
                        }
                        "c" => return this.copy_selection(cx),
                        "v" => return this.paste_from_clipboard(cx),
                        _ => {}
                    }
                }
                if ctrl && ks.modifiers.shift && ks.key.eq_ignore_ascii_case("z") {
                    if this.component.redo() {
                        cx.notify();
                    }
                    return;
                }
                if let Some(ev) = events::key_down(event) {
                    this.handle_view_event(ev, cx);
                }
            }))
            .child(OfdElement { view: cx.entity() })
    }
}

/// The painting element — one per render, holding only the entity handle.
struct OfdElement {
    view: Entity<OfdGpuiView>,
}

impl gpui::IntoElement for OfdElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for OfdElement {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Fill the host wrapper. The HOST is responsible for clipping
        // (overflow_hidden on the flex_1 wrapper div) — without that, a
        // relative(1.0) child in a flex_col fights the parent for 100%.
        let style = Style {
            size: size(relative(1.0).into(), relative(1.0).into()),
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Bookkeeping FIRST: drain the kernel-callback queue into entity
        // events (hosts subscribed via cx.subscribe see state changes
        // before this frame's UI reads it), remember the element origin
        // for mouse-event inverse translation, and push effective-focus
        // transitions into the kernel (FocusLost force-commits an
        // in-flight preedit; FocusGained restarts blink).
        let focused = self.view.update(cx, |view, cx| {
            for event in view.events.drain() {
                cx.emit(event);
            }
            view.element_origin
                .set((f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
            let focused = view.focus_handle.is_focused(window);
            let was = view.had_focus.replace(focused);
            if focused && !was {
                view.component.handle_event(&ViewEvent::FocusGained);
            } else if !focused && was {
                view.component.handle_event(&ViewEvent::FocusLost);
            }
            focused
        });
        // IME registration: the pull-model handler is (re)declared every
        // paint, scoped to this element's bounds (no-op while unfocused).
        let focus = self.view.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            gpui::ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        // Record: viewport push + blink tick + kernel scene -> PaintCmds
        // (no window state needed). gpui Pixels are LOGICAL px — the
        // kernel's viewport unit — so no scale-factor division happens
        // here.
        let cmds = self.view.update(cx, |view, _| {
            let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            if view.last_size.get() != size {
                view.last_size.set(size);
                view.component
                    .set_viewport_size(size.0 as f64, size.1 as f64);
            }
            if focused {
                view.component.tick_blink();
            }
            view.component.update_scene();
            let mut sink = GpuiSink::new();
            imaging::record::replay(view.component.scene(), &mut sink);
            sink.finish()
        });
        // Replay: cmds -> window, resolving fonts through the shared cache.
        {
            let mut fonts = self.view.read(cx).fonts.borrow_mut();
            gpui_sink::replay(window, &cmds, &mut fonts, bounds.origin);
        }
        // Keep frames coming only while a caret can blink; an unfocused
        // or hidden window quiesces (no rAF loop, no timers).
        if focused {
            window.request_animation_frame();
        }
    }
}
