# Test font fixtures — provenance

These fonts are **test-only** fixtures (`include_bytes!`-ed by render /
export / convert tests). They never ship in any published package
(convert spec §7: published packages carry zero fonts).

| File | What | Provenance / license |
|---|---|---|
| `TestFont.ttf` | DejaVu Sans (regular, full face) | DejaVu fonts — Bitstream Vera derived, permissive (MIT-style reserved-font-name license). Committed with an explicit license note (`3e5c85d`, "DejaVuSans test font fixture (permissive license)"). |
| `TestFontCjk.otf` | 5-glyph CJK subset | Generated in-repo for a system-font-independent fallback test (`153b757`); the source font was not recorded at commit time — believed derived from an OFL-licensed CJK family. If airtight provenance is ever required, regenerate from a named OFL font. |
| `TwoFamily.ttc` | Two-face TTC (two identical DejaVu Sans subsets, one face renamed "DejaVu Sans Two") | Generated 2026-11-07 with fontTools 4.38 (two minimal "t" + .notdef subsets, shared tables) to pin cross-store TTC face-selection determinism (convert phase-2 Task 2b). Same DejaVu license as above; the generator script lives gitignored at `target/build-twofamily-ttc.py` — regenerate with fontTools if needed. |
