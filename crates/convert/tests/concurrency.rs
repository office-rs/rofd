//! Concurrency and history-independence byte stability (convert spec §8):
//! eight threads, each with its own `Converter`, must produce output
//! byte-identical to a single-threaded baseline, and one long-lived
//! instance must carry no cross-call state (A, B, A -> both A calls
//! byte-identical).
//!
//! Registry rule: each integration test binary is its own process, so the
//! process-global font registry starts empty here. This binary never
//! registers a font and never anchors a default — keeping the registry
//! untouched pins every conversion in this file to the same (empty)
//! registry input, which is the premise of the byte-stability claims
//! (convert_api.rs asserts `!has_default_font()` for the same reason).

mod common;

use std::sync::Arc;

use rofd_convert::{ConvertOptions, Converter, OutputFormat};

use common::{annotated_fixture, fixture_bytes};

#[test]
fn eight_threads_byte_stable() {
    let ofd = Arc::new(annotated_fixture());
    // Baseline: one converter, one call, on the main thread.
    let baseline = {
        let mut c = Converter::new();
        c.convert(OutputFormat::Pdf, &ofd, &ConvertOptions::default())
            .expect("baseline convert succeeds")
    };
    assert!(
        baseline.bytes.starts_with(b"%PDF"),
        "baseline is a real PDF"
    );
    // Eight threads, each converting the same bytes through its own
    // instance — outputs must match the baseline byte-for-byte.
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let ofd = Arc::clone(&ofd);
            std::thread::spawn(move || {
                let mut c = Converter::new();
                c.convert(OutputFormat::Pdf, &ofd, &ConvertOptions::default())
                    .expect("thread convert succeeds")
                    .bytes
            })
        })
        .collect();
    for (i, h) in handles.into_iter().enumerate() {
        let bytes = h.join().expect("no thread may panic");
        assert_eq!(bytes, baseline.bytes, "thread {i} diverged from baseline");
    }
}

#[test]
fn sequence_is_history_independent() {
    // Review Focus 5: one long-lived converter converts A, then B, then A
    // again. If any per-call state (FontStore, session remnants) leaked
    // across calls, the second A would diverge from the first.
    let doc_a = annotated_fixture();
    let doc_b = fixture_bytes();
    let mut c = Converter::new();
    let first_a = c
        .convert(OutputFormat::Pdf, &doc_a, &ConvertOptions::default())
        .expect("first A convert succeeds")
        .bytes;
    let b = c
        .convert(OutputFormat::Pdf, &doc_b, &ConvertOptions::default())
        .expect("B convert succeeds")
        .bytes;
    let second_a = c
        .convert(OutputFormat::Pdf, &doc_a, &ConvertOptions::default())
        .expect("second A convert succeeds")
        .bytes;
    // Premise guard: B's output genuinely differs from A's, so the
    // interleaved call could actually perturb the instance if state
    // leaked (otherwise the sequence would prove nothing).
    assert_ne!(b, first_a, "premise: A and B fixtures convert differently");
    assert_eq!(
        first_a, second_a,
        "second A conversion must be byte-identical to the first (no cross-call state)"
    );
}
