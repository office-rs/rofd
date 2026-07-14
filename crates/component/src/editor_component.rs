use std::sync::Arc;

use rofd_dom::{AnnotationSelection, OfdDocument};
use rofd_editor::{Editor, TextCursor};
use rofd_render::{FontStore, RenderEngine, Scene, Viewport, PX_PER_MM};

use crate::callbacks::Callbacks;
use crate::config::EditorConfig;
use crate::event::EventOutcome;
use crate::render_target::RenderTarget;

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
    }

    pub fn new_document(&mut self) {
        self.editor.load_document(OfdDocument::default());
        self.font_store = Some(self.build_font_store());
        self.modified = false;
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
        self.render.composite(
            self.editor.document(),
            &self.viewport,
            fonts,
            &AnnotationSelection::None,
            None,
        )
    }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        let scene = self.build_scene();
        target.draw_scene(&scene);
    }

    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::ViewEvent;
        match event {
            ViewEvent::PointerDown {
                button: crate::event::MouseButton::Left,
                x,
                y,
                ..
            } => {
                let target = rofd_render::hit_test(
                    self.editor.document(),
                    &self.viewport,
                    &AnnotationSelection::None,
                    (*x, *y),
                );
                match target {
                    rofd_render::HitTarget::Annotation(id) => {
                        self.editor.select(id.clone());
                        if let Some(len) = self.text_content_len(&id) {
                            self.editor.set_cursor(id.clone(), len);
                        }
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome {
                            needs_repaint: true,
                        }
                    }
                    _ => {
                        self.editor.clear_selection();
                        self.editor.clear_cursor();
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome {
                            needs_repaint: true,
                        }
                    }
                }
            }
            ViewEvent::Scroll { dx, dy } => {
                self.viewport.scroll.0 += dx;
                self.viewport.scroll.1 += dy;
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::Zoom { factor } => {
                self.viewport.zoom *= factor;
                EventOutcome {
                    needs_repaint: true,
                }
            }
            ViewEvent::Resize { width, height } => {
                self.viewport.size = (*width, *height);
                EventOutcome {
                    needs_repaint: true,
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

    use crate::event::{Key, Modifiers, ViewEvent};
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
    use std::sync::Mutex;

    fn component_with_note() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
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
            size: (800.0, 600.0),
            page_gap: 20.0,
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
}
