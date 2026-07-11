use rofd_component::RenderTarget;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use wgpu::util::TextureBlitter;
use wgpu::TextureFormat;
use winit::window::Window;

/// Owns the wgpu GPU state + vello renderer. Implements [`RenderTarget`] to
/// draw a [`vello::Scene`] to the window's surface.
///
/// # Render path (intermediate texture + blit)
/// Vello renders with a compute pipeline that writes to a storage image. Most
/// surface textures cannot be bound as storage (notably all sRGB formats, which
/// is the default swap-chain format on Windows), so vello cannot render directly
/// to the surface. Instead we render into an intermediate `Rgba8Unorm` texture
/// (the format vello requires, with `STORAGE_BINDING`), then blit it to the
/// surface with [`TextureBlitter`]. This mirrors vello's own `RenderSurface`
/// in `vello::util`.
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
    /// Intermediate render target vello writes to (Rgba8Unorm + STORAGE_BINDING).
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    /// Copies [`target_view`] -> surface view. Built for the surface format.
    blitter: TextureBlitter,
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
        // Prefer a non-sRGB format: sRGB formats cannot be storage-bound, and
        // vello's Rgba8Unorm output is already sRGB-encoded, so an sRGB surface
        // would double-apply gamma. Fall back to the surface's first supported
        // format if neither canonical option is available (the blitter handles
        // the conversion).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm))
            .or_else(|| caps.formats.first().copied())
            .unwrap_or(TextureFormat::Bgra8Unorm);
        let config = wgpu::SurfaceConfiguration {
            // The surface texture is only used as the blitter's render target,
            // so it needs RENDER_ATTACHMENT only (no STORAGE_BINDING).
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let (target_texture, target_view) = create_render_target(&device, width, height);
        let blitter = TextureBlitter::new(&device, format);

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
            target_texture,
            target_view,
            blitter,
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
        // Recreate the intermediate target at the new size.
        let (texture, view) = create_render_target(&self.device, width, height);
        self.target_texture = texture;
        self.target_view = view;
    }

    /// Surface dimensions in physical pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Create the intermediate `Rgba8Unorm` texture vello renders into.
///
/// `STORAGE_BINDING` is required because vello writes via a compute pipeline;
/// `TEXTURE_BINDING` lets the blitter sample it when copying to the surface.
fn create_render_target(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rofd vello target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        format: TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
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
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 1) Vello renders the scene into the intermediate Rgba8Unorm storage
        //    texture. render_to_texture submits its own command buffer.
        if let Err(e) = self.renderer.render_to_texture(
            &self.device,
            &self.queue,
            scene,
            &self.target_view,
            &RenderParams {
                base_color: vello::peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            eprintln!("vello render failed: {e}");
        }

        // 2) Blit the intermediate texture to the surface texture, then present.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rofd blit") });
        self.blitter
            .copy(&self.device, &mut encoder, &self.target_view, &surface_view);
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    fn size(&self) -> (f64, f64) {
        (self.width as f64, self.height as f64)
    }
}
