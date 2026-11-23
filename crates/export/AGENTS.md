# rofd-export

> Auto-loaded when you work in this subtree. Leaf crate: **no rofd-internal
> dependencies** (the Scene→PDF encoder).

- Encodes `imaging::record::Scene` → krilla PDF (feature `pdf`; swash reads
  cmaps for ToUnicode); the shared conversion layer for print/export
  artifacts.
- Font-face degradation (spec 2026-11-06 §8, since phase-2 Task 1): a font
  face unsupported by krilla's subsetter is **degraded, not failed** — one
  warning line + its glyphs skipped (`encode_with_degradation`), exit code
  0; the degradation lines surface on all four runtimes via
  `ConvertWarning::Encode`.
- Byte-identical parity with the editor's export is guarded by convert's
  parity test (`crates/convert/tests/parity.rs`).
