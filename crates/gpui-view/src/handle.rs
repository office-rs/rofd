// Ported from rword@4e0b25b6 (crates/gpui-view/src/handle.rs) — rofd
// divergences: undo/redo call the kernel's direct verbs (rofd's component
// owns undo()/redo(); no synthesized ctrl+z/y key events), delete verbs
// follow the wasm adapter's existence-check shapes, and zoom rides the
// kernel's anchored `ZoomAt` (center = component viewport center) with the
// kernel-side clamp, not an absolute set.

//! GpuiOfdHandle: host convenience API over the editor entity.
//!
//! Hosts own an [`Entity<OfdGpuiView>`]; this wraps the common kernel
//! verbs so they never touch component internals. Every mutating verb is
//! one `entity.update` (verb + `cx.notify`); every query is one
//! `entity.read`. Bool/u32/Option returns are the kernel's own result
//! flags (hosts surface "nothing selected" style feedback from them).

use gpui::{App, Entity};
use office_rs_print::{JobHandle, PrintSettings};
use rofd_component::{
    default_tooltip_lines, AnnotationSelection, PrintContent, ScrollDirection, Tool, ViewEvent,
    ZoomMode,
};
use rofd_dom::{AnnotationId, AnnotationKind, Color, OfdDocument};

use crate::OfdGpuiView;

pub struct GpuiOfdHandle {
    entity: Entity<OfdGpuiView>,
}

impl GpuiOfdHandle {
    pub fn new(entity: Entity<OfdGpuiView>) -> Self {
        Self { entity }
    }

    pub fn entity(&self) -> Entity<OfdGpuiView> {
        self.entity.clone()
    }

    // -- Document lifecycle --

    pub fn load_document(&self, document: OfdDocument, cx: &mut App) {
        self.simple(cx, |c| c.load_document(document));
    }

    pub fn new_document(&self, cx: &mut App) {
        self.simple(cx, |c| c.new_document());
    }

    // -- Tools & markup --

    pub fn set_tool(&self, tool: Tool, cx: &mut App) {
        self.simple(cx, |c| c.set_tool(tool));
    }

    /// Set the color a Highlight markup is applied with.
    pub fn set_highlight_color(&self, color: Color, cx: &mut App) {
        self.simple(cx, |c| c.set_highlight_color(color));
    }

    /// Set the color `apply_markup` uses for the given markup kind
    /// (each kind has its own color dropdown).
    pub fn set_markup_color(&self, kind: AnnotationKind, color: Color, cx: &mut App) {
        self.simple(cx, |c| c.set_markup_color(&kind, color));
    }

    /// Apply a markup (highlight/underline/strikeout/squiggly) to the
    /// current body-text selection. Returns the new annotation's id
    /// string, or `None` when nothing is applicable.
    pub fn apply_markup(&self, kind: AnnotationKind, cx: &mut App) -> Option<String> {
        self.entity.update(cx, |view, cx| {
            let id = view.component.apply_markup(kind);
            cx.notify();
            id.map(|id| id.0)
        })
    }

    /// Delete the annotation with the given id. Returns `false` when no
    /// annotation with that id exists.
    pub fn delete_annotation(&self, id: AnnotationId, cx: &mut App) -> bool {
        self.entity.update(cx, |view, cx| {
            let exists = view.component.document().annotations.find(&id).is_some();
            if exists {
                view.component.delete_annotation(&id);
                cx.notify();
            }
            exists
        })
    }

    /// Delete all currently-selected annotations. Returns the count
    /// deleted.
    pub fn delete_selected(&self, cx: &mut App) -> u32 {
        self.entity.update(cx, |view, cx| {
            let ids: Vec<AnnotationId> = match view.component.selection() {
                AnnotationSelection::None => vec![],
                AnnotationSelection::Single(id) => vec![id.clone()],
                AnnotationSelection::Multi(ids) => ids.clone(),
            };
            let count = ids.len() as u32;
            for id in &ids {
                view.component.delete_annotation(id);
            }
            cx.notify();
            count
        })
    }

    // -- History --

    /// Undo the last command (toolbar button; same path as Ctrl+Z).
    pub fn undo(&self, cx: &mut App) {
        self.simple(cx, |c| {
            c.undo();
        });
    }

    /// Redo the last undone command (toolbar button; same path as Ctrl+Y).
    pub fn redo(&self, cx: &mut App) {
        self.simple(cx, |c| {
            c.redo();
        });
    }

    pub fn can_undo(&self, cx: &App) -> bool {
        self.entity.read(cx).component.can_undo()
    }

    pub fn can_redo(&self, cx: &App) -> bool {
        self.entity.read(cx).component.can_redo()
    }

    // -- Zoom & scroll --

    pub fn set_zoom_mode(&self, mode: ZoomMode, cx: &mut App) {
        self.simple(cx, |c| c.set_zoom_mode(mode));
    }

    pub fn zoom_mode(&self, cx: &App) -> ZoomMode {
        self.entity.read(cx).component.zoom_mode()
    }

    /// The current viewport zoom (px per mm; initial `PX_PER_MM`).
    pub fn zoom(&self, cx: &App) -> f64 {
        self.entity.read(cx).component.zoom()
    }

    /// Zoom multiplicatively by `factor`, anchored at the viewport center
    /// (the kernel clamps and adjusts scroll so the center point stays on
    /// the same document position; manual zoom exits any fit mode).
    pub fn zoom_at(&self, factor: f64, cx: &mut App) {
        self.entity.update(cx, |view, cx| {
            let (w, h) = view.component.viewport_size();
            view.handle_view_event(
                ViewEvent::ZoomAt {
                    factor,
                    center: (w / 2.0, h / 2.0),
                },
                cx,
            );
        });
    }

    /// Scroll by one page height (PgUp/PgDn equivalent).
    pub fn scroll_page(&self, direction: ScrollDirection, cx: &mut App) {
        self.entity.update(cx, |view, cx| {
            view.handle_view_event(ViewEvent::ScrollPage { direction }, cx);
        });
    }

    // -- Text selection & paste --

    /// The selected body text, or `None` when there is no selection.
    pub fn selected_text(&self, cx: &App) -> Option<String> {
        self.entity.read(cx).component.selected_text()
    }

    /// Whether a body-text selection currently exists (markup buttons'
    /// enabled state).
    pub fn has_text_selection(&self, cx: &App) -> bool {
        self.entity.read(cx).component.has_text_selection()
    }

    /// Paste text at the current text cursor. Returns `false` when no
    /// cursor is set or the text is empty.
    pub fn paste_text(&self, text: &str, cx: &mut App) -> bool {
        self.simple(cx, |c| c.paste_text(text))
    }

    // -- Clock & tooltip --

    /// Set the annotation clock (author + timestamp) for subsequent edits.
    pub fn set_clock(&self, author: String, ts: i64, cx: &mut App) {
        self.simple(cx, |c| c.set_clock(author, ts));
    }

    /// Toggle the default hover tooltip (titled author + creation time,
    /// UTC — same formatter the xilem host installs). `false` removes it
    /// (e.g. the host renders its own tooltip UI).
    pub fn set_tooltip_enabled(&self, enabled: bool, cx: &mut App) {
        self.entity.update(cx, |view, cx| {
            if enabled {
                view.component
                    .set_tooltip_formatter(|ann| default_tooltip_lines(ann, 0));
            } else {
                view.component.clear_tooltip_formatter();
            }
            cx.notify();
        });
    }

    // -- Print --

    /// One-call direct print (native): snapshot -> page plan -> spooler.
    /// `printer` overrides `settings.printer` when `Some`. Returns the
    /// async job handle — the component is not blocked.
    pub fn print_direct(
        &self,
        printer: Option<&str>,
        settings: &PrintSettings,
        content: PrintContent,
        title: &str,
        cx: &mut App,
    ) -> JobHandle {
        self.entity.update(cx, |view, cx| {
            let job = view
                .component
                .print_direct(printer, settings, content, title);
            cx.notify();
            job
        })
    }

    fn simple<R>(&self, cx: &mut App, f: impl FnOnce(&mut rofd_component::OfdComponent) -> R) -> R {
        self.entity.update(cx, |view, cx| {
            let r = f(&mut view.component);
            cx.notify();
            r
        })
    }
}

impl From<Entity<OfdGpuiView>> for GpuiOfdHandle {
    fn from(entity: Entity<OfdGpuiView>) -> Self {
        Self::new(entity)
    }
}
