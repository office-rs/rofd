//! gpui-app shell: session tab strip + toolbar + editor + status bar
//! (gpui plan Task 10, spec 2026-10-31 §6). Subscribes to every session's
//! [`OfdGpuiEvent`] and mirrors the state the chrome renders per session;
//! toolbar drives [`GpuiOfdHandle`] verbs. All dropdowns are kit Popovers
//! with controlled open state routed through the [`AppActionBridge`]
//! Global (popover content contexts cannot reach this entity); group
//! builders return `Vec<AnyElement>` + `.children()` instead of chained
//! `.child()` calls (spec pitfall 1/3).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    div, prelude::*, px, rgb, Anchor, AnyElement, App, ClickEvent, Context, CursorStyle, ElementId,
    Entity, PathPromptOptions, Render, Window,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, Sizable};
use office_rs_print::JobState;
use rofd_component::{
    ContextTarget, CreateKind, OfdConfig, PointerCursor, PrintContent, ScrollDirection, Tool,
    ZoomMode,
};
use rofd_dom::{AnnotationId, AnnotationKind, Color as OfdColor, ShapeKind};
use rofd_gpui_view::print_dialog_state::{DiscoverState, PrintServices};
use rofd_gpui_view::print_services::NativePrintServices;
use rofd_gpui_view::{GpuiOfdHandle, OfdGpuiEvent, OfdGpuiView};

use crate::host;
use crate::print_dialog::{print_panel_overlay, PrintPanelState, SubmitBlock};
use crate::sessions::{
    AppAction, AppActionBridge, BridgeEvent, DocumentSession, SessionManager, INITIAL_ZOOM,
};

// Exit-fullscreen action (Esc). Registered as a key binding in main.
gpui_kit::actions!([ExitFullscreen]);

/// Auto-page interval (web parity: 3 seconds per page).
const AUTO_PAGE_INTERVAL: Duration = Duration::from_secs(3);

/// Print job terminal-state polling interval (drain step ① driver).
const PRINT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Bounded wait for the discovery terminal message at dialog open (xilem
/// `do_print` precedent: the worker thread races the open).
const DISCOVER_TIMEOUT: Duration = Duration::from_millis(500);

/// Zoom percent presets (web ZoomMenu parity).
const ZOOM_PRESETS: &[f64] = &[10., 25., 50., 75., 100., 125., 150., 200., 300., 400., 500.];

/// Right-click annotation menu state: the anchor is in WINDOW
/// coordinates (kernel viewport point + element origin), ready for a
/// popover anchor.
struct ContextMenuState {
    x: f64,
    y: f64,
    id: AnnotationId,
}

pub struct AppShell {
    /// Multi-document sessions; the shell renders sessions[active]'s
    /// editor entity (spec 2026-10-31 §7).
    pub sessions: SessionManager,
    /// One-line host status (operation outcomes).
    pub status: String,
    /// Config stamped onto every new session's editor (startup default
    /// font). Document-declared families are fed per session by the
    /// system font channel (`font_index`), not through this config.
    font_config: OfdConfig,
    /// Controlled open state for the toolbar Popovers.
    file_menu_open: bool,
    shape_menu_open: bool,
    zoom_menu_open: bool,
    /// Right-click annotation menu (R7: only annotation hits ever set it).
    context_menu: Option<ContextMenuState>,
    /// Close-confirm popover pending on this session index (modified
    /// sessions only; None = no confirm open).
    confirm_close: Option<usize>,
    /// Highlight color mirror — starts at the kernel's default yellow so
    /// new sessions and the picker agree without a round-trip.
    highlight_color: OfdColor,
    /// Kit color picker state entity (highlight color; applies on
    /// selection, web R4).
    color_picker: Entity<ColorPickerState>,
    /// 自动翻页 toggle + generation token: every toggle or session change
    /// bumps the generation; timer loops exit when their token goes stale
    /// (a stale loop must never page a switched-to document).
    auto_paging: bool,
    auto_page_gen: u64,
    /// Pointer-cursor mirror (PointerCursorChanged events of the ACTIVE
    /// session; applied to the editor wrapper's hover cursor).
    pointer_cursor: PointerCursor,
    /// Editor that should take keyboard focus at the next render. Set from
    /// paths that have no `&mut Window` (async open/close completion);
    /// consumed at the top of Render, where a window is available —
    /// otherwise keyboard focus dies with a closed editor until a click.
    pending_focus: Option<Entity<OfdGpuiView>>,
    /// Open print panel (Task 13); None = no dialog. Bound to the session
    /// active at open — a tab switch closes it (modal reset).
    pub(crate) print_panel: Option<PrintPanelState>,
    /// Shared spec-input state for the print panel's Custom range field
    /// (one entity, reused across dialogs; its Change subscription
    /// mirrors the value into the open panel's settings).
    print_spec_input: Entity<InputState>,
}

impl AppShell {
    /// Build the shell with its first session: the CLI path argument when
    /// it loads, otherwise a blank document.
    pub fn new(
        font_config: OfdConfig,
        cli_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        AppActionBridge::ensure(cx);
        let color_picker = {
            let state = cx.new(|cx| ColorPickerState::new(window, cx));
            cx.subscribe_in(
                &state,
                window,
                |this, _picker, event: &ColorPickerEvent, _window, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        let (r, g, b) = hsla_to_rgb(*color);
                        this.highlight_color = OfdColor::Rgb(r, g, b);
                        let picked = this.highlight_color;
                        this.handle().set_highlight_color(picked, cx);
                        // Applying on selection (web R4): with a body-text
                        // selection, picking the color commits a highlight
                        // immediately.
                        let has_selection = this
                            .sessions
                            .active()
                            .map(|s| s.has_text_selection)
                            .unwrap_or(false);
                        if has_selection {
                            this.handle().apply_markup(AnnotationKind::Highlight, cx);
                        }
                    }
                    cx.notify();
                },
            )
            .detach();
            state
        };
        let print_spec_input = {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("如 1-3,5"));
            cx.subscribe_in(
                &state,
                window,
                |this, input, event: &InputEvent, _window, cx| {
                    // Mirror the Custom-range spec edits into the open
                    // panel's settings (with revalidation through the
                    // dialog machine).
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        if let Some(panel) = this.print_panel.as_mut() {
                            if matches!(
                                panel.settings.range,
                                office_rs_print::PageRange::Custom { .. }
                            ) {
                                panel.edit_settings(|s| {
                                    s.range = office_rs_print::PageRange::Custom {
                                        spec: value.clone(),
                                    }
                                });
                                cx.notify();
                            }
                        }
                    }
                },
            )
            .detach();
            state
        };
        let mut shell = Self {
            sessions: SessionManager::default(),
            status: "就绪".to_string(),
            font_config,
            file_menu_open: false,
            shape_menu_open: false,
            zoom_menu_open: false,
            context_menu: None,
            confirm_close: None,
            highlight_color: OfdColor::Rgb(255, 255, 0),
            color_picker,
            auto_paging: false,
            auto_page_gen: 0,
            pointer_cursor: PointerCursor::default(),
            pending_focus: None,
            print_panel: None,
            print_spec_input,
        };
        if let Some(path) = cli_path {
            match host::document_io::load_ofd(&path) {
                Ok(loaded) => shell.install_loaded_session(loaded, path, cx),
                Err(e) => {
                    eprintln!("failed to load {}: {e}", path.display());
                    shell.status = format!("加载失败：{e}");
                }
            }
        }
        if shell.sessions.sessions.is_empty() {
            let editor = shell.create_editor(cx);
            shell.sessions.new_blank(editor);
        }
        shell
    }

    /// Convenience handle over the ACTIVE session's editor.
    fn handle(&self) -> GpuiOfdHandle {
        GpuiOfdHandle::new(self.sessions.active_editor().expect("at least one session"))
    }

    /// Create a subscribed, clock-seeded editor entity for a new session.
    fn create_editor(&self, cx: &mut Context<Self>) -> Entity<OfdGpuiView> {
        let config = self.font_config.clone();
        let highlight = self.highlight_color;
        let editor = cx.new(|cx| {
            let mut view = OfdGpuiView::new(config, cx);
            // Host-injected annotation clock (Task 4 / xilem precedent:
            // author "rofd", ts 0 — the library never reads a clock).
            view.component.set_clock("rofd".into(), 0);
            // Keep the kernel's highlight color in step with the toolbar
            // picker (new sessions inherit the picked color).
            view.component.set_highlight_color(highlight);
            view
        });
        Self::subscribe_editor(&editor, cx);
        editor
    }

    fn subscribe_editor(editor: &Entity<OfdGpuiView>, cx: &mut Context<Self>) {
        cx.subscribe(editor, |this, emitter, event: &OfdGpuiEvent, cx| {
            this.on_editor_event(&emitter, event, cx);
        })
        .detach();
        cx.observe(editor, |_, _, cx| cx.notify()).detach();
    }

    /// Route one kernel event from any session into that session's
    /// mirrors. Session-scoped rows update the OWNING session (index
    /// resolved by entity identity); window-level state (context menu,
    /// pointer cursor) only reacts to the ACTIVE editor.
    fn on_editor_event(
        &mut self,
        emitter: &Entity<OfdGpuiView>,
        event: &OfdGpuiEvent,
        cx: &mut Context<Self>,
    ) {
        use OfdGpuiEvent::*;
        let Some(idx) = self.sessions.index_of(emitter) else {
            return;
        };
        let is_active = idx == self.sessions.active;
        match event {
            Changed => {
                let session = &mut self.sessions.sessions[idx];
                session.modified = true;
                session.pristine = false;
            }
            TextSelectionChanged => {
                let has = emitter.read(cx).component.has_text_selection();
                self.sessions.sessions[idx].has_text_selection = has;
            }
            // Cursor movement dismisses the context menu (xilem parity;
            // right-click does NOT fire on_cursor_change, so opening is
            // never clobbered).
            CursorChanged => {
                if is_active {
                    self.context_menu = None;
                }
            }
            SaveRequested => {
                if is_active {
                    self.save_session(idx, false, cx);
                }
            }
            PageChanged(page) => {
                self.sessions.sessions[idx].page = page + 1;
            }
            ZoomChanged(zoom) => {
                self.sessions.sessions[idx].zoom = *zoom;
            }
            ZoomModeChanged(mode) => {
                self.sessions.sessions[idx].zoom_mode = *mode;
            }
            Warned(warnings) => {
                for warning in warnings {
                    eprintln!("[warning] {warning:?}");
                }
                // Discrete batches: append, never replace (runtime warnings
                // must not erase load warnings).
                self.sessions.sessions[idx].append_warnings(warnings.clone());
            }
            PointerCursorChanged(cursor) => {
                if is_active {
                    self.pointer_cursor = *cursor;
                }
            }
            ContextMenuRequested((x, y), target) => {
                if !is_active {
                    return;
                }
                match target {
                    // R7: only an annotation hit opens the menu. The
                    // kernel point is viewport-local; add the element
                    // origin for a window-coordinate popover anchor.
                    ContextTarget::Annotation(id) => {
                        let (ox, oy) = emitter.read(cx).element_origin();
                        self.context_menu = Some(ContextMenuState {
                            x: x + ox,
                            y: y + oy,
                            id: id.clone(),
                        });
                    }
                    ContextTarget::Page | ContextTarget::Empty => {
                        self.context_menu = None;
                    }
                }
            }
            // CopyRequested carries no text (queue policy): re-run the
            // kernel copy and write the OS clipboard through the view.
            CopyRequested => emitter.update(cx, |view, cx| view.copy_selection(cx)),
            // The kernel paints focus rings/selection handles itself;
            // these notifications need no shell mirror for v1.
            SelectionChanged(_) | AnnotationFocused(_) | AnnotationInteracted(_) => {}
        }
        cx.notify();
    }

    // -- Session lifecycle --

    /// Insert a loaded document as a new ACTIVE session and subscribe it.
    fn install_loaded_session(
        &mut self,
        loaded: host::document_io::LoadedOfd,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        for warning in &loaded.warnings {
            eprintln!("[warning] {warning:?}");
        }
        let pages = loaded.document.pages.len();
        let editor = self.create_editor(cx);
        editor.update(cx, |view, cx| {
            view.component.load_document(loaded.document);
            // System font channel: the document's declared families are
            // resolved against the lazily-scanned local font index and the
            // found files' bytes register into the kernel (font gen bumps,
            // page caches invalidate). Uninstalled families stay on the
            // startup default font.
            crate::font_index::register_required_system_fonts(&mut view.component);
            cx.notify();
        });
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "文档".to_string());
        let idx = self
            .sessions
            .open_file(name, path.clone(), loaded.package, editor.clone());
        let session = &mut self.sessions.sessions[idx];
        session.warnings = loaded.warnings;
        session.page = 1;
        self.on_active_changed(cx);
        // Focus repair: install runs from a windowless async path — the
        // new editor takes focus at the next render.
        self.pending_focus = Some(editor);
        self.status = format!("已打开 {}（{pages} 页）", path.display());
        cx.notify();
    }

    /// 文件菜单「打开」: platform prompt -> load -> NEW session.
    fn open_document(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("打开 .ofd 文档".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            match host::document_io::load_ofd(&path) {
                Ok(loaded) => {
                    let _ = this.update(cx, |shell, cx| {
                        shell.install_loaded_session(loaded, path, cx);
                    });
                }
                Err(e) => {
                    eprintln!("[ERROR] open failed: {e}");
                    let _ = this.update(cx, |shell, cx| {
                        shell.status = format!("打开失败：{e}");
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// 文件菜单「保存」/ Ctrl+S: in-place when a path exists, else the
    /// Save-As prompt. A failed save keeps `modified` set (xilem
    /// result-slot contract: an unsaved document must never look clean).
    fn save_session(&mut self, idx: usize, close_after: bool, cx: &mut Context<Self>) {
        if self.sessions.sessions.get(idx).is_none() {
            return;
        }
        if self.sessions.sessions[idx].path.is_none() {
            self.save_session_as(idx, close_after, cx);
            return;
        }
        let path = self.sessions.sessions[idx]
            .path
            .clone()
            .expect("checked above");
        let package = self.sessions.sessions[idx].package.clone();
        let document = self.sessions.sessions[idx]
            .editor
            .read(cx)
            .component
            .document()
            .clone();
        match host::document_io::save_ofd(&document, package.as_ref(), &path) {
            Ok(()) => {
                self.sessions.sessions[idx].modified = false;
                self.status = format!("已保存 {}", path.display());
                if close_after {
                    self.close_session(idx, cx);
                }
            }
            Err(e) => {
                self.status = format!("保存失败：{e}");
                eprintln!("[ERROR] save failed: {e}");
            }
        }
        cx.notify();
    }

    /// 文件菜单「另存为」: new-path prompt -> save (surgical when the
    /// session has a package, full write otherwise) -> rebind the session
    /// path. Save-As never mints a PackageHandle (xilem precedent).
    fn save_session_as(&mut self, idx: usize, close_after: bool, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.sessions.get(idx) else {
            return;
        };
        let editor = session.editor.clone();
        let package = session.package.clone();
        let (dir, suggested) = save_prompt_target(session);
        let receiver = cx.prompt_for_new_path(&dir, suggested.as_deref());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| {
                // The prompt is async: the session may have closed or
                // moved while it was open. Resolve by editor identity.
                let Some(idx) = shell.sessions.index_of(&editor) else {
                    return;
                };
                let document = shell.sessions.sessions[idx]
                    .editor
                    .read(cx)
                    .component
                    .document()
                    .clone();
                match host::document_io::save_ofd(&document, package.as_ref(), &path) {
                    Ok(()) => {
                        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string());
                        let session = &mut shell.sessions.sessions[idx];
                        session.modified = false;
                        session.path = Some(path.clone());
                        if let Some(stem) = stem {
                            session.file_name = stem;
                        }
                        shell.status = format!("已保存 {}", path.display());
                        if close_after {
                            shell.close_session(idx, cx);
                        }
                    }
                    Err(e) => {
                        shell.status = format!("保存失败：{e}");
                        eprintln!("[ERROR] save failed: {e}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 文件菜单「导出为 PDF」: new-path prompt -> snapshot through the live
    /// component. An export never touches the modified mirror in either
    /// direction (host/export_pdf contract). The editor is captured BEFORE
    /// the prompt and resolved by identity after — a tab switch (or close)
    /// during the dialog must not redirect or resurrect the export.
    fn export_pdf(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let editor = session.editor.clone();
        let dir = session
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let receiver = cx.prompt_for_new_path(&dir, Some("document.pdf"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| {
                let Some(idx) = shell.sessions.index_of(&editor) else {
                    // Session closed while the prompt was open: abort.
                    return;
                };
                let editor = shell.sessions.sessions[idx].editor.clone();
                match host::export_pdf::export_pdf(&editor.read(cx).component, &path) {
                    Ok(()) => shell.status = format!("已导出 {}", path.display()),
                    Err(e) => {
                        shell.status = format!("导出失败：{e}");
                        eprintln!("[ERROR] export failed: {e}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -- Print panel (Task 13; xilem `drain_print_dialog` contract) --

    /// 文件菜单「打印…」: open the modal panel for the active session.
    /// The 0-page guard runs synchronously first (fresh/blank documents
    /// never open the dialog); the paper/orientation defaults sync at open
    /// time against a FRESH print session — xilem defers this to its
    /// drain step ③ only because its component sits behind a command
    /// queue, so an open-time read would see the stale material.
    fn open_print_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let editor = session.editor.clone();
        let print_session = editor
            .read(cx)
            .component
            .begin_print(PrintContent::default());
        let page_count = print_session.page_count();
        if page_count == 0 {
            self.status = "无可打印页面".to_string();
            cx.notify();
            return;
        }
        let (paper, orientation) = print_session.page_parameters(0);
        drop(print_session);
        let mut panel = PrintPanelState::new(page_count);
        panel.open();
        panel.sync_document_defaults(paper, orientation);
        // Bounded discovery drain (xilem do_print): the worker races the
        // dialog open, so drain until the terminal message or the timeout;
        // a straggler receiver stays for render-tick draining.
        let receiver = NativePrintServices.discover();
        while let Ok(update) = receiver.recv_timeout(DISCOVER_TIMEOUT) {
            let terminal = !matches!(update, DiscoverState::Finding);
            panel.apply_discovery(update);
            if terminal {
                break;
            }
        }
        panel.receiver = Some(receiver);
        // Fresh spec field per dialog.
        self.print_spec_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.print_panel = Some(panel);
        cx.notify();
    }

    /// Confirmed submit (single-track): pure guards + freeze run in
    /// `try_submit`; the pending triple then executes synchronously —
    /// steps ⑤/⑥ of the xilem contract collapse here because gpui
    /// reaches the component directly (no command queue, so no job slot
    /// and no "unpicked handle" window for the exit path to race).
    fn submit_print(&mut self, cx: &mut Context<Self>) {
        // Resolve the editor's CURRENT page (1-based mirror) for the
        // precheck and the Current→Custom freeze, so the live editor page
        // cannot diverge from the spooled job.
        let Some(editor_page) = self.sessions.active().map(|s| s.page) else {
            return;
        };
        let Some(panel) = self.print_panel.as_mut() else {
            return;
        };
        match panel.try_submit(editor_page) {
            Ok(()) => {}
            // Silent guard (xilem parity): no duplicate job, no status
            // churn while one submit already owns the dialog.
            Err(SubmitBlock::Busy) => return,
            // try_submit already surfaced the no-printers status.
            Err(SubmitBlock::NoPrinters) => {
                cx.notify();
                return;
            }
            Err(SubmitBlock::Invalid(message)) => {
                panel.status = message;
                cx.notify();
                return;
            }
        }
        let Some((printer, settings, content)) =
            self.print_panel.as_mut().and_then(|p| p.take_pending())
        else {
            return;
        };
        let Some(editor) = self.sessions.active_editor() else {
            return;
        };
        // ⑤ print_direct inside the entity update.
        let job = GpuiOfdHandle::new(editor).print_direct(
            printer.as_deref(),
            &settings,
            content,
            "rofd",
            cx,
        );
        // ⑥ handle → live job + Submitted status.
        if let Some(panel) = self.print_panel.as_mut() {
            panel.submit_accepted(job);
        }
        Self::spawn_print_poll(cx);
        cx.notify();
    }

    /// 取消: close request — cancels a live job (the exit racing a
    /// just-started job cancels it rather than letting it spool
    /// untracked) and drops any un-executed pending. The drain closes the
    /// overlay on the exit flag.
    fn cancel_print(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.print_panel.as_mut() {
            panel.request_exit();
        }
        cx.notify();
    }

    /// Terminal-state polling (drain ① driver): poll the live job every
    /// 200ms; the first terminal state (or the panel closing) ends the
    /// loop after exactly one notify.
    fn spawn_print_poll(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(PRINT_POLL_INTERVAL).await;
            let mut stop = false;
            let updated = this.update(cx, |shell, cx| {
                let Some(panel) = shell.print_panel.as_mut() else {
                    stop = true;
                    return;
                };
                if panel.poll_job_terminal() {
                    cx.notify();
                }
                let terminal = panel.job.as_ref().is_some_and(|job| {
                    matches!(
                        job.snapshot().state,
                        JobState::Done | JobState::Failed | JobState::Aborted
                    )
                });
                if terminal {
                    stop = true;
                }
            });
            if updated.is_err() || stop {
                break;
            }
        })
        .detach();
    }

    /// Print-dialog drain at the top of every render pass (xilem
    /// `drain_print_dialog`, adapted):
    /// ① poll the live job for a terminal state (the spawn loop is the
    ///    primary driver; this keeps the status honest without it) and
    ///    drain discovery stragglers;
    /// ② the exit flag closes the overlay (the cancelled handle was
    ///    already cancelled by `request_exit` — no unpicked slot exists
    ///    in this host);
    /// ③ defaults synced at open (direct component access);
    /// ④ no content-derived display — the panel shows the page count,
    ///    which is content-independent, so a content-flag edit needs no
    ///    re-materialize (justified no-op);
    /// ⑤⑥ executed synchronously in `submit_print`.
    fn drain_print_dialog(&mut self) {
        let Some(panel) = self.print_panel.as_mut() else {
            return;
        };
        panel.poll_job_terminal();
        panel.drain_discovery();
        if panel.exit {
            self.print_panel = None;
        }
    }

    /// Tab click: switch the active session and focus its editor.
    fn switch_session(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.sessions.sessions.len() || idx == self.sessions.active {
            return;
        }
        self.sessions.active = idx;
        self.on_active_changed(cx);
        if let Some(editor) = self.sessions.active_editor() {
            let focus = editor.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        }
    }

    /// 「+」tab: open a new blank session and focus it.
    fn new_blank_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.create_editor(cx);
        self.sessions.new_blank(editor.clone());
        self.on_active_changed(cx);
        let focus = editor.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
        self.status = "新建空白文档".to_string();
        cx.notify();
    }

    /// Tab ×: modified sessions ask first (confirm popover), clean ones
    /// close immediately.
    fn request_close(&mut self, idx: usize, cx: &mut Context<Self>) {
        let modified = self
            .sessions
            .sessions
            .get(idx)
            .map(|s| s.modified)
            .unwrap_or(false);
        if modified {
            self.confirm_close = Some(idx);
            cx.notify();
        } else {
            self.close_session(idx, cx);
        }
    }

    /// Close a session; the last close auto-opens a blank (spec §7).
    fn close_session(&mut self, idx: usize, cx: &mut Context<Self>) {
        let active_before = self.sessions.active_editor();
        self.confirm_close = None;
        self.context_menu = None;
        if self.sessions.close(idx) {
            let editor = self.create_editor(cx);
            self.sessions.new_blank(editor);
            self.status = "已关闭，新建空白文档".to_string();
        } else {
            self.status = "已关闭".to_string();
        }
        if self.sessions.active_editor() != active_before {
            self.on_active_changed(cx);
            // Focus repair: closing the focused editor leaves focus dead
            // until a click; hand it to the new active editor at the next
            // render (this path runs without a Window).
            self.pending_focus = self.sessions.active_editor();
        } else {
            cx.notify();
        }
    }

    /// Cross-session reset after any change of the ACTIVE editor
    /// (spec §7 modal reset): dismiss overlays, stop the auto-page timer
    /// (a stale loop would page the wrong document), and reset transient
    /// hover state.
    fn on_active_changed(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.confirm_close = None;
        self.file_menu_open = false;
        self.shape_menu_open = false;
        self.zoom_menu_open = false;
        self.auto_paging = false;
        self.auto_page_gen += 1;
        self.pointer_cursor = PointerCursor::default();
        // The print panel is bound to the session active at its open; a
        // tab switch closes it WITHOUT cancelling — a submitted job is
        // already in the spooler's hands (join_all flushes the worker at
        // exit), and switching tabs is not a cancel intent.
        self.print_panel = None;
        cx.notify();
    }

    // -- Toolbar verbs --

    fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.context_menu = None;
        if let Some(session) = self.sessions.active_mut() {
            session.tool = tool.clone();
            GpuiOfdHandle::new(session.editor.clone()).set_tool(tool, cx);
        }
        cx.notify();
    }

    /// Apply a markup to the active session's body-text selection. The
    /// buttons are disabled without one; the kernel result id is a no-op
    /// to surface for v1.
    fn apply_markup(&mut self, kind: AnnotationKind, cx: &mut Context<Self>) {
        self.handle().apply_markup(kind, cx);
        cx.notify();
    }

    /// Zoom percent preset: multiplicative zoom anchored at the viewport
    /// center (the kernel clamps; manual zoom exits fit modes).
    fn pick_zoom_percent(&mut self, percent: f64, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let factor = (percent / 100.0 * INITIAL_ZOOM) / session.zoom;
        if factor.is_finite() && (factor - 1.0).abs() > f64::EPSILON {
            self.handle().zoom_at(factor, cx);
        }
        cx.notify();
    }

    /// 全屏 toggle: window fullscreen; Esc (ExitFullscreen action) exits.
    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        cx.notify();
    }

    /// 自动翻页 toggle: a 3s timer loop pages the ACTIVE editor down. The
    /// generation token cancels stale loops on toggle-off and on session
    /// changes (on_active_changed bumps it).
    fn toggle_auto_page(&mut self, cx: &mut Context<Self>) {
        self.auto_page_gen += 1;
        if self.auto_paging {
            self.auto_paging = false;
        } else {
            self.auto_paging = true;
            let generation = self.auto_page_gen;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(AUTO_PAGE_INTERVAL).await;
                let mut keep_running = false;
                let updated = this.update(cx, |shell, cx| {
                    if shell.auto_paging && shell.auto_page_gen == generation {
                        shell.handle().scroll_page(ScrollDirection::Down, cx);
                        keep_running = true;
                    }
                });
                if updated.is_err() || !keep_running {
                    break;
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// Consume bridge events pushed by Popover content since the last
    /// render: actions in push order, then the close-popover mutation.
    fn on_action(&mut self, action: AppAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            AppAction::OpenFile => self.open_document(cx),
            AppAction::SaveFile => {
                let idx = self.sessions.active;
                self.save_session(idx, false, cx);
            }
            AppAction::SaveAsFile => {
                let idx = self.sessions.active;
                self.save_session_as(idx, false, cx);
            }
            AppAction::ExportPdf => self.export_pdf(cx),
            AppAction::Print => self.open_print_dialog(window, cx),
            AppAction::PrintSubmit => self.submit_print(cx),
            AppAction::PrintCancel => self.cancel_print(cx),
            AppAction::PickShape(kind) => {
                self.set_tool(Tool::Create(CreateKind::Shape(kind)), cx);
            }
            AppAction::PickZoomMode(mode) => {
                self.handle().set_zoom_mode(mode, cx);
                cx.notify();
            }
            AppAction::PickZoomPercent(percent) => self.pick_zoom_percent(percent, cx),
            AppAction::ContextDeleteAnnotation => {
                if let Some(menu) = self.context_menu.take() {
                    self.handle().delete_annotation(menu.id, cx);
                    self.status = "已删除批注".to_string();
                }
                cx.notify();
            }
            AppAction::ContextMenuDismiss => {
                self.context_menu = None;
                cx.notify();
            }
            AppAction::CloseConfirmSave => {
                if let Some(idx) = self.confirm_close.take() {
                    self.save_session(idx, true, cx);
                }
            }
            AppAction::CloseConfirmDiscard => {
                if let Some(idx) = self.confirm_close.take() {
                    self.close_session(idx, cx);
                }
            }
            AppAction::CloseConfirmCancel => {
                self.confirm_close = None;
                cx.notify();
            }
        }
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drain bridge events first: actions then popover-close mutations,
        // in push order (menu entries push [Action, SetOpen(false)]).
        for event in AppActionBridge::drain(cx) {
            match event {
                BridgeEvent::SetOpen(f) => f(self),
                BridgeEvent::Action(action) => self.on_action(action, window, cx),
            }
        }

        // Focus repair AFTER the drain (an OpenFile action installs its
        // session above): windowless paths queued an editor to focus, and
        // Render is where a &mut Window is available. window.focus is a
        // no-op when the handle already holds focus.
        if let Some(editor) = self.pending_focus.take() {
            let handle = editor.read(cx).focus_handle.clone();
            window.focus(&handle, cx);
        }

        // Print-dialog drain (①/② of the xilem contract; ③-⑥ live in
        // open/submit — see drain_print_dialog).
        self.drain_print_dialog();

        let active = self.sessions.active();
        let active_idx = self.sessions.active;
        let session_count = self.sessions.sessions.len();
        let editor = self.sessions.active_editor().expect("at least one session");
        let tool = active.map(|s| s.tool.clone()).unwrap_or(Tool::Text);
        let has_selection = active.map(|s| s.has_text_selection).unwrap_or(false);
        let zoom_percent =
            (active.map(|s| s.zoom).unwrap_or(INITIAL_ZOOM) / INITIAL_ZOOM * 100.0).round() as i32;
        let zoom_mode = active.map(|s| s.zoom_mode).unwrap_or(ZoomMode::Custom);
        let page = active.map(|s| s.page).unwrap_or(1);
        let modified = active.map(|s| s.modified).unwrap_or(false);
        let warnings: Vec<String> = active
            .map(|s| s.warnings.iter().map(warning_text).collect())
            .unwrap_or_default();
        let can_undo = self.handle().can_undo(cx);
        let can_redo = self.handle().can_redo(cx);
        let fullscreen = window.is_fullscreen();
        let auto_paging = self.auto_paging;
        // Print panel overlay (top layer). The spec-input entity handle is
        // cloned out first: building the overlay mutably borrows the panel
        // while the input reference borrows the shell.
        let print_spec_input = self.print_spec_input.clone();
        let print_overlay = self
            .print_panel
            .as_mut()
            .map(|panel| print_panel_overlay(panel, &print_spec_input, page, cx));

        // -- Toolbar groups (built as Vec<AnyElement> + .children(); spec
        // pitfall 1) --
        let tools_group = group(vec![
            tool_btn(
                "tool-hand",
                IconName::Hand,
                "手型",
                tool == Tool::Hand,
                false,
                |this, _e, _w, cx| this.set_tool(Tool::Hand, cx),
                cx,
            ),
            tool_btn(
                "tool-text",
                IconName::TextCursorInput,
                "文本",
                tool == Tool::Text,
                false,
                |this, _e, _w, cx| this.set_tool(Tool::Text, cx),
                cx,
            ),
            tool_btn(
                "tool-freehand",
                IconName::Signature,
                "手绘",
                tool == Tool::Create(CreateKind::Freehand),
                false,
                |this, _e, _w, cx| {
                    this.set_tool(Tool::Create(CreateKind::Freehand), cx);
                },
                cx,
            ),
            shape_menu(tool, self.shape_menu_open).into_any_element(),
        ]);

        let markup_group = group(vec![
            tool_btn(
                "markup-highlight",
                IconName::Highlighter,
                "高亮",
                false,
                !has_selection,
                |this, _e, _w, cx| this.apply_markup(AnnotationKind::Highlight, cx),
                cx,
            ),
            tool_btn(
                "markup-underline",
                IconName::Underline,
                "下划线",
                false,
                !has_selection,
                |this, _e, _w, cx| this.apply_markup(AnnotationKind::Underline, cx),
                cx,
            ),
            tool_btn(
                "markup-strikeout",
                IconName::Strikethrough,
                "删除线",
                false,
                !has_selection,
                |this, _e, _w, cx| this.apply_markup(AnnotationKind::Strikeout, cx),
                cx,
            ),
            tool_btn(
                "markup-squiggly",
                IconName::LineSquiggle,
                "波浪线",
                false,
                !has_selection,
                |this, _e, _w, cx| this.apply_markup(AnnotationKind::Squiggly, cx),
                cx,
            ),
            // Highlight color picker (kit ColorPicker, overlay layer):
            // picking a color sets the kernel highlight color and applies
            // on selection (web R4) — wired in the shell's subscribe_in.
            ColorPicker::new(&self.color_picker)
                .icon(
                    Icon::new(IconName::Palette)
                        .size(px(15.))
                        .text_color(rgb(0x3A3A3A)),
                )
                .with_size(gpui_kit::component::Size::Small)
                .into_any_element(),
        ]);

        let history_group = group(vec![
            tool_btn(
                "undo",
                IconName::Undo2,
                "撤销",
                false,
                !can_undo,
                |this, _e, _w, cx| {
                    this.handle().undo(cx);
                    cx.notify();
                },
                cx,
            ),
            tool_btn(
                "redo",
                IconName::Redo2,
                "恢复",
                false,
                !can_redo,
                |this, _e, _w, cx| {
                    this.handle().redo(cx);
                    cx.notify();
                },
                cx,
            ),
        ]);

        let zoom_group = group(vec![zoom_menu(
            zoom_percent,
            zoom_mode,
            self.zoom_menu_open,
        )
        .into_any_element()]);

        let view_group = group(vec![
            tool_btn(
                "fullscreen",
                IconName::Maximize,
                "全屏",
                fullscreen,
                false,
                |this, _e, window, cx| this.toggle_fullscreen(window, cx),
                cx,
            ),
            tool_btn(
                "auto-page",
                IconName::Timer,
                "自动翻页",
                auto_paging,
                false,
                |this, _e, _w, cx| this.toggle_auto_page(cx),
                cx,
            ),
        ]);

        div()
            .id("shell-root")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0xF2F2F2))
            .on_action(cx.listener(|this, _: &ExitFullscreen, window, cx| {
                // Esc exits fullscreen (binding dispatch precedes the raw
                // key's focus-path delivery; see main.rs).
                if window.is_fullscreen() {
                    this.toggle_fullscreen(window, cx);
                }
            }))
            // == Tab strip: file menu chip + document tabs + new tab ==
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h(px(32.))
                    .bg(rgb(0xE8EBF0))
                    .flex_shrink_0()
                    .child(file_menu(self.file_menu_open))
                    .children(
                        (0..session_count).map(|i| {
                            session_tab(i, i == active_idx, &self.sessions.sessions[i], cx)
                        }),
                    )
                    .child(
                        div()
                            .id("tab-new")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.))
                            .h(px(24.))
                            .rounded(px(4.))
                            .text_size(px(14.))
                            .cursor_pointer()
                            .hover(|d| d.bg(rgb(0xF0F2F5)))
                            .on_click(cx.listener(|this, _e: &ClickEvent, window, cx| {
                                this.new_blank_session(window, cx);
                            }))
                            .child("+"),
                    ),
            )
            // == Toolbar row (single row of labelled groups) ==
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .flex_shrink_0()
                    .bg(rgb(0xFAFAFA))
                    .border_b_1()
                    .border_color(rgb(0xDDDDDD))
                    .child(tools_group)
                    .child(markup_group)
                    .child(history_group)
                    .child(zoom_group)
                    .child(view_group),
            )
            // == Editor (flex_1 + overflow_hidden keeps the editor's
            // relative(1.0) element clipped to its slot; hover cursor from
            // the PointerCursorChanged mirror) ==
            .child(
                div()
                    .id("editor-area")
                    .flex_1()
                    .overflow_hidden()
                    .cursor(cursor_style(self.pointer_cursor))
                    .child(editor),
            )
            // == Warnings bar (active session, dismissible) ==
            .when(!warnings.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_size(px(12.))
                        .bg(rgb(0xFFF7E6))
                        .border_t_1()
                        .border_color(rgb(0xFFD591))
                        .child(format!("⚠ {} 条加载警告", warnings.len()))
                        .child(
                            div()
                                .flex_1()
                                .overflow_x_hidden()
                                .text_ellipsis()
                                .child(warnings.first().cloned().unwrap_or_default()),
                        )
                        .child(
                            div()
                                .id("warnings-dismiss")
                                .px_1()
                                .cursor_pointer()
                                .text_color(rgb(0x999999))
                                .hover(|d| d.text_color(rgb(0xCC0000)))
                                .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                                    if let Some(session) = this.sessions.active_mut() {
                                        session.warnings.clear();
                                    }
                                    cx.notify();
                                }))
                                .child("×"),
                        ),
                )
            })
            // == Status bar ==
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .gap_4()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .bg(rgb(0xFAFAFA))
                    .border_t_1()
                    .border_color(rgb(0xDDDDDD))
                    .child(format!("第 {page} 页"))
                    .child(format!("{zoom_percent}%"))
                    .when(modified, |this| this.child("● 未保存"))
                    .child(self.status.clone()),
            )
            // == Context menu (R7): anchored at the window-space right-click
            // point via an invisible 1px trigger; only annotation hits ever
            // set the state. Rendered through the kit Popover overlay layer
            // (spec pitfall 3 — absolute siblings would be painted over).
            .when_some(self.context_menu.as_ref(), |this, menu| {
                this.child(context_menu_anchor(menu.x, menu.y))
            })
            // == Close-confirm (modified sessions): centered popover with
            // 保存 / 不保存 / 取消.
            .when(self.confirm_close.is_some(), |this| {
                this.child(confirm_close_anchor())
            })
            // == Print panel (Task 13): centered modal overlay.
            .children(print_overlay)
    }
}

// -- Popover chrome (bridge-routed; content contexts cannot reach the
// shell entity) --

/// 文件 chip + Popover: 打开 / 保存 / 另存为 / 导出为 PDF / 打印… (the
/// print action opens the modal panel, Task 13).
fn file_menu(open: bool) -> AnyElement {
    Popover::new("file-menu")
        .open(open)
        .anchor(Anchor::TopLeft)
        .on_open_change(set_open_bridge(|shell, open| shell.file_menu_open = open))
        .trigger(Button::new("file-menu-chip").label("☰ 文件").compact())
        .content(|_state, _window, _cx| {
            div()
                .flex()
                .flex_col()
                .min_w(px(180.))
                .py_1()
                .children(vec![
                    menu_entry("fm-open", "打开", AppAction::OpenFile, close_file_menu()),
                    menu_entry("fm-save", "保存", AppAction::SaveFile, close_file_menu()),
                    menu_entry(
                        "fm-save-as",
                        "另存为",
                        AppAction::SaveAsFile,
                        close_file_menu(),
                    ),
                    menu_entry(
                        "fm-export-pdf",
                        "导出为 PDF",
                        AppAction::ExportPdf,
                        close_file_menu(),
                    ),
                    menu_entry("fm-print", "打印…", AppAction::Print, close_file_menu()),
                ])
                .into_any_element()
        })
        .into_any_element()
}

fn close_file_menu() -> impl Fn(&mut AppShell) + 'static {
    |shell: &mut AppShell| shell.file_menu_open = false
}

/// 图形 dropdown: the six shape sub-tools (ShapeKind set). The trigger
/// shows the active shape (rect before any pick).
fn shape_menu(tool: Tool, open: bool) -> AnyElement {
    let active_shape = match &tool {
        Tool::Create(CreateKind::Shape(kind)) => Some(*kind),
        _ => None,
    };
    let trigger_label = shape_label(active_shape.unwrap_or(ShapeKind::Rect));
    Popover::new("shape-menu")
        .open(open)
        .anchor(Anchor::TopLeft)
        .on_open_change(set_open_bridge(|shell, open| shell.shape_menu_open = open))
        .trigger(
            Button::new("shape-trigger")
                .label(trigger_label)
                .compact()
                .dropdown_caret(true),
        )
        .content(move |_state, _window, _cx| {
            let items = SHAPE_ITEMS
                .iter()
                .map(|&(kind, label, icon)| {
                    shape_menu_item(kind, label, icon, active_shape == Some(kind))
                })
                .collect::<Vec<_>>();
            div()
                .flex()
                .flex_col()
                .min_w(px(130.))
                .py_1()
                .children(items)
                .into_any_element()
        })
        .into_any_element()
}

/// One shape item: click = pick the tool + close the popover (bridge
/// order: action first, then close).
fn shape_menu_item(kind: ShapeKind, label: &str, icon: IconName, active: bool) -> AnyElement {
    div()
        .id(ElementId::Name(format!("shape-{kind:?}").into()))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .text_size(px(13.))
        .cursor_pointer()
        .when(active, |d| d.bg(rgb(0xE3F0FC)).text_color(rgb(0x1470C8)))
        .hover(|d| d.bg(rgb(0xE8EBEF)))
        .child(Icon::new(icon).size(px(15.)).text_color(rgb(0x4B5260)))
        .child(label.to_string())
        .child(div().flex_1())
        .when(active, |d| d.text_color(rgb(0x1470C8)).child("✓"))
        .on_click(move |_e: &ClickEvent, _window, cx| {
            AppActionBridge::push(AppAction::PickShape(kind), cx);
            AppActionBridge::push_raw(
                BridgeEvent::SetOpen(Box::new(|shell: &mut AppShell| {
                    shell.shape_menu_open = false;
                })),
                cx,
            );
        })
        .into_any_element()
}

/// 显示比例 dropdown: fit modes (checked against the active mode) + the
/// percent presets (web ZoomMenu parity).
fn zoom_menu(percent: i32, mode: ZoomMode, open: bool) -> AnyElement {
    Popover::new("zoom-menu")
        .open(open)
        .anchor(Anchor::TopLeft)
        .on_open_change(set_open_bridge(|shell, open| shell.zoom_menu_open = open))
        .trigger(
            Button::new("zoom-trigger")
                .label(format!("{percent}%"))
                .compact()
                .dropdown_caret(true),
        )
        .content(move |_state, _window, _cx| {
            let fits = vec![
                (ZoomMode::FitWidth, "适应宽度"),
                (ZoomMode::FitHeight, "适应高度"),
                (ZoomMode::FitPage, "适应页面"),
            ];
            let fit_items = fits
                .into_iter()
                .map(|(m, label)| {
                    let active = m == mode;
                    div()
                        .id(ElementId::Name(format!("zoom-fit-{label}").into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .min_w(px(120.))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(rgb(0xE3F0FC)).text_color(rgb(0x1470C8)))
                        .hover(|d| d.bg(rgb(0xE8EBEF)))
                        .child(label.to_string())
                        .child(div().flex_1())
                        .when(active, |d| d.text_color(rgb(0x1470C8)).child("✓"))
                        .on_click(move |_e: &ClickEvent, _window, cx| {
                            AppActionBridge::push(AppAction::PickZoomMode(m), cx);
                            AppActionBridge::push_raw(
                                BridgeEvent::SetOpen(Box::new(|shell: &mut AppShell| {
                                    shell.zoom_menu_open = false;
                                })),
                                cx,
                            );
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            let preset_items = ZOOM_PRESETS
                .iter()
                .map(|&p| {
                    let active = (p.round() as i32) == percent;
                    div()
                        .id(ElementId::Name(format!("zoom-preset-{p}").into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .min_w(px(120.))
                        .text_size(px(13.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(rgb(0xE3F0FC)).text_color(rgb(0x1470C8)))
                        .hover(|d| d.bg(rgb(0xE8EBEF)))
                        .child(format!("{}%", p as i64))
                        .child(div().flex_1())
                        .when(active, |d| d.text_color(rgb(0x1470C8)).child("✓"))
                        .on_click(move |_e: &ClickEvent, _window, cx| {
                            AppActionBridge::push(AppAction::PickZoomPercent(p), cx);
                            AppActionBridge::push_raw(
                                BridgeEvent::SetOpen(Box::new(|shell: &mut AppShell| {
                                    shell.zoom_menu_open = false;
                                })),
                                cx,
                            );
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            div()
                .id("zoom-menu-list")
                .flex()
                .flex_col()
                .max_h(px(260.))
                .overflow_scroll()
                .py_1()
                .children(fit_items)
                .child(
                    div()
                        .h(px(1.))
                        .mx_2()
                        .my_1()
                        .bg(rgb(0xD8DDE3))
                        .flex_shrink_0(),
                )
                .children(preset_items)
                .into_any_element()
        })
        .into_any_element()
}

/// File-menu entry: click = push the action, then close the popover
/// (bridge order matters — the shell drains in order). The close setter
/// is shared (Rc): the click handler is an Fn that may fire repeatedly.
fn menu_entry(
    id: &str,
    label: &str,
    action: AppAction,
    close: impl Fn(&mut AppShell) + 'static,
) -> AnyElement {
    let close = std::rc::Rc::new(close);
    div()
        .id(ElementId::Name(format!("me-{id}").into()))
        .px_3()
        .py_1()
        .text_size(px(13.))
        .cursor_pointer()
        .hover(|d| d.bg(rgb(0xEAF1FF)))
        .on_click(move |_e: &ClickEvent, _window, cx| {
            AppActionBridge::push(action.clone(), cx);
            let close = close.clone();
            AppActionBridge::push_raw(
                BridgeEvent::SetOpen(Box::new(move |shell: &mut AppShell| close(shell))),
                cx,
            );
        })
        .child(label.to_string())
        .into_any_element()
}

/// Controlled-popover open/close hop: the Popover's on_open_change runs
/// with a plain App context, so the mutation rides the bridge. The setter
/// is shared (Rc) because on_open_change is an Fn that may run repeatedly.
fn set_open_bridge(
    f: impl Fn(&mut AppShell, bool) + 'static,
) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let f = std::rc::Rc::new(f);
    move |open: &bool, _window, cx| {
        let open = *open;
        let f = f.clone();
        AppActionBridge::push_raw(
            BridgeEvent::SetOpen(Box::new(move |shell: &mut AppShell| f(shell, open))),
            cx,
        );
    }
}

/// The context-menu anchor: an invisible 1px element at the window-space
/// right-click point; the popover opens controlled (the dismiss gesture is
/// the popover's own overlay).
fn context_menu_anchor(x: f64, y: f64) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .absolute()
                .left(px(x as f32))
                .top(px(y as f32))
                .size_2()
                .overflow_hidden()
                .opacity(0.)
                .child(
                    Popover::new("ctx-menu")
                        .open(true)
                        .anchor(Anchor::TopLeft)
                        .overlay_closable(true)
                        .on_open_change(|open: &bool, _window, cx| {
                            if !*open {
                                AppActionBridge::push(AppAction::ContextMenuDismiss, cx);
                            }
                        })
                        .trigger(Button::new("ctx-trigger").label("").compact())
                        .content(|_state, _window, _cx| {
                            menu_entry(
                                "ctx-delete",
                                "删除批注",
                                AppAction::ContextDeleteAnnotation,
                                |shell: &mut AppShell| shell.context_menu = None,
                            )
                        }),
                ),
        )
        .into_any_element()
}

/// The close-confirm anchor: an invisible 1px element at the window
/// center; the popover carries 保存 / 不保存 / 取消.
fn confirm_close_anchor() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div().size_2().overflow_hidden().opacity(0.).child(
                Popover::new("close-confirm")
                    .open(true)
                    .anchor(Anchor::TopLeft)
                    .overlay_closable(true)
                    .on_open_change(|open: &bool, _window, cx| {
                        if !*open {
                            AppActionBridge::push_raw(
                                BridgeEvent::SetOpen(Box::new(|shell: &mut AppShell| {
                                    shell.confirm_close = None
                                })),
                                cx,
                            );
                        }
                    })
                    .trigger(Button::new("close-confirm-trigger").label("").compact())
                    .content(|_state, _window, _cx| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .min_w(px(240.))
                            .py_1()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .child("文档已修改，关闭前是否保存？"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .justify_end()
                                    .child(confirm_button(
                                        "cc-cancel",
                                        "取消",
                                        AppAction::CloseConfirmCancel,
                                        false,
                                    ))
                                    .child(confirm_button(
                                        "cc-discard",
                                        "不保存",
                                        AppAction::CloseConfirmDiscard,
                                        false,
                                    ))
                                    .child(confirm_button(
                                        "cc-save",
                                        "保存",
                                        AppAction::CloseConfirmSave,
                                        true,
                                    )),
                            )
                    }),
            ),
        )
        .into_any_element()
}

/// One close-confirm button (bridge-routed; the popover closes via the
/// SetOpen push, the shell drains the action first).
fn confirm_button(id: &'static str, label: &str, action: AppAction, primary: bool) -> AnyElement {
    let mut button = Button::new(id).label(label).compact();
    if primary {
        button = button.primary();
    }
    button
        .on_click(move |_e: &ClickEvent, _window, cx| {
            AppActionBridge::push(action.clone(), cx);
            AppActionBridge::push_raw(
                BridgeEvent::SetOpen(Box::new(|shell: &mut AppShell| {
                    shell.confirm_close = None;
                })),
                cx,
            );
        })
        .into_any_element()
}

// -- Session tabs --

/// One document tab: name (with a modified dot) + close chip. Clicking
/// the tab switches the session; clicking × requests a close.
fn session_tab(
    idx: usize,
    active: bool,
    session: &DocumentSession,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let name = session.file_name.clone();
    let modified = session.modified;
    let index = idx;
    div()
        .id(ElementId::Name(format!("doc-tab-{idx}").into()))
        .flex()
        .items_center()
        .gap_1()
        .h(px(24.))
        .px_2()
        .rounded(px(4.))
        .text_size(px(12.))
        .cursor_pointer()
        .when(active, |d| d.bg(rgb(0xFFFFFF)).text_color(rgb(0x1E6CFF)))
        .when(!active, |d| d.hover(|h| h.bg(rgb(0xF0F2F5))))
        .on_click(cx.listener(move |this, _e: &ClickEvent, window, cx| {
            this.switch_session(index, window, cx);
        }))
        .child(name)
        .when(modified, |d| {
            d.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(0x1E6CFF))
                    .child("●"),
            )
        })
        .child(
            div()
                .id(ElementId::Name(format!("doc-tab-close-{idx}").into()))
                .flex()
                .items_center()
                .px_1()
                .text_size(px(10.))
                .text_color(rgb(0x999999))
                .hover(|d| d.text_color(rgb(0xCC0000)))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    // Stop the tab-switch click from also firing.
                    cx.listener(|_this, _e, _w, cx| cx.stop_propagation()),
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                    this.request_close(index, cx);
                }))
                .child("×"),
        )
        .into_any_element()
}

// -- Toolbar primitives --

/// Icon-over-label tool button with active highlight and disabled state.
fn tool_btn(
    id: &'static str,
    icon: IconName,
    label: &str,
    active: bool,
    disabled: bool,
    on_click: impl Fn(&mut AppShell, &ClickEvent, &mut Window, &mut Context<AppShell>) + 'static,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let button = div()
        .id(id)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .min_w(px(46.))
        .h(px(44.))
        .rounded(px(4.))
        .text_size(px(10.))
        .when(active, |d| d.bg(rgb(0xD9E7F9)))
        .when(!disabled, |d| {
            d.cursor_pointer().hover(|h| h.bg(rgb(0xEDEDED)))
        })
        .when(disabled, |d| d.opacity(0.4))
        .child(
            Icon::new(icon)
                .size(px(16.))
                .when(active, |i| i.text_color(rgb(0x1E6CFF)))
                .text_color(rgb(0x3A3A3A)),
        )
        .child(div().text_color(rgb(0x555555)).child(label.to_string()));
    if disabled {
        button.into_any_element()
    } else {
        button.on_click(cx.listener(on_click)).into_any_element()
    }
}

/// Toolbar group: one row with a right divider (the "no Ribbon component"
/// shape from the spec — groups are label + divider stacks).
fn group(items: Vec<AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .border_r_1()
        .border_color(rgb(0xE0E0E0))
        .children(items)
}

// -- Small maps --

const SHAPE_ITEMS: &[(ShapeKind, &str, IconName)] = &[
    (ShapeKind::Rect, "矩形", IconName::Square),
    (ShapeKind::Ellipse, "椭圆", IconName::Circle),
    (ShapeKind::Line, "直线", IconName::Minus),
    (ShapeKind::Arrow, "箭头", IconName::ArrowRight),
    (ShapeKind::Polygon, "多边形", IconName::Pentagon),
    (ShapeKind::PolyLine, "折线", IconName::Spline),
];

fn shape_label(kind: ShapeKind) -> &'static str {
    SHAPE_ITEMS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, label, _)| *label)
        .unwrap_or("图形")
}

/// Kernel pointer-cursor -> gpui hover cursor (the editor wrapper div
/// carries it; set_cursor_style needs a paint-phase hitbox, so an event
/// callback cannot call it directly).
fn cursor_style(cursor: PointerCursor) -> CursorStyle {
    match cursor {
        PointerCursor::Default => CursorStyle::Arrow,
        PointerCursor::Grab => CursorStyle::OpenHand,
        PointerCursor::Grabbing => CursorStyle::ClosedHand,
        PointerCursor::Text => CursorStyle::IBeam,
    }
}

/// One-line warning summary for the warnings bar.
fn warning_text(warning: &rofd_dom::OfdWarning) -> String {
    use rofd_dom::OfdWarning;
    match warning {
        OfdWarning::MissingFeature { feature, .. } => format!("未支持特性：{feature}"),
        OfdWarning::SkippedObject { reason, .. } => format!("已跳过对象：{reason}"),
        OfdWarning::FontSubstituted { requested, used } => {
            format!("字体替换：{requested} → {used}")
        }
        OfdWarning::ResourceNotFound { kind, id } => format!("资源缺失：{kind} {id}"),
    }
}

/// Directory + suggested `.ofd` name for a Save-As prompt: next to the
/// session's file when it has one, else the session name in the current
/// directory.
fn save_prompt_target(session: &DocumentSession) -> (PathBuf, Option<String>) {
    let dir = session
        .path
        .as_ref()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    (dir, Some(format!("{}.ofd", session.file_name)))
}

/// gpui Hsla -> sRGB bytes (for the kernel's Color::Rgb).
fn hsla_to_rgb(color: gpui::Hsla) -> (u8, u8, u8) {
    let rgba = gpui::Rgba::from(color);
    (
        (rgba.r * 255.0) as u8,
        (rgba.g * 255.0) as u8,
        (rgba.b * 255.0) as u8,
    )
}
