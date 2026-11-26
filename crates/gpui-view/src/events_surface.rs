// Ported from rword@4e0b25b6 (crates/gpui-view/src/events_surface.rs) —
// the enum is re-cut one-to-one onto rofd's 14 kernel callbacks (rword's
// Sdt/ImageSelection/LayoutComplete rows have no rofd counterpart; rofd
// adds TextSelection/AnnotationFocus/AnnotationInteract/PageChange/
// Warning/ZoomMode events).

//! Kernel callback surface as one gpui-subscribable event queue.
//!
//! Kernel callbacks fire SYNCHRONOUSLY inside `handle_event` — with no
//! gpui context at hand — so they push into a thread-safe queue
//! ([`EventQueue`], capturable by the `Send` callback closures), and the
//! element's paint drains it into `cx.emit(OfdGpuiEvent)` at the top of
//! the next paint. Masonry's action-queue rhythm, translated to gpui's
//! subscribe model.

use std::sync::{Arc, Mutex};

use rofd_component::{
    AnnotationId, AnnotationSelection, ContextTarget, OfdComponent, OfdWarning, PointerCursor,
    SealPlacement, ZoomMode,
};

/// The kernel callback surface as one gpui-subscribable event enum.
///
/// Payload policy: only the cheap-Clone carriers ship. `on_change` hands
/// over `&OfdDocument`, `on_text_selection_change` an
/// `Option<&BodyTextSelection>`, `on_cursor_change` an `Option<&TextCursor>`
/// — hosts re-query those through the component instead of the event
/// (mirroring the xilem line's borrow-based handlers). `CopyRequested`
/// likewise carries no text: the host re-runs `component.copy_selection()`
/// when it services the clipboard write.
#[derive(Debug, Clone, PartialEq)]
pub enum OfdGpuiEvent {
    Changed,
    SelectionChanged(AnnotationSelection),
    TextSelectionChanged,
    CursorChanged,
    SaveRequested,
    AnnotationFocused(AnnotationId),
    AnnotationInteracted(AnnotationId),
    ContextMenuRequested((f64, f64), ContextTarget),
    PageChanged(usize),
    ZoomChanged(f64),
    ZoomModeChanged(ZoomMode),
    Warned(Vec<OfdWarning>),
    PointerCursorChanged(PointerCursor),
    CopyRequested,
    /// The user placed a seal by clicking (spec 2026-11-23 §11): the page
    /// (index from 0) and the boundary in page millimetres. The host signs
    /// with it and reloads the document.
    SealPlaced(SealPlacement),
    /// A seal placement ended without a seal: Esc, the right mouse
    /// button, or `GpuiOfdHandle::cancel_seal_placement`.
    SealPlacementCancelled,
}

/// Thread-safe event queue: kernel callbacks push (from inside
/// `handle_event`), the paint drain takes everything. `Clone` shares the
/// same backing store (the view keeps one handle, the host another). No
/// dedup, no loss between drains.
#[derive(Default, Clone)]
pub struct EventQueue(Arc<Mutex<Vec<OfdGpuiEvent>>>);

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, event: OfdGpuiEvent) {
        self.0.lock().expect("event queue poisoned").push(event);
    }

    /// Take all queued events (order preserved).
    pub fn drain(&self) -> Vec<OfdGpuiEvent> {
        std::mem::take(&mut *self.0.lock().expect("event queue poisoned"))
    }
}

/// Register every kernel callback, each routing into `queue`.
/// Standalone (component-level) so the routing is headless-testable; the
/// view's constructor calls it with its shared queue.
pub fn install(component: &mut OfdComponent, queue: &EventQueue) {
    component.on_change({
        let queue = queue.clone();
        move |_doc| queue.push(OfdGpuiEvent::Changed)
    });
    component.on_selection_change({
        let queue = queue.clone();
        move |sel| queue.push(OfdGpuiEvent::SelectionChanged(sel.clone()))
    });
    component.on_text_selection_change({
        let queue = queue.clone();
        move |_sel| queue.push(OfdGpuiEvent::TextSelectionChanged)
    });
    component.on_cursor_change({
        let queue = queue.clone();
        move |_cursor| queue.push(OfdGpuiEvent::CursorChanged)
    });
    component.on_save_request({
        let queue = queue.clone();
        move || queue.push(OfdGpuiEvent::SaveRequested)
    });
    component.on_annotation_focus({
        let queue = queue.clone();
        move |id| queue.push(OfdGpuiEvent::AnnotationFocused(id.clone()))
    });
    component.on_annotation_interact({
        let queue = queue.clone();
        move |id| queue.push(OfdGpuiEvent::AnnotationInteracted(id.clone()))
    });
    component.on_context_menu({
        let queue = queue.clone();
        move |point, target| queue.push(OfdGpuiEvent::ContextMenuRequested(point, target))
    });
    component.on_page_change({
        let queue = queue.clone();
        move |page| queue.push(OfdGpuiEvent::PageChanged(page))
    });
    component.on_zoom_change({
        let queue = queue.clone();
        move |zoom| queue.push(OfdGpuiEvent::ZoomChanged(zoom))
    });
    component.on_zoom_mode_change({
        let queue = queue.clone();
        move |mode| queue.push(OfdGpuiEvent::ZoomModeChanged(mode))
    });
    component.on_warning({
        let queue = queue.clone();
        move |warnings| queue.push(OfdGpuiEvent::Warned(warnings.to_vec()))
    });
    component.on_pointer_cursor({
        let queue = queue.clone();
        move |cursor| queue.push(OfdGpuiEvent::PointerCursorChanged(cursor))
    });
    // on_copy is intentionally non-Send in the kernel (single-threaded
    // web contract); the queue is Sync so the closure satisfies it.
    component.on_copy({
        let queue = queue.clone();
        move |_text| queue.push(OfdGpuiEvent::CopyRequested)
    });
    // Seal placement (spec 2026-11-23 §11): entering and cancelling are the
    // handle's verbs; where the seal went, and that it was cancelled,
    // arrive here.
    component.on_seal_placed({
        let queue = queue.clone();
        move |placement| queue.push(OfdGpuiEvent::SealPlaced(placement))
    });
    component.on_seal_placement_cancelled({
        let queue = queue.clone();
        move || queue.push(OfdGpuiEvent::SealPlacementCancelled)
    });
}
