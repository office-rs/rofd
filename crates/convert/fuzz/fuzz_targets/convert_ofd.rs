#![no_main]

use libfuzzer_sys::fuzz_target;

// Hostile-input target (convert spec §8): arbitrary bytes through the
// full decode -> budget guards -> layout/render -> encode path. Errors
// are data (bad zip/xml/etc.) and are discarded; a panic here is a bug
// the fuzzer catches (kernel contract: never panic on hostile input).
// Correctness is pinned by the convert tests; this target only proves
// "never crashes on hostile input".
fuzz_target!(|data: &[u8]| {
    let _ = rofd_convert::Converter::new().convert(
        rofd_convert::OutputFormat::Pdf,
        data,
        &rofd_convert::ConvertOptions::default(),
    );
});
