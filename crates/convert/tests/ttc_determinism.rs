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

/// simsun.ttc fixture candidates: the repo's web-app fonts dir first
/// (gitignored - dev machines only), then the Windows system font dir
/// (the phase-2 Task 1 observation source). Absent everywhere -> skip: a
/// single-face fixture cannot reproduce a face flip, and no TTC fixture
/// ships in git.
fn locate_simsun_ttc() -> Option<Vec<u8>> {
    for path in [
        "../../web-app/public/fonts/simsun.ttc",
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
    // A TTC whose faces carry distinct family names (simsun.ttc: face 0
    // "SimSun" + face 1 "NSimSun") registered through the process
    // registry: every fresh per-call FontStore must resolve the same face
    // for the same text, or identical conversions flip faces across
    // processes (JVM-vs-CLI byte comparison, phase-2 Task 7, breaks). The
    // watermark in `annotated_fixture` has no document font bytes, so its
    // shaping rides the registry-anchored default family - the exact path
    // the flip was observed on.
    let Some(ttc) = locate_simsun_ttc() else {
        eprintln!("skipping: no simsun.ttc fixture (repo fonts dir or C:/Windows/Fonts)");
        return;
    };
    rofd_convert::register_font(ttc).expect("simsun.ttc registers");

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
