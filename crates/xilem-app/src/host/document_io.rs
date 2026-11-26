//! Plain file I/O for the native host.
//!
//! The view layer owns file dialogs and path state; host commands snapshot
//! the component's document. Loading returns the parsed document, which
//! carries the package it came from (`OfdDocument.package`). Commands cannot
//! OS clipboard; that path stays in the widget itself.

use std::path::{Path, PathBuf};

use rofd_dom::{OfdDocument, TextLayoutHints};
use rofd_io::{parse_ofd, save_ofd as preserving_save};

/// Result of loading an `.ofd` file from disk.
#[derive(Debug)]
pub struct LoadedOfd {
    /// Parsed document model.
    pub document: OfdDocument,
    /// Degraded-load warnings (templates/JBIG2/font substitution, ...).
    pub warnings: Vec<rofd_dom::OfdWarning>,
    /// The signatures the file carries, with their verdicts (spec
    /// 2026-11-23 U12: verified the moment it is opened).
    pub signatures: Vec<rofd_sign::SignatureReport>,
}

/// Read and parse an `.ofd` file.
pub fn load_ofd(path: &Path) -> Result<LoadedOfd, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let report = parse_ofd(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(LoadedOfd {
        document: report.document,
        warnings: report.warnings,
        signatures: rofd_sign::verify(&bytes, &rofd_sign::VerifyOptions::default()),
    })
}

/// Serialize the document and write it back to `path`.
///
/// The preserving save (spec 2026-11-23 §7): with the loaded package every
/// entry stays byte-identical except the annotation entries, which are
/// rewritten in place. Without a package (a new document) the full write
/// generates one. The write itself is atomic (sibling temp file + rename).
pub fn save_ofd(
    document: &OfdDocument,
    text_layout: &TextLayoutHints,
    path: &Path,
) -> Result<(), String> {
    let bytes = preserving_save(document, text_layout)
        .map(|preserved| preserved.bytes)
        .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}

/// The bytes a save writes (spec 2026-11-23 §7): the preserving save's
/// own output. With the loaded package every entry stays byte-identical
/// except the annotation entries, which are rewritten in place; without a
/// package (a new document) the full write generates one. The host runs
/// the pre-save check on the result and writes it with [`write_atomic`].
pub fn save_bytes(
    document: &OfdDocument,
    text_layout: &TextLayoutHints,
) -> Result<rofd_io::Preserved, String> {
    preserving_save(document, text_layout).map_err(|e| format!("serialize: {e}"))
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
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
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
    fn preserving_save_keeps_entries_identical_for_unchanged_document() {
        // A package written by another tool, in markup the full write would
        // reformat: an unchanged document must come back byte for byte.
        let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
        let document_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData><ofd:Pages><ofd:Page ID=\"7\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
        let page_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>";
        let bytes = rofd_io::zip_util::write_zip(&[
            ("OFD.xml".to_owned(), ofd_xml.as_bytes().to_vec()),
            (
                "Doc_0/Document.xml".to_owned(),
                document_xml.as_bytes().to_vec(),
            ),
            (
                "Doc_0/Pages/Page_0/Content.xml".to_owned(),
                page_xml.as_bytes().to_vec(),
            ),
        ])
        .expect("seed package");
        let tmp =
            std::env::temp_dir().join(format!("rofd_doc_io_preserving_{}.ofd", std::process::id()));
        std::fs::write(&tmp, &bytes).expect("seed file");

        let loaded = load_ofd(&tmp).expect("load_ofd");
        save_ofd(&loaded.document, &TextLayoutHints::new(), &tmp).expect("save_ofd");

        let written = std::fs::read(&tmp).expect("file written");
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(
            rofd_io::zip_util::read_all_entries(&written).expect("read saved entries"),
            rofd_io::zip_util::read_all_entries(&bytes).expect("read original entries"),
            "an unchanged document saves every entry byte-identical"
        );
    }

    #[test]
    fn load_and_save_round_trip() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_{}.ofd", std::process::id()));
        let bytes = write_ofd(&OfdDocument::default()).expect("write_ofd seeds a package");
        std::fs::write(&tmp, bytes).expect("seed file");

        let loaded = load_ofd(&tmp).expect("load_ofd");
        assert!(
            loaded.document.package.is_some(),
            "parsed file retains a package"
        );
        assert!(loaded.warnings.is_empty(), "minimal doc has no warnings");

        save_ofd(&loaded.document, &TextLayoutHints::new(), &tmp).expect("save_ofd");

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
        save_ofd(&OfdDocument::default(), &TextLayoutHints::new(), &tmp).expect("full write");
        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("full-write output re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
