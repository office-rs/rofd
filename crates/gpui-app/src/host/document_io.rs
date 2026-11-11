//! Plain file I/O for the native host.
//!
//! The view layer owns file dialogs and path state; host commands snapshot
//! the component's document. Loading returns the parsed document together
//! with the `PackageHandle` passthrough transport needs. Commands cannot
//! OS clipboard; that path stays in the widget itself.

use std::path::{Path, PathBuf};

use rofd_dom::{OfdDocument, TextLayoutHints};
use rofd_io::{parse_ofd, write_ofd_with_text_layout, PackageHandle};

/// Result of loading an `.ofd` file from disk.
#[derive(Debug)]
pub struct LoadedOfd {
    /// Parsed document model.
    pub document: OfdDocument,
    /// Original package for passthrough transport. Always Some for a
    /// successfully parsed file; Option keeps the host's "new document"
    /// state uniform.
    pub package: Option<PackageHandle>,
    /// Degraded-load warnings (templates/JBIG2/font substitution, ...).
    pub warnings: Vec<rofd_dom::OfdWarning>,
}

/// Read and parse an `.ofd` file.
pub fn load_ofd(path: &Path) -> Result<LoadedOfd, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let report = parse_ofd(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(LoadedOfd {
        document: report.document,
        package: Some(report.package),
        warnings: report.warnings,
    })
}

/// Serialize the document and write it back to `path`.
///
/// Full write with passthrough (unconsumed entries transported verbatim,
/// spec 2026-11-11 D12). The write itself is atomic
/// (sibling temp file + rename).
pub fn save_ofd(
    document: &OfdDocument,
    package: Option<&PackageHandle>,
    text_layout: &TextLayoutHints,
    path: &Path,
) -> Result<(), String> {
    let bytes = write_ofd_with_text_layout(document, package, text_layout)
        .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}

/// Removes the temp file on drop unless disarmed after a successful rename.
struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write bytes to a sibling `<dest>.tmp` file, then rename over the
/// destination. Shared by OFD save and PDF export; for `.ofd` targets the
/// temp name is identical to the previous `set_extension("ofd.tmp")`
/// form, and it now derives from the destination instead of hardcoding
/// the OFD extension.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let _guard = TempFileGuard(tmp.clone());
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Dropping _guard removes the temp file; error semantics unchanged.
        return Err(format!("rename {}: {e}", path.display()));
    }
    std::mem::forget(_guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_io::write_ofd;

    #[test]
    fn load_and_save_round_trip() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_{}.ofd", std::process::id()));
        let bytes = write_ofd(&OfdDocument::default()).expect("write_ofd seeds a package");
        std::fs::write(&tmp, bytes).expect("seed file");

        let loaded = load_ofd(&tmp).expect("load_ofd");
        assert!(loaded.package.is_some(), "parsed file retains a package");
        assert!(loaded.warnings.is_empty(), "minimal doc has no warnings");

        save_ofd(
            &loaded.document,
            loaded.package.as_ref(),
            &TextLayoutHints::new(),
            &tmp,
        )
        .expect("save_ofd");

        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("written file re-parses");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_missing_path_errors_with_read_context() {
        let err = load_ofd(Path::new("no-such-rofd-file.ofd")).unwrap_err();
        assert!(err.contains("read"), "error names the failing stage: {err}");
    }

    #[test]
    fn save_without_package_full_writes() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_full_{}.ofd", std::process::id()));
        save_ofd(&OfdDocument::default(), None, &TextLayoutHints::new(), &tmp).expect("full write");
        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("full-write output re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
