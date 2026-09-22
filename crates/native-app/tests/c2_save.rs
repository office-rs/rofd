//! Host-layer real-sample integration test for surgical save.
//!
//! Migrated from rofd-native-view after the winit bridge removal; the
//! binary crate's helpers are reached through the `native_app` library
//! surface. Exercises the full host path —
//! `document_io::load_ofd` (retains PackageHandle) →
//! `document_io::save_ofd` (routes to surgical save for a package) — and
//! asserts invariant 4.3 (body `Content.xml` byte-identical) at the host
//! layer.
//!
//! Marked `#[ignore]`: `test/ru-yuan-ji-lu.ofd` is gitignored (not in CI).
//! Run locally:
//! `cargo test -p native-app --test c2_save -- --ignored`.

use std::path::Path;

use native_app::host::document_io::{load_ofd, save_ofd};
use rofd_io::zip_util::read_all_entries;

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn host_layer_surgical_save_preserves_body() {
    // Integration tests run with the package dir as CWD; the workspace
    // root sample is at ../../test (same convention as the io/render tests).
    let source = Path::new("../../test/ru-yuan-ji-lu.ofd");
    let bytes = std::fs::read(source).expect("test sample present");

    let loaded = load_ofd(source).expect("load_ofd succeeds on real sample");
    assert!(loaded.package.is_some(), "package retained after load_ofd");

    // Save to a temp destination: keep the source untouched.
    let target = std::env::temp_dir().join(format!("rofd_c2_{}.ofd", std::process::id()));
    save_ofd(&loaded.document, loaded.package.as_ref(), &target)
        .expect("save_ofd succeeds on real sample");
    let saved = std::fs::read(&target).expect("saved file present");

    // Body Content.xml entries byte-identical before/after (invariant 4.3
    // at the host layer — surgical save preserves unmodelled body).
    let orig_entries = read_all_entries(&bytes).expect("read original entries");
    let saved_entries = read_all_entries(&saved).expect("read saved entries");

    let body_names: Vec<&str> = orig_entries
        .iter()
        .filter(|(name, _)| name.ends_with("Content.xml"))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        !body_names.is_empty(),
        "real sample has at least one body Content.xml entry"
    );

    for name in body_names {
        let orig = orig_entries
            .iter()
            .find(|(n, _)| n == name)
            .expect("orig entry exists");
        let saved_entry = saved_entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("saved body entry {name} missing"));
        assert_eq!(
            orig.1, saved_entry.1,
            "body {name} byte-identical via host-layer surgical save"
        );
    }
    let _ = std::fs::remove_file(&target);
}
