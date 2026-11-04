// Ported from rword@4e0b25b6 (crates/gpui-view/src/view.rs) — paint-pipeline
// core only: struct + Focusable + the record/replay element. Event
// listeners, IME, the callback event queue, and the blink loop land with
// the later gpui-line tasks.

//! OfdGpuiView: the gpui Element/Entity hosting an [`OfdComponent`].
//!
//! - `OfdElement::paint` pushes the element size into the kernel as the
//!   viewport (`set_viewport_size`), recomposes the scene, records it
//!   through [`GpuiSink`], and replays the commands into the window.
//! - Input listeners, IME, focus transitions, and blink are NOT wired
//!   yet; this stage proves the window + scene replay pipeline only.

use std::cell::RefCell;

use gpui::{
    div, prelude::*, relative, size, App, Bounds, Context, Element, ElementId, Entity, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, LayoutId, Pixels, Style, Window,
};
use rofd_component::{OfdComponent, OfdConfig};

use crate::font_cache::FontCache;
use crate::gpui_sink::{self, GpuiSink};

/// The host-facing editor view. Construct via [`OfdGpuiView::new`] inside
/// `cx.new`, then drive it like any gpui entity.
pub struct OfdGpuiView {
    pub component: OfdComponent,
    pub focus_handle: FocusHandle,
    /// Interior-mutability cache: replay needs `(&mut Window, &mut cache)`
    /// at once, which entity-update scoping cannot express.
    pub(crate) fonts: RefCell<FontCache>,
    /// Previous element size — change detection only.
    pub(crate) last_size: std::cell::Cell<(f32, f32)>,
}

impl OfdGpuiView {
    pub fn new(config: OfdConfig, cx: &mut Context<Self>) -> Self {
        Self {
            component: OfdComponent::new_native(config),
            focus_handle: cx.focus_handle(),
            fonts: RefCell::new(FontCache::new()),
            last_size: std::cell::Cell::new((0.0, 0.0)),
        }
    }
}

impl Focusable for OfdGpuiView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for OfdGpuiView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("rofd-editor")
            .size_full()
            .track_focus(&self.focus_handle)
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
        // Viewport push + record: kernel scene -> PaintCmds (no window
        // state needed). gpui Pixels are LOGICAL px — the kernel's
        // viewport unit — so no scale-factor division happens here.
        let cmds = self.view.update(cx, |view, _| {
            let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            if view.last_size.get() != size {
                view.last_size.set(size);
                view.component
                    .set_viewport_size(size.0 as f64, size.1 as f64);
            }
            view.component.update_scene();
            let mut sink = GpuiSink::new();
            imaging::record::replay(view.component.scene(), &mut sink);
            sink.finish()
        });
        // Replay: cmds -> window, resolving fonts through the shared cache.
        let mut fonts = self.view.read(cx).fonts.borrow_mut();
        gpui_sink::replay(window, &cmds, &mut fonts, bounds.origin);
    }
}
