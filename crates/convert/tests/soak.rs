//! RSS soak skeleton (convert spec §8): one long-lived [`Converter`] runs
//! 2000 conversions of a synthetic fixture while `VmRSS` is sampled from
//! `/proc/self/status` every 500 iterations. The fixture switches variant
//! every 500 iterations (plain / annotated / a derived 4-page shape) so no
//! single-document cache luck flatters the run. Final assertion:
//! first-to-last `VmRSS` growth stays under 64 MiB.
//!
//! Linux-only (`/proc`) and manual (`#[ignore]`):
//! `cargo test -p rofd-convert --test soak -- --ignored --nocapture`
//!
//! The 64 MiB bound is a first-principles magnitude and ADJUSTABLE: spec §8
//! defers the threshold to a measured soak run — tighten or loosen it once
//! a real-hardware baseline exists.

#![cfg(target_os = "linux")]

mod common;

use rofd_convert::{ConvertOptions, Converter, OutputFormat};
use rofd_dom::OfdDocument;

use common::{annotated_fixture, fixture_bytes, page};

/// Total conversions of the soak loop.
const TOTAL_ITERATIONS: usize = 2000;
/// Conversions between fixture-variant switches and between `VmRSS`
/// samples.
const SAMPLE_INTERVAL: usize = 500;
/// First-to-last `VmRSS` growth bound, kB (64 MiB). Adjustable pending a
/// measured baseline (spec §8).
const RSS_GROWTH_LIMIT_KB: usize = 64 * 1024;

/// The third variant, derived: a 4-page plain package — a page count the
/// two two-page fixtures lack, so the third phase changes the scene work.
/// Built through common's [`page`] helper (reused, not duplicated) plus
/// the real `rofd-io` writer.
fn four_page_fixture() -> Vec<u8> {
    let mut doc = OfdDocument::default();
    for i in 0..4 {
        doc.pages.push(page(&format!("PB{i}")));
    }
    rofd_io::write_ofd(&doc).expect("four-page fixture writes")
}

/// `VmRSS` in kB from `/proc/self/status`. Panics loudly when the file or
/// the line is missing/unparsable — a soak that cannot measure must not
/// silently pass.
fn vm_rss_kb() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        return rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("unparsable VmRSS line: {line:?}"));
    }
    panic!("no VmRSS line in /proc/self/status");
}

#[test]
#[ignore = "manual Linux soak, 2000 conversions: cargo test -p rofd-convert --test soak -- --ignored --nocapture"]
fn rss_stays_bounded_over_2000_conversions() {
    let variants = [fixture_bytes(), annotated_fixture(), four_page_fixture()];
    // Premise guard (concurrency.rs posture): every variant converts
    // cleanly before the loop, or the soak would measure nothing.
    let mut premise = Converter::new();
    for (v, ofd) in variants.iter().enumerate() {
        let out = premise
            .convert(OutputFormat::Pdf, ofd, &ConvertOptions::default())
            .unwrap_or_else(|e| panic!("variant {v} convert failed: {e}"));
        assert!(out.bytes.starts_with(b"%PDF"), "variant {v} is a real PDF");
    }

    // The soak: ONE long-lived converter, 2000 calls, variant switching
    // every 500, `VmRSS` sampled every 500.
    let mut converter = Converter::new();
    let mut samples_kb = Vec::with_capacity(TOTAL_ITERATIONS / SAMPLE_INTERVAL + 1);
    let mut sink_bytes = 0usize; // black-box sink so release runs cannot elide work
    for i in 0..TOTAL_ITERATIONS {
        let ofd = &variants[(i / SAMPLE_INTERVAL) % variants.len()];
        let out = converter
            .convert(OutputFormat::Pdf, ofd, &ConvertOptions::default())
            .unwrap_or_else(|e| panic!("conversion {i} failed: {e}"));
        sink_bytes = sink_bytes.wrapping_add(out.bytes.len());
        if i % SAMPLE_INTERVAL == 0 {
            let rss = vm_rss_kb();
            println!("iteration {i:5}: VmRSS {rss} kB");
            samples_kb.push(rss);
        }
    }
    let final_rss = vm_rss_kb();
    println!("final   VmRSS {final_rss} kB");
    samples_kb.push(final_rss);

    assert!(sink_bytes > 0, "soak must observe real output bytes");
    let first = samples_kb.first().copied().expect("at least one sample");
    let last = samples_kb.last().copied().expect("at least one sample");
    let growth_kb = last.saturating_sub(first);
    println!(
        "RSS growth {growth_kb} kB over {TOTAL_ITERATIONS} conversions \
         (limit {RSS_GROWTH_LIMIT_KB} kB)"
    );
    assert!(
        last.saturating_sub(first) < RSS_GROWTH_LIMIT_KB,
        "RSS grew {growth_kb} kB (first {first}, last {last}), limit \
         {RSS_GROWTH_LIMIT_KB} kB — the threshold is adjustable pending a \
         measured baseline (spec §8)"
    );
}
