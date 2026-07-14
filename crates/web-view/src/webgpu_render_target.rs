//! WebGPU render target for browser rendering.
//!
//! Manages wgpu Device/Queue, Vello [`Renderer`], surface, and a blit pipeline
//! to render Vello output to the browser canvas.
//!
//! # Render path (intermediate texture + blit)
//! Vello renders with a compute pipeline that writes to a storage image. Surface
//! textures on WebGPU typically cannot be bound as storage, so vello cannot
//! render directly to the surface. Instead we render into an intermediate
//! `Rgba8Unorm` texture (the format vello requires, with `STORAGE_BINDING`),
//! then blit it to the surface with [`wgpu::util::TextureBlitter`]. This mirrors
//! vello's own `RenderSurface` in `vello::util` and the native-view's
//! [`VelloRenderTarget`](../../native_view/vello_render_target/struct.VelloRenderTarget.html).

use imaging::kurbo::Rect as KurboRect;
use imaging::record::Scene;
use imaging_vello::VelloSceneSink;
use rofd_component::RenderTarget;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions};
use wgpu::util::TextureBlitter;
use wgpu::TextureFormat;

/// Render target that draws to an HTML canvas via WebGPU + Vello.
///
/// Owns the wgpu GPU state (device/queue/surface) and a Vello renderer.
/// Implements [`RenderTarget`] so the editor component can blit a
/// [`vello::Scene`] without knowing it runs in a browser.
pub struct WebGpuRenderTarget {
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

impl WebGpuRenderTarget {
    /// Initialize WebGPU and create the render target.
    ///
    /// This is async because `request_adapter` and `request_device` are
    /// async on wasm32 (they call browser WebGPU Promises).
    pub async fn new(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("failed to create surface: {e:?}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no suitable adapter: {e:?}"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rofd device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
            })
            .await
            .map_err(|e| format!("device request failed: {e:?}"))?;

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
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
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
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(|e| format!("vello renderer creation failed: {e:?}"))?;

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

    /// Reconfigure the surface on canvas resize. No-ops for zero dimensions
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

    /// Do a warmup render to force shader compilation.
    pub fn warmup(&mut self) {
        let mut vello_scene = vello::Scene::new();
        vello_scene.fill(
            vello::peniko::Fill::NonZero,
            vello::kurbo::Affine::IDENTITY,
            vello::peniko::Color::TRANSPARENT,
            None,
            &vello::kurbo::Rect::ZERO,
        );
        self.render_vello_scene(&vello_scene);
    }

    /// Render a [`vello::Scene`] to the surface via Vello + blit.
    fn render_vello_scene(&mut self, vello_scene: &vello::Scene) {
        // 1) Vello renders the scene into the intermediate Rgba8Unorm storage
        //    texture. render_to_texture submits its own command buffer.
        if let Err(e) = self.renderer.render_to_texture(
            &self.device,
            &self.queue,
            vello_scene,
            &self.target_view,
            &RenderParams {
                base_color: vello::peniko::Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF),
                width: self.width,
                height: self.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            web_sys::console::error_1(&format!("vello render failed: {e:?}").into());
            return;
        }

        // 2) Blit the intermediate texture to the surface texture, then present.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                web_sys::console::warn_1(&format!("surface error: {e:?}").into());
                return;
            }
        };
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rofd blit"),
            });
        self.blitter
            .copy(&self.device, &mut encoder, &self.target_view, &surface_view);
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}

impl RenderTarget for WebGpuRenderTarget {
    fn draw_scene(&mut self, scene: &Scene) {
        // Convert the backend-agnostic imaging scene to a vello::Scene via
        // VelloSceneSink, then render via the existing vello + blit path.
        let mut vello_scene = vello::Scene::new();
        let bounds = KurboRect::new(0.0, 0.0, self.width as f64, self.height as f64);
        let mut sink = VelloSceneSink::new(&mut vello_scene, bounds);
        imaging::record::replay(scene, &mut sink);
        let _ = sink.finish();
        self.render_vello_scene(&vello_scene);
    }

    fn size(&self) -> (f64, f64) {
        (self.width as f64, self.height as f64)
    }
}

/// Create the intermediate `Rgba8Unorm` texture vello renders into.
///
/// `STORAGE_BINDING` is required because vello writes via a compute pipeline;
/// `TEXTURE_BINDING` lets the blitter sample it when copying to the surface.
fn create_render_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rofd vello target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
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
