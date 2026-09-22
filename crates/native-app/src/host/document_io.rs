//! Plain file I/O for the native host.
//!
//! The view layer owns file dialogs and path state; host commands snapshot
//! the component's document. Loading returns the parsed document together
//! with the `PackageHandle` surgical save needs. Commands cannot touch the
//! OS clipboard; that path stays in the widget itself.

use std::path::{Path, PathBuf};

use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, save_ofd as io_save_ofd, write_ofd, PackageHandle};

/// Result of loading an `.ofd` file from disk.
#[derive(Debug)]
pub struct LoadedOfd {
    /// Parsed document model.
    pub document: OfdDocument,
    /// Original package skeleton for surgical save. Always Some for a
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
/// With a package handle: surgical save (untouched entries byte-preserved).
/// Without: full write for a new document. The write itself is atomic
/// (sibling temp file + rename).
pub fn save_ofd(
    document: &OfdDocument,
    package: Option<&PackageHandle>,
    path: &Path,
) -> Result<(), String> {
    let bytes = match package {
        Some(pkg) => io_save_ofd(document, pkg),
        None => write_ofd(document),
    }
    .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}

/// Write bytes to a sibling `.ofd.tmp` file, then rename over destination.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp: PathBuf = path.to_path_buf();
    tmp.set_extension("ofd.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_and_save_round_trip() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_{}.ofd", std::process::id()));
        let bytes = write_ofd(&OfdDocument::default()).expect("write_ofd seeds a package");
        std::fs::write(&tmp, bytes).expect("seed file");

        let loaded = load_ofd(&tmp).expect("load_ofd");
        assert!(loaded.package.is_some(), "parsed file retains a package");
        assert!(loaded.warnings.is_empty(), "minimal doc has no warnings");

        save_ofd(&loaded.document, loaded.package.as_ref(), &tmp).expect("save_ofd");

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
        save_ofd(&OfdDocument::default(), None, &tmp).expect("full write");
        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("full-write output re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
