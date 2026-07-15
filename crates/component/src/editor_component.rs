use std::sync::Arc;

use rofd_dom::{
    AnnotationKind, AnnotationPayload, AnnotationSelection, Color, OfdDocument, OfdWarning, PageId,
    PathCommand, PathData, Point, Rect,
};
use rofd_editor::{Editor, TextCursor};
use rofd_render::{DragPreview, FontStore, HandlePos, RenderEngine, Scene, Viewport, PX_PER_MM};

use crate::callbacks::{Callbacks, ContextTarget};
use crate::config::EditorConfig;
use crate::event::EventOutcome;
use crate::render_target::RenderTarget;

/// The active editing tool. The host selects a tool (e.g. via a toolbar) and
/// the component uses it to interpret pointer drags (T3 wires the drag logic).
///
/// `Select` clicks select/drag existing annotations; `Create` begins a new
/// annotation of the given [`AnnotationKind`] on the next pointer drag.
#[derive(Debug, Clone, PartialEq)]
pub enum Tool {
    Select,
    Create(AnnotationKind),
}

/// In-progress pointer drag state. Internal to the component - T3's
/// PointerDown/Move/Up handlers create/update/clear this, and `build_scene`
/// maps it to a [`DragPreview`] for live rendering.
///
/// `Create` covers rect-bounded kinds (Shape/Note/TextBox/...) and Freehand
/// (accumulating a path). `Move`/`Resize` track the source annotation.
///
/// **Preview-based drag (one undo per drag):** during PointerMove, `Move`/
/// `Resize` do NOT call `editor.move_annotation`/`resize_annotation` (which
/// would push one Transaction per pixel, flooding history). Instead they
/// update the in-progress preview geometry here; `drag_to_preview` renders a
/// semi-transparent overlay at the would-be position. On PointerUp the
/// component issues a single `editor.move_annotation`/`resize_annotation`
/// call with the cumulative delta/final rect -> exactly one Transaction ->
/// one undo restores the original.
#[derive(Debug, Clone)]
pub(crate) enum DragState {
    /// Creating a new annotation; `start`/`current` bound a rect, `path`
    /// accumulates Freehand points (viewport-space).
    Create {
        kind: AnnotationKind,
        start: (f64, f64),
        current: (f64, f64),
        path: Vec<(f64, f64)>,
    },
    /// Moving an existing annotation via a preview-based drag.
    /// `before_rect` is the annotation's page-local rect captured at drag
    /// start; `start`/`last` are the viewport-space pointer positions at drag
    /// start and most-recent PointerMove. The cumulative page-local delta is
    /// `(last - start) / zoom`; the preview rect = `before_rect` + that delta.
    /// `moved` is true once any PointerMove shifted beyond the start point
    /// (so PointerUp can skip the editor command for a pure
    /// click-without-drag, avoiding a no-op Transaction).
    Move {
        id: rofd_dom::AnnotationId,
        before_rect: Rect,
        start: (f64, f64),
        last: (f64, f64),
        moved: bool,
    },
    /// Resizing an existing annotation via a preview-based drag. `handle`
    /// identifies the dragged handle, `anchor` is the opposite corner (fixed),
    /// `orig` is the annotation's rect at drag start (page-local), and
    /// `current_local` is the most-recent pointer position in page-local
    /// coordinates. The preview rect = `compute_resize(handle, anchor, orig,
    /// current_local)`. `moved` is true once any PointerMove resized beyond
    /// the start point.
    Resize {
        id: rofd_dom::AnnotationId,
        handle: HandlePos,
        anchor: (f64, f64),
        orig: Rect,
        current_local: (f64, f64),
        moved: bool,
    },
}

pub struct EditorComponent {
    pub(crate) editor: Editor,
    pub(crate) render: RenderEngine,
    pub(crate) viewport: Viewport,
    /// Cached `FontStore` (document fonts + default + registered). Rebuilt on
    /// `load_document`/`new_document` so a large default CJK font is not
    /// re-registered every frame.
    pub(crate) font_store: Option<FontStore>,
    /// Font bytes registered at runtime via [`Self::register_font_data`] (e.g.
    /// by the web SDK loading fonts after construction). Folded into the
    /// `FontStore` when it is (re)built.
    pub(crate) registered_font_bytes: Vec<Arc<Vec<u8>>>,
    pub(crate) callbacks: Callbacks,
    pub(crate) modified: bool,
    /// Active editing tool (Select or Create{kind}). Defaults to `Select`.
    pub(crate) tool: Tool,
    /// In-progress pointer drag, if any. `None` when no drag is active.
    pub(crate) drag: Option<DragState>,
    /// The index of the currently visible page (the page containing the
    /// viewport's vertical center), or `None` if no page is visible / no
    /// document loaded. Updated after Scroll/Resize; when it changes,
    /// `on_page_change` fires. T4.
    pub(crate) current_page: Option<usize>,
}

impl EditorComponent {
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
            font_store: None,
            registered_font_bytes: Vec::new(),
            callbacks: Callbacks::default(),
            modified: false,
            tool: Tool::Select,
            drag: None,
            current_page: None,
        }
    }

    /// Build a `FontStore` from the document's fonts + the default font + any
    /// runtime-registered fonts. Called on `load_document`/`new_document` and
    /// lazily by `build_scene`.
    fn build_font_store(&self) -> FontStore {
        let font_bytes = self.render.default_font_bytes.clone();
        let mut store = FontStore::from_resources(&self.editor.document().resources, font_bytes);
        for bytes in &self.registered_font_bytes {
            store.register_font(bytes.clone());
        }
        store
    }

    /// Register font data (raw bytes) at runtime - e.g. a CJK font loaded by
    /// the web SDK after the editor is constructed. Mirrors reditor's
    /// `register_font_data`. If a `FontStore` already exists, the font is
    /// registered with it immediately; otherwise it is folded in when the
    /// `FontStore` is next built (on `load_document`/`build_scene`).
    ///
    /// Returns `true` if the bytes parsed as a valid font.
    pub fn register_font_data(&mut self, bytes: Vec<u8>) -> bool {
        let bytes = Arc::new(bytes);
        self.registered_font_bytes.push(bytes.clone());
        if let Some(store) = self.font_store.as_mut() {
            store.register_font(bytes)
        } else {
            // Will be registered when the FontStore is built.
            true
        }
    }

    pub fn load_document(&mut self, doc: OfdDocument) {
        self.editor.load_document(doc);
        self.font_store = Some(self.build_font_store());
        self.modified = false;
        // Reset the visible-page cache: a newly loaded document may have a
        // different page count/layout, so the stale index must not persist
        // (it is recomputed on the next Scroll/Resize via maybe_fire_page_change).
        self.current_page = None;
    }

    pub fn new_document(&mut self) {
        self.editor.load_document(OfdDocument::default());
        self.font_store = Some(self.build_font_store());
        self.modified = false;
        self.current_page = None;
    }

    pub fn document(&self) -> &OfdDocument {
        self.editor.document()
    }
    pub fn selection(&self) -> &AnnotationSelection {
        self.editor.selection()
    }
    pub fn text_cursor(&self) -> Option<&TextCursor> {
        self.editor.text_cursor()
    }
    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }
    pub fn is_modified(&self) -> bool {
        self.modified
    }
    /// Reset the modified flag (call after a successful save).
    pub fn clear_modified(&mut self) {
        self.modified = false;
    }
    pub fn set_clock(&mut self, author: String, ts: i64) {
        self.editor.set_clock(author, ts);
    }

    /// Set the active editing tool. Switching tools cancels any in-progress
    /// drag (clears `drag`).
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.drag = None;
    }

    // Command pass-throughs. The host calls these for programmatic annotation
    // manipulation; handle_event is for keyboard/mouse. Both paths go through
    // after_annotation_change -> on_change.
    pub fn create_annotation(
        &mut self,
        kind: rofd_dom::AnnotationKind,
        page: rofd_dom::PageId,
        payload: rofd_dom::AnnotationPayload,
    ) -> rofd_dom::AnnotationId {
        let id = self.editor.create_annotation(kind, page, payload);
        self.after_annotation_change();
        self.fire_selection_change();
        id
    }

    pub fn delete_annotation(&mut self, id: &rofd_dom::AnnotationId) {
        self.editor.delete_annotation(id);
        self.after_annotation_change();
        self.fire_selection_change();
        self.fire_cursor_change();
    }

    pub fn move_annotation(&mut self, id: &rofd_dom::AnnotationId, dx: f64, dy: f64) {
        self.editor.move_annotation(id, dx, dy);
        self.after_annotation_change();
    }

    pub fn resize_annotation(&mut self, id: &rofd_dom::AnnotationId, new_rect: rofd_dom::Rect) {
        self.editor.resize_annotation(id, new_rect);
        self.after_annotation_change();
    }

    /// Build the current editor scene (paper-on-desk) for the host to paint.
    ///
    /// The native xilem canvas consumes the returned [`Scene`] via
    /// `Painter::replay`; the wasm host converts it to a `vello::Scene` via
    /// `imaging_vello::VelloSceneSink`. The `font_store` is lazily built on
    /// first call and reused (so a large default CJK font is not re-registered
    /// every frame).
    pub fn build_scene(&mut self) -> Scene {
        if self.font_store.is_none() {
            self.font_store = Some(self.build_font_store());
        }
        let fonts = self.font_store.as_ref().expect("font_store initialized");
        let drag_preview = self
            .drag
            .as_ref()
            .and_then(|d| drag_to_preview(self.editor.document(), d, &self.viewport));
        self.render.composite(
            self.editor.document(),
            &self.viewport,
            fonts,
            self.editor.selection(),
            drag_preview.as_ref(),
        )
    }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        let scene = self.build_scene();
        target.draw_scene(&scene);
    }

    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::{MouseButton, ScrollDirection, ViewEvent};
        match event {
            ViewEvent::PointerDown {
                button: MouseButton::Left,
                x,
                y,
                ..
            } => {
                let p = (*x, *y);
                match &self.tool {
                    Tool::Create(kind) => {
                        // Start a create-drag. The payload is built on PointerUp.
                        self.drag = Some(DragState::Create {
                            kind: kind.clone(),
                            start: p,
                            current: p,
                            path: vec![p],
                        });
                    }
                    Tool::Select => {
                        let target = rofd_render::hit_test(
                            self.editor.document(),
                            &self.viewport,
                            self.editor.selection(),
                            p,
                        );
                        match target {
                            rofd_render::HitTarget::Handle(id, h) => {
                                let was_selected = self.editor.selection().contains(&id);
                                if !was_selected {
                                    self.editor.select(id.clone());
                                    self.fire_annotation_focus(&id);
                                }
                                self.fire_annotation_interact(&id);
                                self.fire_selection_change();
                                // Markup/Freehand annotations are not resizable
                                // (they have no meaningful rect to drag-handle).
                                // Select only and skip the Resize drag setup so
                                // PointerUp doesn't commit a phantom Transaction.
                                let Some(ann) = self.editor.document().annotations.find(&id) else {
                                    // Annotation vanished between hit-test and
                                    // lookup (should not happen for a valid
                                    // selection). No-op repaint to refresh handles.
                                    return EventOutcome {
                                        needs_repaint: true,
                                    };
                                };
                                if matches!(
                                    &ann.payload,
                                    AnnotationPayload::Markup { .. }
                                        | AnnotationPayload::Freehand { .. }
                                ) {
                                    return EventOutcome {
                                        needs_repaint: true,
                                    };
                                }
                                // Set up resize drag: orig + anchor in page-local.
                                let orig =
                                    rofd_render::annotation_local_rect(ann).unwrap_or_default();
                                let anchor = opposite_corner(&orig, &h);
                                // current_local starts at the handle corner (no
                                // resize until the pointer moves).
                                let current_local = match &h {
                                    HandlePos::Nw => (orig.x, orig.y),
                                    HandlePos::Ne => (orig.x + orig.w, orig.y),
                                    HandlePos::Sw => (orig.x, orig.y + orig.h),
                                    HandlePos::Se => (orig.x + orig.w, orig.y + orig.h),
                                    HandlePos::N => ((orig.x + orig.w) / 2.0, orig.y),
                                    HandlePos::S => ((orig.x + orig.w) / 2.0, orig.y + orig.h),
                                    HandlePos::E => (orig.x + orig.w, (orig.y + orig.h) / 2.0),
                                    HandlePos::W => (orig.x, (orig.y + orig.h) / 2.0),
                                };
                                self.drag = Some(DragState::Resize {
                                    id: id.clone(),
                                    handle: h,
                                    anchor,
                                    orig,
                                    current_local,
                                    moved: false,
                                });
                            }
                            rofd_render::HitTarget::Annotation(id)
                            | rofd_render::HitTarget::AnnotationText(id, _) => {
                                let was_selected = self.editor.selection().contains(&id);
                                self.editor.select(id.clone());
                                if !was_selected {
                                    self.fire_annotation_focus(&id);
                                }
                                self.fire_annotation_interact(&id);
                                // For text annotations, position cursor at end.
                                if let Some(len) = self.text_content_len(&id) {
                                    self.editor.set_cursor(id.clone(), len);
                                }
                                self.fire_selection_change();
                                self.fire_cursor_change();
                                // Capture the annotation's page-local rect at
                                // drag start for the preview-based move.
                                let Some(ann) = self.editor.document().annotations.find(&id) else {
                                    // Annotation vanished between hit-test and
                                    // lookup (should not happen for a valid
                                    // selection). No-op repaint to refresh state.
                                    return EventOutcome {
                                        needs_repaint: true,
                                    };
                                };
                                let before_rect =
                                    rofd_render::annotation_local_rect(ann).unwrap_or_default();
                                self.drag = Some(DragState::Move {
                                    id: id.clone(),
                                    before_rect,
                                    start: p,
                                    last: p,
                                    moved: false,
                                });
                            }
                            _ => {
                                // Page or Empty: clear selection + cursor.
                                self.editor.clear_selection();
                                self.editor.clear_cursor();
                                self.fire_selection_change();
                                self.fire_cursor_change();
                            }
                        }
                    }
                }
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::PointerDown {
                button: MouseButton::Right,
                x,
                y,
                ..
            } => {
                // Right-click: hit-test to determine the context target and
                // fire on_context_menu. Does NOT change selection -- the host
                // shows a context menu (annotation actions vs page actions).
                let target = rofd_render::hit_test(
                    self.editor.document(),
                    &self.viewport,
                    self.editor.selection(),
                    (*x, *y),
                );
                let ct = match target {
                    rofd_render::HitTarget::Annotation(id)
                    | rofd_render::HitTarget::AnnotationText(id, _)
                    | rofd_render::HitTarget::Handle(id, _) => ContextTarget::Annotation(id),
                    rofd_render::HitTarget::Page(_) => ContextTarget::Page,
                    rofd_render::HitTarget::Empty => ContextTarget::Empty,
                };
                self.fire_context_menu((*x, *y), ct);
                EventOutcome {
                    needs_repaint: false,
                }
            }
            ViewEvent::PointerMove { x, y } => {
                let p = (*x, *y);
                match &mut self.drag {
                    Some(DragState::Create { current, path, .. }) => {
                        *current = p;
                        path.push(p);
                    }
                    Some(DragState::Move { last, moved, .. }) => {
                        // Preview-based: update `last` only; do NOT call
                        // editor.move_annotation (would flood history with one
                        // Transaction per pixel). The preview rect is computed
                        // in drag_to_preview from before_rect + (last - start).
                        *last = p;
                        *moved = true;
                    }
                    Some(DragState::Resize {
                        id,
                        handle: _,
                        anchor: _,
                        orig: _,
                        current_local,
                        moved,
                    }) => {
                        // Preview-based: convert the viewport point to
                        // page-local and store it; do NOT call
                        // editor.resize_annotation. The preview rect is
                        // computed in drag_to_preview via compute_resize.
                        let ann = self.editor.document().annotations.find(id);
                        if let Some(ann) = ann {
                            if let Some(local) = viewport_to_page_local(
                                self.editor.document(),
                                &self.viewport,
                                &ann.page,
                                p,
                            ) {
                                *current_local = local;
                                *moved = true;
                            }
                        }
                    }
                    None => {}
                }
                if self.drag.is_some() {
                    EventOutcome {
                        needs_repaint: true,
                    }
                } else {
                    EventOutcome {
                        needs_repaint: false,
                    }
                }
            }
            ViewEvent::PointerUp {
                button: MouseButton::Left,
                ..
            } => {
                if let Some(drag) = self.drag.take() {
                    match drag {
                        DragState::Create {
                            kind,
                            start,
                            current,
                            path,
                        } => {
                            // Resolve the page from the viewport-space `current`
                            // point, then convert start/current/path (viewport)
                            // to page-local before building the payload. The
                            // default zoom is PX_PER_MM (~3.78), so without this
                            // conversion created annotations would have
                            // viewport-space geometry baked in (wrong by the
                            // zoom factor + page origin). Mirrors how Move
                            // (divides delta by zoom) and Resize (uses
                            // viewport_to_page_local) already convert.
                            let doc = self.editor.document();
                            let page = current_page_id(doc, &self.viewport, current);
                            let start_local =
                                viewport_to_page_local(doc, &self.viewport, &page, start);
                            let current_local =
                                viewport_to_page_local(doc, &self.viewport, &page, current);
                            let (start_l, current_l) = match (start_local, current_local) {
                                (Some(s), Some(c)) => (s, c),
                                // Page not found (empty doc): fall back to
                                // raw viewport coords so a degenerate doc
                                // does not panic. current_page_id already
                                // returns a default PageId in this case.
                                _ => (start, current),
                            };
                            let path_local: Vec<(f64, f64)> = path
                                .iter()
                                .map(|&p| {
                                    viewport_to_page_local(doc, &self.viewport, &page, p)
                                        .unwrap_or(p)
                                })
                                .collect();
                            let payload =
                                build_create_payload(&kind, start_l, current_l, &path_local);
                            let id = self.editor.create_annotation(kind.clone(), page, payload);
                            self.editor.select(id.clone());
                            self.set_tool(Tool::Select);
                            self.after_annotation_change();
                            self.fire_annotation_focus(&id);
                            self.fire_annotation_interact(&id);
                            self.fire_selection_change();
                        }
                        DragState::Move {
                            id,
                            before_rect: _,
                            start,
                            last,
                            moved,
                        } => {
                            // Preview-based drag commit: issue a single
                            // editor.move_annotation with the cumulative
                            // page-local delta (one Transaction -> one undo).
                            // The annotation is still at its original position
                            // (the document was not mutated during PointerMove),
                            // so move_annotation reads the pre-drag rect.
                            if moved {
                                let zoom = self.viewport.zoom;
                                let dx = (last.0 - start.0) / zoom;
                                let dy = (last.1 - start.1) / zoom;
                                // Guard against a no-op (click-without-drag
                                // where last == start but moved was set true
                                // by a sub-pixel jitter): skip if delta is
                                // negligible.
                                if dx.abs() > f64::EPSILON || dy.abs() > f64::EPSILON {
                                    self.editor.move_annotation(&id, dx, dy);
                                    self.after_annotation_change();
                                }
                            }
                        }
                        DragState::Resize {
                            id,
                            handle,
                            anchor,
                            orig,
                            current_local,
                            moved,
                        } => {
                            // Preview-based drag commit: issue a single
                            // editor.resize_annotation with the final rect
                            // (one Transaction -> one undo).
                            if moved {
                                let final_rect =
                                    compute_resize(&handle, anchor, orig, current_local);
                                // Skip if the rect didn't actually change.
                                if final_rect != orig {
                                    self.editor.resize_annotation(&id, final_rect);
                                    self.after_annotation_change();
                                }
                            }
                        }
                    }
                    EventOutcome {
                        needs_repaint: true,
                    }
                } else {
                    EventOutcome {
                        needs_repaint: false,
                    }
                }
            }
            ViewEvent::Scroll { dx, dy } => {
                self.viewport.scroll.0 += dx;
                self.viewport.scroll.1 += dy;
                self.maybe_fire_page_change();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::Zoom { factor } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom *= factor;
                // Only fire on_zoom_change if the zoom actually changed (a
                // factor of 1.0 is a no-op). ZoomAt already has this guard (T2).
                if (self.viewport.zoom - old_zoom).abs() > f64::EPSILON {
                    self.fire_zoom_change(self.viewport.zoom);
                }
                // Zoom can also shift which page is at the viewport center
                // (page heights scale with zoom), so re-check page change.
                self.maybe_fire_page_change();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::Resize { width, height } => {
                self.viewport.size = (*width, *height);
                self.maybe_fire_page_change();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::ScrollPage { direction } => {
                let page_h = self
                    .editor
                    .document()
                    .pages
                    .first()
                    .map(|p| p.physical_box.h * self.viewport.zoom)
                    .unwrap_or(0.0);
                let delta = page_h + self.viewport.page_gap;
                self.viewport.scroll.1 += match direction {
                    ScrollDirection::Down => delta,
                    ScrollDirection::Up => -delta,
                };
                self.maybe_fire_page_change();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::ZoomAt { factor, center } => {
                let old_zoom = self.viewport.zoom;
                self.viewport.zoom *= factor;
                // Adjust scroll so the `center` viewport point maps to the
                // same document position before and after zoom. Derivation:
                // doc_pos = (viewport_pt - scroll) / old_zoom; after zoom,
                // viewport_pt = doc_pos * new_zoom + new_scroll. Solving for
                // new_scroll: new_scroll = center - (center - old_scroll) *
                // (new_zoom / old_zoom).
                let ratio = self.viewport.zoom / old_zoom;
                self.viewport.scroll.0 = center.0 - (center.0 - self.viewport.scroll.0) * ratio;
                self.viewport.scroll.1 = center.1 - (center.1 - self.viewport.scroll.1) * ratio;
                if (self.viewport.zoom - old_zoom).abs() > f64::EPSILON {
                    self.fire_zoom_change(self.viewport.zoom);
                }
                self.maybe_fire_page_change();
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::Ime { text } => {
                if let Some(cursor) = self.editor.text_cursor().cloned() {
                    let new_off = cursor.offset + text.chars().count();
                    self.editor
                        .insert_text(&cursor.annotation, cursor.offset, text);
                    self.editor.set_cursor(cursor.annotation.clone(), new_off);
                    self.after_annotation_change();
                    self.fire_cursor_change();
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
                EventOutcome {
                    needs_repaint: false,
                }
            }
            ViewEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
            _ => EventOutcome {
                needs_repaint: false,
            },
        }
    }

    fn text_content_len(&self, id: &rofd_dom::AnnotationId) -> Option<usize> {
        let ann = self.editor.document().annotations.find(id)?;
        use rofd_dom::AnnotationPayload;
        match &ann.payload {
            AnnotationPayload::Note { content, .. }
            | AnnotationPayload::TextBox { content, .. }
            | AnnotationPayload::Watermark { content, .. } => Some(content.chars().count()),
            _ => None,
        }
    }

    fn handle_key(
        &mut self,
        key: &crate::event::Key,
        modifiers: &crate::event::Modifiers,
    ) -> EventOutcome {
        use crate::event::Key;
        // Ctrl+Z: undo
        if modifiers.control && !modifiers.shift && matches!(key, Key::Char('z') | Key::Char('Z')) {
            if self.editor.undo() {
                self.after_annotation_change();
                return EventOutcome {
                    needs_repaint: true,
                };
            }
            return EventOutcome {
                needs_repaint: false,
            };
        }
        // Ctrl+Y or Ctrl+Shift+Z: redo
        if (modifiers.control && matches!(key, Key::Char('y') | Key::Char('Y')))
            || (modifiers.control
                && modifiers.shift
                && matches!(key, Key::Char('z') | Key::Char('Z')))
        {
            if self.editor.redo() {
                self.after_annotation_change();
                return EventOutcome {
                    needs_repaint: true,
                };
            }
            return EventOutcome {
                needs_repaint: false,
            };
        }
        // Ctrl+S: save request
        if modifiers.control && matches!(key, Key::Char('s') | Key::Char('S')) {
            self.fire_save_request();
            return EventOutcome {
                needs_repaint: false,
            };
        }
        // Delete/Backspace: delete selected (when no text cursor)
        if matches!(key, Key::Delete | Key::Backspace)
            && self.editor.text_cursor().is_none()
            && !matches!(
                self.editor.selection(),
                rofd_editor::AnnotationSelection::None
            )
        {
            self.editor.delete_selected();
            self.after_annotation_change();
            self.fire_selection_change();
            return EventOutcome {
                needs_repaint: true,
            };
        }
        // Text editing (if text cursor set)
        if let Some(cursor) = self.editor.text_cursor().cloned() {
            match key {
                Key::Char(c) => {
                    let s = c.to_string();
                    let new_off = cursor.offset + s.chars().count();
                    self.editor
                        .insert_text(&cursor.annotation, cursor.offset, &s);
                    self.editor.set_cursor(cursor.annotation.clone(), new_off);
                    self.after_annotation_change();
                    self.fire_cursor_change();
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
                Key::Backspace => {
                    if cursor.offset > 0 {
                        self.editor
                            .delete_text(&cursor.annotation, cursor.offset - 1, 1);
                        self.editor
                            .set_cursor(cursor.annotation.clone(), cursor.offset - 1);
                        self.after_annotation_change();
                        self.fire_cursor_change();
                        return EventOutcome {
                            needs_repaint: true,
                        };
                    }
                }
                Key::Delete => {
                    self.editor
                        .delete_text(&cursor.annotation, cursor.offset, 1);
                    self.after_annotation_change();
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
                Key::ArrowLeft => {
                    if cursor.offset > 0 {
                        self.editor
                            .set_cursor(cursor.annotation.clone(), cursor.offset - 1);
                        self.fire_cursor_change();
                        return EventOutcome {
                            needs_repaint: true,
                        };
                    }
                }
                Key::ArrowRight => {
                    self.editor
                        .set_cursor(cursor.annotation.clone(), cursor.offset + 1);
                    self.fire_cursor_change();
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
                Key::Escape => {
                    self.editor.clear_cursor();
                    self.editor.clear_selection();
                    self.fire_cursor_change();
                    self.fire_selection_change();
                    return EventOutcome {
                        needs_repaint: true,
                    };
                }
                _ => {}
            }
        } else if matches!(key, Key::Escape) {
            self.editor.clear_selection();
            self.fire_selection_change();
            return EventOutcome {
                needs_repaint: true,
            };
        }
        EventOutcome {
            needs_repaint: false,
        }
    }

    // Called by annotation-mutating commands (text editing, undo/redo, delete).
    fn after_annotation_change(&mut self) {
        self.modified = true;
        if let Some(cb) = &self.callbacks.on_change {
            cb(self.editor.document());
        }
    }

    fn fire_selection_change(&self) {
        if let Some(cb) = &self.callbacks.on_selection_change {
            cb(self.editor.selection());
        }
    }

    fn fire_cursor_change(&self) {
        if let Some(cb) = &self.callbacks.on_cursor_change {
            cb(self.editor.text_cursor());
        }
    }

    // Called by Ctrl+S / save shortcut.
    fn fire_save_request(&self) {
        if let Some(cb) = &self.callbacks.on_save_request {
            cb();
        }
    }

    // Called when an annotation is first selected (focused) or interacted with
    // (any PointerDown on it). T3 wires these in the PointerDown handler.
    fn fire_annotation_focus(&self, id: &rofd_dom::AnnotationId) {
        if let Some(cb) = &self.callbacks.on_annotation_focus {
            cb(id);
        }
    }

    fn fire_annotation_interact(&self, id: &rofd_dom::AnnotationId) {
        if let Some(cb) = &self.callbacks.on_annotation_interact {
            cb(id);
        }
    }

    // T4: context_menu / page_change / zoom_change fire helpers.

    /// Fire `on_context_menu` with the right-click point and target. Called
    /// from the right-click (PointerDown Right) handler.
    fn fire_context_menu(&self, point: (f64, f64), target: ContextTarget) {
        if let Some(cb) = &self.callbacks.on_context_menu {
            cb(point, target);
        }
    }

    /// Fire `on_page_change` with the new visible page index. Called from
    /// `maybe_fire_page_change` when the current page actually changes.
    fn fire_page_change(&self, idx: usize) {
        if let Some(cb) = &self.callbacks.on_page_change {
            cb(idx);
        }
    }

    /// Fire `on_zoom_change` with the new zoom factor. Called from the Zoom
    /// event handler after `self.viewport.zoom` is updated.
    fn fire_zoom_change(&self, zoom: f64) {
        if let Some(cb) = &self.callbacks.on_zoom_change {
            cb(zoom);
        }
    }

    /// Fire `on_warning` with the given warnings. Called by the adapter layer
    /// (EditorApp/WasmEditor) after `parse_ofd` returns a `LoadReport` with
    /// warnings - the component itself is io-free and never parses, so it never
    /// generates warnings; it only relays them from the adapter (AGENTS.md §4.6:
    /// degraded-input path surfaces to the host via callback).
    pub fn fire_warnings(&self, warnings: &[OfdWarning]) {
        if let Some(cb) = &self.callbacks.on_warning {
            cb(warnings);
        }
    }

    /// Recompute the current visible page (the page whose viewport rect
    /// contains the viewport's vertical center) and, if it differs from
    /// `self.current_page`, update the field and fire `on_page_change`.
    ///
    /// The page-stacking geometry is computed via [`rofd_render::page_origins`]
    /// (the shared batch helper). The viewport center Y is `size.1 / 2`.
    fn maybe_fire_page_change(&mut self) {
        let new_page = self.visible_page_index();
        if new_page != self.current_page {
            self.current_page = new_page;
            if let Some(idx) = new_page {
                self.fire_page_change(idx);
            }
        }
    }

    /// Compute the index of the page whose viewport rect contains the
    /// viewport's vertical center. Returns `None` if the document has no
    /// pages or no page spans the center line.
    ///
    /// Uses [`rofd_render::page_origins`] (the shared page-stacking batch
    /// helper) for the viewport Y geometry.
    fn visible_page_index(&self) -> Option<usize> {
        let doc = self.editor.document();
        if doc.pages.is_empty() {
            return None;
        }
        let center_y = self.viewport.size.1 / 2.0;
        // Compute all origins in one O(n) pass (avoids O(n²) from per-page
        // page_origin calls in both passes below).
        let origins = rofd_render::page_origins(doc, &self.viewport);
        // Pass 1: find a page whose viewport rect spans the center line.
        for (i, page) in doc.pages.iter().enumerate() {
            let page_h = page.physical_box.h * self.viewport.zoom;
            if let Some(&(_, origin_y)) = origins.get(i) {
                if center_y >= origin_y && center_y < origin_y + page_h {
                    return Some(i);
                }
            }
        }
        // Pass 2 (fallback): center is in a gap or past the last page - return
        // the last page whose top is above the center (the most-recently-
        // scrolled-past page).
        let mut last_above: Option<usize> = None;
        for (i, _page) in doc.pages.iter().enumerate() {
            if let Some(&(_, origin_y)) = origins.get(i) {
                if origin_y <= center_y {
                    last_above = Some(i);
                }
            }
        }
        last_above
    }

    // Callback setters. Each emits two cfg-gated copies: `+ Send` on native
    // (the host in Phase 4b stores callbacks across threads) and plain on wasm
    // (single-threaded). This mirrors the `Send`-gating in `callbacks.rs`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_change(&mut self, cb: impl Fn(&OfdDocument) + 'static + Send) {
        self.callbacks.on_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_change(&mut self, cb: impl Fn(&OfdDocument) + 'static) {
        self.callbacks.on_change = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_selection_change(&mut self, cb: impl Fn(&AnnotationSelection) + 'static + Send) {
        self.callbacks.on_selection_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_selection_change(&mut self, cb: impl Fn(&AnnotationSelection) + 'static) {
        self.callbacks.on_selection_change = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_cursor_change(&mut self, cb: impl Fn(Option<&TextCursor>) + 'static + Send) {
        self.callbacks.on_cursor_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_cursor_change(&mut self, cb: impl Fn(Option<&TextCursor>) + 'static) {
        self.callbacks.on_cursor_change = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_save_request(&mut self, cb: impl Fn() + 'static + Send) {
        self.callbacks.on_save_request = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_save_request(&mut self, cb: impl Fn() + 'static) {
        self.callbacks.on_save_request = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_annotation_focus(&mut self, cb: impl Fn(&rofd_dom::AnnotationId) + 'static + Send) {
        self.callbacks.on_annotation_focus = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_annotation_focus(&mut self, cb: impl Fn(&rofd_dom::AnnotationId) + 'static) {
        self.callbacks.on_annotation_focus = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_annotation_interact(
        &mut self,
        cb: impl Fn(&rofd_dom::AnnotationId) + 'static + Send,
    ) {
        self.callbacks.on_annotation_interact = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_annotation_interact(&mut self, cb: impl Fn(&rofd_dom::AnnotationId) + 'static) {
        self.callbacks.on_annotation_interact = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_context_menu(&mut self, cb: impl Fn((f64, f64), ContextTarget) + 'static + Send) {
        self.callbacks.on_context_menu = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_context_menu(&mut self, cb: impl Fn((f64, f64), ContextTarget) + 'static) {
        self.callbacks.on_context_menu = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_page_change(&mut self, cb: impl Fn(usize) + 'static + Send) {
        self.callbacks.on_page_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_page_change(&mut self, cb: impl Fn(usize) + 'static) {
        self.callbacks.on_page_change = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_zoom_change(&mut self, cb: impl Fn(f64) + 'static + Send) {
        self.callbacks.on_zoom_change = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_zoom_change(&mut self, cb: impl Fn(f64) + 'static) {
        self.callbacks.on_zoom_change = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_warning(&mut self, cb: impl Fn(&[OfdWarning]) + 'static + Send) {
        self.callbacks.on_warning = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_warning(&mut self, cb: impl Fn(&[OfdWarning]) + 'static) {
        self.callbacks.on_warning = Some(Box::new(cb));
    }
}

/// Map an in-progress [`DragState`] to a renderable [`DragPreview`].
///
/// For `Create`, the rect is the page-local bounding box of `start`/`current`
/// (for rect-bounded kinds) or the accumulated viewport-space `path` (for
/// Freehand, passed through as viewport-space to match
/// `DragPreview::CreateFreehand`).
///
/// For `Move`/`Resize`, a semi-transparent preview overlay is returned showing
/// the would-be position/rect. The actual annotation stays at its original
/// position during the drag (the editor command is issued once on PointerUp),
/// so this preview is what the user sees following the pointer.
///
/// `doc` is required to resolve the target page for the viewport->page-local
/// conversion of `Create` rect-bounded previews (so `draw_drag_preview`'s
/// page-local transform maps the preview back under the cursor).
fn drag_to_preview(doc: &OfdDocument, d: &DragState, vp: &Viewport) -> Option<DragPreview> {
    match d {
        DragState::Create {
            kind,
            start,
            current,
            path,
        } => {
            if *kind == AnnotationKind::Freehand {
                Some(DragPreview::CreateFreehand { path: path.clone() })
            } else {
                // Resolve the page from the viewport-space `current` point
                // (same page-selection logic as the PointerUp handler), then
                // convert start/current to page-local so the rect is in the
                // coordinate space `draw_drag_preview` expects for
                // `DragPreview::Create` (page-local, transformed back to
                // viewport by * zoom + page origin).
                let page = current_page_id(doc, vp, *current);
                let start_local = viewport_to_page_local(doc, vp, &page, *start).unwrap_or(*start);
                let current_local =
                    viewport_to_page_local(doc, vp, &page, *current).unwrap_or(*current);
                let rect = bbox(start_local, current_local);
                Some(DragPreview::Create {
                    kind: kind.clone(),
                    rect,
                })
            }
        }
        DragState::Move {
            id,
            before_rect,
            start,
            last,
            ..
        } => {
            // Preview rect = before_rect translated by the cumulative
            // page-local delta ((last - start) / zoom).
            let zoom = vp.zoom;
            let dx = (last.0 - start.0) / zoom;
            let dy = (last.1 - start.1) / zoom;
            let rect = Rect {
                x: before_rect.x + dx,
                y: before_rect.y + dy,
                w: before_rect.w,
                h: before_rect.h,
            };
            Some(DragPreview::Move {
                id: id.clone(),
                rect,
            })
        }
        DragState::Resize {
            id,
            handle,
            anchor,
            orig,
            current_local,
            ..
        } => {
            let rect = compute_resize(handle, *anchor, *orig, *current_local);
            Some(DragPreview::Resize {
                id: id.clone(),
                rect,
            })
        }
    }
}

/// Bounding box of two points as a [`Rect`] (x/y = min, w/h = delta).
fn bbox(a: (f64, f64), b: (f64, f64)) -> Rect {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    let w = (a.0 - b.0).abs();
    let h = (a.1 - b.1).abs();
    Rect { x, y, w, h }
}

// --- Default colors for create-drag (spec section 3.5) ---

/// Default highlight color: yellow (spec section 3.5; matches io parse fallback).
const DEFAULT_HIGHLIGHT_COLOR: Color = Color::Rgb(255, 255, 0);
/// Default underline/strikeout/squiggly color: blue.
const DEFAULT_MARKUP_COLOR: Color = Color::Rgb(0, 0, 255);
/// Default freehand color: black.
const DEFAULT_FREEHAND_COLOR: Color = Color::Rgb(0, 0, 0);
/// Default shape (Rect) stroke color: red.
const DEFAULT_SHAPE_COLOR: Color = Color::Rgb(255, 0, 0);
/// Default freehand stroke width.
const DEFAULT_FREEHAND_WIDTH: f64 = 1.5;
/// Default shape stroke width.
const DEFAULT_SHAPE_WIDTH: f64 = 2.0;

/// Compute the new rect when dragging a resize handle.
///
/// `handle` identifies the dragged handle, `anchor` is the fixed opposite
/// corner/edge (page-local), `orig` is the annotation's rect at drag start
/// (page-local), and `point` is the current pointer position (page-local).
///
/// Corner handles (Nw/Ne/Sw/Se): the new rect is the bounding box of `anchor`
/// and `point`.
///
/// Edge handles (N/S/E/W): the opposite edge stays fixed, and the dragged edge
/// moves to `point`. For example, dragging the E (east) edge fixes x/y/h and
/// sets w = point.x - orig.x; dragging the N (north) edge fixes x/w and sets
/// y = point.y, h = orig.y + orig.h - point.y.
fn compute_resize(handle: &HandlePos, anchor: (f64, f64), orig: Rect, point: (f64, f64)) -> Rect {
    match handle {
        HandlePos::Nw | HandlePos::Ne | HandlePos::Sw | HandlePos::Se => {
            // Corner: new rect = bbox(anchor, point).
            bbox(anchor, point)
        }
        HandlePos::E => Rect {
            x: orig.x,
            y: orig.y,
            w: point.0 - orig.x,
            h: orig.h,
        },
        HandlePos::W => Rect {
            x: point.0,
            y: orig.y,
            w: orig.x + orig.w - point.0,
            h: orig.h,
        },
        HandlePos::S => Rect {
            x: orig.x,
            y: orig.y,
            w: orig.w,
            h: point.1 - orig.y,
        },
        HandlePos::N => Rect {
            x: orig.x,
            y: point.1,
            w: orig.w,
            h: orig.y + orig.h - point.1,
        },
    }
}

/// Build the [`AnnotationPayload`] for a newly created annotation from the
/// drag geometry.
///
/// - Markup (Highlight/Underline/Strikeout/Squiggly): quad_points = [start, current].
/// - Freehand: path from accumulated `path` points (M + L commands).
/// - Shape (Rect): rect = bbox(start, current).
fn build_create_payload(
    kind: &AnnotationKind,
    start: (f64, f64),
    current: (f64, f64),
    path: &[(f64, f64)],
) -> AnnotationPayload {
    match kind {
        AnnotationKind::Highlight => AnnotationPayload::Markup {
            quad_points: vec![
                Point {
                    x: start.0,
                    y: start.1,
                },
                Point {
                    x: current.0,
                    y: current.1,
                },
            ],
            color: DEFAULT_HIGHLIGHT_COLOR,
        },
        AnnotationKind::Underline | AnnotationKind::Strikeout | AnnotationKind::Squiggly => {
            AnnotationPayload::Markup {
                quad_points: vec![
                    Point {
                        x: start.0,
                        y: start.1,
                    },
                    Point {
                        x: current.0,
                        y: current.1,
                    },
                ],
                color: DEFAULT_MARKUP_COLOR,
            }
        }
        AnnotationKind::Freehand => {
            let commands = path
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| {
                    if i == 0 {
                        PathCommand::M(x, y)
                    } else {
                        PathCommand::L(x, y)
                    }
                })
                .collect();
            AnnotationPayload::Freehand {
                path: PathData { commands },
                color: DEFAULT_FREEHAND_COLOR,
                width: DEFAULT_FREEHAND_WIDTH,
            }
        }
        AnnotationKind::Shape(shape_kind) => AnnotationPayload::Shape {
            kind: *shape_kind,
            rect: bbox(start, current),
            stroke: DEFAULT_SHAPE_COLOR,
            fill: None,
            width: DEFAULT_SHAPE_WIDTH,
            points: vec![],
        },
        // Note/TextBox/Stamp/Watermark: use bbox as rect with minimal defaults.
        // The host can refine via property panels later.
        AnnotationKind::Note => AnnotationPayload::Note {
            rect: bbox(start, current),
            color: Color::Rgb(255, 200, 0),
            content: String::new(),
            icon: rofd_dom::NoteIcon::Note,
        },
        AnnotationKind::TextBox => AnnotationPayload::TextBox {
            rect: bbox(start, current),
            content: String::new(),
            font: rofd_dom::FontId::new(""),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
        },
        AnnotationKind::Stamp => AnnotationPayload::Stamp {
            rect: bbox(start, current),
            image: rofd_dom::ImageId::new(""),
        },
        AnnotationKind::Watermark => AnnotationPayload::Watermark {
            rect: bbox(start, current),
            content: String::new(),
            opacity: 0.3,
            angle: 45.0,
            font: rofd_dom::FontId::new(""),
            size: 48.0,
            color: Color::Rgb(200, 200, 200),
        },
    }
}

/// Find the page whose viewport rect contains `point`, returning its [`PageId`].
/// Uses [`rofd_render::page_origins`] (the shared page-stacking batch helper)
/// for the viewport geometry.
fn current_page_id(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> PageId {
    let origins = rofd_render::page_origins(doc, vp);
    for (i, page) in doc.pages.iter().enumerate() {
        if let Some(&(origin_x, origin_y)) = origins.get(i) {
            let page_w = page.physical_box.w * vp.zoom;
            let page_h = page.physical_box.h * vp.zoom;
            if point.0 >= origin_x
                && point.0 <= origin_x + page_w
                && point.1 >= origin_y
                && point.1 <= origin_y + page_h
            {
                return page.id.clone();
            }
        }
    }
    // Fallback: first page if any, else a default PageId.
    doc.pages.first().map(|p| p.id.clone()).unwrap_or_default()
}

/// Convert a viewport-space point to page-local coordinates for a specific page.
/// Returns `None` if the page is not found. Uses [`rofd_render::page_origin`]
/// (the shared page-stacking helper) for the viewport geometry.
fn viewport_to_page_local(
    doc: &OfdDocument,
    vp: &Viewport,
    page_id: &PageId,
    point: (f64, f64),
) -> Option<(f64, f64)> {
    let page_idx = doc.pages.iter().position(|p| p.id == *page_id)?;
    let (origin_x, origin_y) = rofd_render::page_origin(doc, vp, page_idx)?;
    Some((
        (point.0 - origin_x) / vp.zoom,
        (point.1 - origin_y) / vp.zoom,
    ))
}

/// Compute the corner/point opposite to a handle on a rect (page-local).
///
/// For corner handles (Nw/Ne/Sw/Se), this is the diagonally opposite corner.
/// For edge handles (N/S/E/W), this is the midpoint of the opposite edge --
/// used as the fixed anchor during resize.
fn opposite_corner(rect: &Rect, handle: &HandlePos) -> (f64, f64) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w;
    let y1 = rect.y + rect.h;
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    match handle {
        HandlePos::Nw => (x1, y1), // opposite = Se
        HandlePos::Ne => (x0, y1), // opposite = Sw
        HandlePos::Sw => (x1, y0), // opposite = Ne
        HandlePos::Se => (x0, y0), // opposite = Nw
        HandlePos::N => (cx, y1),  // opposite edge = S midpoint
        HandlePos::S => (cx, y0),  // opposite edge = N midpoint
        HandlePos::E => (x0, cy),  // opposite edge = W midpoint
        HandlePos::W => (x1, cy),  // opposite edge = E midpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_target::RenderTarget;
    use rofd_render::Scene;
    use std::sync::Arc;

    struct MockRenderTarget {
        drawn: usize,
        w: f64,
        h: f64,
    }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _: &Scene) {
            self.drawn += 1;
        }
        fn size(&self) -> (f64, f64) {
            (self.w, self.h)
        }
    }

    #[test]
    fn new_constructs_with_defaults() {
        let c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        assert!(!c.is_modified());
        assert!(!c.can_undo());
    }

    #[test]
    fn render_draws_to_target() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let mut rt = MockRenderTarget {
            drawn: 0,
            w: 800.0,
            h: 600.0,
        };
        c.render(&mut rt);
        assert_eq!(rt.drawn, 1);
    }

    use crate::event::{Key, Modifiers, MouseButton, ScrollDirection, ViewEvent};
    use rofd_dom::{
        AnnotationKind, AnnotationPayload, AnnotationSelection, Color, Layer, NoteIcon,
        OfdDocument, Page, PageId, PathCommand, PathData, Point, Rect, ShapeKind,
    };
    use std::sync::Mutex;

    fn component_with_note() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        // Insert a page P0 so hit_test / current_page_id can resolve. physical_box
        // starts at (0,0); with size=(0,0) + page_gap=0 + zoom=1, the page origin
        // is (0,0) so page-local coords == viewport coords (simplifies test points).
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
        c.editor.create_annotation(
            AnnotationKind::Note,
            PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "hi".into(),
                icon: NoteIcon::Note,
            },
        );
        // The create_annotation above bypasses handle_event (direct editor call for test setup).
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        c
    }

    #[test]
    fn scroll_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.scroll, (10.0, 20.0));
    }

    #[test]
    fn zoom_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Zoom { factor: 2.0 });
        assert!(outcome.needs_repaint);
        // Default zoom is PX_PER_MM (96 DPI); Zoom multiplies on top.
        assert_eq!(c.viewport.zoom, rofd_render::PX_PER_MM * 2.0);
    }

    #[test]
    fn resize_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Resize {
            width: 1024.0,
            height: 768.0,
        });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.size, (1024.0, 768.0));
    }

    #[test]
    fn on_change_fires_after_scroll_callback_set() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = component_with_note();
        c.on_change(move |_| {
            *f.lock().unwrap() = true;
        });
        // Scroll doesn't change annotations -> no on_change. But it does need_repaint.
        c.handle_event(&ViewEvent::Scroll { dx: 1.0, dy: 0.0 });
        assert!(!*fired.lock().unwrap(), "scroll does not fire on_change");
    }

    #[test]
    fn on_warning_fires_with_load_warnings() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_warning(move |_warnings| {
            *f.lock().unwrap() = true;
        });
        // fire_warnings is the adapter's entry point for relaying LoadReport
        // warnings (component itself never parses, so it never generates
        // warnings). Test the fire path directly.
        c.fire_warnings(&[OfdWarning::MissingFeature {
            feature: "test".into(),
            entry: "test".into(),
        }]);
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn on_warning_does_not_fire_when_no_callback_set() {
        let c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        // No callback set -> fire_warnings is a no-op (must not panic).
        c.fire_warnings(&[OfdWarning::MissingFeature {
            feature: "test".into(),
            entry: "test".into(),
        }]);
    }

    #[test]
    fn undo_redo_via_keydown() {
        let mut c = component_with_note();
        // undo the create_annotation (done via direct editor call in setup)
        let outcome = c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('z'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        assert!(outcome.needs_repaint);
        assert!(!c.can_undo(), "undo consumed the create");
        // redo
        let outcome = c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('y'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        assert!(outcome.needs_repaint);
        assert!(c.can_undo(), "redo restored it");
    }

    #[test]
    fn ctrl_s_fires_save_request() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_save_request(move || {
            *f.lock().unwrap() = true;
        });
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('s'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn char_key_inserts_text_when_cursor_set() {
        let mut c = component_with_note();
        // Select the note (which sets cursor to end via PointerDown on the annotation).
        // For test setup, directly set cursor.
        let id = c.editor.selection().clone();
        if let rofd_editor::AnnotationSelection::Single(id) = id {
            c.editor.set_cursor(id.clone(), 2); // "hi" has 2 chars; cursor at end
        }
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('!'),
            modifiers: Modifiers::default(),
        });
        // The note's content should now be "hi!"
        let sel = c.editor.selection().clone();
        if let rofd_editor::AnnotationSelection::Single(id) = sel {
            let ann = c.editor.document().annotations.find(&id).unwrap();
            assert!(
                matches!(&ann.payload, rofd_dom::AnnotationPayload::Note { content, .. } if content == "hi!")
            );
        } else {
            panic!("expected single selection");
        }
    }

    #[test]
    fn clear_modified_resets_flag() {
        let mut c = component_with_note();
        // component_with_note() uses a direct editor call (bypasses
        // after_annotation_change), so establish modified=true via a real
        // component-level mutation before clearing.
        let sel = c.editor.selection().clone();
        if let rofd_editor::AnnotationSelection::Single(id) = sel {
            c.move_annotation(&id, 1.0, 1.0);
        }
        assert!(c.is_modified(), "mutation sets modified");
        c.clear_modified();
        assert!(!c.is_modified(), "clear_modified resets");
    }

    #[test]
    fn escape_clears_selection() {
        let mut c = component_with_note();
        assert!(!matches!(
            c.editor.selection(),
            rofd_editor::AnnotationSelection::None
        ));
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Escape,
            modifiers: Modifiers::default(),
        });
        assert!(matches!(
            c.editor.selection(),
            rofd_editor::AnnotationSelection::None
        ));
    }

    #[test]
    fn set_tool_changes_tool_state() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        assert!(matches!(c.tool, Tool::Select));
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        assert!(matches!(c.tool, Tool::Create(_)));
        c.set_tool(Tool::Select);
        assert!(matches!(c.tool, Tool::Select));
    }

    #[test]
    fn create_rect_via_drag() {
        let mut c = component_with_note();
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 10.0,
            y: 10.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 50.0, y: 60.0 });
        let outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 60.0,
        });
        assert!(outcome.needs_repaint);
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::Single(_)),
            "new rect selected"
        );
        assert!(
            matches!(c.tool, Tool::Select),
            "tool back to Select after create"
        );
        // Verify the annotation was created with the expected rect.
        if let AnnotationSelection::Single(id) = c.editor.selection() {
            let ann = c.editor.document().annotations.find(id).unwrap();
            match &ann.payload {
                AnnotationPayload::Shape { rect, .. } => {
                    // bbox((10,10),(50,60)) = (10,10,40,50)
                    assert_eq!(
                        *rect,
                        Rect {
                            x: 10.0,
                            y: 10.0,
                            w: 40.0,
                            h: 50.0
                        }
                    );
                }
                _ => panic!("expected Shape payload"),
            }
        }
    }

    /// Regression test for C3 review finding I1: at zoom != 1, a create-drag
    /// must store page-local geometry (viewport / zoom), not raw viewport
    /// pixels. Without the conversion the annotation rect would be off by the
    /// zoom factor (e.g. 2x too large at zoom=2).
    ///
    /// Setup: page P0 200x200 at origin, zoom=2.0, scroll=(0,0), page_gap=0,
    /// viewport size 0 (so page origin is (0,0)). Drag from viewport (40,40)
    /// to (140,140). Page-local = viewport / 2 = (20,20) -> (70,70), so the
    /// annotation rect should be bbox((20,20),(70,70)) = (20,20,50,50).
    #[test]
    fn create_rect_via_drag_converts_to_page_local_at_non_unit_zoom() {
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
        // zoom=2.0, size=(0,0) -> page origin (0,0); page-local = viewport / 2.
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 2.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 40.0,
            y: 40.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 140.0, y: 140.0 });
        let outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 140.0,
            y: 140.0,
        });
        assert!(outcome.needs_repaint);
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::Single(_)),
            "new rect selected"
        );
        if let AnnotationSelection::Single(id) = c.editor.selection() {
            let ann = c.editor.document().annotations.find(id).unwrap();
            match &ann.payload {
                AnnotationPayload::Shape { rect, .. } => {
                    // page-local = (40/2, 40/2) -> (140/2, 140/2) = (20,20)->(70,70)
                    // bbox = (20,20,50,50). NOT the viewport-space (40,40,100,100).
                    assert_eq!(
                        *rect,
                        Rect {
                            x: 20.0,
                            y: 20.0,
                            w: 50.0,
                            h: 50.0
                        },
                        "create-drag rect must be page-local (viewport / zoom), not viewport-space"
                    );
                }
                _ => panic!("expected Shape payload"),
            }
        }
    }

    #[test]
    fn move_annotation_via_drag() {
        let mut c = component_with_note();
        // The note is at rect (0,0,100,100). Click at (50,50) -- the center,
        // away from the 8px handle hit radius -- to select + start move.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        // Drag to (60,60) -> dx=10, dy=10 (page-local == viewport at zoom=1).
        c.handle_event(&ViewEvent::PointerMove { x: 60.0, y: 60.0 });
        let outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 60.0,
            y: 60.0,
        });
        assert!(outcome.needs_repaint);
        // The note's rect should have moved by (10,10): (10,10,100,100).
        if let AnnotationSelection::Single(id) = c.editor.selection() {
            let ann = c.editor.document().annotations.find(id).unwrap();
            match &ann.payload {
                AnnotationPayload::Note { rect, .. } => {
                    assert_eq!(
                        *rect,
                        Rect {
                            x: 10.0,
                            y: 10.0,
                            w: 100.0,
                            h: 100.0
                        },
                        "rect moved by (10,10)"
                    );
                }
                _ => panic!("expected Note payload"),
            }
        } else {
            panic!("expected single selection after move");
        }
    }

    #[test]
    fn move_then_single_undo_restores() {
        let mut c = component_with_note();
        // The note starts at rect (0,0,100,100). Record the undo depth before
        // the drag: component_with_note() pushed one Transaction (the create).
        let undo_before = c.editor.history_len();
        // Click center to select + start move.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        // Simulate a 100-pixel drag via many PointerMove events. With the
        // old (per-pixel) approach this would push ~100 Transactions; with
        // the preview-based approach it pushes ZERO during PointerMove.
        for i in 1..=100 {
            let v = 50.0 + i as f64;
            c.handle_event(&ViewEvent::PointerMove { x: v, y: v });
        }
        // PointerUp commits a single move_annotation -> one Transaction.
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 150.0,
            y: 150.0,
        });
        // Exactly one Transaction should have been added (drag = 1 undo step).
        let undo_after = c.editor.history_len();
        assert_eq!(
            undo_after,
            undo_before + 1,
            "a drag move must push exactly one Transaction (got {} added)",
            undo_after - undo_before
        );
        // The annotation should have moved by (100,100) -> (100,100,100,100).
        let id = match c.editor.selection().clone() {
            AnnotationSelection::Single(id) => id,
            _ => panic!("expected single selection"),
        };
        let ann = c.editor.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Note { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 100.0,
                        y: 100.0,
                        w: 100.0,
                        h: 100.0
                    },
                    "rect moved by (100,100) after drag"
                );
            }
            _ => panic!("expected Note payload"),
        }
        // ONE undo must restore the original rect (not 100 undos).
        assert!(c.editor.undo(), "undo succeeds");
        assert!(
            !c.editor.can_undo() || c.editor.history_len() == undo_before,
            "a single undo consumed the drag transaction"
        );
        let ann = c.editor.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Note { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 100.0,
                        h: 100.0
                    },
                    "one undo restores the original rect"
                );
            }
            _ => panic!("expected Note payload"),
        }
    }

    #[test]
    fn resize_then_single_undo_restores() {
        let mut c = component_with_note();
        let undo_before = c.editor.history_len();
        // Select the note (rect (0,0,100,100)) first.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        // Grab the Se handle (at (100,100)) and drag through many points.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 100.0,
            y: 100.0,
            modifiers: Modifiers::default(),
        });
        for i in 1..=50 {
            let v = 100.0 + i as f64;
            c.handle_event(&ViewEvent::PointerMove { x: v, y: v });
        }
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 150.0,
            y: 150.0,
        });
        // Exactly one Transaction for the resize.
        let undo_after = c.editor.history_len();
        assert_eq!(
            undo_after,
            undo_before + 1,
            "a drag resize must push exactly one Transaction (got {} added)",
            undo_after - undo_before
        );
        let id = match c.editor.selection().clone() {
            AnnotationSelection::Single(id) => id,
            _ => panic!("expected single selection"),
        };
        // Se corner resize: anchor = Nw = (0,0), new rect = bbox((0,0),(150,150)).
        let ann = c.editor.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Note { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 150.0,
                        h: 150.0
                    },
                    "rect resized to (0,0,150,150)"
                );
            }
            _ => panic!("expected Note payload"),
        }
        // ONE undo restores the original rect.
        assert!(c.editor.undo(), "undo succeeds");
        let ann = c.editor.document().annotations.find(&id).unwrap();
        match &ann.payload {
            AnnotationPayload::Note { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 100.0,
                        h: 100.0
                    },
                    "one undo restores the original rect"
                );
            }
            _ => panic!("expected Note payload"),
        }
    }

    #[test]
    fn resize_annotation_via_se_handle_drag() {
        let mut c = component_with_note();
        // The note is at rect (0,0,100,100). First select it by clicking center.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        // Now the Se handle is at (100,100). Click on it (within 8px radius)
        // and drag to (120,130) to resize.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 100.0,
            y: 100.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 120.0, y: 130.0 });
        let outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 120.0,
            y: 130.0,
        });
        assert!(outcome.needs_repaint);
        // Se corner resize: anchor = Nw = (0,0), new rect = bbox((0,0),(120,130)).
        if let AnnotationSelection::Single(id) = c.editor.selection() {
            let ann = c.editor.document().annotations.find(id).unwrap();
            match &ann.payload {
                AnnotationPayload::Note { rect, .. } => {
                    assert_eq!(
                        *rect,
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 120.0,
                            h: 130.0
                        },
                        "rect resized to (0,0,120,130)"
                    );
                }
                _ => panic!("expected Note payload"),
            }
        }
    }

    /// Build a component with one page (P0, 200x200) and a Highlight (Markup)
    /// annotation covering the full page. Viewport: zoom=1, size=(0,0), gap=0,
    /// scroll=(0,0) so page origin is (0,0) and viewport coords == page-local.
    /// Used to test the resize guard for Markup annotations.
    fn component_with_markup() -> EditorComponent {
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
        c.editor.create_annotation(
            AnnotationKind::Highlight,
            PageId::new("P0"),
            AnnotationPayload::Markup {
                // Quad pair spanning (10,10)-(100,100) -> bbox (10,10,90,90).
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
                color: Color::Rgb(255, 255, 0),
            },
        );
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        c
    }

    /// Resize guard: clicking a handle on a Markup (Highlight) annotation must
    /// NOT enter Resize. The annotation is not resizable, so no DragState::Resize
    /// should be set up, and a subsequent PointerUp must not push a Transaction.
    #[test]
    fn markup_handle_down_does_not_enter_resize() {
        let mut c = component_with_markup();
        let undo_before = c.editor.history_len();
        // Select the markup by clicking inside it (at (50,50) -- inside the
        // (10,10)-(100,100) bbox).
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        // Now the markup is selected. Its viewport rect is (10,10,90,90) (bbox
        // of the quad pair at zoom 1, page origin (0,0)). The Se handle is at
        // (100,100). Click on it.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 100.0,
            y: 100.0,
            modifiers: Modifiers::default(),
        });
        // No DragState::Resize should have been set up.
        assert!(
            c.drag.is_none(),
            "Markup handle must not enter Resize (drag should be None)"
        );
        // Move + Up should not push a Transaction (no resize happened).
        c.handle_event(&ViewEvent::PointerMove { x: 120.0, y: 120.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 120.0,
            y: 120.0,
        });
        assert_eq!(
            c.editor.history_len(),
            undo_before,
            "Markup handle drag must not push a Transaction (no phantom resize)"
        );
        // The annotation's quad_points should be unchanged.
        if let AnnotationSelection::Single(id) = c.editor.selection() {
            let ann = c.editor.document().annotations.find(id).unwrap();
            match &ann.payload {
                AnnotationPayload::Markup { quad_points, .. } => {
                    assert_eq!(
                        quad_points.len(),
                        2,
                        "Markup quad_points unchanged after handle drag"
                    );
                }
                _ => panic!("expected Markup payload"),
            }
        }
    }

    /// Build a component with one page (P0, 200x200) and a Freehand annotation.
    /// Viewport: zoom=1, size=(0,0), gap=0, scroll=(0,0) so page origin is (0,0).
    fn component_with_freehand() -> EditorComponent {
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
        c.editor.create_annotation(
            AnnotationKind::Freehand,
            PageId::new("P0"),
            AnnotationPayload::Freehand {
                path: PathData {
                    // M(10,10) L(100,100) -> bbox (10,10,90,90).
                    commands: vec![PathCommand::M(10.0, 10.0), PathCommand::L(100.0, 100.0)],
                },
                color: Color::Rgb(0, 0, 255),
                width: 1.5,
            },
        );
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        c
    }

    /// Resize guard: clicking a handle on a Freehand annotation must NOT enter
    /// Resize. Same logic as the Markup test.
    #[test]
    fn freehand_handle_down_does_not_enter_resize() {
        let mut c = component_with_freehand();
        let undo_before = c.editor.history_len();
        // Select the freehand by clicking inside its bbox (at (50,50)).
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        // The freehand bbox is (10,10,90,90). Se handle at (100,100). Click it.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 100.0,
            y: 100.0,
            modifiers: Modifiers::default(),
        });
        assert!(
            c.drag.is_none(),
            "Freehand handle must not enter Resize (drag should be None)"
        );
        // Move + Up should not push a Transaction.
        c.handle_event(&ViewEvent::PointerMove { x: 120.0, y: 120.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 120.0,
            y: 120.0,
        });
        assert_eq!(
            c.editor.history_len(),
            undo_before,
            "Freehand handle drag must not push a Transaction (no phantom resize)"
        );
    }

    #[test]
    fn annotation_focus_and_interact_fire_on_select() {
        let focus_fired = Arc::new(Mutex::new(false));
        let interact_fired = Arc::new(Mutex::new(false));
        let ff = focus_fired.clone();
        let if_ = interact_fired.clone();
        let mut c = component_with_note();
        // component_with_note leaves the annotation selected (from setup).
        // Clear selection so the first click is a "first select".
        c.editor.clear_selection();
        c.on_annotation_focus(move |_| {
            *ff.lock().unwrap() = true;
        });
        c.on_annotation_interact(move |_| {
            *if_.lock().unwrap() = true;
        });
        // Click center of the annotation to select it (first time -> focus + interact).
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        assert!(*focus_fired.lock().unwrap(), "focus fires on first select");
        assert!(
            *interact_fired.lock().unwrap(),
            "interact fires on pointer down"
        );
        // Reset and click again -- already selected, so focus should NOT fire
        // but interact SHOULD.
        *focus_fired.lock().unwrap() = false;
        *interact_fired.lock().unwrap() = false;
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        assert!(
            !*focus_fired.lock().unwrap(),
            "focus does not fire on re-select"
        );
        assert!(
            *interact_fired.lock().unwrap(),
            "interact fires on every pointer down"
        );
    }

    #[test]
    fn click_without_drag_does_not_set_modified() {
        let mut c = component_with_note();
        // Click center of the annotation and release without moving.
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });
        assert!(
            !c.is_modified(),
            "click without drag should not set modified"
        );
    }

    // --- T4: context_menu / page_change / zoom_change callbacks ---

    #[test]
    fn right_click_fires_context_menu() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_context_menu(move |point, target| {
            *f.lock().unwrap() = Some((point, format!("{target:?}")));
        });
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Right,
            x: 10.0,
            y: 20.0,
            modifiers: Modifiers::default(),
        });
        assert!(fired.lock().unwrap().is_some(), "context_menu fired");
    }

    #[test]
    fn right_click_on_annotation_fires_annotation_target() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = component_with_note();
        c.on_context_menu(move |_point, target| {
            *f.lock().unwrap() = Some(format!("{target:?}"));
        });
        // Click center of the note annotation (rect 0,0,100,100 at zoom=1).
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Right,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        let got = fired.lock().unwrap().clone().expect("context_menu fired");
        assert!(
            got.starts_with("Annotation("),
            "expected ContextTarget::Annotation, got {got}"
        );
    }

    #[test]
    fn right_click_does_not_change_selection() {
        let mut c = component_with_note();
        // Ensure nothing is selected first (component_with_note leaves it
        // selected from setup; clear to make the assertion clear).
        c.editor.clear_selection();
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Right,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::None),
            "right-click must not change selection"
        );
    }

    /// Build a component with two stacked pages so scrolling can move the
    /// visible page from page 0 to page 1. Page physical_box 200x200 mm,
    /// zoom=1, page_gap=0, viewport size 200x200 (exactly one page tall).
    fn component_with_two_pages() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        let mut doc = OfdDocument::default();
        for id in ["P0", "P1"] {
            doc.pages.push(Page {
                id: PageId::new(id),
                physical_box: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 200.0,
                },
                layers: vec![Layer::default()],
                template: None,
            });
        }
        c.load_document(doc);
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 0.0,
        };
        c
    }

    #[test]
    fn scroll_fires_page_change() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = component_with_two_pages();
        c.on_page_change(move |idx| {
            *f.lock().unwrap() = Some(idx);
        });
        // Scroll down by 200px (one full page height) -> page 1 becomes visible.
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        assert_eq!(
            *fired.lock().unwrap(),
            Some(1),
            "page_change fired with idx=1"
        );
    }

    #[test]
    fn page_change_does_not_fire_when_page_unchanged() {
        let fired = Arc::new(Mutex::new(0));
        let f = fired.clone();
        let mut c = component_with_two_pages();
        c.on_page_change(move |_idx| {
            *f.lock().unwrap() += 1;
        });
        // Prime current_page to match the viewport (in real usage a Resize
        // event establishes this before any Scroll). The test sets viewport
        // directly, so also set current_page directly.
        c.current_page = Some(0);
        // Tiny scroll that stays on page 0 (viewport center still within page 0).
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 10.0 });
        assert_eq!(
            *fired.lock().unwrap(),
            0,
            "no page_change when page unchanged"
        );
    }

    /// Regression test for C3 review finding I2: `load_document` and
    /// `new_document` must reset `current_page` to `None`. Otherwise, after a
    /// scroll establishes `current_page = Some(1)` on a multi-page doc, loading
    /// a new (single-page or empty) document leaves a stale index that would
    /// suppress the next legitimate `on_page_change` fire (the new doc's page 0
    /// differs from the stale `Some(1)` but the recomputation must start from a
    /// clean slate, not a stale one).
    #[test]
    fn load_document_resets_current_page() {
        let mut c = component_with_two_pages();
        // Establish a non-None current_page by scrolling to page 1.
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        assert_eq!(c.current_page, Some(1), "scrolled to page 1");

        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        c.on_page_change(move |idx| {
            *f.lock().unwrap() = Some(idx);
        });

        // Load a fresh single-page document.
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
        assert_eq!(
            c.current_page, None,
            "load_document resets current_page to None"
        );

        // A subsequent Scroll that lands on page 0 must fire on_page_change
        // (proving the stale Some(1) did not suppress it).
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 0.0 });
        assert_eq!(
            *fired.lock().unwrap(),
            Some(0),
            "page_change fires for page 0 after load reset current_page"
        );
    }

    #[test]
    fn new_document_resets_current_page() {
        let mut c = component_with_two_pages();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        assert_eq!(c.current_page, Some(1), "scrolled to page 1");
        c.new_document();
        assert_eq!(
            c.current_page, None,
            "new_document resets current_page to None"
        );
    }

    #[test]
    fn zoom_fires_zoom_change() {
        let fired = Arc::new(Mutex::new(None));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_zoom_change(move |z| {
            *f.lock().unwrap() = Some(z);
        });
        c.handle_event(&ViewEvent::Zoom { factor: 2.0 });
        assert_eq!(
            *fired.lock().unwrap(),
            Some(rofd_render::PX_PER_MM * 2.0),
            "zoom_change fired with new zoom value"
        );
    }

    /// Zoom guard: a Zoom event with factor=1.0 (no-op) must NOT fire
    /// on_zoom_change. Only an actual zoom change should fire the callback.
    #[test]
    fn zoom_no_change_does_not_fire_zoom_change() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.on_zoom_change(move |_z| {
            *f.lock().unwrap() = true;
        });
        c.handle_event(&ViewEvent::Zoom { factor: 1.0 });
        assert!(
            !*fired.lock().unwrap(),
            "zoom_change must not fire when factor=1.0 (no zoom change)"
        );
    }

    // --- C4 Task 2: ScrollPage / ZoomAt / Ime ---

    #[test]
    fn scroll_page_moves_by_page_height() {
        let mut c = component_with_note();
        c.viewport.size = (800.0, 600.0);
        let page_h = c.editor.document().pages[0].physical_box.h * c.viewport.zoom;
        let outcome = c.handle_event(&ViewEvent::ScrollPage {
            direction: ScrollDirection::Down,
        });
        assert!(outcome.needs_repaint);
        assert!(
            (c.viewport.scroll.1 - page_h - c.viewport.page_gap).abs() < 0.01,
            "scrolled down one page"
        );
    }

    #[test]
    fn scroll_page_up_moves_negative() {
        let mut c = component_with_note();
        c.viewport.size = (800.0, 600.0);
        // Start with some downward scroll so Up has a visible effect.
        c.viewport.scroll.1 = 500.0;
        let page_h = c.editor.document().pages[0].physical_box.h * c.viewport.zoom;
        let outcome = c.handle_event(&ViewEvent::ScrollPage {
            direction: ScrollDirection::Up,
        });
        assert!(outcome.needs_repaint);
        assert!(
            (c.viewport.scroll.1 - (500.0 - page_h - c.viewport.page_gap)).abs() < 0.01,
            "scrolled up one page from 500"
        );
    }

    #[test]
    fn zoom_at_keeps_center_point_stable() {
        let mut c = component_with_note();
        c.viewport.zoom = 1.0;
        c.viewport.scroll = (100.0, 100.0);
        let center = (400.0, 300.0);
        let outcome = c.handle_event(&ViewEvent::ZoomAt {
            factor: 2.0,
            center,
        });
        assert!(outcome.needs_repaint);
        // zoom doubled
        assert!((c.viewport.zoom - 2.0).abs() < 0.01);
        // center point should map to the same document position:
        // new_scroll = center - (center - old_scroll) * (new_zoom / old_zoom)
        let ratio = 2.0 / 1.0;
        let expected_x = center.0 - (center.0 - 100.0) * ratio;
        let expected_y = center.1 - (center.1 - 100.0) * ratio;
        assert!(
            (c.viewport.scroll.0 - expected_x).abs() < 0.01,
            "scroll.x adjusted for center anchor"
        );
        assert!(
            (c.viewport.scroll.1 - expected_y).abs() < 0.01,
            "scroll.y adjusted for center anchor"
        );
    }

    #[test]
    fn ime_inserts_text_at_cursor() {
        let mut c = component_with_note();
        // Select the note (set cursor at offset 2, end of "hi").
        let id = match c.editor.selection().clone() {
            rofd_editor::AnnotationSelection::Single(id) => id,
            _ => panic!("expected single selection from component_with_note"),
        };
        c.editor.set_cursor(id.clone(), 2);
        let outcome = c.handle_event(&ViewEvent::Ime {
            text: "你好".into(),
        });
        assert!(outcome.needs_repaint);
        let ann = c.editor.document().annotations.find(&id).unwrap();
        match &ann.payload {
            rofd_dom::AnnotationPayload::Note { content, .. } => {
                assert!(
                    content == "hi你好",
                    "note content should be 'hi你好', got '{content}'"
                );
            }
            _ => panic!("expected Note payload"),
        }
        // Cursor should have advanced by the char count of the inserted text.
        let cursor = c.editor.text_cursor().expect("cursor still set");
        assert_eq!(cursor.offset, 4, "cursor advanced by 2 chars (4 = 2 + 2)");
    }

    #[test]
    fn ime_without_cursor_does_nothing() {
        let mut c = component_with_note();
        c.editor.clear_cursor();
        let outcome = c.handle_event(&ViewEvent::Ime {
            text: "你好".into(),
        });
        assert!(
            !outcome.needs_repaint,
            "Ime without cursor should not need repaint"
        );
    }

    // --- C3 Task 7: end-to-end integration smoke test ---

    /// Build a component with one page (P0, 200x200 at origin) and a viewport
    /// where viewport coords == page-local coords (zoom=1, no scroll/gap, size
    /// 0 so page origin is (0,0)). No annotation yet -- the smoke test creates
    /// one via drag. (Mirrors `component_with_note`'s viewport setup.)
    fn component_with_page() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("tester".into(), 1_700_000_000_000);
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
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (0.0, 0.0),
            page_gap: 0.0,
        };
        c
    }

    /// C3 smoke test: chain every interactive annotation operation in one flow.
    ///
    /// create (drag) -> select -> move (drag) -> resize (handle drag) ->
    /// delete (Delete key) -> undo (Ctrl+Z). Each step asserts the annotation's
    /// existence and geometry, proving the Tool/DragState state machine,
    /// hit_test handle routing, preview-based drag commit, and history all
    /// compose correctly end-to-end.
    #[test]
    fn c3_smoke_create_select_move_resize_delete_undo() {
        let mut c = component_with_page();

        // --- Step 1: CREATE via drag (set_tool Create + PointerDown/Move/Up).
        // Drag from (20,20) to (80,80) -> bbox = (20,20,60,60). ---
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 20.0,
            y: 20.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 80.0, y: 80.0 });
        let create_outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 80.0,
            y: 80.0,
        });
        assert!(create_outcome.needs_repaint, "create needs repaint");
        assert!(
            matches!(c.tool, Tool::Select),
            "tool reverts to Select after create"
        );
        let id = match c.editor.selection() {
            AnnotationSelection::Single(id) => id.clone(),
            _ => panic!("expected single selection after create"),
        };
        let ann = c
            .editor
            .document()
            .annotations
            .find(&id)
            .expect("annotation exists after create");
        match &ann.payload {
            AnnotationPayload::Shape { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 20.0,
                        y: 20.0,
                        w: 60.0,
                        h: 60.0
                    },
                    "created rect = bbox((20,20),(80,80))"
                );
            }
            _ => panic!("expected Shape payload"),
        }

        // --- Step 2: SELECT (deselect then re-select via PointerDown on body).
        // Click empty page at (150,150) to deselect, then click body at (50,50). ---
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 150.0,
            y: 150.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 150.0,
            y: 150.0,
        });
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::None),
            "deselected after clicking empty area"
        );
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::Single(_)),
            "re-selected by clicking annotation body"
        );
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
        });

        // --- Step 3: MOVE via drag (PointerDown on body + Move + Up).
        // Click center (50,50), drag to (60,60) -> dx=10, dy=10.
        // Expected rect: (30,30,60,60). ---
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 50.0,
            y: 50.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 60.0, y: 60.0 });
        let move_outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 60.0,
            y: 60.0,
        });
        assert!(move_outcome.needs_repaint, "move needs repaint");
        let ann = c
            .editor
            .document()
            .annotations
            .find(&id)
            .expect("annotation still exists after move");
        match &ann.payload {
            AnnotationPayload::Shape { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 30.0,
                        y: 30.0,
                        w: 60.0,
                        h: 60.0
                    },
                    "rect moved by (10,10) -> (30,30,60,60)"
                );
            }
            _ => panic!("expected Shape payload"),
        }

        // --- Step 4: RESIZE via Se handle drag.
        // After move, rect is (30,30,60,60); Se handle at (90,90).
        // Drag to (100,100) -> new rect = bbox(Nw(30,30),(100,100))
        //                              = (30,30,70,70). ---
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 90.0,
            y: 90.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 100.0, y: 100.0 });
        let resize_outcome = c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 100.0,
            y: 100.0,
        });
        assert!(resize_outcome.needs_repaint, "resize needs repaint");
        let ann = c
            .editor
            .document()
            .annotations
            .find(&id)
            .expect("annotation still exists after resize");
        match &ann.payload {
            AnnotationPayload::Shape { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 30.0,
                        y: 30.0,
                        w: 70.0,
                        h: 70.0
                    },
                    "rect resized via Se handle -> (30,30,70,70)"
                );
            }
            _ => panic!("expected Shape payload"),
        }

        // --- Step 5: DELETE via Delete key (selection is still the annotation). ---
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::Single(_)),
            "annotation selected before delete"
        );
        let delete_outcome = c.handle_event(&ViewEvent::KeyDown {
            key: Key::Delete,
            modifiers: Modifiers::default(),
        });
        assert!(delete_outcome.needs_repaint, "delete needs repaint");
        assert!(
            c.editor.document().annotations.find(&id).is_none(),
            "annotation gone after delete"
        );
        assert!(
            matches!(c.editor.selection(), AnnotationSelection::None),
            "selection cleared after delete"
        );

        // --- Step 6: UNDO via Ctrl+Z -> annotation restored. ---
        let undo_outcome = c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('z'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        assert!(undo_outcome.needs_repaint, "undo needs repaint");
        let ann = c
            .editor
            .document()
            .annotations
            .find(&id)
            .expect("annotation restored after undo of delete");
        match &ann.payload {
            AnnotationPayload::Shape { rect, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 30.0,
                        y: 30.0,
                        w: 70.0,
                        h: 70.0
                    },
                    "undo restores the annotation with its pre-delete rect"
                );
            }
            _ => panic!("expected Shape payload"),
        }
    }

    /// After the full smoke flow (create + delete + undo), rendering must not
    /// panic and must produce exactly one scene (no stale drag state, no crash
    /// on the restored annotation).
    #[test]
    fn c3_smoke_render_after_full_flow() {
        let mut c = component_with_page();
        c.set_tool(Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x: 20.0,
            y: 20.0,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::PointerMove { x: 80.0, y: 80.0 });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x: 80.0,
            y: 80.0,
        });
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Delete,
            modifiers: Modifiers::default(),
        });
        c.handle_event(&ViewEvent::KeyDown {
            key: Key::Char('z'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        });
        let mut rt = MockRenderTarget {
            drawn: 0,
            w: 200.0,
            h: 200.0,
        };
        c.render(&mut rt);
        assert_eq!(rt.drawn, 1, "render drew exactly one scene");
    }
}
