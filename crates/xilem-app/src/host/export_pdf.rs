//! Save-as-PDF for the native host.
//!
//! Same shape as `document_io::save_ofd`: snapshot through the live
//! component (`begin_print`), encode, then atomically write the bytes.
//! The document itself is never mutated — an export is not a save, so the
//! host's unsaved-changes mirror must not react to its outcome.

use std::path::Path;

use rofd_component::{OfdComponent, PrintContent};
use rofd_convert::pdf_pages;
use rofd_export::pdf_from_page_scenes;

use super::document_io::write_atomic;

/// Export the component's current document to `path` as a one-file PDF:
/// every page of a `begin_print` session with the default all-on content
/// preset (watermark/annotations/seal on, convert spec D17), assembled
/// through the shared `rofd_convert::pdf_pages` loop (spec §8/D18 — host
/// export and convert can never drift), encoded by `rofd-export` and
/// written atomically (sibling temp file + rename).
pub fn export_pdf(component: &OfdComponent, path: &Path) -> Result<(), String> {
    let mut session = component.begin_print(PrintContent::default());
    let pages = pdf_pages(&mut session);
    let bytes =
        pdf_from_page_scenes(&pages).map_err(|e| format!("export {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}
