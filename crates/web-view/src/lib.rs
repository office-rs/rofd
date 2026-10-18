//! rofd_web_view - WASM/WebGPU adapter for rofd.
//!
//! Mirrors reditor's web-view: a `create_wasm_ofd` async factory handles
//! WebGPU init + warmup, and `WasmOfd` exposes `register_font`, event
//! handlers, and a JS callback bridge. The web SDK (`@office-rs/rofd`) drives the
//! full boot flow (font loading, event binding, render loop).
//!
//! Only meaningful when compiled for `wasm32-unknown-unknown`. The
//! [`wasm_ofd::parse_key`] helper and its tests are NOT cfg-gated - they are
//! pure Rust and run on native, giving TDD coverage for the JS key-string ->
//! [`rofd_component::Key`] mapping without needing a browser.

#[cfg(target_arch = "wasm32")]
pub mod cpu_canvas_render_target;
pub mod raster_cap;
pub mod wasm_ofd;
#[cfg(target_arch = "wasm32")]
pub mod webgpu_render_target;
pub mod zoom_gesture;

#[cfg(target_arch = "wasm32")]
pub use cpu_canvas_render_target::{CpuCanvasRenderTarget, CpuTuning};
#[cfg(target_arch = "wasm32")]
pub use wasm_ofd::WasmOfd;
#[cfg(target_arch = "wasm32")]
pub use webgpu_render_target::WebGpuRenderTarget;

#[cfg(target_arch = "wasm32")]
mod startup {
    use wasm_bindgen::prelude::*;

    /// Entry point invoked by wasm-bindgen when the module is instantiated.
    /// Installs the panic hook so Rust panics surface in the browser console.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }
}

#[cfg(target_arch = "wasm32")]
pub use startup::start;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Create a new `WasmOfd` instance with its own WebGPU context.
///
/// Async because `WebGpuRenderTarget::new` requests a wgpu adapter + device
/// (browser WebGPU Promises). Runs a warmup render to force shader compilation
/// before the first user-visible frame. Fonts are NOT loaded here - the SDK
/// calls [`WasmOfd::register_font`] after this returns (mirrors reditor).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn create_wasm_ofd(
    canvas: web_sys::HtmlCanvasElement,
) -> Result<wasm_ofd::WasmOfd, JsValue> {
    let rect = canvas.get_bounding_client_rect();
    let width = rect.width() as u32;
    let height = rect.height() as u32;

    let mut render_target = WebGpuRenderTarget::new(canvas, width, height)
        .await
        .map_err(|e| JsValue::from_str(&e))?;
    render_target.warmup();

    wasm_ofd::WasmOfd::new_internal(
        width,
        height,
        wasm_ofd::RenderTargetKind::WebGpu(Box::new(render_target)),
    )
}

/// CPU 回退工厂：WebGPU 不可用时 SDK 走这里（spec 2026-10-18-no-gpu-support
/// §9）。无 warmup（无 shader 编译），首帧即栅格。调优参数：`max_cached_pages`
/// 页位图 LRU 容量（钳 ≥1），`max_page_pixels` 单页物理像素上限（≤0 取
/// 8_000_000 默认）。创建出的实例 `isCpuFallback()` 恒为 true。
///
/// Async 仅为与 [`create_wasm_ofd`] 的 JS Promise 形态一致（无 await 点）。
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn create_wasm_ofd_cpu(
    canvas: web_sys::HtmlCanvasElement,
    max_cached_pages: u32,
    max_page_pixels: f64,
) -> Result<wasm_ofd::WasmOfd, JsValue> {
    let rect = canvas.get_bounding_client_rect();
    let tuning = cpu_canvas_render_target::CpuTuning {
        max_cached_pages: max_cached_pages.max(1) as usize,
        max_page_pixels: if max_page_pixels > 0.0 {
            max_page_pixels as u64
        } else {
            8_000_000
        },
    };
    let render_target = cpu_canvas_render_target::CpuCanvasRenderTarget::new(
        canvas,
        rect.width() as u32,
        rect.height() as u32,
        tuning,
    )
    .map_err(|e| JsValue::from_str(&e))?;

    wasm_ofd::WasmOfd::new_internal(
        rect.width() as u32,
        rect.height() as u32,
        wasm_ofd::RenderTargetKind::Cpu(render_target),
    )
}
