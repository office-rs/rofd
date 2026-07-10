//! rofd_web_view - WASM/WebGPU adapter for rofd.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
