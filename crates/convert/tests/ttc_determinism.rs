//! TTC face-selection determinism through the real convert pipeline
//! (convert spec §8, phase-2 Task 2b): the same registry bytes must pick
//! the same face - and therefore encode the same bytes - in every per-call
//! `FontStore` a [`Converter`] builds. Each `convert` call constructs a
//! fresh `FontStore` with a fresh fontique collection, so N conversions in
//! this binary model N independent font states (the cross-process CLI
//! probe is the test of record on a real machine: pre-fix, two sha256
//! values across 6 fresh processes - phase-2 Task 1's observation).
//!
//! Own test binary, on purpose: it anchors the process font registry
//! (append-only) with a TTC, which must not race the fresh-registry
//! assertions in other binaries (same isolation rule as
//! `convert_pipeline`).

mod common;

use rofd_convert::{ConvertOptions, Converter, OutputFormat};

use common::annotated_fixture;

/// Two-family TTC fixture candidates: the committed minimal fixture
/// first (render's `TwoFamily.ttc` — two identical DejaVu Sans subset
/// faces whose name tables carry distinct families "DejaVu Sans" /
/// "DejaVu Sans Two"; present on every checkout, so this determinism
/// test runs on any machine), then the real-world specimens simsun.ttc
/// (the repo's web-app fonts dir — gitignored, dev machines only — and
/// the Windows system font dir, the phase-2 Task 1 observation source).
/// Absent everywhere -> skip.
fn locate_two_family_ttc() -> Option<Vec<u8>> {
    for path in [
        "../render/tests/fixtures/fonts/TwoFamily.ttc",
        "../web-app/public/fonts/simsun.ttc",
        "C:/Windows/Fonts/simsun.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

#[test]
fn registered_ttc_converts_byte_identically_across_fresh_stores() {
    // A TTC whose faces carry distinct family names (TwoFamily.ttc:
    // "DejaVu Sans" + "DejaVu Sans Two"; the simsun.ttc fallback: "SimSun"
    // + "NSimSun") registered through the process registry: every fresh
    // per-call FontStore must resolve the same face for the same text, or
    // identical conversions flip faces across processes (JVM-vs-CLI byte
    // comparison, phase-2 Task 7, breaks). The two-distinct-families
    // precondition itself is pinned on the same fixture by the render
    // crate's `ttc_registration_is_deterministic_across_fresh_contexts`
    // (the registry records only the canonical family). The watermark in
    // `annotated_fixture` has no document font bytes, so its shaping
    // rides the registry-anchored default family - the exact path the
    // flip was observed on.
    let Some(ttc) = locate_two_family_ttc() else {
        eprintln!("skipping: no two-family TTC fixture (TwoFamily.ttc / simsun.ttc)");
        return;
    };
    rofd_convert::register_font(ttc).expect("two-family TTC registers");

    let bytes = annotated_fixture();
    let opts = ConvertOptions::default();
    let first = Converter::new()
        .convert(OutputFormat::Pdf, &bytes, &opts)
        .expect("first convert succeeds")
        .bytes;
    for i in 1..32 {
        let next = Converter::new()
            .convert(OutputFormat::Pdf, &bytes, &opts)
            .unwrap_or_else(|e| panic!("convert {i} succeeds: {e}"))
            .bytes;
        assert_eq!(
            first, next,
            "fresh per-call FontStore #{i} flipped the TTC face (convert spec §8)"
        );
    }
}
