//! rofd-convert-py: PyO3 surface over the rofd-convert session kernel
//! (convert spec §6.6, D3/D15). The cdylib is the
//! `rofd_convert.rofd_convert_native` extension (built by maturin;
//! PyPI wheel `rofd-convert`, abi3-py39, no sdist — D5).
//!
//! FFI discipline (spec §6.1) applies here too: kernel calls run under
//! `catch_panics` (an escaped panic would abort the interpreter), and
//! failures raise the module's `ConvertError` exception — never a Rust
//! panic across the boundary.
//!
//! Concurrency: `convert` releases the GIL (`py.detach`) so Python
//! threads overlap on the CPU work — which means two threads COULD enter
//! the same instance concurrently. The interior lease (take → convert →
//! put, `Mutex<Option<Converter>>`) mechanically enforces D15
//! ("one thread at a time per instance"): a concurrent call raises
//! ConvertError("converter in use...") instead of racing. The documented
//! pool recipe (spec §6.6) is one instance per thread via
//! `threading.local` — see demo/run.py.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use rofd_convert::{ConvertOptions, OutputFormat};
use std::sync::Mutex;

create_exception!(
    rofd_convert,
    ConvertError,
    PyException,
    "Conversion failure at the kernel boundary (decode/encode/budget/internal)."
);

fn to_py(e: rofd_convert::ConvertError) -> PyErr {
    ConvertError::new_err(e.to_string())
}

/// Map the JS-of-Python optional numbers onto kernel budgets: None / <=0
/// / out-of-range all mean "kernel default" (same contract as the CLI
/// and the Node binding at their boundaries).
fn opt_budget_u32(v: Option<i64>) -> Option<u32> {
    u32::try_from(v?).ok().filter(|n| *n > 0)
}

fn opt_budget_usize(v: Option<i64>) -> Option<usize> {
    usize::try_from(v?).ok().filter(|n| *n > 0)
}

/// Map convert kwargs onto the kernel's ConvertOptions (None flags keep
/// the all-on kernel default, D17).
fn kernel_options(
    watermark: Option<bool>,
    annotations: Option<bool>,
    seal: Option<bool>,
    max_pages: Option<i64>,
    max_media_bytes: Option<i64>,
) -> ConvertOptions {
    let mut opts = ConvertOptions::default();
    if let Some(w) = watermark {
        opts.content.watermark = w;
    }
    if let Some(a) = annotations {
        opts.content.annotations = a;
    }
    if let Some(s) = seal {
        opts.content.seal = s;
    }
    opts.max_pages = opt_budget_u32(max_pages);
    opts.max_media_bytes = opt_budget_usize(max_media_bytes);
    opts
}

/// ConvertWarning stream -> display lines (bindings surface strings).
fn warnings_to_strings(warnings: Vec<rofd_convert::ConvertWarning>) -> Vec<String> {
    warnings.iter().map(|w| w.to_string()).collect()
}

/// Registry-result mapping shared by every register entry (same contract
/// as the Node binding and the CLI): Ok -> true, unparsable font ->
/// false (no raise), I/O failure -> the message for a raised
/// ConvertError.
fn font_reg_result(
    r: std::result::Result<(), rofd_convert::FontRegisterError>,
) -> Result<bool, String> {
    match r {
        Ok(()) => Ok(true),
        Err(rofd_convert::FontRegisterError::InvalidFont) => Ok(false),
        Err(e) => Err(format!("cannot read font file: {e}")),
    }
}

/// Register font bytes into the process-level registry (append-only,
/// process-wide, idempotent on identical bytes).
#[pyfunction]
fn register_font(bytes: Vec<u8>) -> bool {
    rofd_convert::register_font(bytes).is_ok()
}

/// Register a font file into the process-level registry. I/O failure
/// raises ConvertError; an unparsable font returns False (kernel
/// contract, same as the Node binding).
#[pyfunction]
fn register_font_file(path: &str) -> PyResult<bool> {
    font_reg_result(rofd_convert::register_font_file(path)).map_err(ConvertError::new_err)
}

/// Anchor the process default font (kernel D19); a later anchor wins.
/// Unparsable bytes return False.
#[pyfunction]
fn register_default_font(bytes: Vec<u8>) -> bool {
    rofd_convert::register_default_font(bytes).is_ok()
}

/// Whether a default font is anchored in the process registry.
#[pyfunction]
fn has_default_font() -> bool {
    rofd_convert::has_default_font()
}

/// Font families visible to the process registry (registrations only;
/// system-discovered families are never listed).
#[pyfunction]
fn registered_families() -> Vec<String> {
    rofd_convert::registered_families()
}

/// A long-lived converter instance (convert spec §6.1). Thread contract
/// (D15): one call at a time — concurrent calls raise ConvertError. For
/// throughput, give each thread its own instance (`threading.local`;
/// see demo/run.py for the ThreadPoolExecutor recipe).
#[pyclass]
struct Converter {
    /// Lease slot: Some(instance) when idle; taken for the duration of a
    /// call. Same mechanics as the JNI handle table (phase 2): the guard
    /// is dropped BETWEEN take and put — holding it across the GIL
    /// release would deadlock the interpreter (a second thread would
    /// block on the mutex while holding the GIL the first thread needs
    /// to re-enter after its kernel call).
    lease: Mutex<Option<rofd_convert::Converter>>,
}

/// Take the instance out of the lease (guard dropped on return). Err =
/// the stable "in use" message a concurrent caller reports.
fn take_leased(
    lease: &Mutex<Option<rofd_convert::Converter>>,
) -> Result<rofd_convert::Converter, &'static str> {
    let mut slot = lease
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    slot.take()
        .ok_or("converter in use: one call at a time per instance")
}

/// Put the instance back (fresh guard; no-op if the slot was closed —
/// same no-resurrection semantics as the JNI table's put).
fn put_leased(lease: &Mutex<Option<rofd_convert::Converter>>, inner: rofd_convert::Converter) {
    let mut slot = lease
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *slot = Some(inner);
}

#[pymethods]
impl Converter {
    #[new]
    fn new() -> Self {
        Converter {
            lease: Mutex::new(Some(rofd_convert::Converter::new())),
        }
    }

    /// Convert OFD bytes to PDF. Returns (pdf_bytes, warnings). The GIL
    /// is released during the kernel call; a concurrent call on the SAME
    /// instance raises ConvertError("converter in use").
    // Eight parameters mirror the Python kwargs API one-for-one (the
    // plan's L2 surface); folding them into a struct would need a
    // pyclass the call sites would then construct.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (ofd, *, watermark=None, annotations=None, seal=None, max_pages=None, max_media_bytes=None))]
    fn convert(
        &self,
        ofd: Vec<u8>,
        watermark: Option<bool>,
        annotations: Option<bool>,
        seal: Option<bool>,
        max_pages: Option<i64>,
        max_media_bytes: Option<i64>,
        py: Python<'_>,
    ) -> PyResult<(Vec<u8>, Vec<String>)> {
        let mut inner = take_leased(&self.lease).map_err(ConvertError::new_err)?;
        let opts = kernel_options(watermark, annotations, seal, max_pages, max_media_bytes);
        // pyo3 0.30's `Python::detach` is the former `allow_threads`: the
        // GIL is released for the closure (must be Send — Converter and
        // the byte buffers are), overlapping Python threads is the point.
        // The lease guard is NOT held here (see take_leased) — a second
        // caller gets the documented "in use" error instead of blocking
        // with the GIL held.
        let r = py
            .detach(|| rofd_convert::catch_panics(|| inner.convert(OutputFormat::Pdf, &ofd, &opts)))
            .map_err(to_py);
        // The instance goes back on every path (a lease lost to a
        // poisoned/crashed call is unrecoverable — treat as closed).
        put_leased(&self.lease, inner);
        r.map(|out| (out.bytes, warnings_to_strings(out.warnings)))
    }
}

/// Module init: `import rofd_convert` (the python package re-exports).
#[pymodule]
fn rofd_convert_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("ConvertError", m.py().get_type::<ConvertError>())?;
    m.add_function(wrap_pyfunction!(register_font, m)?)?;
    m.add_function(wrap_pyfunction!(register_font_file, m)?)?;
    m.add_function(wrap_pyfunction!(register_default_font, m)?)?;
    m.add_function(wrap_pyfunction!(has_default_font, m)?)?;
    m.add_function(wrap_pyfunction!(registered_families, m)?)?;
    m.add_class::<Converter>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Content flags: explicit False flips exactly that layer; None keeps
    /// the all-on kernel default (D17).
    #[test]
    fn kernel_options_maps_flags_and_defaults() {
        let d = kernel_options(None, None, None, None, None);
        assert!(d.content.watermark && d.content.annotations && d.content.seal);
        assert_eq!(d.max_pages, None);

        let o = kernel_options(Some(false), None, Some(true), Some(2), Some(0));
        assert!(!o.content.watermark, "explicit False flips");
        assert!(o.content.annotations, "None keeps default");
        assert!(o.content.seal);
        assert_eq!(o.max_pages, Some(2));
        assert_eq!(o.max_media_bytes, None, "<=0 budget = kernel default");
    }

    /// Budget mapping: None / <=0 / negative -> kernel default; positive
    /// ints map through (PyO3 delivers i64).
    #[test]
    fn number_budgets_map_none_and_values() {
        assert_eq!(opt_budget_u32(None), None);
        assert_eq!(opt_budget_u32(Some(0)), None);
        assert_eq!(opt_budget_u32(Some(-5)), None);
        assert_eq!(opt_budget_u32(Some(3)), Some(3));
        assert_eq!(opt_budget_u32(Some(i64::from(u32::MAX) + 1)), None);
        assert_eq!(opt_budget_usize(Some(4096)), Some(4096));
    }

    /// Registry-result mapping (same contract as the Node binding and the
    /// CLI): Ok -> true, unparsable font -> False WITHOUT raising, I/O
    /// failure -> error string for the raised ConvertError.
    #[test]
    fn font_reg_result_false_on_invalid_not_raise() {
        assert_eq!(font_reg_result(Ok(())), Ok(true));
        assert_eq!(
            font_reg_result(Err(rofd_convert::FontRegisterError::InvalidFont)),
            Ok(false)
        );
        assert!(font_reg_result(Err(rofd_convert::FontRegisterError::Io(
            std::io::Error::other("gone")
        )))
        .is_err());
    }

    /// Lease semantics: a take without a put makes the NEXT take report
    /// the stable "in use" error; putting back restores the slot. This is
    /// the mechanical guarantee the convert method relies on while the
    /// GIL is released (guards dropped between take and put — holding one
    /// across the release would deadlock the interpreter).
    #[test]
    fn lease_take_put_reports_in_use_between_take_and_put() {
        let lease = Mutex::new(Some(rofd_convert::Converter::new()));
        let taken = take_leased(&lease).expect("idle lease must yield the instance");
        let second = take_leased(&lease);
        assert_eq!(
            second.err(),
            Some("converter in use: one call at a time per instance"),
            "concurrent caller sees the documented error, never a block"
        );
        put_leased(&lease, taken);
        assert!(
            take_leased(&lease)
                .map(|c| {
                    put_leased(&lease, c);
                })
                .is_ok(),
            "put back makes the lease reusable"
        );
    }

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
