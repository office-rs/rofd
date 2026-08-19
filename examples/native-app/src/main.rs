//! rofd native app - Xilem + masonry host for the OFD editor.
//!
//! A xilem app whose view is a vertical column: a toolbar (xilem `text_button`s)
//! on top and the OFD canvas (masonry `canvas(...)` widget) filling the rest.
//! The canvas closure calls [`EditorApp::build_scene`] and replays the resulting
//! `imaging::record::Scene` into the canvas widget's scene via `Painter::replay`;
//! masonry's internal imaging_vello backend converts that to vello -> wgpu.
//!
//! Input is routed at the winit layer (not through masonry's widget events):
//! [`WinitEventBridge::translate`] converts winit events to rofd `ViewEvent`s,
//! which are dispatched to the editor. The canvas widget is render-only. This
//! hybrid mirrors reditor's native-app.
//!
//! The "Open" toolbar button pops a native file dialog (`rfd`) and loads the
//! chosen `.ofd`. A `task` view + `MessageProxy` wakes the app after load so the
//! canvas repaints with the new document.
//!
//! The window cursor is declarative: `app_logic` wraps the root in
//! `xilem::window(...).with_options(|o| o.with_cursor(...))`, mapping the
//! component's `PointerCursor` (pushed via `on_pointer_cursor` into shared
//! state) to a `winit::CursorIcon`; xilem diffs and applies it on rebuild.
//!
//! Known limitation (hand tool cursor): xilem@bf81712 re-runs app_logic only
//! on widget Actions, and MasonryState exposes no winit window handle, so the
//! Grabbing cursor cannot refresh mid-drag (Grab/Default update on toolbar
//! clicks). Revisit on the next Linebender stack bump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use masonry_winit::app::{AppDriver, MasonryState, MasonryUserEvent};
use rfd::FileDialog;
use rofd_component::{ContextTarget, PointerCursor, Tool, ViewEvent};
use rofd_dom::{AnnotationId, AnnotationKind, ShapeKind};
use rofd_native_view::{EditorApp, WinitEventBridge};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::window::CursorIcon;

use xilem::core::MessageProxy;
use xilem::kurbo::{Point, Size};
use xilem::masonry::core::WidgetId;
use xilem::masonry::imaging::{kurbo::Rect as KurboRect, peniko::Color, record::Scene, Painter};
use xilem::style::{Padding, Style};
use xilem::view::{canvas, flex_col, flex_row, sized_box, task, text_button, FlexExt};
use xilem::{EventLoop, WidgetView, Xilem};

const BTN_PAD: Padding = Padding::from_vh(0.0, 6.0);

type SharedEditor = Arc<Mutex<EditorApp>>;
type SharedCanvasId = Arc<Mutex<Option<WidgetId>>>;
type SharedWakeProxy = Arc<Mutex<Option<MessageProxy<()>>>>;
/// Right-click target stashed by the `on_context_menu` callback (flag-poll
/// pattern, mirroring `save_requested`). When non-empty, the `window_event`
/// handler deletes the annotation and clears it.
type SharedContextMenu = Arc<Mutex<Option<AnnotationId>>>;
/// Pointer-cursor UI state pushed by the component's `on_pointer_cursor`
/// callback (flag-pattern, like `context_menu_target`). `app_logic` reads it
/// on each rebuild and maps it to the declarative window cursor.
type SharedPointerCursor = Arc<Mutex<PointerCursor>>;

/// Build a toolbar tool button. On click it sets the component's active tool
/// to `tool`. Mirrors the `btn_open`/`btn_save` pattern (text_button + padding
/// + border + corner radius).
fn tool_button(label: &'static str, tool: Tool) -> impl WidgetView<AppState> + use<> {
    text_button(label, move |app: &mut AppState| {
        app.editor.lock().unwrap().component.set_tool(tool.clone());
    })
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0)
}

/// Combined application state for the xilem view.
struct AppState {
    editor: SharedEditor,
    canvas_widget_id: SharedCanvasId,
    wake_proxy: SharedWakeProxy,
    window_id: xilem::WindowId,
    pointer_cursor: SharedPointerCursor,
}

impl xilem::AppState for AppState {
    fn keep_running(&self) -> bool {
        true
    }
}

/// Save the editor's document to `current_file` (overwrite), or prompt a
/// Save As dialog if no file is set. Resets modified on success. Errors
/// print to stderr (v1: no status-bar UI wired).
fn do_save(editor: &mut EditorApp) {
    let bytes = match editor.save_ofd() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] save failed: {e}");
            return;
        }
    };
    if let Some(path) = editor.current_file.clone() {
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("[ERROR] write {}: {}", path.display(), e);
            return;
        }
    } else {
        // New document, no current_file -> Save As dialog.
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("OFD document", &["ofd"])
            .set_file_name("untitled.ofd")
            .save_file()
        {
            if let Err(e) = std::fs::write(&path, &bytes) {
                eprintln!("[ERROR] write {}: {}", path.display(), e);
                return;
            }
            editor.current_file = Some(path);
        } else {
            return; // user cancelled
        }
    }
    editor.component.clear_modified();
}

fn app_logic(app: &mut AppState) -> std::iter::Once<xilem::WindowView<AppState>> {
    let btn_open = text_button("Open", |app: &mut AppState| {
        if let Some(path) = FileDialog::new()
            .add_filter("OFD document", &["ofd"])
            .pick_file()
        {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Err(e) = app.editor.lock().unwrap().open_file(&bytes, path.clone()) {
                        eprintln!("[ERROR] open_file failed for {}: {}", path.display(), e);
                    }
                    // Wake the app so the canvas repaints with the new document.
                    if let Some(proxy) = app.wake_proxy.lock().unwrap().as_ref() {
                        let _ = proxy.message(());
                    }
                }
                Err(e) => eprintln!("[ERROR] read {}: {}", path.display(), e),
            }
        }
    })
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0);

    let btn_save = text_button("Save", |app: &mut AppState| {
        let mut editor = app.editor.lock().unwrap();
        do_save(&mut editor);
    })
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0);

    let file_row =
        flex_row((btn_open, btn_save)).gap(xilem::masonry::layout::Length::const_px(2.0));

    // Tool buttons, WPS-style grouping: the two browse-mode tools
    // (Hand / Text) first, then the annotation create groups. No spring-back:
    // a create tool stays active after each commit (spec §3.3).
    let btn_hand = tool_button("手型", Tool::Hand);
    // WPS "文本" = unified tool: selects annotations AND drag-selects body text.
    let btn_text = tool_button("文本", Tool::Text);
    let group_tools =
        flex_row((btn_hand, btn_text)).gap(xilem::masonry::layout::Length::const_px(2.0));

    let btn_highlight = tool_button("高亮", Tool::Create(AnnotationKind::Highlight));
    let btn_underline = tool_button("下划线", Tool::Create(AnnotationKind::Underline));
    let btn_strikeout = tool_button("删除线", Tool::Create(AnnotationKind::Strikeout));
    let btn_squiggly = tool_button("波浪线", Tool::Create(AnnotationKind::Squiggly));
    let btn_freehand = tool_button("手写", Tool::Create(AnnotationKind::Freehand));
    let btn_rect = tool_button("矩形", Tool::Create(AnnotationKind::Shape(ShapeKind::Rect)));

    // v1 native demo: wide gap between groups instead of a separator widget.
    let tool_row = flex_row((
        group_tools,
        btn_highlight,
        btn_underline,
        btn_strikeout,
        btn_squiggly,
        btn_freehand,
        btn_rect,
    ))
    .gap(xilem::masonry::layout::Length::const_px(8.0));

    let menu_bar = sized_box(flex_col((file_row, tool_row)))
        .padding(Padding::from_vh(2.0, 4.0))
        .background_color(Color::from_rgb8(240, 240, 240));

    // OFD canvas: a masonry Canvas widget whose paint closure builds the editor
    // scene each frame and replays it into the widget's imaging scene.
    let doc_canvas = canvas(|app: &mut AppState, ctx, scene: &mut Scene, size: Size| {
        let mut editor = app.editor.lock().unwrap();
        editor.set_size(size.width, size.height);
        drop(editor);
        *app.canvas_widget_id.lock().unwrap() = Some(ctx.widget_id());

        // Gray desk background (matches RenderEngine's base color).
        let mut painter = Painter::new(scene);
        painter.fill_rect(
            KurboRect::new(0.0, 0.0, size.width, size.height),
            Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
        );

        let mut editor = app.editor.lock().unwrap();
        let doc_scene = editor.build_scene();
        Painter::new(scene).replay(&doc_scene);
    });

    let main_view = flex_col((menu_bar, sized_box(doc_canvas).flex(1.0)));

    // Non-visual task: stash the MessageProxy in AppState so the Open callback
    // can wake xilem to rebuild + repaint after loading a file. The init closure
    // is zero-sized (no captures); it stores the proxy and returns an immortal
    // future to keep the task alive for the view's lifetime.
    let wake_task = task(
        |proxy: MessageProxy<()>, state: &mut AppState| {
            *state.wake_proxy.lock().unwrap() = Some(proxy);
            std::future::pending::<()>()
        },
        |_state: &mut AppState, _: ()| {},
    );

    let root = xilem::core::fork(main_view, wake_task);

    // Declarative window cursor: map the component's PointerCursor (shared
    // state, updated by the on_pointer_cursor callback) to a winit CursorIcon.
    // xilem diffs WindowOptions on each rebuild and calls window.set_cursor.
    let cursor_icon = match *app.pointer_cursor.lock().unwrap() {
        PointerCursor::Default => CursorIcon::Default,
        PointerCursor::Grab => CursorIcon::Grab,
        PointerCursor::Grabbing => CursorIcon::Grabbing,
        PointerCursor::Text => CursorIcon::Text,
    };
    std::iter::once(
        xilem::window(app.window_id, "rofd - OFD Editor", root)
            .with_options(|o| o.with_cursor(cursor_icon)),
    )
}

/// winit `ApplicationHandler` host owning the masonry state + the editor/bridge.
struct NativeApp {
    masonry_state: MasonryState<'static>,
    app_driver: Box<dyn AppDriver>,
    editor: SharedEditor,
    bridge: WinitEventBridge,
    canvas_widget_id: SharedCanvasId,
    save_requested: Arc<AtomicBool>,
    /// Right-click target stashed by `on_context_menu` (flag-poll pattern).
    /// When non-empty, `window_event` deletes the annotation and clears it.
    context_menu_target: SharedContextMenu,
}

impl ApplicationHandler<MasonryUserEvent> for NativeApp {
    fn resumed(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.masonry_state.handle_resumed(el, &mut *self.app_driver);
        // ScaleFactorChanged only fires on DPI changes, not at window creation;
        // seed the bridge from the primary monitor.
        if let Some(monitor) = el.primary_monitor() {
            self.bridge.set_scale_factor(monitor.scale_factor());
        }
    }

    fn suspended(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.masonry_state.handle_suspended(el);
    }

    fn about_to_wait(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.masonry_state.handle_about_to_wait(el);
    }

    fn user_event(&mut self, el: &winit::event_loop::ActiveEventLoop, ev: MasonryUserEvent) {
        self.masonry_state
            .handle_user_event(el, ev, self.app_driver.as_mut());
        // A wake from the Open callback: force the canvas to repaint.
        self.request_canvas_render();
    }

    fn window_event(
        &mut self,
        el: &winit::event_loop::ActiveEventLoop,
        wid: winit::window::WindowId,
        ev: WindowEvent,
    ) {
        // Xilem::new (no ExitOnClose wrapper): handle window close ourselves.
        if matches!(ev, WindowEvent::CloseRequested) {
            el.exit();
        }

        // Refresh the canvas origin (window-logical coords) before any
        // coord-bearing event so the bridge can translate to canvas-local.
        self.update_canvas_origin();

        // Modifiers + scale factor are not handled by translate(); update them
        // directly from their dedicated winit events.
        match &ev {
            WindowEvent::ModifiersChanged(m) => self.bridge.update_modifiers(&m.state()),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.bridge.set_scale_factor(*scale_factor);
            }
            _ => {}
        }

        // IME: winit delivers composition commits as WindowEvent::Ime(Commit).
        // Only Commit is mapped (insert text at the text cursor); Preedit /
        // Enabled / Disabled are ignored in v1 (no inline preedit rendering).
        // Handled here rather than in bridge.translate because the commit text
        // is an owned String extracted from the event.
        if let WindowEvent::Ime(winit::event::Ime::Commit(text)) = &ev {
            let view_event = ViewEvent::Ime { text: text.clone() };
            let mut editor = self.editor.lock().unwrap();
            let outcome = editor.handle_event(&view_event);
            drop(editor);
            if outcome.needs_repaint {
                self.request_canvas_render();
            }
        }

        // Route input to the editor (canvas-local coords). The bridge drops
        // pointer events until the canvas origin is known.
        if let Some(view_event) = self.bridge.translate(&ev) {
            let mut editor = self.editor.lock().unwrap();
            let outcome = editor.handle_event(&view_event);
            drop(editor);
            if outcome.needs_repaint {
                self.request_canvas_render();
            }
        }

        // Ctrl+S routed through component's on_save_request -> flag -> poll here.
        // Polling after handle_event (rather than invoking do_save from inside
        // the callback) sidesteps the EditorApp non-Send + &mut self borrow
        // conflict: the callback only sets a flag, the save runs on the winit
        // thread with a fresh lock.
        if self.save_requested.swap(false, Ordering::SeqCst) {
            let mut editor = self.editor.lock().unwrap();
            do_save(&mut editor);
        }

        // Right-click Delete: the component's on_context_menu callback stashes
        // the right-clicked annotation id (flag-poll, like save_requested).
        // Here we poll, confirm via a Yes/No dialog, then delete. Page/Empty
        // targets are just logged in the callback (no delete). This complements
        // the Delete key (handled in component handle_event, which deletes
        // immediately without confirm).
        let target = self.context_menu_target.lock().unwrap().take();
        if let Some(id) = target {
            // Confirm before deleting (mirrors web-app's confirm() dialog).
            let confirmed = rfd::MessageDialog::new()
                .set_title("Delete")
                .set_description("Delete this annotation?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if confirmed == rfd::MessageDialogResult::Yes {
                let mut editor = self.editor.lock().unwrap();
                editor.component.delete_annotation(&id);
                drop(editor);
                self.request_canvas_render();
            }
        }

        // Forward the original event to masonry (by value). For RedrawRequested
        // masonry paints; the canvas closure rebuilds the scene fresh.
        self.masonry_state
            .handle_window_event(el, wid, ev, self.app_driver.as_mut());
    }

    fn new_events(&mut self, el: &winit::event_loop::ActiveEventLoop, c: winit::event::StartCause) {
        self.masonry_state.handle_new_events(el, c);
    }

    fn exiting(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.masonry_state.handle_exiting(el);
    }

    fn memory_warning(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.masonry_state.handle_memory_warning(el);
    }
}

impl NativeApp {
    /// Query the masonry widget tree for the canvas widget's window-logical
    /// origin and push it into the bridge (so pointer coords become canvas-local).
    #[allow(clippy::never_loop)]
    fn update_canvas_origin(&mut self) {
        let cid = match *self.canvas_widget_id.lock().unwrap() {
            Some(id) => id,
            None => return,
        };
        for root in self.masonry_state.roots() {
            let origin = root.edit_widget(cid, |w| {
                let p = w.ctx.to_window(Point::new(0.0, 0.0));
                (p.x, p.y)
            });
            self.bridge.set_canvas_origin(origin.0, origin.1);
            break;
        }
    }

    /// Mark the canvas widget as needing render so its paint closure re-runs
    /// (rebuilding the scene) on the next masonry paint pass.
    #[allow(clippy::never_loop)]
    fn request_canvas_render(&mut self) {
        let cid = match *self.canvas_widget_id.lock().unwrap() {
            Some(id) => id,
            None => return,
        };
        for root in self.masonry_state.roots() {
            root.edit_widget(cid, |mut w| {
                w.ctx.request_render();
            });
            break;
        }
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    // No bundled default font: FontStore::shape falls back to a generic system
    // family (SansSerif) when no document/default font is resolved, and
    // fontique's script fallback covers CJK. So the native app renders text
    // using the system's installed fonts - no font file needed.
    let default_font_bytes: Arc<Vec<u8>> = Arc::new(vec![]);
    let mut editor = EditorApp::new(rofd_component::EditorConfig::new(default_font_bytes));
    editor.set_clock("rofd".into(), 0);

    // Load file from command-line arg if provided.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        match std::fs::read(&args[1]) {
            Ok(bytes) => {
                if let Err(e) = editor.open_file(&bytes, std::path::PathBuf::from(&args[1])) {
                    eprintln!("failed to load {}: {}", args[1], e);
                }
            }
            Err(e) => eprintln!("failed to read {}: {}", args[1], e),
        }
    }

    // EditorApp holds a `Rc<RefCell<parley::FontContext>>` (via FontStore), so it
    // is not `Send`; the app is single-threaded, but `Arc<Mutex>` mirrors
    // reditor's pattern (shared between the xilem view closures and the winit
    // ApplicationHandler).
    #[allow(clippy::arc_with_non_send_sync)]
    let editor = Arc::new(Mutex::new(editor));
    let canvas_widget_id: SharedCanvasId = Arc::new(Mutex::new(None));
    let wake_proxy: SharedWakeProxy = Arc::new(Mutex::new(None));

    // Ctrl+S: the component fires on_save_request when the user presses the
    // shortcut. The callback only sets an AtomicBool flag (Send + 'static);
    // the NativeApp's window_event handler polls it and runs do_save on the
    // winit thread. This avoids re-entrancy / &mut self borrow issues that
    // would arise from doing the save directly inside the callback.
    let save_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    {
        let flag = save_requested.clone();
        editor.lock().unwrap().component.on_save_request(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    // Right-click: the component fires on_context_menu on PointerDown Right
    // (T4). The callback only stashes the target annotation id into a shared
    // mutex (flag-poll, like save_requested); the NativeApp's window_event
    // handler polls it and runs delete_annotation. Page/Empty targets are
    // logged to stderr (no delete). This avoids re-entrancy / &mut self
    // borrow issues that would arise from deleting inside the callback.
    let context_menu_target: SharedContextMenu = Arc::new(Mutex::new(None));
    {
        let target = context_menu_target.clone();
        editor
            .lock()
            .unwrap()
            .component
            .on_context_menu(move |point, ctx| match ctx {
                ContextTarget::Annotation(id) => {
                    eprintln!(
                        "[context-menu] right-click on annotation {} at ({:.0}, {:.0}) -> Delete",
                        id.0, point.0, point.1
                    );
                    *target.lock().unwrap() = Some(id);
                }
                ContextTarget::Page => {
                    eprintln!(
                        "[context-menu] right-click on page at ({:.0}, {:.0}) (no action)",
                        point.0, point.1
                    );
                }
                ContextTarget::Empty => {
                    eprintln!(
                        "[context-menu] right-click on desk at ({:.0}, {:.0}) (no action)",
                        point.0, point.1
                    );
                }
            });
    }

    // Pointer cursor: the component fires on_pointer_cursor whenever the
    // hover/tool state changes the desired cursor (e.g. Hand tool -> Grab).
    // The callback only stashes the value into shared state (flag-pattern,
    // like save_requested); app_logic reads it on rebuild and applies it as
    // the declarative window cursor.
    let window_id = xilem::WindowId::next();
    let pointer_cursor: SharedPointerCursor = Arc::new(Mutex::new(PointerCursor::Default));
    {
        let pc = pointer_cursor.clone();
        editor
            .lock()
            .unwrap()
            .component
            .on_pointer_cursor(move |c| *pc.lock().unwrap() = c);
    }

    let app_state = AppState {
        editor: editor.clone(),
        canvas_widget_id: canvas_widget_id.clone(),
        wake_proxy: wake_proxy.clone(),
        window_id,
        pointer_cursor: pointer_cursor.clone(),
    };

    // Xilem::new takes an app_logic returning a window iterator (vs
    // new_simple's single root + WindowOptions) and brings its own tokio
    // runtime, so the `task` view keeps working. It does NOT wrap the state
    // in ExitOnClose, so window close is handled in NativeApp::window_event.
    let xilem = Xilem::new(app_state, app_logic);

    let event_loop = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let (driver, windows) =
        xilem.into_driver_and_windows(move |event| proxy.send_event(event).map_err(|err| err.0));
    let masonry_state = MasonryState::new(
        event_loop.create_proxy(),
        windows,
        xilem::masonry::theme::default_property_set(),
    );

    let mut app = NativeApp {
        masonry_state,
        app_driver: Box::new(driver),
        editor,
        bridge: WinitEventBridge::new(),
        canvas_widget_id,
        save_requested,
        context_menu_target,
    };

    event_loop.run_app(&mut app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::OfdDocument;

    #[test]
    fn do_save_writes_to_current_file_and_clears_modified() {
        let mut app = EditorApp::new(rofd_component::EditorConfig::new(Arc::new(vec![])));
        // load a package (sets package)
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        app.load_ofd(&bytes).unwrap();
        // set current_file to a temp file
        let tmp = std::env::temp_dir().join(format!("rofd_c2_test_{}.ofd", std::process::id()));
        app.current_file = Some(tmp.clone());
        // make an annotation edit to set modified
        app.set_clock("t".into(), 1);
        app.component.create_annotation(
            rofd_dom::AnnotationKind::Note,
            rofd_dom::PageId::new("1"),
            rofd_dom::AnnotationPayload::Note {
                rect: rofd_dom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 5.0,
                    h: 5.0,
                },
                color: rofd_dom::Color::Rgb(0, 0, 0),
                content: "x".into(),
                icon: rofd_dom::NoteIcon::Note,
            },
        );
        assert!(app.is_modified());
        do_save(&mut app);
        assert!(tmp.exists(), "file written");
        assert!(!app.is_modified(), "clear_modified after save");
        // written file re-parses
        let written = std::fs::read(&tmp).unwrap();
        rofd_io::parse_ofd(&written).expect("written file re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
