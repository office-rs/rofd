use rofd_dom::OfdDocument;
use rofd_editor::{Editor, AnnotationSelection, TextCursor};
use rofd_render::{RenderEngine, PageSceneCache, Viewport};

use crate::callbacks::Callbacks;
use crate::config::EditorConfig;
use crate::event::EventOutcome;
use crate::render_target::RenderTarget;

pub struct EditorComponent {
    pub(crate) editor: Editor,
    pub(crate) render: RenderEngine,
    pub(crate) cache: PageSceneCache,
    pub(crate) viewport: Viewport,
    pub(crate) callbacks: Callbacks,
    pub(crate) modified: bool,
}

impl EditorComponent {
    pub fn new(config: EditorConfig) -> Self {
        let page_gap = config.page_gap;
        Self {
            editor: Editor::new(),
            render: RenderEngine::new(config.default_font_bytes.clone()),
            cache: PageSceneCache::new(),
            viewport: Viewport { zoom: 1.0, page_gap, ..Default::default() },
            callbacks: Callbacks::default(),
            modified: false,
        }
    }

    pub fn load_document(&mut self, doc: OfdDocument) {
        self.editor.load_document(doc);
        self.cache = PageSceneCache::new();
        self.modified = false;
    }

    pub fn new_document(&mut self) {
        self.editor.load_document(OfdDocument::default());
        self.cache = PageSceneCache::new();
        self.modified = false;
    }

    pub fn document(&self) -> &OfdDocument { self.editor.document() }
    pub fn selection(&self) -> &AnnotationSelection { self.editor.selection() }
    pub fn text_cursor(&self) -> Option<&TextCursor> { self.editor.text_cursor() }
    pub fn can_undo(&self) -> bool { self.editor.can_undo() }
    pub fn can_redo(&self) -> bool { self.editor.can_redo() }
    pub fn is_modified(&self) -> bool { self.modified }
    pub fn set_clock(&mut self, author: String, ts: i64) { self.editor.set_clock(author, ts); }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        let scene = self.render.composite(self.editor.document(), &self.viewport, &mut self.cache);
        target.draw_scene(&scene);
    }

    pub fn handle_event(&mut self, event: &crate::event::ViewEvent) -> EventOutcome {
        use crate::event::ViewEvent;
        match event {
            ViewEvent::PointerDown { button: crate::event::MouseButton::Left, x, y, .. } => {
                let target = rofd_render::hit_test(self.editor.document(), &self.viewport, (*x, *y));
                match target {
                    rofd_render::HitTarget::Annotation(id) => {
                        self.editor.select(id.clone());
                        if let Some(len) = self.text_content_len(&id) {
                            self.editor.set_cursor(id.clone(), len);
                        }
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome { needs_repaint: true }
                    }
                    _ => {
                        self.editor.clear_selection();
                        self.editor.clear_cursor();
                        self.fire_selection_change();
                        self.fire_cursor_change();
                        EventOutcome { needs_repaint: true }
                    }
                }
            }
            ViewEvent::Scroll { dx, dy } => {
                self.viewport.scroll.0 += dx; self.viewport.scroll.1 += dy;
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::Zoom { factor } => {
                self.viewport.zoom *= factor;
                EventOutcome { needs_repaint: true }
            }
            ViewEvent::Resize { width, height } => {
                self.viewport.size = (*width, *height);
                EventOutcome { needs_repaint: true }
            }
            _ => EventOutcome { needs_repaint: false },
        }
    }

    fn text_content_len(&self, id: &rofd_dom::AnnotationId) -> Option<usize> {
        let ann = self.editor.document().annotations.find(id)?;
        use rofd_dom::AnnotationPayload;
        match &ann.payload {
            AnnotationPayload::Note { content, .. } | AnnotationPayload::TextBox { content, .. } | AnnotationPayload::Watermark { content, .. } => Some(content.chars().count()),
            _ => None,
        }
    }

    // Called by later tasks (annotation create/delete/move/resize, save shortcut).
    #[allow(dead_code)]
    fn after_annotation_change(&mut self) {
        self.modified = true;
        let pages: Vec<rofd_dom::PageId> = self.editor.document().pages.iter().map(|p| p.id.clone()).collect();
        for pid in &pages { self.cache.invalidate(pid); }
        if let Some(cb) = &self.callbacks.on_change { cb(self.editor.document()); }
    }

    fn fire_selection_change(&self) {
        if let Some(cb) = &self.callbacks.on_selection_change { cb(self.editor.selection()); }
    }

    fn fire_cursor_change(&self) {
        if let Some(cb) = &self.callbacks.on_cursor_change { cb(self.editor.text_cursor()); }
    }

    // Called by later tasks (Ctrl+S / save shortcut).
    #[allow(dead_code)]
    fn fire_save_request(&self) {
        if let Some(cb) = &self.callbacks.on_save_request { cb(); }
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
    use std::sync::Arc;
    use vello::Scene;

    struct MockRenderTarget { drawn: usize, w: f64, h: f64 }
    impl RenderTarget for MockRenderTarget {
        fn draw_scene(&mut self, _: &Scene) { self.drawn += 1; }
        fn size(&self) -> (f64, f64) { (self.w, self.h) }
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
        let mut rt = MockRenderTarget { drawn: 0, w: 800.0, h: 600.0 };
        c.render(&mut rt);
        assert_eq!(rt.drawn, 1);
    }

    use crate::event::ViewEvent;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
    use std::sync::Mutex;

    fn component_with_note() -> EditorComponent {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        c.editor.create_annotation(
            AnnotationKind::Note, PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 },
                color: Color::Rgb(0, 0, 0), content: "hi".into(), icon: NoteIcon::Note,
            },
        );
        // The create_annotation above bypasses handle_event (direct editor call for test setup).
        // Invalidate cache to stay consistent.
        for p in &c.editor.document().pages.clone() { c.cache.invalidate(&p.id); }
        c.viewport = rofd_render::Viewport { scroll: (0.0, 0.0), zoom: 1.0, size: (800.0, 600.0), page_gap: 20.0 };
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
        assert_eq!(c.viewport.zoom, 2.0);
    }

    #[test]
    fn resize_updates_viewport() {
        let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = c.handle_event(&ViewEvent::Resize { width: 1024.0, height: 768.0 });
        assert!(outcome.needs_repaint);
        assert_eq!(c.viewport.size, (1024.0, 768.0));
    }

    #[test]
    fn on_change_fires_after_scroll_callback_set() {
        let fired = Arc::new(Mutex::new(false));
        let f = fired.clone();
        let mut c = component_with_note();
        c.on_change(move |_| { *f.lock().unwrap() = true; });
        // Scroll doesn't change annotations -> no on_change. But it does need_repaint.
        c.handle_event(&ViewEvent::Scroll { dx: 1.0, dy: 0.0 });
        assert!(!*fired.lock().unwrap(), "scroll does not fire on_change");
    }
}
