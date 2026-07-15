//! Real-sample integration test against the gitignored local OFD
//! `test/ru-yuan-ji-lu.ofd`. Marked `#[ignore]` so CI does not run it (the
//! sample is not checked in); run locally with
//! `cargo test -p rofd-io --test real_sample -- --ignored`.
//!
//! Verifies the two end-to-end invariants of Cluster 1:
//! 1. `parse_ofd` recovers the 4 known annotations on PageID "1" and parses
//!    `MaxUnitID` (>= 1500) from Document.xml.
//! 2. `save_ofd` (surgical) preserves every body `Content.xml` entry
//!    byte-for-byte (invariant 4.3) while rewriting only annotation entries.

use rofd_dom::{AnnotationKind, PageId, ShapeKind};

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn real_sample_parses_and_surgically_saves() {
    // Cargo runs integration tests with the crate dir as CWD, so the
    // workspace-root sample is at ../../test/ru-yuan-ji-lu.ofd.
    let bytes = std::fs::read("../../test/ru-yuan-ji-lu.ofd").expect("test sample present");

    let report = rofd_io::parse_ofd(&bytes).expect("parse_ofd succeeds on real sample");

    // 4 annotations on PageID "1": Underline, Strikeout, Squiggly, Rectangle.
    let anns = report.document.annotations.for_page(&PageId::new("1"));
    assert_eq!(
        anns.len(),
        4,
        "4 annots (Underline/Strikeout/Squiggly/Rectangle)"
    );
    assert!(
        anns.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Underline)),
        "Underline present"
    );
    assert!(
        anns.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Strikeout)),
        "Strikeout present"
    );
    assert!(
        anns.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Squiggly)),
        "Squiggly present (parsed natively, not degraded to Highlight)"
    );
    assert!(
        anns.iter()
            .any(|a| matches!(a.kind, AnnotationKind::Shape(ShapeKind::Rect))),
        "Shape(Rect) present (Path/Subtype=Rectangle)"
    );

    // MaxUnitID parsed from Document.xml CommonData.
    assert!(
        report.document.max_unit_id >= 1500,
        "MaxUnitID parsed (got {})",
        report.document.max_unit_id
    );

    // Surgical save: body Content.xml entries byte-identical before/after (invariant 4.3).
    let saved = rofd_io::save_ofd(&report.document, &report.package)
        .expect("save_ofd succeeds on real sample");
    let orig_entries = rofd_io::zip_util::read_all_entries(&bytes).expect("read original entries");
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).expect("read saved entries");

    for name in orig_entries
        .iter()
        .filter(|(n, _)| n.ends_with("Content.xml"))
        .map(|(n, _)| n.as_str())
    {
        let orig = orig_entries
            .iter()
            .find(|(n, _)| n == name)
            .expect("orig entry exists");
        let saved_entry = saved_entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("saved entry {name} missing"));
        assert_eq!(
            orig.1, saved_entry.1,
            "body {name} byte-identical after surgical save"
        );
    }
}
