//! Byte-level parity acceptance (convert spec D18): the same OFD bytes
//! under the same content flags MUST convert to exactly the bytes the
//! editor export path produces — `parse_ofd` -> `OfdComponent` ->
//! `begin_print` -> per-page `page_scene` -> `rofd_export::pdf_from_page_scenes`.
//!
//! Registry rule: this binary never registers a process font. The two
//! paths share `FontStore` inputs only while the registry is empty and no
//! default is anchored (the editor path is registry-blind by
//! construction; the convert path snapshots the process registry), so
//! anchoring anything here would break the comparison's premise. The
//! registry-anchoring tests live in `convert_pipeline` — a separate
//! binary, hence a separate process.

mod common;

use rofd_component::PrintContent;

use common::{annotated_fixture, convert_with, editor_path_pdf};

#[test]
fn convert_bytes_equal_editor_export_all_on() {
    // D18, all content layers on: identical inputs (same bytes, same
    // flags, same FontStore inputs) must encode byte-for-byte identically
    // through both pipelines. A mismatch means the convert pipeline
    // carries state the editor path does not see — a kernel bug.
    let bytes = annotated_fixture();
    let all_on = PrintContent {
        watermark: true,
        annotations: true,
        seal: true,
    };
    assert_eq!(
        convert_with(&bytes, all_on),
        editor_path_pdf(&bytes, all_on),
        "D18: convert output must equal the editor export path's bytes"
    );
}

#[test]
fn convert_bytes_equal_editor_export_all_off() {
    // D18, all content layers off: the second mandated configuration —
    // pages carry only their body scenes (white page bases), so this also
    // covers the flag plumbing reaching the same empty-filter result on
    // both sides.
    let bytes = annotated_fixture();
    let all_off = PrintContent {
        watermark: false,
        annotations: false,
        seal: false,
    };
    assert_eq!(
        convert_with(&bytes, all_off),
        editor_path_pdf(&bytes, all_off),
        "D18: convert output must equal the editor export path's bytes"
    );
}

#[test]
fn convert_same_input_twice_is_byte_identical() {
    // Pins the byte-determinism assumption the D18 equality leans on: the
    // same bytes + options through two fresh converters (fresh per-call
    // FontStore each time) encode identically. Empty registry throughout —
    // see the module doc.
    let bytes = annotated_fixture();
    let first = convert_with(&bytes, PrintContent::default());
    let second = convert_with(&bytes, PrintContent::default());
    assert_eq!(
        first, second,
        "same input converted twice must be byte-identical"
    );
}
