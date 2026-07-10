use std::path::PathBuf;
use std::sync::Arc;

use rofd_component::EditorConfig;
use rofd_native_view::{EditorApp, VelloRenderTarget, WinitEventBridge};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// winit 0.30 ApplicationHandler host for the rofd native editor.
///
/// Owns the framework-agnostic [`EditorApp`] plus the native (winit/wgpu/vello)
/// plumbing: the window, the [`VelloRenderTarget`], and the [`WinitEventBridge`].
///
/// # Field drop order
/// `render_target` is declared before `window` so that, on drop, the wgpu
/// `Surface` (created from the window's raw handles and stored inside
/// `VelloRenderTarget`) is released *before* the `Window` is destroyed. This
/// honors the `Surface<'static>` safety invariant of [`VelloRenderTarget::new`].
struct NativeApp {
    app: EditorApp,
    render_target: Option<VelloRenderTarget>,
    bridge: WinitEventBridge,
    window: Option<Window>,
    #[allow(dead_code)]
    default_font_bytes: Arc<Vec<u8>>,
}

impl ApplicationHandler for NativeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = event_loop
            .create_window(
                winit::window::WindowAttributes::default()
                    .with_title("rofd - OFD Editor")
                    .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0)),
            )
            .expect("failed to create window");

        // Seed scale factor from the primary monitor (ScaleFactorChanged only fires on changes).
        if let Some(monitor) = event_loop.primary_monitor() {
            self.bridge.set_scale_factor(monitor.scale_factor());
        }

        let size = window.inner_size();
        let render_target = VelloRenderTarget::new(&window, size.width, size.height)
            .expect("failed to create render target");
        self.render_target = Some(render_target);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            // winit 0.30: ModifiersChanged holds `Modifiers`, whose `.state()`
            // yields the `ModifiersState` that WinitEventBridge::update_modifiers expects.
            WindowEvent::ModifiersChanged(modifiers) => {
                self.bridge.update_modifiers(&modifiers.state());
                return;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.bridge.set_scale_factor(*scale_factor);
                return;
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(rt) = &mut self.render_target {
                    rt.resize(physical_size.width, physical_size.height);
                }
                let (w, h) = (
                    physical_size.width as f64 / self.bridge.scale_factor,
                    physical_size.height as f64 / self.bridge.scale_factor,
                );
                self.app.handle_event(&rofd_component::ViewEvent::Resize { width: w, height: h });
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return;
            }
            WindowEvent::RedrawRequested => {
                if let Some(rt) = &mut self.render_target {
                    self.app.render(rt);
                }
                return;
            }
            _ => {}
        }

        // Translate winit event -> ViewEvent -> EditorApp.
        if let Some(view_event) = self.bridge.translate(&event) {
            let outcome = self.app.handle_event(&view_event);
            if outcome.needs_repaint {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: StartCause) {}

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

fn main() {
    // v1: empty default font bytes. The render engine's font fallback will produce
    // no glyphs for text - acceptable for a first runnable version. To render text,
    // the host must register a real font (e.g. DejaVuSans.ttf) via EditorConfig.
    let default_font_bytes: Arc<Vec<u8>> = Arc::new(vec![]);

    let mut app = EditorApp::new(EditorConfig::new(default_font_bytes.clone()));
    app.set_clock("rofd".into(), 0);

    // Load file from command-line arg if provided.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        let path = PathBuf::from(&args[1]);
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(e) = app.load_ofd(&bytes) {
                    eprintln!("failed to load {}: {}", path.display(), e);
                }
            }
            Err(e) => eprintln!("failed to read {}: {}", path.display(), e),
        }
    }

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut native_app = NativeApp {
        app,
        render_target: None,
        bridge: WinitEventBridge::new(),
        window: None,
        default_font_bytes,
    };
    event_loop.run_app(&mut native_app).expect("event loop error");
}
