//! rofd-convert-java: JNI surface over the rofd-convert kernel (convert
//! spec §6.4, phase 2). One cdylib (`rofdconvert`), Java 8 compatible L2
//! classes in `io/github/officers/`.
//!
//! FFI discipline shared by every binding crate (spec §6.1): each entry
//! point runs under `rofd_convert::catch_panics`; failures surface as
//! `io.github.officers.OfdConvertException` — a Rust panic never crosses
//! the boundary.
//!
//! The Java-side counterparts are declared in
//! `io/github/officers/NativeLink.java` (L1) and wrapped by the L2
//! converter class (phase 2 Task 5). Argument raw handles arriving from
//! the JVM are wrapped in the jni crate's typed wrappers
//! (`JByteArray::from_raw` etc.) — safe by the JNI contract that the JVM
//! passes valid local references.

use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jobjectArray, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub mod handles;

use handles::{HandleTable, TakeError};

/// JVM class name of the exception every failure maps onto.
const EXCEPTION_CLASS: &str = "io/github/officers/OfdConvertException";

static TABLE: LazyLock<HandleTable> = LazyLock::new(HandleTable::new);

/// Per-handle warnings of the last SUCCESSFUL `nConvert` call, keyed by
/// handle id. The slot is cleared at the start of every attempt (so a
/// failed convert leaves it empty — never a stale pre-failure set) and
/// drained by `nClose`; `nTakeWarnings` drains and clears it.
static LAST_WARNINGS: LazyLock<Mutex<HashMap<u64, Vec<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Throw the boundary exception with `msg` (best effort — JNIEnv calls
/// can fail while an exception is already pending).
fn throw(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new(EXCEPTION_CLASS, msg);
}

/// Map a kernel error onto the boundary exception.
fn throw_convert(env: &mut JNIEnv, e: &rofd_convert::ConvertError) {
    throw(env, &e.to_string());
}

/// Map a lease failure with a stable, L2-recognizable message prefix.
fn throw_take(env: &mut JNIEnv, e: TakeError) {
    match e {
        TakeError::InUse => throw(
            env,
            "converter handle in use: one call at a time per instance",
        ),
        TakeError::Stale => throw(env, "stale converter handle (closed or never created)"),
    }
}

/// `JString` argument → `Option<String>` (`None` for a null reference or
/// an empty string — empty carries no information for path fields).
fn opt_string(env: &mut JNIEnv, raw: jstring) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    let s = unsafe { JString::from_raw(raw) };
    let java = env.get_string(&s).ok()?.to_string_lossy().into_owned();
    if java.is_empty() {
        None
    } else {
        Some(java)
    }
}

/// `Vec<String>` → Java `String[]`.
fn string_array(env: &mut JNIEnv, items: Vec<String>) -> jobjectArray {
    let Ok(arr) = env.new_object_array(items.len() as i32, "java/lang/String", JObject::null())
    else {
        // Allocation failed with an exception pending; the JVM will throw
        // it when the native method returns.
        return std::ptr::null_mut();
    };
    for (i, item) in items.iter().enumerate() {
        if let Ok(jstr) = env.new_string(item) {
            let _ = env.set_object_array_element(&arr, i as i32, jstr);
        }
    }
    arr.as_raw()
}

fn bool_to_jboolean(b: bool) -> jboolean {
    if b {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// JVM boolean (u8: 0 = false, anything else = true — the JNI contract
/// only ever sends 0/1, non-zero is the defensive read).
fn from_jboolean(b: jboolean) -> bool {
    b != 0
}

/// `jlong` budget knob → `Option<u32>`. Per the Java signature contract,
/// `<= 0` means "not set" (kernel defaults apply). Values above the
/// target type's range also fall back to unset rather than wrapping.
fn opt_u32(v: jlong) -> Option<u32> {
    if v <= 0 {
        None
    } else {
        u32::try_from(v).ok()
    }
}

/// Same as [`opt_u32`] for the usize media budget.
fn opt_usize(v: jlong) -> Option<usize> {
    if v <= 0 {
        None
    } else {
        usize::try_from(v).ok()
    }
}

/// `formatOrdinal` → [`OutputFormat`]. v1 has a single arm (pdf = 0);
/// the mapping stays a pure function so future enum arms extend here
/// without touching the Java method signature.
fn map_format(ordinal: i32) -> Result<rofd_convert::OutputFormat, String> {
    match ordinal {
        0 => Ok(rofd_convert::OutputFormat::Pdf),
        other => Err(format!("format ordinal {other} is not 0 (pdf)")),
    }
}

/// The three content flags + budget knobs → [`ConvertOptions`].
fn build_options(
    watermark: bool,
    annotations: bool,
    seal: bool,
    max_pages: jlong,
    max_media_bytes: jlong,
) -> rofd_convert::ConvertOptions {
    rofd_convert::ConvertOptions {
        content: rofd_convert::PrintContent {
            watermark,
            annotations,
            seal,
        },
        max_pages: opt_u32(max_pages),
        max_media_bytes: opt_usize(max_media_bytes),
    }
}

/// Kernel `OfdWarning`s → one display line each (the strings Java's
/// `takeWarnings` returns).
fn warnings_to_strings(warnings: Vec<rofd_convert::OfdWarning>) -> Vec<String> {
    warnings.iter().map(|w| w.to_string()).collect()
}

/// Park a call's warnings under its handle id (overwrites any previous
/// parked set — `takeWarnings` documents last-call-wins semantics).
fn park_warnings(id: u64, warnings: Vec<rofd_convert::OfdWarning>) {
    LAST_WARNINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, warnings_to_strings(warnings));
}

/// Remove the handle's parked warnings without reading them: the start
/// of every `nConvert` attempt (a failed convert must not leave the
/// previous call's set behind) and every `nClose` path that removes the
/// handle (a closed id has no parked warnings, and its slot does not
/// linger for the lifetime of the process).
fn clear_warnings(id: u64) {
    LAST_WARNINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
}

/// Drain the handle's parked warnings (empty when none were parked —
/// the Java side always gets an array).
fn take_warnings(id: u64) -> Vec<String> {
    LAST_WARNINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id)
        .unwrap_or_default()
}

/// Kernel font-registration result → the boolean JNI contract: `Ok(true)`
/// registered, `Ok(false)` = unparsable font (caller feedback, not an
/// error), `Err(msg)` = io trouble (exception on the Java side). The
/// catch-all arm keeps this future-proof for new `FontRegisterError`
/// variants (the kernel enum is `#[non_exhaustive]`).
fn map_font_register_result(
    r: Result<(), rofd_convert::FontRegisterError>,
) -> Result<bool, String> {
    match r {
        Ok(()) => Ok(true),
        Err(rofd_convert::FontRegisterError::InvalidFont) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nNewConverter(
    env: JNIEnv,
    _class: JClass,
) -> jlong {
    let mut env = env;
    match rofd_convert::catch_panics(|| Ok(TABLE.insert(rofd_convert::Converter::new()) as jlong)) {
        Ok(id) => id,
        Err(e) => {
            throw_convert(&mut env, &e);
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nClose(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jboolean {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| Ok(TABLE.close(id as u64)));
    match r {
        // Stale close (already closed / never existed) reports false —
        // harmless for the PhantomReference last-resort path, which races
        // explicit closes by design.
        Ok(Ok(())) => {
            clear_warnings(id as u64);
            JNI_TRUE
        }
        Ok(Err(TakeError::Stale)) => JNI_FALSE,
        Ok(Err(TakeError::InUse)) => {
            // close() while leased removes the map entry; the lease's
            // put() no-ops. The instance is released either way.
            clear_warnings(id as u64);
            JNI_TRUE
        }
        Err(e) => {
            throw_convert(&mut env, &e);
            JNI_FALSE
        }
    }
}

#[no_mangle]
// Raw array handle arrives from the JVM (valid local ref by the JNI
// contract); these exports are never called from Rust code.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn Java_io_github_officers_NativeLink_nRegisterFont(
    env: JNIEnv,
    _class: JClass,
    bytes: jbyteArray,
) -> jboolean {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| -> Result<bool, rofd_convert::ConvertError> {
        let arr = unsafe { JByteArray::from_raw(bytes) };
        let data = env
            .convert_byte_array(&arr)
            .map_err(|e| rofd_convert::ConvertError::Internal(format!("byte[] access: {e:?}")))?;
        map_font_register_result(rofd_convert::register_font(data))
            .map_err(rofd_convert::ConvertError::Internal)
    });
    match r {
        Ok(ok) => bool_to_jboolean(ok),
        Err(e) => {
            throw_convert(&mut env, &e);
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nRegisterFontFile(
    env: JNIEnv,
    _class: JClass,
    path: jstring,
) -> jboolean {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| -> Result<bool, rofd_convert::ConvertError> {
        let p = opt_string(&mut env, path).ok_or_else(|| {
            rofd_convert::ConvertError::Internal("font path must be non-empty".into())
        })?;
        map_font_register_result(rofd_convert::register_font_file(&p))
            .map_err(rofd_convert::ConvertError::Internal)
    });
    match r {
        Ok(ok) => bool_to_jboolean(ok),
        Err(e) => {
            throw_convert(&mut env, &e);
            JNI_FALSE
        }
    }
}

/// Anchor the default font (kernel D19): same boolean contract as
/// [`nRegisterFont`](Java_io_github_officers_NativeLink_nRegisterFont).
#[no_mangle]
// Raw array handle arrives from the JVM (valid local ref by the JNI
// contract); this export is never called from Rust code.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn Java_io_github_officers_NativeLink_nRegisterDefaultFont(
    env: JNIEnv,
    _class: JClass,
    bytes: jbyteArray,
) -> jboolean {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| -> Result<bool, rofd_convert::ConvertError> {
        let arr = unsafe { JByteArray::from_raw(bytes) };
        let data = env
            .convert_byte_array(&arr)
            .map_err(|e| rofd_convert::ConvertError::Internal(format!("byte[] access: {e:?}")))?;
        map_font_register_result(rofd_convert::register_default_font(data))
            .map_err(rofd_convert::ConvertError::Internal)
    });
    match r {
        Ok(ok) => bool_to_jboolean(ok),
        Err(e) => {
            throw_convert(&mut env, &e);
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nHasDefaultFont(
    env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| Ok(rofd_convert::has_default_font()));
    match r {
        Ok(has) => bool_to_jboolean(has),
        Err(e) => {
            throw_convert(&mut env, &e);
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nRegisteredFamilies(
    env: JNIEnv,
    _class: JClass,
) -> jobjectArray {
    let mut env = env;
    match rofd_convert::catch_panics(|| Ok(rofd_convert::registered_families())) {
        Ok(families) => string_array(&mut env, families),
        Err(e) => {
            throw_convert(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

/// Package version (version lockstep, spec §9: CLI `--version` and Java
/// `getVersion()` both resolve to the crate version). Every release bump
/// must move the whole `rofd-convert-v*` lockstep checklist in one step —
/// `crates/convert{,-cli,-java}/Cargo.toml` +
/// `crates/convert-java/pom.xml` — or this value drifts from the
/// published Maven coordinates.
#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| -> Result<jstring, rofd_convert::ConvertError> {
        env.new_string(env!("CARGO_PKG_VERSION"))
            .map(|s| s.as_raw())
            .map_err(|e| rofd_convert::ConvertError::Internal(format!("string alloc: {e:?}")))
    });
    match r {
        Ok(raw) => raw,
        Err(e) => {
            throw_convert(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

/// One-call conversion through the leased instance. The attempt clears
/// the handle's warnings slot up front, and only a SUCCESSFUL conversion
/// parks its warnings in LAST_WARNINGS[id] for nTakeWarnings — after a
/// failed attempt (exception) the slot is empty, so takeWarnings never
/// returns a stale pre-failure set. The instance is put back on every
/// path (InUse/Stale throw without touching the instance).
#[no_mangle]
// Raw array handle arrives from the JVM (valid local ref by the JNI
// contract); this export is never called from Rust code.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn Java_io_github_officers_NativeLink_nConvert(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    format_ordinal: jint,
    ofd: jbyteArray,
    watermark: jboolean,
    annotations: jboolean,
    seal: jboolean,
    max_pages: jlong,
    max_media_bytes: jlong,
) -> jbyteArray {
    let mut env = env;
    let format = match map_format(format_ordinal) {
        Ok(f) => f,
        Err(msg) => {
            throw(&mut env, &msg);
            return std::ptr::null_mut();
        }
    };
    let mut converter = match TABLE.take(id as u64) {
        Ok(c) => c,
        Err(e) => {
            throw_take(&mut env, e);
            return std::ptr::null_mut();
        }
    };
    // Last-call-wins with a sharp edge: the slot is cleared BEFORE the
    // attempt, so a failed convert leaves it empty (takeWarnings then
    // reports the last successful call's set — or none).
    clear_warnings(id as u64);
    // Kernel call under the panic boundary; the instance goes back on
    // every path (a Stale race makes put a no-op).
    let result = rofd_convert::catch_panics(
        || -> Result<rofd_convert::ConvertOutput, rofd_convert::ConvertError> {
            let arr = unsafe { JByteArray::from_raw(ofd) };
            let bytes = env.convert_byte_array(&arr).map_err(|e| {
                rofd_convert::ConvertError::Internal(format!("byte[] access: {e:?}"))
            })?;
            let opts = build_options(
                from_jboolean(watermark),
                from_jboolean(annotations),
                from_jboolean(seal),
                max_pages,
                max_media_bytes,
            );
            converter.convert(format, &bytes, &opts)
        },
    );
    TABLE.put(id as u64, converter);
    match result {
        Ok(out) => {
            park_warnings(id as u64, out.warnings);
            match env.byte_array_from_slice(&out.bytes) {
                Ok(arr) => arr.as_raw(),
                Err(e) => {
                    throw(&mut env, &format!("byte[] allocation failed: {e:?}"));
                    std::ptr::null_mut()
                }
            }
        }
        Err(e) => {
            throw_convert(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_officers_NativeLink_nTakeWarnings(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jobjectArray {
    let mut env = env;
    let r = rofd_convert::catch_panics(|| Ok(take_warnings(id as u64)));
    match r {
        Ok(warnings) => string_array(&mut env, warnings),
        Err(e) => {
            throw_convert(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ids used by the LAST_WARNINGS tests: unique per test so parallel
    // test threads never observe each other's parked warnings.
    const WARN_ID_A: u64 = 9_001;
    const WARN_ID_B: u64 = 9_002;
    const WARN_ID_C: u64 = 9_003;

    #[test]
    fn format_ordinal_zero_is_pdf_and_only_pdf() {
        assert_eq!(map_format(0), Ok(rofd_convert::OutputFormat::Pdf));
        assert!(map_format(1).is_err());
        assert!(map_format(-1).is_err());
        // The error message names the rejected ordinal (diagnosable).
        assert!(map_format(2).unwrap_err().contains('2'));
    }

    #[test]
    fn jboolean_roundtrip() {
        assert!(!from_jboolean(0));
        assert!(from_jboolean(1));
        // Defensive: any non-zero byte reads as true.
        assert!(from_jboolean(42));
        assert!(bool_to_jboolean(true) == JNI_TRUE);
        assert!(bool_to_jboolean(false) == JNI_FALSE);
    }

    #[test]
    fn budget_knobs_zero_and_negative_mean_kernel_defaults() {
        assert_eq!(opt_u32(0), None);
        assert_eq!(opt_u32(-1), None);
        assert_eq!(opt_u32(7), Some(7));
        // Out-of-range values fall back to unset instead of wrapping.
        assert_eq!(opt_u32((u32::MAX as i64) + 1), None);
        assert_eq!(opt_usize(0), None);
        assert_eq!(opt_usize(-5), None);
        assert_eq!(opt_usize(1 << 40), Some(1 << 40));
    }

    #[test]
    fn build_options_carries_content_flags() {
        let opts = build_options(true, false, true, 3, 4);
        assert!(opts.content.watermark);
        assert!(!opts.content.annotations);
        assert!(opts.content.seal);
        assert_eq!(opts.max_pages, Some(3));
        assert_eq!(opts.max_media_bytes, Some(4));
        // All-off flags + zero budgets: budgets None (kernel defaults
        // 10k pages / 1 GiB), every content layer off.
        let defaults = build_options(false, false, false, 0, -1);
        assert_eq!(defaults.max_pages, None);
        assert_eq!(defaults.max_media_bytes, None);
        assert_eq!(
            defaults.content,
            rofd_convert::PrintContent {
                watermark: false,
                annotations: false,
                seal: false,
            }
        );
    }

    #[test]
    fn font_register_result_maps_onto_the_boolean_contract() {
        assert_eq!(map_font_register_result(Ok(())), Ok(true));
        assert_eq!(
            map_font_register_result(Err(rofd_convert::FontRegisterError::InvalidFont)),
            Ok(false)
        );
        let io = rofd_convert::FontRegisterError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file",
        ));
        assert!(matches!(map_font_register_result(Err(io)), Err(msg) if !msg.is_empty()));
    }

    #[test]
    fn warnings_display_lines_are_parked_verbatim() {
        let w = rofd_convert::OfdWarning::FontSubstituted {
            requested: "SimSun".into(),
            used: "default".into(),
        };
        assert_eq!(
            warnings_to_strings(vec![w.clone()]),
            vec!["font 'SimSun' not found; substituted with 'default'"]
        );
        park_warnings(WARN_ID_A, vec![w]);
        assert_eq!(
            take_warnings(WARN_ID_A),
            vec!["font 'SimSun' not found; substituted with 'default'"]
        );
    }

    #[test]
    fn take_warnings_removes_on_take_and_defaults_to_empty() {
        park_warnings(WARN_ID_B, vec![]);
        // Empty parking drains as an empty array (never null on the Java
        // side), and a second take of the same id is empty again.
        assert_eq!(take_warnings(WARN_ID_B), Vec::<String>::new());
        assert_eq!(take_warnings(WARN_ID_B), Vec::<String>::new());
        // An id with nothing parked also yields the empty default.
        assert_eq!(take_warnings(123_456), Vec::<String>::new());
    }

    #[test]
    fn warnings_of_different_handles_do_not_leak_into_each_other() {
        let w = rofd_convert::OfdWarning::MissingFeature {
            feature: "JBIG2".into(),
            entry: "Doc_0/Res/Img_0.xml".into(),
        };
        park_warnings(WARN_ID_A + 10, vec![w]);
        assert_eq!(
            take_warnings(WARN_ID_A + 11),
            Vec::<String>::new(),
            "warnings parked under one handle must not surface under another"
        );
        assert_eq!(
            take_warnings(WARN_ID_A + 10).len(),
            1,
            "the parking handle sees its own warnings"
        );
    }

    #[test]
    fn warning_slot_clears_on_attempt_start_and_close() {
        // The nConvert/nClose contract in miniature: every attempt clears
        // the slot before running, so a FAILED convert leaves it empty —
        // never the previous call's stale set; only success parks again,
        // and nClose drains the slot of the handle it removes.
        let old = rofd_convert::OfdWarning::FontSubstituted {
            requested: "SimSun".into(),
            used: "default".into(),
        };
        park_warnings(WARN_ID_C, vec![old]);
        clear_warnings(WARN_ID_C); // start of the next attempt (it fails)
        assert_eq!(
            take_warnings(WARN_ID_C),
            Vec::<String>::new(),
            "a failed attempt must not expose the previous call's warnings"
        );
        let fresh = rofd_convert::OfdWarning::MissingFeature {
            feature: "JBIG2".into(),
            entry: "Doc_0/Res/Img_0.xml".into(),
        };
        park_warnings(WARN_ID_C, vec![fresh]); // a later success parks
        assert_eq!(
            take_warnings(WARN_ID_C).len(),
            1,
            "the successful call's warnings are parked after the clear"
        );
        clear_warnings(WARN_ID_C); // what nClose performs
        assert_eq!(
            take_warnings(WARN_ID_C),
            Vec::<String>::new(),
            "a closed handle leaves no parked warnings behind"
        );
    }

    #[test]
    fn second_thread_take_of_a_leased_handle_reports_in_use() {
        // Lease on this thread via the real process-wide TABLE (what
        // nConvert hits); a second thread must observe InUse — not race,
        // not block forever.
        let id = TABLE.insert(rofd_convert::Converter::new());
        let leased = TABLE.take(id).expect("fresh handle must take");
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), TakeError>>();
        let handle = std::thread::spawn(move || {
            tx.send(TABLE.take(id).map(|c| TABLE.put(id, c))).unwrap();
        });
        let from_thread = rx.recv().unwrap();
        assert!(matches!(from_thread, Err(TakeError::InUse)));
        handle.join().unwrap();
        // After the lease is released the handle takes again.
        TABLE.put(id, leased);
        let re = TABLE.take(id).expect("released handle must take");
        TABLE.put(id, re);
    }

    #[test]
    fn convert_after_close_sees_stale_on_the_process_table() {
        // The same close-then-convert semantics nConvert hits first: any
        // take of a closed id on the real process-wide TABLE is Stale.
        let id = TABLE.insert(rofd_convert::Converter::new());
        assert!(TABLE.close(id).is_ok());
        assert!(matches!(TABLE.take(id), Err(TakeError::Stale)));
    }
}
