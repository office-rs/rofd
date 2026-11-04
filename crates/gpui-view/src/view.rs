// Ported from rword@4e0b25b6 (crates/gpui-view/src/view.rs) — rofd
// divergences: no rich clipboard port (gpui's text clipboard only), the
// shortcut set is rofd's (ctrl+x is copy-only like the xilem line — the
// body is read-only, nothing is ever deleted; ctrl+s stays kernel-owned),
// scroll mapping takes the pointer position for the anchored ZoomAt, and
// out-of-bounds releases synthesize the bare rofd `PointerUp` shape (no
// modifiers).

//! OfdGpuiView: the gpui Element/Entity hosting an [`OfdComponent`].
//!
//! - `OfdElement::paint` pushes the element size into the kernel as the
//!   viewport (`set_viewport_size`), advances the blink tick, and then
//!   FRAME-GATES the record: the kernel's `needs_frame()` is `Now`
//!   exactly when the scene is dirty, so a clean scene replays the
//!   cached `PaintCmd`s instead of re-recording every vsync while the
//!   view holds focus (final-review I-1). Recording runs through
//!   [`GpuiSink`] over the view-owned [`ImageDecodeCache`].
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
//!   spirit; the kernel's `tick_blink` owns the 500ms timing and marks
//!   the scene dirty when the caret flips, so the gate re-records).

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
use crate::gpui_sink::{self, GpuiSink, ImageDecodeCache, PaintCmd};
use crate::ime::ImeBridge;

/// The host-facing editor view. Construct via [`OfdGpuiView::new`] inside
/// `cx.new`, then drive it like any gpui entity.
pub struct OfdGpuiView {
    pub component: OfdComponent,
    pub focus_handle: FocusHandle,
    /// Interior-mutability cache: replay needs `(&mut Window, &mut cache)`
    /// at once, which entity-update scoping cannot express.
    pub(crate) fonts: RefCell<FontCache>,
    /// Cross-frame image decode cache (final-review I-1): keyed by the
    /// scene blob's Arc identity, owned by the view so the per-paint
    /// sink cannot reset it every vsync.
    pub(crate) images: RefCell<ImageDecodeCache>,
    /// The last recorded `PaintCmd`s. Re-record is frame-gated (see
    /// `OfdElement::paint`): a clean scene replays these as-is.
    pub(crate) paint_cmds: RefCell<Option<Vec<PaintCmd>>>,
    /// How many times the record path ran — the seam the frame-gate
    /// tests assert on (one per re-record, none per cached replay).
    pub(crate) record_count: std::cell::Cell<u32>,
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
    /// Previous effective focus — change detection only.
    pub(crate) had_focus: std::cell::Cell<bool>,
    /// Previous element size — change detection only.
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
            images: RefCell::new(ImageDecodeCache::new()),
            paint_cmds: RefCell::new(None),
            record_count: std::cell::Cell::new(0),
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

    /// The element origin in WINDOW coordinates (updated each paint),
    /// read-only. Hosts use it to translate viewport-local kernel events
    /// (e.g. the context-menu point) back into window coordinates for
    /// overlay anchoring.
    pub fn element_origin(&self) -> (f64, f64) {
        let (x, y) = self.element_origin.get();
        (x as f64, y as f64)
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
                // (ctrl+c/x/v), and undo/redo are first-class view
                // commands. ctrl only (the platform key is not ctrl
                // here); ctrl+s is NOT intercepted — the kernel owns it
                // (SaveRequested fires from the KeyDown passthrough).
                // ctrl+x is copy-only (xilem parity): the body is
                // read-only and TextCursor has no extent, so there is
                // nothing to cut — never delete.
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
                        // Copy-only: identical path for c and x.
                        "c" | "x" => return this.copy_selection(cx),
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
        // Record (frame-gated) + replay. gpui Pixels are LOGICAL px — the
        // kernel's viewport unit — so no scale-factor division happens
        // here.
        self.view.update(cx, |view, _| {
            let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let size_changed = view.last_size.get() != size;
            if size_changed {
                view.last_size.set(size);
                view.component
                    .set_viewport_size(size.0 as f64, size.1 as f64);
            }
            if focused {
                view.component.tick_blink();
            }
            // Frame gate (final-review I-1): re-record only when the
            // kernel says the scene changed. `scene_dirty()` is the
            // recompose half of `needs_frame()` (the draw-retry half can
            // never clear in this host - it replays `scene()` itself and
            // never draws through a RenderTarget), and every
            // repaint-worthy kernel mutation couples to it
            // (`handle_event` maps `needs_repaint` to a dirty mark, and
            // so do load_document / font registration / tooltip /
            // viewport changes; `tick_blink` above dirties on a caret
            // flip). A clean scene replays the cached cmds instead of
            // re-recording every vsync of the focused rAF loop; the size
            // term is belt-and-braces (set_viewport_size already
            // dirties).
            let scene_dirty = view.component.scene_dirty();
            if size_changed || view.paint_cmds.borrow().is_none() || scene_dirty {
                view.component.update_scene();
                let cmds = {
                    let mut images = view.images.borrow_mut();
                    let mut sink = GpuiSink::new(&mut images);
                    imaging::record::replay(view.component.scene(), &mut sink);
                    sink.finish()
                };
                view.record_count.set(view.record_count.get() + 1);
                *view.paint_cmds.borrow_mut() = Some(cmds);
            }
        });
        // Replay: cached cmds -> window, resolving fonts through the
        // shared cache.
        {
            let view = self.view.read(cx);
            let mut fonts = view.fonts.borrow_mut();
            let cmds = view.paint_cmds.borrow();
            let cmds = cmds
                .as_deref()
                .expect("the gate above records on the first paint");
            gpui_sink::replay(window, cmds, &mut fonts, bounds.origin);
        }
        // Keep frames coming only while a caret can blink; an unfocused
        // or hidden window quiesces (no rAF loop, no timers).
        if focused {
            window.request_animation_frame();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Frame-gate tests (final-review I-1): a clean scene replays the
    //! cached PaintCmds instead of re-recording every paint, while a
    //! dirtying mutation re-records — and the image decode cache survives
    //! the per-paint sink across both.

    use super::*;
    use gpui::{px, size, TestAppContext, VisualTestContext};
    use rofd_dom::{
        AnnotationKind, AnnotationPayload, Color, ImageId, Layer, LayerType, NoteIcon, ObjectId,
        OfdDocument, Page, PageId, PageObject,
    };

    const WIN_W: f32 = 800.0;
    const WIN_H: f32 = 600.0;

    fn test_config() -> OfdConfig {
        OfdConfig::new(Arc::new(vec![])).with_zoom(1.0)
    }

    /// 2x1 PNG (red | blue) — the kernel decodes it into an RGBA8 scene
    /// image at compose time, exercising the sink's re-pack path.
    fn png_bytes() -> Arc<Vec<u8>> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(2, 1, vec![255, 0, 0, 0, 0, 255]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        Arc::new(buf.into_inner())
    }

    /// One 200x200 mm page carrying one image object.
    fn image_doc() -> OfdDocument {
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: rofd_dom::Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![Layer {
                layer_type: LayerType::Body,
                objects: vec![PageObject::Image(rofd_dom::ImageObject {
                    id: ObjectId::new("i1"),
                    boundary: rofd_dom::Rect {
                        x: 10.0,
                        y: 10.0,
                        w: 100.0,
                        h: 50.0,
                    },
                    ctm: None,
                    image: ImageId::new("I1"),
                })],
            }],
            template: None,
        });
        doc.resources.images.insert(ImageId::new("I1"), png_bytes());
        doc
    }

    /// A window root that renders nothing (the element under test is
    /// painted explicitly via `vcx.draw`, so background effect flushes
    /// cannot add uncontrolled record passes).
    struct EmptyHost;

    impl Render for EmptyHost {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div().size_full()
        }
    }

    /// One explicit paint pass through the view's element (bookkeeping,
    /// frame gate, record, replay all live in `OfdElement::paint`).
    fn paint_once(entity: &Entity<OfdGpuiView>, vcx: &mut VisualTestContext) {
        let entity = entity.clone();
        vcx.draw(
            gpui::point(px(0.), px(0.)),
            size(px(WIN_W), px(WIN_H)),
            move |_window, _cx| OfdElement { view: entity },
        );
    }

    fn open_with_image_doc(cx: &mut TestAppContext) -> Entity<OfdGpuiView> {
        cx.update(|cx| {
            cx.new(|cx| {
                let mut view = OfdGpuiView::new(test_config(), cx);
                view.component.set_clock("t".into(), 1);
                view.component.load_document(image_doc());
                view
            })
        })
    }

    #[gpui::test]
    fn clean_scene_replays_cached_cmds(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(WIN_W), px(WIN_H)), |_, _| EmptyHost);
        let mut vcx = VisualTestContext::from_window(window.into(), cx);
        let entity = open_with_image_doc(cx);

        // First paint: record + decode the page image once.
        paint_once(&entity, &mut vcx);
        let counts = |cx: &TestAppContext| {
            cx.read(|app| {
                let view = entity.read(app);
                let record_count = view.record_count.get();
                let decode_count = view.images.borrow().decode_count();
                let image_cmds = view
                    .paint_cmds
                    .borrow()
                    .as_deref()
                    .map(|cmds| {
                        cmds.iter()
                            .filter(|c| matches!(c, PaintCmd::Image { .. }))
                            .count()
                    })
                    .unwrap_or(0);
                (record_count, decode_count, image_cmds)
            })
        };
        assert_eq!(
            counts(cx),
            (1, 1, 1),
            "first paint: record + decode once, one image cmd"
        );

        // Second paint, unchanged scene: REPLAY ONLY — no re-record, no
        // re-decode (the per-paint sink reset cannot cost either).
        paint_once(&entity, &mut vcx);
        assert_eq!(
            counts(cx),
            (1, 1, 1),
            "a clean scene must replay the cached cmds — no re-record, no re-decode"
        );

        // A dirtying mutation re-records — and still reuses the decode.
        cx.update(|cx| {
            entity.update(cx, |view, _| {
                view.component.create_annotation(
                    AnnotationKind::Note,
                    PageId::new("P0"),
                    AnnotationPayload::Note {
                        rect: rofd_dom::Rect {
                            x: 20.0,
                            y: 20.0,
                            w: 40.0,
                            h: 30.0,
                        },
                        color: Color::Rgb(255, 200, 0),
                        content: "n".into(),
                        icon: NoteIcon::Note,
                    },
                );
            })
        });
        paint_once(&entity, &mut vcx);
        assert_eq!(
            counts(cx),
            (2, 1, 1),
            "a dirty scene re-records and still hits the decode cache"
        );
    }

    #[gpui::test]
    fn viewport_change_invalidates_the_cache(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(WIN_W), px(WIN_H)), |_, _| EmptyHost);
        let mut vcx = VisualTestContext::from_window(window.into(), cx);
        let entity = open_with_image_doc(cx);

        paint_once(&entity, &mut vcx);
        let records = |cx: &TestAppContext| cx.read(|app| entity.read(app).record_count.get());
        assert_eq!(records(cx), 1);

        // Same scene, different element size: the viewport push dirties
        // the kernel scene, so the next paint re-records.
        let entity_draw = entity.clone();
        vcx.draw(
            gpui::point(px(0.), px(0.)),
            size(px(400.), px(300.)),
            move |_window, _cx| OfdElement { view: entity_draw },
        );
        assert_eq!(
            records(cx),
            2,
            "a size change re-records even when nothing else moved"
        );
    }
}
