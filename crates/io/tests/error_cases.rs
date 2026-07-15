//! Error / warning test cases for io parse (C4 Task 1).
//!
//! Verifies:
//! - Bad ZIP -> `OfdError::Zip`
//! - Bad XML -> `OfdError::Xml`
//! - Template page element -> `OfdWarning::MissingFeature` (v1 doesn't expand)
//! - Unknown page element -> `OfdWarning::SkippedObject` (skipped, not fatal)
//! - Missing font file -> `OfdWarning::FontSubstituted`
//! - Missing image file -> `OfdWarning::ResourceNotFound`

#[path = "fixtures/fixtures.rs"]
mod fixtures;

use rofd_io::{parse_ofd, OfdError, OfdWarning, ResourceKind};

#[test]
fn bad_zip_returns_zip_error() {
    let bytes = b"not a zip file";
    let err = parse_ofd(bytes).unwrap_err();
    assert!(
        matches!(err, OfdError::Zip { .. }),
        "bad zip -> OfdError::Zip, got {err:?}"
    );
}

#[test]
fn bad_xml_returns_xml_error() {
    let bytes = fixtures::build_ofd_with_malformed_xml();
    let err = parse_ofd(&bytes).unwrap_err();
    assert!(
        matches!(err, OfdError::Xml { .. }),
        "bad xml -> OfdError::Xml, got {err:?}"
    );
}

#[test]
fn template_annotation_emits_missing_feature_warning() {
    let bytes = fixtures::build_ofd_with_template();
    let report = parse_ofd(&bytes).expect("parse succeeds (template is a warning, not error)");
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            OfdWarning::MissingFeature { feature, .. } if feature == "Template"
        )),
        "template page -> MissingFeature(Template) warning, got {:?}",
        report.warnings
    );
}

#[test]
fn unknown_page_element_emits_skipped_object_warning() {
    let bytes = fixtures::build_ofd_with_unknown_page_element();
    let report = parse_ofd(&bytes).expect("parse succeeds (unknown element is a warning)");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, OfdWarning::SkippedObject { .. })),
        "unknown element -> SkippedObject warning, got {:?}",
        report.warnings
    );
}

#[test]
fn missing_font_file_emits_font_substituted_warning() {
    let bytes = fixtures::build_ofd_with_missing_font_file();
    let report = parse_ofd(&bytes).expect("parse succeeds (missing font is a warning)");
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            OfdWarning::FontSubstituted { requested, .. } if requested == "NotoSans"
        )),
        "missing font file -> FontSubstituted warning, got {:?}",
        report.warnings
    );
}

#[test]
fn missing_image_file_emits_resource_not_found_warning() {
    let bytes = fixtures::build_ofd_with_missing_image();
    let report = parse_ofd(&bytes).expect("parse succeeds (missing image is a warning)");
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            OfdWarning::ResourceNotFound { kind: ResourceKind::Image, id } if id == "9"
        )),
        "missing image file -> ResourceNotFound(Image) warning, got {:?}",
        report.warnings
    );
}
