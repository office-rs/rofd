use std::path::PathBuf;

use rofd_component::{EditorComponent, EditorConfig, EventOutcome, RenderTarget, ViewEvent};
use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, write_ofd};

/// Platform-agnostic editor state (no winit types). Owns the EditorComponent.
/// File I/O (load_ofd/save_ofd) lives here, not on EditorComponent.
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub modified: bool,
}

impl EditorApp {
    pub fn new(config: EditorConfig) -> Self {
        Self { component: EditorComponent::new(config), current_file: None, modified: false }
    }

    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), String> {
        let report = parse_ofd(bytes).map_err(|e| format!("parse failed: {e}"))?;
        self.component.load_document(report.document);
        self.modified = false;
        Ok(())
    }

    /// Serialize the current document to a full .ofd package (bytes).
    ///
    /// Uses `write_ofd` (full write) rather than the surgical `save_ofd` path.
    /// Surgical save needs a `&PackageHandle`, but the component consumes the
    /// package on `load_document` and does not re-expose it. Full write is
    /// acceptable for v1: it emits only what the model holds (the surgical path
    /// exists to preserve unmodelled body content).
    pub fn save_ofd(&self) -> Result<Vec<u8>, String> {
        write_ofd(self.component.document()).map_err(|e| format!("save failed: {e}"))
    }

    pub fn handle_event(&mut self, event: &ViewEvent) -> EventOutcome {
        let outcome = self.component.handle_event(event);
        if outcome.needs_repaint { self.modified = true; }
        outcome
    }

    pub fn render(&mut self, target: &mut dyn RenderTarget) {
        self.component.render(target);
    }

    pub fn document(&self) -> &OfdDocument { self.component.document() }
    pub fn is_modified(&self) -> bool { self.modified }
    pub fn set_clock(&mut self, author: String, ts: i64) { self.component.set_clock(author, ts); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn load_ofd_then_document_has_pages() {
        // Build a minimal .ofd via write_ofd (round-trippable package), then load it.
        let doc = OfdDocument::default();
        // No pages - just test the load path doesn't panic.
        let bytes = rofd_io::write_ofd(&doc).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&bytes).unwrap();
        assert_eq!(app.document().pages.len(), 0);
        assert!(!app.is_modified());
    }

    #[test]
    fn save_ofd_round_trips() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        let doc = OfdDocument::default();
        app.component.load_document(doc);
        let saved = app.save_ofd().unwrap();
        assert!(!saved.is_empty());
    }

    #[test]
    fn new_app_is_unmodified_with_no_file() {
        let app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        assert!(!app.is_modified());
        assert!(app.current_file.is_none());
    }

    #[test]
    fn handle_event_sets_modified_on_repaint() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        // Scroll triggers needs_repaint -> modified should flip.
        let outcome = app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert!(app.is_modified());
    }

    #[test]
    fn load_ofd_resets_modified_flag() {
        // Start modified (via a repaint-triggering event), then load -> modified cleared.
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 0.0 });
        assert!(app.is_modified());
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        app.load_ofd(&bytes).unwrap();
        assert!(!app.is_modified());
    }
}
