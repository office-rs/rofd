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

mod fonts;

pub use fonts::{
    has_default_font, register_default_font, register_font, register_font_file, registered_families,
};

use std::sync::Arc;

use rofd_component::PrintSession;
use rofd_dom::OfdDocument;
use rofd_export::{pdf_from_page_scenes, PdfPage};
use rofd_render::FontStore;

/// Default page budget (spec §6.1): 10_000 pages.
pub const DEFAULT_MAX_PAGES: u32 = 10_000;
/// Default media budget (spec §6.1): 1 GiB of embedded media bytes.
pub const DEFAULT_MAX_MEDIA_BYTES: usize = 1 << 30;

/// Total embedded media bytes of a document: the sum of every image and
/// font payload in `resources` (the media budget's measure). Public and
/// standalone-testable because `write_ofd`'s v1 full-write emits no
/// resource entries, so end-to-end fixtures cannot carry media bytes —
/// the same pub posture as rword's `check_media_budget`.
pub fn media_bytes(doc: &OfdDocument) -> usize {
    doc.resources
        .images
        .values()
        .chain(doc.resources.font_data.values())
        .map(|bytes| bytes.len())
        .sum()
}

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
pub struct Converter {
    /// Registry fonts at [`Converter::new`] time (spec D9: fonts
    /// registered later need a new instance).
    fonts: Vec<Arc<Vec<u8>>>,
    /// Default-font anchor (spec D19) at snapshot time; `None` falls back
    /// to empty bytes (system-font fallback inside `FontStore`).
    default_font: Option<Arc<Vec<u8>>>,
}

impl Converter {
    /// Snapshot the process font registry into a fresh converter
    /// (infallible — see spec §6.1; fonts registered before this call
    /// are visible to every `convert` of this instance, later
    /// registrations need a new instance, spec D9).
    pub fn new() -> Converter {
        let snapshot = fonts::registry_snapshot();
        Converter {
            fonts: snapshot.fonts,
            default_font: snapshot.default_font,
        }
    }

    /// One-call conversion: OFD bytes in, [`OutputFormat`] bytes out
    /// (spec §6.1). Pipeline order is load-bearing:
    ///
    /// 1. `parse_ofd` — decode failures surface as
    ///    [`ConvertError::Decode`] before any budget is consulted,
    /// 2. budget guards (pages, then media) — a pathological document
    ///    fails fast as [`ConvertError::BudgetExceeded`] instead of
    ///    pinning a pool thread mid-render,
    /// 3. per-call `FontStore` from the document resources plus the
    ///    snapshotted registry fonts (default anchored per D19),
    /// 4. a headless `PrintSession` renders every page at print zoom and
    ///    `rofd-export` encodes them; encode failures (including the
    ///    empty-page-list rejection of a 0-page document) surface as
    ///    [`ConvertError::Encode`].
    ///
    /// Non-fatal parse warnings pass through to
    /// [`ConvertOutput::warnings`] unchanged (spec D20). Binding crates
    /// wrap calls with [`catch_panics`] for the panic boundary.
    pub fn convert(
        &mut self,
        format: OutputFormat,
        ofd: &[u8],
        opts: &ConvertOptions,
    ) -> Result<ConvertOutput, ConvertError> {
        match format {
            OutputFormat::Pdf => self.convert_to_pdf(ofd, opts),
        }
    }

    /// The PDF arm of [`Converter::convert`] (v1's only format; the
    /// `OutputFormat` match above stays exhaustive so future formats join
    /// as new arms).
    fn convert_to_pdf(
        &self,
        ofd: &[u8],
        opts: &ConvertOptions,
    ) -> Result<ConvertOutput, ConvertError> {
        // 1. Decode. Errors are data: mapped, never unwrapped.
        let report = rofd_io::parse_ofd(ofd).map_err(|e| ConvertError::Decode(e.to_string()))?;
        let doc = report.document;

        // 2. Budgets, after parse (decode errors win) and before any scene
        //    building.
        let max_pages = u64::from(opts.max_pages.unwrap_or(DEFAULT_MAX_PAGES));
        let actual_pages = doc.pages.len() as u64;
        if actual_pages > max_pages {
            return Err(ConvertError::BudgetExceeded {
                what: "pages",
                limit: max_pages,
                actual: actual_pages,
            });
        }
        let max_media = opts.max_media_bytes.unwrap_or(DEFAULT_MAX_MEDIA_BYTES) as u64;
        let actual_media = media_bytes(&doc) as u64;
        if actual_media > max_media {
            return Err(ConvertError::BudgetExceeded {
                what: "media_bytes",
                limit: max_media,
                actual: actual_media,
            });
        }

        // 3. Per-call font store: document resources first, then every
        //    snapshotted registry font, default anchored from the snapshot.
        let default_font = self
            .default_font
            .clone()
            .unwrap_or_else(|| Arc::new(Vec::new()));
        let mut fonts = FontStore::from_resources(&doc.resources, default_font);
        for bytes in &self.fonts {
            fonts.register_font(bytes.clone());
        }

        // 4. Headless session over every page, encoded by rofd-export.
        let mut session = PrintSession::headless(doc, fonts, opts.content);
        let mut pages = Vec::with_capacity(session.page_count());
        for i in 0..session.page_count() {
            let (width, height) = session.page_rect(i);
            pages.push(PdfPage {
                width_px: width as f32,
                height_px: height as f32,
                scene: session.page_scene(i),
            });
        }
        let bytes =
            pdf_from_page_scenes(&pages).map_err(|e| ConvertError::Encode(e.to_string()))?;
        Ok(ConvertOutput {
            bytes,
            warnings: report.warnings,
        })
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
