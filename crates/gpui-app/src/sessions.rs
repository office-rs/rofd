//! Multi-document session management (web-app/rword sessionManager parity,
//! spec 2026-10-31 §7).
//!
//! One `Entity<OfdGpuiView>` per open document; the shell renders the
//! ACTIVE entity. Switching tabs swaps which entity fills the editor slot
//! (each keeps its own scroll/zoom/cursor — gpui entities retain state).
//! The `PackageHandle` for surgical save lives HERE, per session (io stays
//! host-side; the handle only ever comes from parsing a file).

use std::path::PathBuf;

use gpui::Entity;
use rofd_component::{Tool, ZoomMode};
use rofd_dom::{OfdWarning, ShapeKind};
use rofd_gpui_view::OfdGpuiView;
use rofd_io::PackageHandle;

/// Initial viewport zoom mirror (px per mm) — the component's initial zoom
/// (same value rofd-render defines as PX_PER_MM; gpui-app does not depend
/// on rofd-render, xilem-app keeps an identical local constant).
pub const INITIAL_ZOOM: f64 = 96.0 / 25.4;

/// One open document: identity + save state + the live editor entity, plus
/// the event mirrors the toolbar/status render for this session. Mirrors are
/// owned per session (events from ANY session update its own row), so the
/// shell can always render `sessions[active]` without cross-session leaks.
pub struct DocumentSession {
    /// Display name (tab label, without extension).
    pub file_name: String,
    /// Path for save-in-place; None = never saved (blank document).
    pub path: Option<PathBuf>,
    /// Package skeleton for surgical save; None until a file was parsed
    /// (Save-As never mints one — xilem-app precedent, spec §4.4).
    pub package: Option<PackageHandle>,
    /// Unsaved-changes mirror (on_change sets it; a successful save clears).
    pub modified: bool,
    /// True for a never-edited, never-loaded blank document.
    pub pristine: bool,
    pub editor: Entity<OfdGpuiView>,
    // -- Event mirrors (this session's rows; the shell renders the active) --
    /// 1-based visible page (on_page_change maintains it).
    pub page: usize,
    /// Viewport zoom in px/mm (on_zoom_change maintains it).
    pub zoom: f64,
    /// Active zoom mode (on_zoom_mode_change maintains it).
    pub zoom_mode: ZoomMode,
    /// Body-text selection present (gates the markup buttons).
    pub has_text_selection: bool,
    /// Warnings from the load / runtime warning callback.
    pub warnings: Vec<OfdWarning>,
    /// Active tool mirror for this session (tools only change through the
    /// host toolbar, so the shell copy is authoritative; the toolbar
    /// renders the active session's row without a kernel round-trip).
    pub tool: Tool,
}

impl DocumentSession {
    fn new_blank(name: String, editor: Entity<OfdGpuiView>) -> Self {
        Self {
            file_name: name,
            path: None,
            package: None,
            modified: false,
            pristine: true,
            editor,
            page: 1,
            zoom: INITIAL_ZOOM,
            zoom_mode: ZoomMode::Custom,
            has_text_selection: false,
            warnings: Vec::new(),
            tool: Tool::Text,
        }
    }
}

/// App-level multi-session state.
#[derive(Default)]
pub struct SessionManager {
    pub sessions: Vec<DocumentSession>,
    pub active: usize,
    /// Blank-name counter (批注文稿N source).
    pub blank_counter: usize,
}

impl SessionManager {
    /// Open a blank document, activate it, return its index.
    pub fn new_blank(&mut self, editor: Entity<OfdGpuiView>) -> usize {
        self.blank_counter += 1;
        let name = format!("批注文稿{}", self.blank_counter);
        self.sessions.push(DocumentSession::new_blank(name, editor));
        let idx = self.sessions.len() - 1;
        self.active = idx;
        idx
    }

    /// Insert a session for an opened file, activate, return its index.
    /// The display name drops the extension ("doc.ofd" -> "doc").
    pub fn open_file(
        &mut self,
        name: String,
        path: PathBuf,
        package: Option<PackageHandle>,
        editor: Entity<OfdGpuiView>,
    ) -> usize {
        let name = name
            .rsplit_once('.')
            .map(|(base, _)| base.to_string())
            .unwrap_or(name);
        self.sessions.push(DocumentSession {
            file_name: name,
            path: Some(path),
            package,
            modified: false,
            pristine: false,
            editor,
            page: 1,
            zoom: INITIAL_ZOOM,
            zoom_mode: ZoomMode::Custom,
            has_text_selection: false,
            warnings: Vec::new(),
            tool: Tool::Text,
        });
        let idx = self.sessions.len() - 1;
        self.active = idx;
        idx
    }

    /// Close a session by index; adjust `active` so it still points at the
    /// same surviving session when possible. Returns true if the list is
    /// now empty (the shell auto-opens a blank).
    pub fn close(&mut self, idx: usize) -> bool {
        if idx >= self.sessions.len() {
            return false;
        }
        self.sessions.remove(idx);
        if self.sessions.is_empty() {
            self.active = 0;
        } else if idx < self.active {
            // A earlier tab closed: everything after shifts left, so the
            // active SURVIVOR now sits one index lower.
            self.active -= 1;
        } else if self.active >= self.sessions.len() {
            self.active = self.sessions.len() - 1;
        }
        self.sessions.is_empty()
    }

    pub fn active(&self) -> Option<&DocumentSession> {
        self.sessions.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut DocumentSession> {
        self.sessions.get_mut(self.active)
    }

    pub fn active_editor(&self) -> Option<Entity<OfdGpuiView>> {
        self.sessions.get(self.active).map(|s| s.editor.clone())
    }

    /// Index of the session owning `editor` (subscribe routing).
    pub fn index_of(&self, editor: &Entity<OfdGpuiView>) -> Option<usize> {
        self.sessions.iter().position(|s| &s.editor == editor)
    }
}

/// App-level actions dispatched from Popover content (whose cx belongs to
/// PopoverState, not AppShell). The shell registers a handler that consumes
/// and clears the queue on each render pass.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    // File menu (打开 / 保存 / 另存为 / 导出为 PDF; 打印… stays disabled
    // until Task 13 wires the print dialog).
    OpenFile,
    SaveFile,
    SaveAsFile,
    ExportPdf,
    // Shape dropdown (图形批注子工具).
    PickShape(ShapeKind),
    // Zoom dropdown (fit modes + percent presets).
    PickZoomMode(ZoomMode),
    PickZoomPercent(f64),
    // Right-click context menu on an annotation (single item 删除批注;
    // the target id lives in the shell's context-menu state).
    ContextDeleteAnnotation,
    // Close-confirm popover (保存 / 不保存 / 取消).
    CloseConfirmSave,
    CloseConfirmDiscard,
    CloseConfirmCancel,
    // Dismiss the annotation context menu (popover closed via its overlay).
    ContextMenuDismiss,
}

/// Popovers can't reach the shell entity — events hop through this Global.
#[derive(Default)]
pub struct AppActionBridge {
    pending: Vec<BridgeEvent>,
}

impl gpui::Global for AppActionBridge {}

/// Bridge event: either a plain action or a shell-mutation closure
/// (used for controlled popover open/close from picker items).
pub enum BridgeEvent {
    Action(AppAction),
    SetOpen(Box<dyn Fn(&mut crate::shell::AppShell)>),
}

impl AppActionBridge {
    pub fn push(action: AppAction, cx: &mut gpui::App) {
        Self::push_raw(BridgeEvent::Action(action), cx);
    }

    pub fn push_raw(event: BridgeEvent, cx: &mut gpui::App) {
        Self::ensure(cx);
        cx.global_mut::<AppActionBridge>().pending.push(event);
    }

    /// Install the Global if absent (idempotent; called before first use).
    pub fn ensure(cx: &mut gpui::App) {
        if !cx.has_global::<AppActionBridge>() {
            cx.set_global(AppActionBridge::default());
        }
    }

    pub fn drain(cx: &mut gpui::App) -> Vec<BridgeEvent> {
        std::mem::take(&mut cx.global_mut::<AppActionBridge>().pending)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use gpui::{AppContext, TestAppContext};
    use rofd_component::OfdConfig;

    /// Blank editor entity (empty font config; the tests never paint).
    fn editor(cx: &mut TestAppContext) -> Entity<OfdGpuiView> {
        cx.update(|cx| cx.new(|cx| OfdGpuiView::new(OfdConfig::new(Arc::new(vec![])), cx)))
    }

    #[gpui::test]
    fn new_blank_names_and_activates(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        let e1 = editor(cx);
        let i1 = mgr.new_blank(e1);
        assert_eq!(i1, 0);
        assert_eq!(mgr.active, 0);
        let e2 = editor(cx);
        let i2 = mgr.new_blank(e2);
        assert_eq!(i2, 1);
        assert_eq!(mgr.active, 1);
        assert_eq!(mgr.sessions[0].file_name, "批注文稿1");
        assert_eq!(mgr.sessions[1].file_name, "批注文稿2");
        assert!(mgr.sessions.iter().all(|s| s.pristine));
        assert!(mgr.sessions.iter().all(|s| s.path.is_none()));
        assert!(mgr.sessions.iter().all(|s| s.package.is_none()));
    }

    #[gpui::test]
    fn open_file_strips_extension_stores_and_activates(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        let e = editor(cx);
        mgr.new_blank(editor(cx));
        let idx = mgr.open_file(
            "ru-yuan-ji-lu.ofd".into(),
            PathBuf::from(r"D:\tmp\ru-yuan-ji-lu.ofd"),
            None,
            e.clone(),
        );
        assert_eq!(idx, 1);
        assert_eq!(mgr.active, 1);
        let s = mgr.active().unwrap();
        assert_eq!(s.file_name, "ru-yuan-ji-lu");
        assert_eq!(s.path, Some(PathBuf::from(r"D:\tmp\ru-yuan-ji-lu.ofd")));
        assert!(s.package.is_none(), "test passes None through unchanged");
        assert!(!s.pristine);
        assert!(!s.modified);
        // The editor of the active session is the one just opened.
        assert_eq!(mgr.active_editor().as_ref(), Some(&e));
        assert_eq!(mgr.index_of(&e), Some(1));
    }

    #[gpui::test]
    fn open_file_keeps_extensionless_name(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        let e = editor(cx);
        mgr.open_file("untitled".into(), PathBuf::from("/tmp/untitled"), None, e);
        assert_eq!(mgr.sessions[0].file_name, "untitled");
    }

    #[gpui::test]
    fn close_clamps_active_and_reports_empty(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        mgr.new_blank(editor(cx));
        mgr.new_blank(editor(cx));
        mgr.new_blank(editor(cx));
        assert_eq!(mgr.active, 2);
        // Close the active tail: active clamps to the new last index.
        assert!(!mgr.close(2));
        assert_eq!(mgr.active, 1);
        // Close the head: active shifts down, still the same document.
        assert!(!mgr.close(0));
        assert_eq!(mgr.active, 0);
        assert_eq!(mgr.sessions.len(), 1);
        // Last close reports empty; active stays in range.
        assert!(mgr.close(0));
        assert_eq!(mgr.active, 0);
        assert!(mgr.active().is_none());
        assert!(mgr.active_mut().is_none());
        assert!(mgr.active_editor().is_none());
    }

    #[gpui::test]
    fn close_before_active_keeps_pointing_at_same_session(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        mgr.new_blank(editor(cx));
        mgr.new_blank(editor(cx));
        let e3 = editor(cx);
        mgr.new_blank(e3.clone());
        // Active is session 2 ("批注文稿3"); closing session 0 must leave
        // active pointing at the SAME editor, i.e. index 1.
        assert!(!mgr.close(0));
        assert_eq!(mgr.active, 1);
        assert_eq!(mgr.active_editor().as_ref(), Some(&e3));
    }

    #[gpui::test]
    fn close_out_of_range_is_a_no_op(cx: &mut TestAppContext) {
        let mut mgr = SessionManager::default();
        mgr.new_blank(editor(cx));
        assert!(!mgr.close(5));
        assert_eq!(mgr.sessions.len(), 1);
    }
}
