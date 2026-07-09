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
            viewport: Viewport { page_gap, ..Default::default() },
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

    pub fn handle_event(&mut self, _event: &crate::event::ViewEvent) -> EventOutcome {
        // Task 6+ fills this in.
        EventOutcome { needs_repaint: false }
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
}
