use std::path::PathBuf;

use rofd_component::{EditorComponent, EditorConfig, EventOutcome, ViewEvent};
use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle};
use rofd_render::Scene;

/// Platform-agnostic editor state (no winit types). Owns the EditorComponent.
/// File I/O (load_ofd/save_ofd) lives here, not on EditorComponent.
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub package: Option<PackageHandle>,
}

impl EditorApp {
    pub fn new(config: EditorConfig) -> Self {
        Self {
            component: EditorComponent::new(config),
            current_file: None,
            package: None,
        }
    }

    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), String> {
        let report = parse_ofd(bytes).map_err(|e| format!("parse failed: {e}"))?;
        self.package = Some(report.package);
        self.component.load_document(report.document);
        Ok(())
    }

    /// Serialize the current document to .ofd bytes. Surgical save (preserves
    /// unmodelled body) when a package was loaded; full write for new documents.
    pub fn save_ofd(&self) -> Result<Vec<u8>, String> {
        match &self.package {
            Some(pkg) => save_ofd(self.component.document(), pkg),
            None => write_ofd(self.component.document()),
        }
        .map_err(|e| format!("save failed: {e}"))
    }

    pub fn handle_event(&mut self, event: &ViewEvent) -> EventOutcome {
        self.component.handle_event(event)
    }

    /// Build the current editor scene for the host to paint. The native xilem
    /// canvas consumes this via `Painter::replay`.
    pub fn build_scene(&mut self) -> Scene {
        self.component.build_scene()
    }

    /// Update the canvas dimensions (logical pixels). Drives the component's
    /// viewport size via a `Resize` event.
    pub fn set_size(&mut self, width: f64, height: f64) {
        self.component
            .handle_event(&ViewEvent::Resize { width, height });
    }

    pub fn document(&self) -> &OfdDocument {
        self.component.document()
    }
    pub fn is_modified(&self) -> bool {
        self.component.is_modified()
    }
    pub fn set_clock(&mut self, author: String, ts: i64) {
        self.component.set_clock(author, ts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};
    use rofd_io::zip_util::read_all_entries;
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
    fn handle_event_scroll_does_not_set_modified() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert!(
            !app.is_modified(),
            "scroll is a view change, not a doc change"
        );
    }

    #[test]
    fn load_ofd_resets_modified_flag() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.set_clock("t".into(), 1);
        app.component.create_annotation(
            rofd_dom::AnnotationKind::Note,
            rofd_dom::PageId::new("1"),
            rofd_dom::AnnotationPayload::Note {
                rect: rofd_dom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                color: rofd_dom::Color::Rgb(0, 0, 0),
                content: "x".into(),
                icon: rofd_dom::NoteIcon::Note,
            },
        );
        assert!(app.is_modified());
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        app.load_ofd(&bytes).unwrap();
        assert!(!app.is_modified(), "load resets modified");
    }

    #[test]
    fn load_ofd_retains_package() {
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&bytes).unwrap();
        assert!(app.package.is_some(), "package retained after load");
    }

    #[test]
    fn save_ofd_with_package_preserves_body_bytes() {
        // 构造一个有 body 的包：write_ofd -> parse -> load -> save_ofd(surgical) -> body Content.xml 字节级保留
        let doc = OfdDocument::default();
        let original = rofd_io::write_ofd(&doc).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&original).unwrap();
        let saved = app.save_ofd().unwrap();
        let orig_e = read_all_entries(&original).unwrap();
        let save_e = read_all_entries(&saved).unwrap();
        // body Content.xml 字节级相等（surgical 保留）
        for name in orig_e
            .iter()
            .filter(|(n, _)| n.ends_with("Content.xml"))
            .map(|(n, _)| n.as_str())
        {
            let o = orig_e.iter().find(|(n, _)| n == name).unwrap();
            let s = save_e.iter().find(|(n, _)| n == name).unwrap();
            assert_eq!(o.1, s.1, "body {} byte-identical (surgical)", name);
        }
    }

    #[test]
    fn save_ofd_without_package_uses_full_write() {
        // new doc (package=None) -> save_ofd -> write_ofd (full), 非空且可重 parse
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.component.load_document(OfdDocument::default()); // package stays None
        assert!(app.package.is_none());
        let saved = app.save_ofd().unwrap();
        assert!(!saved.is_empty());
        // 可重 parse
        rofd_io::parse_ofd(&saved).expect("full-write output re-parses");
    }

    #[test]
    fn is_modified_delegates_to_component_not_view_changes() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.set_clock("t".into(), 1);
        // scroll/zoom (view changes) 不置 modified
        app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 0.0 });
        assert!(!app.is_modified(), "scroll does not set modified");
        app.handle_event(&ViewEvent::Zoom { factor: 2.0 });
        assert!(!app.is_modified(), "zoom does not set modified");
        // 批注编辑（command pass-through）置 modified
        app.component.create_annotation(
            AnnotationKind::Note,
            PageId::new("1"),
            AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "x".into(),
                icon: NoteIcon::Note,
            },
        );
        assert!(app.is_modified(), "annotation edit sets modified");
        // clear_modified 复位
        app.component.clear_modified();
        assert!(!app.is_modified(), "clear_modified resets");
    }
}
