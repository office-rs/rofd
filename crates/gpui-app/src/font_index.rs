//! System font channel for the gpui host (desktop font channel, draft
//! 2026-10-18 §3.1): one lazy scan of the system font directories, family
//! names extracted from each face's name table (name IDs 1 and 16, ALL
//! language records — "SimSun" and "宋体" are independent entries, which is
//! what makes the kernel's normalization sufficient on both sides), and
//! per-document lazy byte loading for the families a document declares.
//!
//! Key universe: both the document side (`rofd_component::font_req`) and
//! this index run family names through [`rofd_component::normalize_family_key`]
//! — the kernel re-export — so the two sides can never drift apart.
//!
//! Scan priority: directories are passed in ascending priority (system dir,
//! then per-user dir) and the LAST write wins, matching Windows per-user
//! fonts overriding system fonts.
//!
//! Pure-logic core (`scan_dirs` takes arbitrary directories) so unit tests
//! never touch the real system font directories.

// Ported from crates/tauri-app/src-tauri/src/font_index.rs (desktop font channel).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rofd_component::{normalize_family_key, FontRequirements, OfdComponent};

/// Scan product: kernel-normalized family key -> font file path.
#[derive(Debug, Default, Clone)]
pub struct SystemFontIndex {
    families: HashMap<String, PathBuf>,
}

impl SystemFontIndex {
    /// Scan the system font directories (lazy, once per process):
    /// `%WINDIR%\Fonts` + `%LOCALAPPDATA%\Microsoft\Windows\Fonts`,
    /// ascending priority (per-user overrides system).
    pub fn scan() -> Self {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(Path::new(&windir).join("Fonts"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                Path::new(&local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        Self::scan_dirs(&dirs)
    }

    /// Process-wide index, scanned lazily on the first document install
    /// (a blank-editor startup never pays the scan).
    pub fn shared() -> &'static SystemFontIndex {
        static SHARED: OnceLock<SystemFontIndex> = OnceLock::new();
        SHARED.get_or_init(Self::scan)
    }

    /// Pure scan entry (tests): per directory, per file; unreadable or
    /// unparseable files are skipped (scanning never fails). Same family
    /// key scanned later wins.
    pub fn scan_dirs(dirs: &[PathBuf]) -> Self {
        let mut index = SystemFontIndex::default();
        for dir in dirs {
            let Ok(read_dir) = std::fs::read_dir(dir) else {
                continue; // Missing dir (e.g. no per-user fonts dir) -> skip.
            };
            for entry in read_dir.flatten() {
                index_file(&mut index, &entry.path());
            }
        }
        index
    }

    /// Load the font bytes for every required family that the scan found
    /// locally. `required.families` are kernel-normalized keys (the same
    /// [`normalize_family_key`] universe the index keys on). Absent keys
    /// contribute nothing; one file serving several keys is read once.
    pub fn load_families(&self, required: &FontRequirements) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut seen: HashSet<&Path> = HashSet::new();
        for family in &required.families {
            let Some(path) = self.families.get(family) else {
                continue; // Not installed locally: the kernel's default
                          // family covers the glyphs (its fallback chain).
            };
            if !seen.insert(path.as_path()) {
                continue; // One file already read for another key.
            }
            match std::fs::read(path) {
                Ok(bytes) => out.push(bytes),
                Err(e) => {
                    // The file indexed fine but cannot be read now (moved,
                    // locked, ...): warn and let the kernel fall back.
                    eprintln!("[warning] font unreadable ({}): {e}", path.display());
                }
            }
        }
        out
    }

    /// The font file a normalized family key resolved to (test
    /// observability; `load_families` reads these paths).
    #[cfg(test)]
    pub fn resolve(&self, normalized_key: &str) -> Option<&Path> {
        self.families.get(normalized_key).map(PathBuf::as_path)
    }

    /// Whether the scan found any family at all (test observability).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }
}

/// Index a single file: extension filter (.ttf/.ttc/.otf/.otc, case
/// insensitive) -> mmap -> every face's family names (name IDs 1 and 16,
/// ALL language records) normalized into kernel keys. Any failure is
/// silently skipped (best effort, same as the tauri port).
fn index_file(index: &mut SystemFontIndex, path: &Path) {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return;
    };
    let ext = ext.to_lowercase();
    if !matches!(ext.as_str(), "ttf" | "ttc" | "otf" | "otc") {
        return;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    // mmap instead of fs::read: the name table is a few KB, so only the
    // pages ttf-parser touches get paged in; a full read would slurp a
    // several-hundred-MB font directory. Font dir files are managed by
    // the system, so the mapping is stable.
    let Ok(mmap) = (unsafe { memmap2::Mmap::map(&file) }) else {
        return;
    };
    let data: &[u8] = &mmap;

    let n_faces = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    for i in 0..n_faces {
        let Ok(face) = ttf_parser::Face::parse(data, i) else {
            continue;
        };
        for name in face.names() {
            // Family (1) + preferred family (16), every language record.
            // ttf-parser 0.25: `name_id` is a bare field, not a method.
            let id = name.name_id;
            if id != 1 && id != 16 {
                continue;
            }
            if let Some(family) = name.to_string() {
                // Same key universe as the document side (font_req): the
                // kernel re-export, never a local re-implementation.
                let key = normalize_family_key(&family);
                if !key.is_empty() {
                    // Later write wins: later directories override earlier
                    // ones (per-user overrides system), and within one file
                    // re-inserting the same key is harmless.
                    index.families.insert(key, path.to_path_buf());
                }
            }
        }
    }
}

/// Wire point used by the shell right after `load_document`: compute the
/// kernel-side font requirements, resolve them against the (lazily
/// scanned) system index, and register every found family's bytes into
/// the component. Each registration bumps the font generation and
/// invalidates the page caches; the gpui-side font cache picks the same
/// Arcs up at scene replay.
pub fn register_required_system_fonts(component: &mut OfdComponent) {
    let required = component.font_requirements();
    if required.families.is_empty() {
        return; // Nothing declared: the startup default font covers it.
    }
    let index = SystemFontIndex::shared();
    for bytes in index.load_families(&required) {
        // The register result only says whether the bytes parsed as a
        // font; a false leaves the family on the default-font fallback
        // chain, which is the correct degradation.
        let _ = component.register_font_data(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OFL fixtures (same bytes as the render crate's font tests and the
    /// tauri font channel). Name-table ground truth, verified against the
    /// files: TestFont.ttf exposes family "DejaVu Sans" (name IDs 1 and 16,
    /// Mac + Windows records); TestFontCjk.otf exposes "Noto Sans CJK SC"
    /// (name ID 1). No invented mappings beyond these.
    const TESTFONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");
    const TESTFONT_CJK: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFontCjk.otf");

    /// Fresh temp dir per test (tests must never scan the real system).
    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rofd-gpui-font-index-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Requirements carrying pre-normalized family keys.
    fn req(families: &[&str]) -> FontRequirements {
        FontRequirements {
            families: families.iter().map(|s| s.to_string()).collect(),
            codepoints: Vec::new(),
        }
    }

    fn index_both_fixtures(tag: &str) -> (SystemFontIndex, PathBuf) {
        let dir = fixture_dir(tag);
        std::fs::write(dir.join("TestFont.ttf"), TESTFONT).unwrap();
        std::fs::write(dir.join("TestFontCjk.otf"), TESTFONT_CJK).unwrap();
        let index = SystemFontIndex::scan_dirs(std::slice::from_ref(&dir));
        (index, dir)
    }

    #[test]
    fn extracts_and_serves_family_from_name_table() {
        let (index, dir) = index_both_fixtures("extract");
        // "DejaVu Sans" is unknown to the kernel alias table: its key is
        // the trimmed lowercase form (inner spaces kept — that is what the
        // kernel does, so that is what the index must key on).
        let blobs = index.load_families(&req(&["dejavu sans"]));
        assert_eq!(blobs.len(), 1, "one file carries the family");
        assert_eq!(blobs[0], TESTFONT, "served bytes are the file's bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kernel_normalized_alias_key_is_served() {
        let (index, dir) = index_both_fixtures("alias");
        // A document declaring "Noto Sans CJK SC" produces the alias-table
        // key on the kernel side; the index must serve the same file under
        // that key (name-table name and doc declaration meet in the same
        // normalized universe).
        let key = normalize_family_key("Noto Sans CJK SC");
        assert_eq!(key, "notosanscjksc");
        let blobs = index.load_families(&req(&[&key]));
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0], TESTFONT_CJK);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalization_folds_case_and_outer_whitespace() {
        let (index, dir) = index_both_fixtures("fold");
        // Case folding + trim, computed by the kernel function the index
        // keys on (what font_req produces for document declarations).
        for spelled in ["DejaVu Sans", "dejavu sans", "  DEJAVU SANS  "] {
            let key = normalize_family_key(spelled);
            assert_eq!(key, "dejavu sans", "spelling: {spelled:?}");
            assert!(
                index.resolve(&key).is_some(),
                "key {key:?} must resolve (spelling: {spelled:?})"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absent_key_contributes_nothing() {
        let (index, dir) = index_both_fixtures("absent");
        assert!(index.load_families(&req(&["no-such-family"])).is_empty());
        // Mixed hit + miss: only the hit's bytes come back.
        let blobs = index.load_families(&req(&["dejavu sans", "no-such-family"]));
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0], TESTFONT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_family_in_later_directory_overrides() {
        // Windows per-user override semantics: the same family scanned
        // from a later (higher-priority) directory wins the key.
        let low = fixture_dir("prio-low");
        let high = fixture_dir("prio-high");
        std::fs::write(low.join("a.ttf"), TESTFONT).unwrap();
        std::fs::write(high.join("b.ttf"), TESTFONT).unwrap();
        let index = SystemFontIndex::scan_dirs(&[low.clone(), high.clone()]);
        assert_eq!(
            index.resolve("dejavu sans"),
            Some(high.join("b.ttf").as_path()),
            "later directory must win the family key"
        );
        let _ = std::fs::remove_dir_all(&low);
        let _ = std::fs::remove_dir_all(&high);
    }

    #[test]
    fn scan_never_fails_on_garbage_and_missing_dirs() {
        let dir = fixture_dir("garbage");
        std::fs::write(dir.join("broken.ttf"), b"not a font at all").unwrap();
        std::fs::write(dir.join("ignored.txt"), b"skip me").unwrap();
        let index =
            SystemFontIndex::scan_dirs(&[dir.clone(), PathBuf::from("Z:/definitely/absent")]);
        assert!(
            index.is_empty(),
            "garbage bytes and non-font extensions are skipped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
