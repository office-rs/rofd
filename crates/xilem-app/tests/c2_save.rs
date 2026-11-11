//! Host-layer real-sample integration test for the full write.
//!
//! After the winit bridge removal; the binary crate's helpers are reached
//! through the `xilem_app` library surface. Exercises the full host path —
//! `document_io::load_ofd` (retains PackageHandle) →
//! `document_io::save_ofd` (full write with passthrough transport) — and
//! asserts write fidelity at the host layer (spec 2026-11-11 D2/D12): the
//! saved package re-parses to the same model, and entries parse never
//! consumed (signatures/attachments/unknown files) stay byte-identical.
//!
//! Marked `#[ignore]`: `test/ru-yuan-ji-lu.ofd` is gitignored (not in CI).
//! Run locally:
//! `cargo test -p xilem-app --test c2_save -- --ignored`.

use std::path::Path;

use rofd_dom::TextLayoutHints;
use rofd_io::zip_util::read_all_entries;
use xilem_app::host::document_io::{load_ofd, save_ofd};

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn host_layer_full_write_preserves_model_and_unconsumed_entries() {
    // Integration tests run with the package dir as CWD; the workspace
    // root sample is at ../../test (same convention as the io/render tests).
    let source = Path::new("../../test/ru-yuan-ji-lu.ofd");
    let bytes = std::fs::read(source).expect("test sample present");

    let loaded = load_ofd(source).expect("load_ofd succeeds on real sample");
    assert!(loaded.package.is_some(), "package retained after load_ofd");

    // Save to a temp destination: keep the source untouched.
    let target = std::env::temp_dir().join(format!("rofd_c2_{}.ofd", std::process::id()));
    save_ofd(
        &loaded.document,
        loaded.package.as_ref(),
        &TextLayoutHints::new(),
        &target,
    )
    .expect("save_ofd succeeds on real sample");
    let saved = std::fs::read(&target).expect("saved file present");

    // Write fidelity: the saved package re-parses to the same model.
    let reparsed = rofd_io::parse_ofd(&saved).expect("saved package re-parses");
    assert_eq!(
        reparsed.document.pages, loaded.document.pages,
        "pages survive the host-layer full write"
    );
    assert_eq!(
        reparsed.document.annotations, loaded.document.annotations,
        "annotations survive the host-layer full write"
    );

    // Unconsumed entries transport byte-identical (D12).
    let orig_entries = read_all_entries(&bytes).expect("read original entries");
    let saved_entries = read_all_entries(&saved).expect("read saved entries");
    let package = loaded.package.as_ref().expect("package");
    for (name, data) in &orig_entries {
        if package.consumed.contains(name.as_str()) {
            continue;
        }
        let saved_entry = saved_entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("unconsumed entry {name} missing after transport"));
        assert_eq!(
            &saved_entry.1, data,
            "unconsumed entry {name} byte-identical"
        );
    }
    let _ = std::fs::remove_file(&target);
}
