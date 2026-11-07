//! Pipeline-level conversion tests that ANCHOR the process font registry.
//! A separate test binary — its own process — on purpose: the registry is
//! process-global and append-only (no unregister, by design), so anchoring
//! a default font here must not race `convert_api`'s fresh-registry
//! assertions in another binary.

mod common;

use rofd_component::PrintContent;

use common::{annotated_fixture, convert_with, TEST_FONT};

#[test]
fn content_flags_independently_change_output() {
    // Review Focus 4: each PrintContent layer toggle changes the encoded
    // bytes on its own. The test font is anchored as the process default
    // BEFORE the Converter snapshot so watermark shaping is deterministic
    // (no system-font dependence).
    rofd_convert::register_default_font(TEST_FONT.to_vec()).unwrap();
    let bytes = annotated_fixture();

    let base = convert_with(&bytes, PrintContent::default());
    let no_wm = convert_with(
        &bytes,
        PrintContent {
            watermark: false,
            annotations: true,
            seal: true,
        },
    );
    let no_ann = convert_with(
        &bytes,
        PrintContent {
            watermark: true,
            annotations: false,
            seal: true,
        },
    );
    let no_seal = convert_with(
        &bytes,
        PrintContent {
            watermark: true,
            annotations: true,
            seal: false,
        },
    );

    let all = [base, no_wm, no_ann, no_seal];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a, b, "every content-flag preset must encode distinctly");
        }
    }
}
