//! Skeleton API tests for the rofd-convert crate (convert spec phase 1,
//! Task 3): the panic boundary and the thread-contract pin. Pipeline and
//! registry behavior arrive in Tasks 4/5.

use rofd_convert::{catch_panics, ConvertError, Converter};

const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

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
    rofd_convert::register_font(FONT.to_vec()).unwrap();
    rofd_convert::register_font(FONT.to_vec()).unwrap(); // fingerprint dedup, no second registration
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
    rofd_convert::register_default_font(FONT.to_vec()).unwrap();
    assert!(rofd_convert::has_default_font());
}
