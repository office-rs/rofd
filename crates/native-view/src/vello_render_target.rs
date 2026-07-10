use rofd_component::RenderTarget;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use winit::window::Window;

/// Owns the wgpu GPU state + vello renderer. Implements [`RenderTarget`] to
/// draw a [`vello::Scene`] to the window's surface.
///
/// # Safety invariant (surface lifetime)
/// The surface is created from the window's raw handles via
/// [`wgpu::Instance::create_surface_unsafe`]. The caller MUST keep the
/// `Window` passed to [`VelloRenderTarget::new`] alive for as long as the
/// `VelloRenderTarget` (and its internal `wgpu::Surface`) lives. Dropping the
/// window before the render target is undefined behavior.
pub struct VelloRenderTarget {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    width: u32,
    height: u32,
}

impl VelloRenderTarget {
    /// Create from a winit window. Seeds the wgpu instance/adapter/device/queue,
    /// creates a surface from the window, configures it, and creates a vello [`Renderer`].
    ///
    /// # Safety
    /// The caller must keep `window` alive for the lifetime of the returned
    /// `VelloRenderTarget`. See the type-level safety invariant.
    pub fn new(window: &Window, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

        // Create a `'static` surface from the window's raw handles. The safety
        // obligation (window outlives the surface) is forwarded to the caller.
        let surface_target = unsafe { wgpu::SurfaceTargetUnsafe::from_window(window) }
            .map_err(|e| format!("failed to read window handles: {e}"))?;
        let surface = unsafe { instance.create_surface_unsafe(surface_target) }
            .map_err(|e| format!("failed to create surface: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no suitable adapter: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rofd device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("device request failed: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: std::num::NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .map_err(|e| format!("vello renderer creation failed: {e}"))?;

        Ok(Self {
            device,
            queue,
            surface,
            config,
            renderer,
            width,
            height,
        })
    }

    /// Reconfigure the surface on window resize. No-ops for zero dimensions
    /// (wgpu panics on zero-sized configurations).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Surface dimensions in physical pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl RenderTarget for VelloRenderTarget {
    fn draw_scene(&mut self, scene: &Scene) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                eprintln!("surface error: {e}");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let _ = self.renderer.render_to_texture(
            &self.device,
            &self.queue,
            scene,
            &view,
            &RenderParams {
                base_color: vello::peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        );
        frame.present();
    }

    fn size(&self) -> (f64, f64) {
        (self.width as f64, self.height as f64)
    }
}
