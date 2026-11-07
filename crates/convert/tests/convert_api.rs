//! API tests for the rofd-convert crate: the panic boundary, the
//! thread-contract pin, the font registry (phase 1 Tasks 3/4), and the
//! one-call conversion pipeline's error/budget/warning surface (Task 5).
//! The registry-anchoring content-flag test lives in `convert_pipeline`
//! (a separate binary, so this binary's fresh-registry assertions stay
//! order-independent).

mod common;

use rofd_convert::{catch_panics, ConvertError, ConvertOptions, Converter, OutputFormat};
use rofd_dom::{FontId, ImageId, OfdDocument};

use common::{
    convert_default, fixture_bytes, mediabox_count, page, repack_with_resources, TEST_FONT,
};

#[test]
fn catch_panics_converts_to_internal() {
    // Non-panicking closures pass their result through untouched.
    let r = catch_panics(|| Err::<(), _>(ConvertError::Decode("x".into())));
    assert!(matches!(r, Err(ConvertError::Decode(_))));
    // Panicking closures surface as Internal carrying the panic message.
    let r = catch_panics(|| -> Result<(), ConvertError> { panic!("boom") });
    assert!(matches!(r, Err(ConvertError::Internal(m)) if m.contains("boom")));
}

#[test]
fn converter_is_send_today() {
    // D15 (revised 2026-11-07): FontStore is built per call inside
    // `convert`; the Converter itself only ever holds Arc'd registry
    // snapshots, so it is `Send` today. This test pins that status; if a
    // warm engine holding a FontStore ever lands, this pin (and the
    // documented contract) changes together with it.
    fn assert_send<T: Send>() {}
    assert_send::<Converter>();
}

#[test]
fn register_font_records_family_and_is_idempotent() {
    rofd_convert::register_font(TEST_FONT.to_vec()).unwrap();
    rofd_convert::register_font(TEST_FONT.to_vec()).unwrap(); // fingerprint dedup, no second registration
    let fams = rofd_convert::registered_families();
    assert!(
        !fams.is_empty(),
        "probe must surface the test font's family"
    );
    assert_eq!(
        fams.len(),
        fams.iter().collect::<std::collections::HashSet<_>>().len()
    );
}

#[test]
fn invalid_font_bytes_rejected() {
    assert!(matches!(
        rofd_convert::register_font(vec![0u8; 16]),
        Err(rofd_convert::FontRegisterError::InvalidFont)
    ));
}

#[test]
fn default_font_flag_roundtrip() {
    assert!(!rofd_convert::has_default_font());
    rofd_convert::register_default_font(TEST_FONT.to_vec()).unwrap();
    assert!(rofd_convert::has_default_font());
}

// ---------------------------------------------------------------------------
// Task 5: the one-call conversion pipeline (budgets + warnings + flags)
// ---------------------------------------------------------------------------

#[test]
fn converts_to_pdf_bytes() {
    let out = convert_default(&fixture_bytes());
    assert!(out.bytes.starts_with(b"%PDF"), "PDF magic header");
    assert!(!out.bytes.is_empty());
    assert_eq!(mediabox_count(&out.bytes), 2, "two pages -> two MediaBox");
}

#[test]
fn corrupt_bytes_is_decode_error() {
    let r = Converter::new().convert(OutputFormat::Pdf, b"not a zip", &ConvertOptions::default());
    assert!(matches!(r, Err(ConvertError::Decode(_))));
    // Guard order: budgets run AFTER parse, so a corrupt package is a
    // Decode error even with the tightest budgets.
    let tight = ConvertOptions {
        max_pages: Some(0),
        max_media_bytes: Some(0),
        ..ConvertOptions::default()
    };
    let r = Converter::new().convert(OutputFormat::Pdf, b"not a zip", &tight);
    assert!(
        matches!(r, Err(ConvertError::Decode(_))),
        "decode errors win over budget guards"
    );
}

#[test]
fn zero_page_doc_is_encode_error() {
    // A parsed-but-empty document reaches the encoder with zero page
    // scenes; `pdf_from_page_scenes` rejects the empty page list, which
    // surfaces as Encode.
    let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
    let r = Converter::new().convert(OutputFormat::Pdf, &bytes, &ConvertOptions::default());
    assert!(matches!(r, Err(ConvertError::Encode(_))));
}

#[test]
fn pages_budget_exceeded() {
    // End-to-end: write_ofd serializes all three pages, parse counts them,
    // the guard trips before any scene is built.
    let mut doc = common::two_page_doc();
    doc.pages.push(page("P2"));
    let bytes = rofd_io::write_ofd(&doc).unwrap();
    let opts = ConvertOptions {
        max_pages: Some(2),
        ..ConvertOptions::default()
    };
    let r = Converter::new().convert(OutputFormat::Pdf, &bytes, &opts);
    assert!(matches!(
        r,
        Err(ConvertError::BudgetExceeded {
            what: "pages",
            limit: 2,
            actual: 3,
        })
    ));
}

#[test]
fn media_bytes_sums_images_and_font_data() {
    // The media budget's measure, standalone (write_ofd's v1 full-write
    // emits no resource entries, so the measure cannot be exercised through
    // plain fixtures — same posture as rword's check_media_budget).
    let mut doc = common::two_page_doc();
    doc.resources
        .images
        .insert(ImageId::new("I1"), std::sync::Arc::new(vec![0u8; 4096]));
    doc.resources
        .font_data
        .insert(FontId::new("F1"), std::sync::Arc::new(vec![0u8; 512]));
    assert_eq!(rofd_convert::media_bytes(&doc), 4096 + 512);
}

#[test]
fn media_bytes_budget_exceeded() {
    // End-to-end via a repacked package carrying a real resource entry:
    // the guard sees the parsed image bytes and trips before scene
    // building (the payload need not be a decodable image — nothing is
    // rendered).
    let bytes = repack_with_resources(
        &fixture_bytes(),
        &[
            (
                "Doc_0/Res/DocumentRes.xml".to_string(),
                common::document_res_xml("IMG-BIG", "big.bin").into_bytes(),
            ),
            ("Doc_0/Res/big.bin".to_string(), vec![0u8; 4096]),
        ],
    );
    let opts = ConvertOptions {
        max_media_bytes: Some(1024),
        ..ConvertOptions::default()
    };
    let r = Converter::new().convert(OutputFormat::Pdf, &bytes, &opts);
    assert!(matches!(
        r,
        Err(ConvertError::BudgetExceeded {
            what: "media_bytes",
            limit: 1024,
            actual: 4096,
        })
    ));
}

#[test]
fn warnings_passthrough_equals_parse_report() {
    let (a, b) = (
        convert_default(&fixture_bytes()).warnings,
        rofd_io::parse_ofd(&fixture_bytes()).unwrap().warnings,
    );
    assert_eq!(a, b, "convert warnings must equal the parse report's");
}
