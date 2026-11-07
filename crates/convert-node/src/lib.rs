//! rofd-convert-node: napi-rs surface over the rofd-convert session kernel
//! (convert spec §6.5, D3/D11/D14). The built cdylib is loaded by Node as a
//! `.node` addon (on Windows the DLL is renamed; see demo/run.js).
//!
//! No lease/handle table here — unlike the JNI binding, JS execution is
//! single-threaded per instance by construction: each worker_threads worker
//! (the D11 pool shape) builds its own Converter in its own realm, and the
//! main thread's instance is only touched from the event loop. The panic
//! boundary still applies: a panic inside a napi export would abort the
//! Node process, so every kernel call runs under `catch_panics` and
//! surfaces as a JS exception instead.

use napi::bindgen_prelude::*;
use napi::{Env, Error, Status};
use napi_derive::napi;
use rofd_convert::{ConvertOptions, OutputFormat};

/// Map the kernel's registry result onto the JS boolean contract (same as
/// the JNI binding): Ok -> true, unparsable font -> false (no throw),
/// anything else -> JS error.
fn font_reg_result(r: std::result::Result<(), rofd_convert::FontRegisterError>) -> Result<bool> {
    match r {
        Ok(()) => Ok(true),
        Err(rofd_convert::FontRegisterError::InvalidFont) => Ok(false),
        Err(e) => Err(Error::new(
            Status::GenericFailure,
            format!("font registration: {e}"),
        )),
    }
}

/// Register font bytes into the process-level registry (append-only,
/// process-wide, idempotent on identical bytes).
#[napi(js_name = "registerFont")]
pub fn register_font(bytes: Buffer) -> Result<bool> {
    font_reg_result(rofd_convert::register_font(bytes.as_ref().to_vec()))
}

/// Register a font file into the process-level registry. I/O failure
/// throws; an unparsable font returns false (same as the kernel).
#[napi(js_name = "registerFontFile")]
pub fn register_font_file(path: String) -> Result<bool> {
    font_reg_result(rofd_convert::register_font_file(&path))
}

/// Anchor the process default font (kernel D19); a later anchor wins.
/// Unparsable bytes return false (same contract as registerFont).
#[napi(js_name = "registerDefaultFont")]
pub fn register_default_font(bytes: Buffer) -> Result<bool> {
    font_reg_result(rofd_convert::register_default_font(bytes.as_ref().to_vec()))
}

/// Whether a default font is anchored in the process registry.
#[napi(js_name = "hasDefaultFont")]
pub fn has_default_font() -> bool {
    rofd_convert::has_default_font()
}

/// Font families visible to the process registry (registrations only;
/// system-discovered families are never listed).
#[napi(js_name = "registeredFamilies")]
pub fn registered_families() -> Vec<String> {
    rofd_convert::registered_families()
}

/// One conversion's result: PDF bytes plus non-fatal warning lines
/// (ConvertWarning's Display: parse warnings — aggregated per kind —
/// and encoder degradation lines).
#[napi(object)]
pub struct ConvertOutput {
    pub bytes: Buffer,
    pub warnings: Vec<String>,
}

/// Per-call options; every field optional — `None` (or a non-positive
/// budget) means the kernel default (all content layers on, D17;
/// 10000 pages / 1 GiB media).
#[napi(object)]
pub struct ConvertOptionsJs {
    pub watermark: Option<bool>,
    pub annotations: Option<bool>,
    pub seal: Option<bool>,
    pub max_pages: Option<f64>,
    pub max_media_bytes: Option<f64>,
}

/// Map the JS options object onto the kernel's ConvertOptions. `None`
/// content flags fall back to the kernel default (all on); non-positive
/// or fractional-negative budgets fall back to the kernel limits.
fn kernel_options(o: Option<&ConvertOptionsJs>) -> ConvertOptions {
    let Some(o) = o else {
        return ConvertOptions::default();
    };
    let mut opts = ConvertOptions::default();
    if let Some(w) = o.watermark {
        opts.content.watermark = w;
    }
    if let Some(a) = o.annotations {
        opts.content.annotations = a;
    }
    if let Some(s) = o.seal {
        opts.content.seal = s;
    }
    opts.max_pages = number_to_opt_u32(o.max_pages);
    opts.max_media_bytes = number_to_opt_usize(o.max_media_bytes);
    opts
}

/// JS number -> optional u32 budget: `None`/<=0/fractional/overflow all
/// mean "kernel default" (never an error — the CLI contract maps the
/// same way at its boundary).
fn number_to_opt_u32(v: Option<f64>) -> Option<u32> {
    let v = v?;
    if !v.is_finite() || v <= 0.0 || v.fract() != 0.0 {
        return None;
    }
    u32::try_from(v as u64).ok()
}

fn number_to_opt_usize(v: Option<f64>) -> Option<usize> {
    let v = v?;
    if !v.is_finite() || v <= 0.0 || v.fract() != 0.0 {
        return None;
    }
    usize::try_from(v as u64).ok()
}

/// ConvertWarning stream -> display lines (bindings surface strings).
fn warnings_to_strings(warnings: Vec<rofd_convert::ConvertWarning>) -> Vec<String> {
    warnings.iter().map(|w| w.to_string()).collect()
}

/// A long-lived converter instance. Create one per thread of use — pool
/// via `createPool(n)` (worker_threads, spec D11) instead of sharing.
#[napi(js_name = "Converter")]
pub struct JsConverter {
    inner: rofd_convert::Converter,
}

impl Default for JsConverter {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl JsConverter {
    #[napi(constructor)]
    pub fn new() -> Self {
        JsConverter {
            inner: rofd_convert::Converter::new(),
        }
    }

    /// Convert OFD bytes to PDF bytes. Kernel panics surface as JS errors
    /// (catch_panics boundary — panics must never abort the process).
    #[napi(ts_return_type = "ConvertOutput")]
    pub fn convert_to_pdf(
        &mut self,
        ofd: Buffer,
        options: Option<ConvertOptionsJs>,
    ) -> Result<ConvertOutput> {
        let bytes = ofd.as_ref().to_vec();
        let opts = kernel_options(options.as_ref());
        let r = rofd_convert::catch_panics(|| self.inner.convert(OutputFormat::Pdf, &bytes, &opts));
        match r {
            Ok(out) => Ok(ConvertOutput {
                bytes: Buffer::from(out.bytes),
                warnings: warnings_to_strings(out.warnings),
            }),
            Err(e) => Err(Error::new(Status::GenericFailure, e.to_string())),
        }
    }
}

/// Expose the package version to JS (kept in sync with Cargo.toml).
#[napi(js_name = "version")]
pub fn version(_env: Env) -> Result<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-positive / fractional / overflowing / absent budgets all map to
    /// the kernel default (None); a sane positive integer maps through.
    #[test]
    fn number_budgets_map_none_and_values() {
        assert_eq!(number_to_opt_u32(None), None);
        assert_eq!(number_to_opt_u32(Some(0.0)), None);
        assert_eq!(number_to_opt_u32(Some(-5.0)), None);
        assert_eq!(number_to_opt_u32(Some(2.5)), None);
        assert_eq!(number_to_opt_u32(Some(f64::NAN)), None);
        assert_eq!(number_to_opt_u32(Some(3.0)), Some(3));
        assert_eq!(number_to_opt_u32(Some(f64::MAX)), None);
        assert_eq!(number_to_opt_usize(Some(4096.0)), Some(4096));
    }

    /// Content flags: explicit false flips exactly that layer; absent
    /// flags keep the all-on kernel default (D17).
    #[test]
    fn kernel_options_maps_flags_and_defaults() {
        let d = kernel_options(None);
        assert!(d.content.watermark && d.content.annotations && d.content.seal);
        assert_eq!(d.max_pages, None);

        let js = ConvertOptionsJs {
            watermark: Some(false),
            annotations: None,
            seal: Some(true),
            max_pages: Some(2.0),
            max_media_bytes: Some(0.0),
        };
        let o = kernel_options(Some(&js));
        assert!(!o.content.watermark, "explicit false flips");
        assert!(o.content.annotations, "absent keeps default");
        assert!(o.content.seal);
        assert_eq!(o.max_pages, Some(2));
        assert_eq!(o.max_media_bytes, None, "<=0 budget = default");
    }
}
