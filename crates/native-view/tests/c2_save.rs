//! App-layer real-sample integration test for Cluster 2 (surgical save call
//! chain). Distinct from the io-layer `crates/io/tests/real_sample.rs` (C1),
//! this exercises the full app-layer path: `EditorApp::load_ofd` (retains
//! `PackageHandle`) -> `EditorApp::save_ofd` (routes to surgical `save_ofd`
//! when a package is present) and asserts invariant 4.3 (body `Content.xml`
//! entries byte-identical) holds at the app layer.
//!
//! Marked `#[ignore]`: the sample `test/ru-yuan-ji-lu.ofd` is gitignored (not
//! in CI). Run locally with
//! `cargo test -p rofd-native-view --test c2_save -- --ignored`.

use std::sync::Arc;

use rofd_component::EditorConfig;
use rofd_io::zip_util::read_all_entries;
use rofd_native_view::EditorApp;

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn app_layer_surgical_save_preserves_body() {
    // Cargo runs integration tests with the crate dir as CWD, so the
    // workspace-root sample is at ../../test/ru-yuan-ji-lu.ofd (matches
    // crates/render/tests/render_smoke.rs and crates/io/tests/real_sample.rs).
    let bytes = std::fs::read("../../test/ru-yuan-ji-lu.ofd").expect("test sample present");

    let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
    app.load_ofd(&bytes)
        .expect("load_ofd succeeds on real sample");
    assert!(app.package.is_some(), "package retained after load_ofd");

    let saved = app.save_ofd().expect("save_ofd succeeds on real sample");

    // Body Content.xml entries byte-identical before/after (invariant 4.3 at
    // the app layer - surgical save preserves unmodelled body).
    let orig_entries = read_all_entries(&bytes).expect("read original entries");
    let saved_entries = read_all_entries(&saved).expect("read saved entries");

    let body_names: Vec<&str> = orig_entries
        .iter()
        .filter(|(n, _)| n.ends_with("Content.xml"))
        .map(|(n, _)| n.as_str())
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
            "body {name} byte-identical via app-layer surgical save"
        );
    }
}
