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

pub mod wasm_ofd;
#[cfg(target_arch = "wasm32")]
pub mod webgpu_render_target;

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

    wasm_ofd::WasmOfd::new_internal(width, height, render_target)
}
