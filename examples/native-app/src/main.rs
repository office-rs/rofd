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

use std::sync::{Arc, Mutex};

use masonry_winit::app::{AppDriver, MasonryState, MasonryUserEvent};
use rfd::FileDialog;
use rofd_native_view::{EditorApp, WinitEventBridge};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;

use xilem::core::MessageProxy;
use xilem::kurbo::{Point, Size};
use xilem::masonry::imaging::{kurbo::Rect as KurboRect, peniko::Color, record::Scene, Painter};
use xilem::masonry::core::WidgetId;
use xilem::style::{Padding, Style};
use xilem::view::{canvas, flex_col, flex_row, sized_box, task, text_button, FlexExt};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

const BTN_PAD: Padding = Padding::from_vh(0.0, 6.0);

type SharedEditor = Arc<Mutex<EditorApp>>;
type SharedCanvasId = Arc<Mutex<Option<WidgetId>>>;
type SharedWakeProxy = Arc<Mutex<Option<MessageProxy<()>>>>;

/// Combined application state for the xilem view.
struct AppState {
    editor: SharedEditor,
    canvas_widget_id: SharedCanvasId,
    wake_proxy: SharedWakeProxy,
}

fn app_logic(_app: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let btn_open = text_button("Open", |app: &mut AppState| {
        if let Some(path) = FileDialog::new().add_filter("OFD document", &["ofd"]).pick_file() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Err(e) = app.editor.lock().unwrap().load_ofd(&bytes) {
                        eprintln!("[ERROR] load_ofd failed for {}: {}", path.display(), e);
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

    let menu_bar = sized_box(flex_row((btn_open,)).gap(xilem::masonry::layout::Length::const_px(2.0)))
        .padding(Padding::from_vh(2.0, 4.0))
        .background_color(Color::from_rgb8(240, 240, 240));

    // OFD canvas: a masonry Canvas widget whose paint closure builds the editor
    // scene each frame and replays it into the widget's imaging scene.
    let doc_canvas = canvas(
        |app: &mut AppState, ctx, scene: &mut Scene, size: Size| {
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
        },
    );

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

    xilem::core::fork(main_view, wake_task)
}

/// winit `ApplicationHandler` host owning the masonry state + the editor/bridge.
struct NativeApp {
    masonry_state: MasonryState<'static>,
    app_driver: Box<dyn AppDriver>,
    editor: SharedEditor,
    bridge: WinitEventBridge,
    canvas_widget_id: SharedCanvasId,
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
        self.masonry_state.handle_user_event(el, ev, self.app_driver.as_mut());
        // A wake from the Open callback: force the canvas to repaint.
        self.request_canvas_render();
    }

    fn window_event(
        &mut self,
        el: &winit::event_loop::ActiveEventLoop,
        wid: winit::window::WindowId,
        ev: WindowEvent,
    ) {
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

/// Load a default CJK font (NotoSansSC) from a few candidate paths relative to
/// the current working directory. Returns an empty `Vec` (no glyphs) if none
/// found, logging a warning. The font is downloaded by the web-app's
/// `fetch:font` script into `examples/web-app/public/`.
fn load_default_font() -> Arc<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "examples/web-app/public/NotoSansSC-Regular.otf",
        "../examples/web-app/public/NotoSansSC-Regular.otf",
        "NotoSansSC-Regular.otf",
    ];
    for path in CANDIDATES {
        match std::fs::read(path) {
            Ok(bytes) => {
                eprintln!("[font] loaded {} ({} bytes)", path, bytes.len());
                return Arc::new(bytes);
            }
            Err(_) => continue,
        }
    }
    eprintln!(
        "[font] no default font found at {:?}; text won't render. Run `npm run fetch:font` in examples/web-app first.",
        CANDIDATES
    );
    Arc::new(vec![])
}

fn main() -> Result<(), winit::error::EventLoopError> {
    // Load a default font so text shapes into glyphs. The OFD has no embedded
    // fonts, so without this text won't render. NotoSansSC is the same CJK font
    // the web app uses (downloaded by `npm run fetch:font` into
    // examples/web-app/public/). Try a few candidate paths relative to CWD.
    let default_font_bytes = load_default_font();
    let mut editor = EditorApp::new(rofd_component::EditorConfig::new(default_font_bytes));
    editor.set_clock("rofd".into(), 0);

    // Load file from command-line arg if provided.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        match std::fs::read(&args[1]) {
            Ok(bytes) => {
                if let Err(e) = editor.load_ofd(&bytes) {
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

    let app_state = AppState {
        editor: editor.clone(),
        canvas_widget_id: canvas_widget_id.clone(),
        wake_proxy: wake_proxy.clone(),
    };

    let window_options = WindowOptions::new("rofd - OFD Editor");
    let xilem = Xilem::new_simple(app_state, app_logic, window_options);

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
    };

    event_loop.run_app(&mut app)
}
