//! rofd_web_view - WASM/WebGPU adapter for rofd.
//!
//! This crate is only meaningful when compiled for `wasm32-unknown-unknown`.
//! The WebGPU render target (canvas -> wgpu surface -> vello renderer) is
//! gated behind `cfg(target_arch = "wasm32")` because it uses
//! `wgpu::SurfaceTarget::Canvas`, which only exists under wgpu's `web` cfg.
//!
//! [`wasm_editor::parse_key`] and its tests are **not** cfg-gated - they are
//! pure Rust and run on native (`cargo test -p rofd-web-view`), giving TDD
//! coverage for the JS key-string -> [`rofd_component::Key`] mapping without
//! needing a browser.

pub mod wasm_editor;
#[cfg(target_arch = "wasm32")]
pub mod webgpu_render_target;

#[cfg(target_arch = "wasm32")]
pub use wasm_editor::WasmEditor;
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
