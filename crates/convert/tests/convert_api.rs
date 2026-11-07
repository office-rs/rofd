//! Skeleton API tests for the rofd-convert crate (convert spec phase 1,
//! Task 3): the panic boundary and the thread-contract pin. Pipeline and
//! registry behavior arrive in Tasks 4/5.

use rofd_convert::{catch_panics, ConvertError, Converter};

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
