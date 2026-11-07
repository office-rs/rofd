//! rofd-convert: the headless OFD -> PDF conversion facade (convert spec
//! `docs/superpowers/specs/2026-11-06-rofd-convert-design.md` §6.1).
//!
//! Converter hosts (CLI, FFI bindings, the later Java/Node/Python/wasm
//! binding crates) import ONLY this crate; it layers session semantics
//! over the io/render/export stack:
//!
//! * a long-lived [`Converter`] instance holding a snapshot of the
//!   process font registry (Task 4; font bytes Arc-shared, zero copies),
//! * one-call conversion (OFD bytes in -> PDF bytes out; no
//!   open/edit/close surface to leak),
//! * content-layer flags via [`PrintContent`](rofd_component::PrintContent)
//!   (all-on default, convert spec D17),
//! * budget guards (`max_pages` / `max_media_bytes`) failing fast with
//!   [`ConvertError::BudgetExceeded`] instead of pinning a pool thread
//!   on a pathological document,
//! * a `catch_unwind` boundary turning panics into
//!   [`ConvertError::Internal`] (panics are bugs, errors are data — the
//!   FFI rule every binding crate shares).
//!
//! Font injection has exactly one door: the process-level registry
//! (`register_font` / `register_font_file`, Task 4). There is no per-call
//! font surface and no unregister — a registry would grow unboundedly
//! under per-document uploads (spec §6.1 ruling).

/// Default page budget (spec §6.1): 10_000 pages.
pub const DEFAULT_MAX_PAGES: u32 = 10_000;
/// Default media budget (spec §6.1): 1 GiB of embedded media bytes.
pub const DEFAULT_MAX_MEDIA_BYTES: usize = 1 << 30;

/// Output format. v1 ships PDF only; the enum keeps an extension point
/// (spec D8) so future formats (Png etc.) join as new arms without
/// touching any L1 signature or binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Pdf,
}

/// Per-call conversion options. The derived default is the spec default:
/// `content` all-on (D17 — watermark/annotations/seal) and both budgets
/// unset (`None` = [`DEFAULT_MAX_PAGES`] / [`DEFAULT_MAX_MEDIA_BYTES`]).
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Which content layers the output carries (same flags as the print
    /// path; default = all layers on, convert spec D17).
    pub content: rofd_component::PrintContent,
    /// Page budget; `None` = [`DEFAULT_MAX_PAGES`].
    pub max_pages: Option<u32>,
    /// Total embedded-media byte budget; `None` =
    /// [`DEFAULT_MAX_MEDIA_BYTES`].
    pub max_media_bytes: Option<usize>,
}

/// A successful conversion: output bytes plus non-fatal warnings (v1
/// collects the io-side [`OfdWarning`](rofd_dom::OfdWarning) variants —
/// missing features, font substitutions, missing resources; spec D20).
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertOutput {
    pub bytes: Vec<u8>,
    pub warnings: Vec<rofd_dom::OfdWarning>,
}

/// Failure modes of a conversion call. Stable shape for the FFI boundary:
/// a discriminant plus message strings — never a Rust panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    Decode(String),
    Encode(String),
    /// A budget guard tripped: `what` names the budget ("pages",
    /// "media_bytes"), `limit` the configured maximum, `actual` the
    /// observed value.
    BudgetExceeded {
        what: &'static str,
        limit: u64,
        actual: u64,
    },
    /// A panic was caught at the boundary (a bug, reported as data).
    Internal(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Decode(e) => write!(f, "ofd decode failed: {e}"),
            ConvertError::Encode(e) => write!(f, "encode failed: {e}"),
            ConvertError::BudgetExceeded {
                what,
                limit,
                actual,
            } => {
                write!(f, "{what} budget exceeded: {actual} > {limit}")
            }
            ConvertError::Internal(e) => write!(f, "internal error (panic caught): {e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// Failure modes of the process-level font registry (Task 4's
/// `register_font` / `register_font_file` / `register_default_font`).
/// Not `Clone`/`PartialEq`: [`Io`](FontRegisterError::Io) wraps a
/// `std::io::Error`, which is neither.
#[derive(Debug)]
pub enum FontRegisterError {
    Io(std::io::Error),
    InvalidFont,
}

impl std::fmt::Display for FontRegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontRegisterError::Io(e) => write!(f, "font registration io error: {e}"),
            FontRegisterError::InvalidFont => {
                write!(f, "invalid font: not a usable font file")
            }
        }
    }
}

impl std::error::Error for FontRegisterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FontRegisterError::Io(e) => Some(e),
            FontRegisterError::InvalidFont => None,
        }
    }
}

/// Run `f`, converting any panic into [`ConvertError::Internal`]. The
/// FFI-boundary rule every binding entry shares (spec §6.1); exposed so
/// binding crates guard their own entry points with the identical
/// semantics. On wasm (phase 4) `panic = abort` means `catch_unwind`
/// cannot intercept — the guard then compiles to no behavioral
/// difference, which is the accepted platform posture there.
pub fn catch_panics<T>(f: impl FnOnce() -> Result<T, ConvertError>) -> Result<T, ConvertError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        Err(ConvertError::Internal(msg))
    })
}

/// A long-lived converter instance (spec §6.1). Thread contract (D15,
/// revised 2026-11-07): the instance holds only Arc'd registry snapshots
/// and the per-call `FontStore` is built inside `convert` (never stored),
/// so `Converter` is `Send` today — a status pinned by the
/// `converter_is_send_today` test, not a promise. The recommended L2
/// shape is still instance-bound-to-thread: a safe superset that stays
/// correct if a warm engine holding a `FontStore` ever lands (at which
/// point this contract tightens without any L2 API change).
///
/// Fields arrive in Task 4 (registry snapshot + default-font anchor);
/// `convert` arrives in Task 5.
pub struct Converter {
    /// Placeholder keeping the struct constructible before Task 4's
    /// snapshot fields exist.
    _priv: (),
}

impl Converter {
    /// Snapshot the process font registry into a fresh converter
    /// (infallible — see spec §6.1; fonts registered before this call
    /// are visible to every `convert` of this instance, later
    /// registrations need a new instance, spec D9).
    pub fn new() -> Converter {
        Converter { _priv: () }
    }
}

impl Default for Converter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        assert_eq!(DEFAULT_MAX_PAGES, 10_000);
        assert_eq!(DEFAULT_MAX_MEDIA_BYTES, 1 << 30);
        let opts = ConvertOptions::default();
        // D17: conversion defaults to all content layers on.
        assert!(opts.content.watermark);
        assert!(opts.content.annotations);
        assert!(opts.content.seal);
        assert_eq!(opts.max_pages, None);
        assert_eq!(opts.max_media_bytes, None);
    }

    #[test]
    fn budget_display_carries_numbers() {
        let e = ConvertError::BudgetExceeded {
            what: "pages",
            limit: 3,
            actual: 9,
        };
        assert_eq!(e.to_string(), "pages budget exceeded: 9 > 3");
    }
}
