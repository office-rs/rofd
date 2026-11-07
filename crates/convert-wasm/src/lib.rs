//! rofd-convert-wasm: browser surface over the rofd-convert session
//! kernel (convert spec §6.7, phase 4). wasm-bindgen, --target web; no
//! WebGPU / Canvas dependency — the page-scene → krilla encode pipeline
//! runs anywhere, Web Worker included (recommended: zero main-thread
//! blocking).
//!
//! Fonts are EXPLICIT (spec §6.7): no system discovery in the browser —
//! `registerFont(bytes)` is the only door (same model as the servers).
//!
//! Instance semantics: one lazily-created Converter is kept warm across
//! converts; a successful registration drops it so the next convert
//! snapshots the fresh registry — slightly more permissive than the
//! native "an instance sees only earlier registrations" (D9), noted
//! here deliberately. JS is single-threaded per realm, so no lease is
//! needed (same ruling as the Node binding).
//!
//! Panic posture: the wasm target builds with `panic = abort`, where
//! `catch_panics` cannot intercept — the guard compiles to no
//! behavioral difference (the phase-1 note); errors still surface as
//! JS errors via the kernel's Result.

use wasm_bindgen::prelude::*;

static CONVERTER: std::sync::LazyLock<std::sync::Mutex<Option<rofd_convert::Converter>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

fn lock_converter() -> std::sync::MutexGuard<'static, Option<rofd_convert::Converter>> {
    CONVERTER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn take_converter() -> rofd_convert::Converter {
    lock_converter().take().unwrap_or_default()
}

fn put_converter(c: rofd_convert::Converter) {
    *lock_converter() = Some(c);
}

/// Warnings surface as Display lines (ConvertWarning vocabulary — parse
/// warnings aggregated per kind, then encoder degradation lines).
fn warnings_to_strings(warnings: Vec<rofd_convert::ConvertWarning>) -> Vec<String> {
    warnings.iter().map(|w| w.to_string()).collect()
}

/// One conversion's result.
#[wasm_bindgen]
pub struct ConvertResult {
    bytes: js_sys::Uint8Array,
    warnings: Vec<String>,
}

#[wasm_bindgen]
impl ConvertResult {
    /// PDF bytes (a copy per call — hold the result, don't re-read).
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> js_sys::Uint8Array {
        self.bytes.clone()
    }
    /// Non-fatal kernel warnings of this conversion.
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.clone()
    }
}

/// Register font bytes into the process registry (idempotent on identical
/// bytes; unparsable bytes return false). Invalidates the warm converter
/// so the next convert sees them.
#[wasm_bindgen(js_name = "registerFont")]
pub fn register_font(bytes: &[u8]) -> bool {
    let ok = rofd_convert::register_font(bytes.to_vec()).is_ok();
    if ok {
        // Drop the warm instance: its snapshot predates this registration.
        lock_converter().take();
    }
    ok
}

/// Anchor the process default font (kernel D19); a later anchor wins.
/// Unparsable bytes return false. Same warm-instance invalidation.
#[wasm_bindgen(js_name = "registerDefaultFont")]
pub fn register_default_font(bytes: &[u8]) -> bool {
    let ok = rofd_convert::register_default_font(bytes.to_vec()).is_ok();
    if ok {
        lock_converter().take();
    }
    ok
}

/// Whether a default font is anchored in the registry.
#[wasm_bindgen(js_name = "hasDefaultFont")]
pub fn has_default_font() -> bool {
    rofd_convert::has_default_font()
}

/// Font families visible to the registry (registered only — the browser
/// has no system fonts).
#[wasm_bindgen(js_name = "registeredFamilies")]
pub fn registered_families() -> Vec<String> {
    rofd_convert::registered_families()
}

/// Convert OFD bytes to PDF (kernel-default options: all content layers
/// on, default budgets). Errors surface as a JS Error.
#[wasm_bindgen(js_name = "convertToPdf")]
pub fn convert_to_pdf(ofd: &[u8]) -> Result<ConvertResult, JsValue> {
    let mut converter = take_converter();
    let r = rofd_convert::catch_panics(|| {
        converter.convert(
            rofd_convert::OutputFormat::Pdf,
            ofd,
            &rofd_convert::ConvertOptions::default(),
        )
    });
    put_converter(converter);
    match r {
        Ok(out) => Ok(ConvertResult {
            bytes: js_sys::Uint8Array::from(&out.bytes[..]),
            warnings: warnings_to_strings(out.warnings),
        }),
        Err(e) => Err(js_sys::Error::new(&e.to_string()).into()),
    }
}

/// Package version (kept in sync with Cargo.toml).
#[wasm_bindgen(js_name = "version")]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Warnings surface as Display strings (parse + encode vocabulary).
    #[test]
    fn warnings_to_strings_uses_display() {
        let ws = vec![
            rofd_convert::ConvertWarning::Parse(rofd_convert::OfdWarning::MissingFeature {
                feature: "Template".into(),
                entry: "e".into(),
            }),
            rofd_convert::ConvertWarning::Encode("skipped font subsetting".into()),
        ];
        let s = warnings_to_strings(ws);
        assert!(s[0].contains("missing feature"), "{s:?}");
        assert_eq!(s[1], "skipped font subsetting");
    }
}
