//! rofd-export: encode page-local `imaging::record::Scene`s into PDF bytes
//! via krilla (print design spec 2026-10-31 §3).
//!
//! Ported from the as-built reference project rword-export
//! (`D:/code/rword/crates/rword-export/src/pdf.rs`) with the docx pipeline
//! scaffolding (`ConvertSink`, `ExportConfig`/metadata) stripped: this crate
//! is a leaf that depends on no rofd crate. The PDF carries no dates, so
//! identical input encodes byte-identically (spec §2.7).

mod pdf;

pub use pdf::{pdf_from_page_scenes, ExportError, PdfPage};
