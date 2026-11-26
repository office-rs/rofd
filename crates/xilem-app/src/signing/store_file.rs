//! Where the gpui host keeps its certificate store (spec 2026-11-23 §13):
//! the bytes of rofd-sign's `CertificateStore::to_bytes`, as one file in
//! the application data directory - `rofd/certificates.json` (Windows:
//! `%LOCALAPPDATA%`; elsewhere `$HOME/.config`; neither known: the temp
//! directory). The file holds the certificate files still encrypted and no
//! password; signing asks for the password again (M7).

use std::path::{Path, PathBuf};

use rofd_sign::CertificateStore;

/// The application data directory: `%LOCALAPPDATA%` on Windows, otherwise
/// `$HOME/.config`; the temp directory when neither is set.
fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir);
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config");
    }
    std::env::temp_dir()
}

/// Where the certificate store lives: `<app data>/rofd/certificates.json`.
pub fn store_path() -> PathBuf {
    app_data_dir().join("rofd").join("certificates.json")
}

/// Read the store from `path`; a missing file is an empty store (a first
/// run). Anything else that goes wrong is an error: starting over would
/// lose the user's certificates without a word.
pub fn load_store_at(path: &Path) -> Result<CertificateStore, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CertificateStore::default());
        }
        Err(e) => return Err(format!("读取证书库失败（{}）：{e}", path.display())),
    };
    CertificateStore::from_bytes(&bytes)
        .map_err(|e| format!("证书库无法读取（{}）：{e}", path.display()))
}

/// Write the store to `path`, creating the directories that are missing.
pub fn save_store_at(store: &CertificateStore, path: &Path) -> Result<(), String> {
    let bytes = store
        .to_bytes()
        .map_err(|e| format!("证书库无法保存：{e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("无法创建证书库目录（{}）：{e}", dir.display()))?;
    }
    crate::host::document_io::write_atomic(path, &bytes)
        .map_err(|e| format!("证书库无法保存（{}）：{e}", path.display()))
}

/// The store as it was left: empty on a first run; a store that cannot be
/// read is an empty one for this run (the error says so, and nothing is
/// written over it until the user changes the store).
pub fn load_store() -> (CertificateStore, Option<String>) {
    match load_store_at(&store_path()) {
        Ok(store) => (store, None),
        Err(e) => (CertificateStore::default(), Some(e)),
    }
}

/// Write the store where it lives.
pub fn save_store(store: &CertificateStore) -> Result<(), String> {
    save_store_at(store, &store_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_sign::{CertificateStore, Signer as _};
    use std::path::Path;

    fn fixture_user_p12() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sign/tests/fixtures/USER.p12"
        ))
        .expect("fixture")
    }

    #[test]
    fn store_round_trips_through_the_app_data_file() {
        // Where the store lives, whatever the platform resolves.
        let path = store_path();
        assert!(path.is_absolute(), "{}", path.display());
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("certificates.json")
        );
        assert_eq!(
            path.parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str()),
            Some("rofd")
        );

        // In a temporary directory: save, read back, the same list. Saving
        // creates the directories that are not there yet.
        let dir = std::env::temp_dir().join(format!("rofd_gpui_store_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("rofd").join("certificates.json");
        let mut store = CertificateStore::default();
        let entry = store.import(fixture_user_p12(), "777777").expect("import");
        save_store_at(&store, &file).expect("save");
        let back = load_store_at(&file).expect("load");
        assert_eq!(back.list(), store.list());
        assert_eq!(back.list().len(), 1);
        assert_eq!(back.list()[0].id, entry.id);
        // The file is what rofd-sign writes: the certificate is still in it.
        let mut signer = back
            .signer(&entry.id, "777777")
            .expect("the certificate file survived");
        assert!(!signer.sign(b"m").expect("sign").is_empty());

        // Overwritten with a store that forgot the certificate.
        store.remove(&entry.id);
        save_store_at(&store, &file).expect("save");
        assert!(load_store_at(&file).expect("load").list().is_empty());

        // A missing file is an empty store (first run).
        assert!(load_store_at(&dir.join("missing.json"))
            .expect("no file yet")
            .list()
            .is_empty());
        // A damaged file is an error: starting over would lose the
        // user's certificates without a word.
        std::fs::write(&file, b"not a store").expect("damage");
        assert!(load_store_at(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_where_no_directory_can_be_made_is_an_error() {
        let file =
            std::env::temp_dir().join(format!("rofd_gpui_store_file_{}", std::process::id()));
        std::fs::write(&file, b"i am a file").expect("a file in the way");
        // A directory cannot be created under a file.
        let under = file.join("rofd").join("certificates.json");
        assert!(save_store_at(&CertificateStore::default(), &under).is_err());
        let _ = std::fs::remove_file(&file);
    }
}
