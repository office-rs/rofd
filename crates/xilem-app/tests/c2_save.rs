//! Host-layer real-sample integration test for the preserving save.
//!
//! The binary crate's helpers are reached through the `xilem_app` library
//! surface. Exercises the full host path — `document_io::load_ofd` (retains
//! PackageHandle) → `document_io::save_ofd` (preserving save, spec
//! 2026-11-23 §7) — and asserts at the host layer: the saved package
//! re-parses to the same model, every non-annotation entry is byte-identical,
//! and Document.xml (an unsigned package carrying annotations) differs at
//! most in its MaxUnitID value.
//!
//! Marked `#[ignore]`: `test/ru-yuan-ji-lu.ofd` is gitignored (not in CI).
//! Run locally:
//! `cargo test -p xilem-app --test c2_save -- --ignored`.

use std::path::Path;

use rofd_dom::TextLayoutHints;
use rofd_io::zip_util::read_all_entries;
use xilem_app::host::document_io::{load_ofd, save_ofd};

/// `xml` with the value of its MaxUnitID element masked.
fn mask_max_unit_id(xml: &str) -> String {
    let Some(tag) = xml.find("MaxUnitID") else {
        return xml.to_owned();
    };
    let a = tag + xml[tag..].find('>').expect("MaxUnitID start tag ends") + 1;
    let b = a + xml[a..].find('<').expect("MaxUnitID end tag starts");
    format!("{}N{}", &xml[..a], &xml[b..])
}

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn host_layer_preserving_save_keeps_model_and_body_entries() {
    // Integration tests run with the package dir as CWD; the workspace
    // root sample is at ../../test (same convention as the io/render tests).
    let source = Path::new("../../test/ru-yuan-ji-lu.ofd");
    let bytes = std::fs::read(source).expect("test sample present");

    let loaded = load_ofd(source).expect("load_ofd succeeds on real sample");
    assert!(
        loaded.document.package.is_some(),
        "package retained after load_ofd"
    );

    // Save to a temp destination: keep the source untouched.
    let target = std::env::temp_dir().join(format!("rofd_c2_{}.ofd", std::process::id()));
    save_ofd(&loaded.document, &TextLayoutHints::new(), &target)
        .expect("save_ofd succeeds on real sample");
    let saved = std::fs::read(&target).expect("saved file present");

    // The saved package re-parses to the same model.
    let reparsed = rofd_io::parse_ofd(&saved).expect("saved package re-parses");
    assert_eq!(
        reparsed.document.pages, loaded.document.pages,
        "pages survive the host-layer preserving save"
    );
    assert_eq!(
        reparsed.document.annotations, loaded.document.annotations,
        "annotations survive the host-layer preserving save"
    );

    // Every non-annotation entry stays byte-identical; Document.xml may
    // move its MaxUnitID only.
    let package = loaded.document.package.as_ref().expect("package");
    let document_entry = package
        .document_entry
        .as_deref()
        .expect("document entry recorded");
    let orig_entries = read_all_entries(&bytes).expect("read original entries");
    let saved_entries = read_all_entries(&saved).expect("read saved entries");
    for (name, data) in &orig_entries {
        let is_annotation = package
            .entries
            .iter()
            .find(|e| &e.name == name)
            .is_some_and(|e| e.kind == rofd_io::EntryKind::Annotation);
        if is_annotation {
            continue;
        }
        let saved_entry = saved_entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("entry {name} missing after the save"));
        if name == document_entry {
            let before = String::from_utf8_lossy(data);
            let after = String::from_utf8_lossy(&saved_entry.1);
            assert_eq!(
                mask_max_unit_id(&after),
                mask_max_unit_id(&before),
                "Document.xml differs only in MaxUnitID"
            );
        } else {
            assert_eq!(&saved_entry.1, data, "entry {name} byte-identical");
        }
    }
    let _ = std::fs::remove_file(&target);
}
