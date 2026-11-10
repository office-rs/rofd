//! Text shaping via Parley: text string -> glyph IDs + positions.
//!
//! Shaping MUST use the document font (registered with Parley's
//! `FontContext`), not system fonts. Two entry points:
//! - [`FontStore::shape`](super::FontStore::shape) (preferred): reuses the
//!   store's shared `FontContext` (document + default fonts registered once in
//!   `from_resources`).
//! - [`shape_text`]: creates a fresh `FontContext` per call (correct but
//!   slower; for callers that only have raw `FontData`).
//!
//! Both register the font's bytes with a `FontContext`, capture the family
//! name, set it as the default family in the Parley builder, and shape.
//!
//! Ligatures are disabled (`liga=0`) so there is a 1:1 char<->glyph
//! correspondence. Body-text rendering (Task 9) uses the glyph IDs +
//! the document's deltas (NOT the shaper's x/y) to position glyphs;
//! annotation text uses the shaper's x/y directly.

use parley::setting::Tag;
use parley::style::{FontFamily, FontFeature, FontStyle as ParleyFontStyle, FontWeight};
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};
use peniko::FontData;

/// A shaped glyph: its ID and the shaper's natural position.
///
/// The body scene ignores `x`/`y` and positions glyphs by the
/// document's deltas; annotation text uses `x`/`y` directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub x: f32,
    pub y: f32,
}

/// Shape `text` with `font` at `size`, using a fresh `FontContext`.
///
/// This registers `font`'s bytes with a new `FontContext`, sets the
/// font's family as the default, and shapes. It is correct (shapes
/// with the given document font, not system fonts) but slower than
/// `FontStore::shape` (which reuses a `FontContext`).
///
/// Ligatures OFF (`liga=0`): 1:1 char<->glyph for delta alignment.
///
/// If `font` fails to register (e.g. garbage bytes), returns an empty `Vec` -
/// the caller explicitly passed a font, so silent system-font fallback would be
/// wrong. Use [`FontStore::shape`](super::FontStore::shape) for the
/// system-fallback behavior.
///
/// Returns glyph IDs + the shaper's natural positions.
pub fn shape_text(text: &str, font: &FontData, size: f64) -> Vec<ShapedGlyph> {
    let mut fcx = FontContext::new();
    let family = register_font(&mut fcx, font);
    let family_name = match family {
        Some(name) => name,
        None => return Vec::new(),
    };
    shape_with_family(&mut fcx, text, size, FontFamily::named(&family_name)).1
}

/// Register `font`'s bytes with `fcx.collection` and return the
/// registration's canonical family name alongside the family ids in a
/// deterministic order.
///
/// The bytes are shared (not copied): `font.data` is a `Blob<u8>`
/// which clones cheaply (Arc internally), and `register_fonts` takes
/// ownership of a `Blob<u8>`.
///
/// The returned ids let the caller also append the font to a generic family
/// (e.g. `SansSerif`) so parley can fall back to it when a named family lacks
/// a glyph - critical on wasm, where system fonts are unavailable and
/// registered fonts are the only fallback source.
///
/// Determinism (convert spec §8, phase-2 Task 2b): fontique's
/// `register_fonts` builds its return list by iterating an internal
/// `HashMap<FamilyId, _>`, so the list order - and with it the old
/// `families.first()` name pick - is RANDOM PER PROCESS. A TTC whose
/// faces carry distinct family names (simsun.ttc: face 0 "SimSun" +
/// face 1 "NSimSun") would otherwise anchor a coin-flip family name per
/// process: two CLI runs of the same conversion shaped different faces
/// and emitted different bytes (phase-2 Task 1; rword hit the same
/// fontique behavior in their convert demo and fixed it identically).
/// Families are therefore ranked by their LOWEST face index - face 0 is
/// what `FontData::new(blob, 0)` (the `make_font` draw handle) carries,
/// so the canonical name must resolve shaping to that same face, not to
/// a name-sorted one - with family name as the tiebreak, making both the
/// canonical name and the id order (SansSerif appends) process-independent.
pub(crate) fn register_font_with_ids(
    fcx: &mut FontContext,
    font: &FontData,
) -> (Option<String>, Vec<parley::fontique::FamilyId>) {
    let blob = font.data.clone();
    let families = fcx.collection.register_fonts(blob, None);
    let mut ranked: Vec<(u32, String, parley::fontique::FamilyId)> = Vec::new();
    for (id, fonts) in &families {
        let Some(name) = fcx.collection.family_name(*id) else {
            continue;
        };
        let lowest_face = fonts.iter().map(|f| f.index()).min().unwrap_or(u32::MAX);
        ranked.push((lowest_face, name.to_owned(), *id));
    }
    ranked.sort();
    let name = ranked.first().map(|(_, name, _)| name.clone());
    let ids = ranked.into_iter().map(|(_, _, id)| id).collect();
    (name, ids)
}

/// Register `font`'s bytes with `fcx.collection` and return the
/// registered family name (the first family reported by the font).
///
/// The bytes are shared (not copied): `font.data` is a `Blob<u8>`
/// which clones cheaply (Arc internally), and `register_fonts` takes
/// ownership of a `Blob<u8>`.
pub(crate) fn register_font(fcx: &mut FontContext, font: &FontData) -> Option<String> {
    register_font_with_ids(fcx, font).0
}

/// Shape `text` at `size` using `family` as the font family.
///
/// Ligatures OFF (`liga=0`): 1:1 char<->glyph for delta alignment.
///
/// Returns the `FontData` that actually shaped the glyphs (captured from the
/// parley `GlyphRun`) alongside the glyphs. This is critical for the system
/// fallback: when `FontStore::shape` falls back to a generic system family, the
/// returned font is the system font parley selected, so the caller draws the
/// glyphs with the SAME font that shaped them (mismatched font + glyph ids
/// would panic in vello/skrifa). `None` only when no glyphs were produced.
///
/// The caller chooses the family - [`shape_text`](super::shape_text) passes the
/// explicitly-registered document font (and returns empty if registration
/// failed, before reaching here), while
/// [`FontStore::shape`](super::FontStore::shape) passes the resolved
/// document/default family or a generic system family as a fallback.
pub(crate) fn shape_with_family(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family: FontFamily<'_>,
) -> (Option<FontData>, Vec<ShapedGlyph>) {
    let (groups, _) = shape_grouped_with_family(fcx, text, size, family);
    let mut iter = groups.into_iter();
    match iter.next() {
        Some((font, glyphs)) => {
            // Legacy single-font view: first run's font + all glyphs flat.
            // Correct only when the whole line shaped in one font.
            let flat = iter.fold(glyphs, |mut acc, (_, mut more)| {
                acc.append(&mut more);
                acc
            });
            (Some(font), flat)
        }
        None => (None, Vec::new()),
    }
}

/// Shape `text` at `size` using `family`, grouping the glyphs by the font
/// that shaped them, plus the shaped line width (max across lines, at
/// `size`).
///
/// Font fallback can split one line into several runs - e.g. a CJK label +
/// Latin digits when the default font is Latin-only - and a glyph id is only
/// valid with ITS font: drawing a whole line with the first run's font
/// renders the other runs' glyphs as garbage. Each group keeps the shared
/// layout-relative positions, so every group draws at the same line
/// transform. Consecutive runs of the same font are merged.
pub(crate) fn shape_grouped_with_family(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family: FontFamily<'_>,
) -> (Vec<(FontData, Vec<ShapedGlyph>)>, f64) {
    let (groups, lines) = shape_layout(fcx, text, size, family, None);
    let width = lines.iter().map(|l| l.width).fold(0.0_f64, f64::max);
    (groups, width)
}

/// Style metadata for one shaped glyph group.
#[derive(Debug, Clone)]
pub(crate) struct ShapedSpanMeta {
    pub style_index: usize,
    pub font: FontData,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
}

/// Shape `text` with per-run styles (the annotation span layout). `families[i]`
/// is the resolved family list for `runs[i]`; the resolved style index of each
/// parley glyph run is the `LayoutRun` covering its text range.
///
/// Keeps the same defaults as the flat path: `liga=0`, greedy UAX #14 breaking
/// with `OverflowWrap::Anywhere`, and the first run's family as the default.
pub(crate) fn shape_layout_runs(
    fcx: &mut FontContext,
    text: &str,
    runs: &[super::spans::LayoutRun],
    families: &[FontFamily<'_>],
    max_advance: Option<f32>,
) -> (Vec<(ShapedSpanMeta, Vec<ShapedGlyph>)>, Vec<RawLine>) {
    let mut lcx: LayoutContext = LayoutContext::new();
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(
        runs.first().map(|r| r.size as f32).unwrap_or(12.0),
    ));
    builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
    builder.push_default(StyleProperty::FontStyle(ParleyFontStyle::Normal));
    if let Some(first) = families.first() {
        builder.push_default(StyleProperty::FontFamily(first.clone()));
    }
    let liga_off = [FontFeature::new(Tag::from_bytes(*b"liga"), 0)];
    builder.push_default(StyleProperty::FontFeatures(liga_off.as_slice().into()));
    builder.push_default(StyleProperty::OverflowWrap(
        parley::style::OverflowWrap::Anywhere,
    ));
    for (run, family) in runs.iter().zip(families.iter()) {
        let range = run.byte_range.clone();
        builder.push(StyleProperty::FontSize(run.size as f32), range.clone());
        builder.push(StyleProperty::FontFamily(family.clone()), range.clone());
        builder.push(
            StyleProperty::FontWeight(if run.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            }),
            range.clone(),
        );
        builder.push(
            StyleProperty::FontStyle(if run.italic {
                ParleyFontStyle::Italic
            } else {
                ParleyFontStyle::Normal
            }),
            range,
        );
    }

    let mut layout = builder.build(text);
    layout.break_all_lines(max_advance);

    // Char -> byte index for resolving each glyph's style run. Parley groups
    // glyph runs by SHAPING style (font/size/weight/italic); color-only span
    // boundaries do not split them, so the run's text_range cannot identify
    // the span. Ligatures are disabled (liga=0): glyph i on a line is char i,
    // which gives every glyph an exact content char offset.
    let char_bytes: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    let style_at_char = |char_offset: usize| -> usize {
        let byte = char_bytes.get(char_offset).copied().unwrap_or(text.len());
        runs.partition_point(|r| r.byte_range.end <= byte)
            .min(runs.len() - 1)
    };

    let mut groups: Vec<(ShapedSpanMeta, Vec<ShapedGlyph>)> = Vec::new();
    let mut lines: Vec<RawLine> = Vec::new();
    for line in layout.lines() {
        let metrics = line.metrics();
        let line_range = line.text_range();
        let line_start_char = text[..line_range.start.min(text.len())].chars().count();
        let mut glyph_in_line = 0usize;
        let mut glyph_count = 0usize;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            if runs.is_empty() {
                continue;
            }
            let font = glyph_run.run().font().clone();
            for positioned in glyph_run.positioned_glyphs() {
                let style_index = style_at_char(line_start_char + glyph_in_line);
                glyph_in_line += 1;
                glyph_count += 1;
                let spec = &runs[style_index];
                let glyph = ShapedGlyph {
                    glyph_id: positioned.id,
                    x: positioned.x,
                    y: positioned.y,
                };
                match groups.last_mut() {
                    Some((prev, glyphs))
                        if prev.style_index == style_index && prev.font == font =>
                    {
                        glyphs.push(glyph);
                    }
                    _ => groups.push((
                        ShapedSpanMeta {
                            style_index,
                            font: font.clone(),
                            size: spec.size,
                            bold: spec.bold,
                            italic: spec.italic,
                        },
                        vec![glyph],
                    )),
                }
            }
        }
        lines.push(RawLine {
            byte_range: line_range,
            glyph_count,
            width: metrics.advance as f64,
            top: metrics.min_coord as f64,
            baseline: metrics.baseline as f64,
            height: metrics.line_height as f64,
        });
    }
    (groups, lines)
}

/// One visual line's raw parley geometry, before char offsets are resolved.
pub(crate) struct RawLine {
    pub byte_range: std::ops::Range<usize>,
    pub glyph_count: usize,
    pub width: f64,
    pub top: f64,
    pub baseline: f64,
    pub height: f64,
}

fn shape_layout(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family: FontFamily<'_>,
    max_advance: Option<f32>,
) -> (Vec<(FontData, Vec<ShapedGlyph>)>, Vec<RawLine>) {
    let mut lcx: LayoutContext = LayoutContext::new();
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(size as f32));
    builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
    builder.push_default(StyleProperty::FontStyle(ParleyFontStyle::Normal));
    builder.push_default(StyleProperty::FontFamily(family));
    // Disable ligatures: liga=0 (keeps 1:1 char<->glyph for delta alignment).
    let liga_off = [FontFeature::new(Tag::from_bytes(*b"liga"), 0)];
    builder.push_default(StyleProperty::FontFeatures(liga_off.as_slice().into()));
    // Emergency breaks inside an over-long word (annotation text boxes must
    // never overflow their frame).
    builder.push_default(StyleProperty::OverflowWrap(
        parley::style::OverflowWrap::Anywhere,
    ));

    let mut layout = builder.build(text);
    layout.break_all_lines(max_advance);

    let mut groups: Vec<(FontData, Vec<ShapedGlyph>)> = Vec::new();
    let mut lines: Vec<RawLine> = Vec::new();
    for line in layout.lines() {
        let metrics = line.metrics();
        let mut glyph_count = 0usize;
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let font = run.run().font().clone();
                let shaped: Vec<ShapedGlyph> = run
                    .positioned_glyphs()
                    .map(|g| ShapedGlyph {
                        glyph_id: g.id,
                        x: g.x,
                        y: g.y,
                    })
                    .collect();
                glyph_count += shaped.len();
                match groups.last_mut() {
                    Some((prev_font, glyphs)) if *prev_font == font => glyphs.extend(shaped),
                    _ => groups.push((font, shaped)),
                }
            }
        }
        lines.push(RawLine {
            byte_range: line.text_range(),
            glyph_count,
            width: metrics.advance as f64,
            top: metrics.min_coord as f64,
            baseline: metrics.baseline as f64,
            height: metrics.line_height as f64,
        });
    }
    (groups, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Two-family TTC fixture candidates: the committed minimal fixture
    /// first (`tests/fixtures/fonts/TwoFamily.ttc` — two identical DejaVu
    /// Sans subset faces whose name tables carry distinct families
    /// "DejaVu Sans" / "DejaVu Sans Two"; present on every checkout, so
    /// the face-selection tests run on any machine), then the real-world
    /// specimens simsun.ttc (the repo's web-app fonts dir — gitignored,
    /// dev machines only — and the Windows system font dir, the phase-2
    /// Task 1 observation source). Absent everywhere -> None: a
    /// single-family fixture cannot reproduce a face flip.
    fn locate_two_family_ttc() -> Option<Vec<u8>> {
        for path in [
            "tests/fixtures/fonts/TwoFamily.ttc",
            "../web-app/public/fonts/simsun.ttc",
            "C:/Windows/Fonts/simsun.ttc",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        None
    }

    /// The observable contract of one `register_font_with_ids` call: the
    /// canonical family name plus every registered family in ranked order
    /// with its lowest face index (raw `FamilyId`s are per-collection, so
    /// names + face indexes are the comparable signature).
    type RegistrationSignature = (Option<String>, Vec<(String, u32)>);

    fn registration_signature(fcx: &mut FontContext, font: &FontData) -> RegistrationSignature {
        let (name, ids) = register_font_with_ids(fcx, font);
        let ranked = ids
            .iter()
            .map(|id| {
                let family = fcx
                    .collection
                    .family(*id)
                    .expect("just-registered family resolves");
                let lowest = family
                    .fonts()
                    .iter()
                    .map(|f| f.index())
                    .min()
                    .expect("family carries at least one font");
                (family.name().to_owned(), lowest)
            })
            .collect();
        (name, ranked)
    }

    #[test]
    fn ttc_registration_is_deterministic_across_fresh_contexts() {
        // Cross-process byte stability (convert spec §8, phase-2 Task 2b):
        // fontique's `register_fonts` returns the registration's families
        // by iterating an internal HashMap, so both the list order and the
        // old `families.first()` pick are RANDOM PER PROCESS. A TTC whose
        // faces carry distinct family names (TwoFamily.ttc: face 0
        // "DejaVu Sans" + face 1 "DejaVu Sans Two"; the simsun.ttc
        // fallback: face 0 "SimSun" + face 1 "NSimSun") must not let that
        // order decide the canonical family, or two CLI runs of the same
        // conversion shape different faces -> different bytes (observed
        // live: two sha256 values across 6 processes, phase-2 Task 1).
        // 32 fresh FontContexts model 32 fresh font states: pre-fix the
        // coin flips disagree (P(all agree) ~ 2^-31); post-fix the
        // signature is pinned.
        let Some(ttc) = locate_two_family_ttc() else {
            eprintln!("skipping: no two-family TTC fixture (TwoFamily.ttc / simsun.ttc)");
            return;
        };
        let font = FontData::new(peniko::Blob::new(Arc::new(ttc)), 0);

        let mut reference: Option<RegistrationSignature> = None;
        for i in 0..32 {
            let mut fcx = FontContext::new();
            let got = registration_signature(&mut fcx, &font);
            assert!(!got.1.is_empty(), "TTC registers at least one family");
            // Two DISTINCT families are the precondition of this test:
            // the flip under guard is between named families, and a
            // single-family registration cannot represent it.
            assert!(
                got.1.len() >= 2 && got.1.windows(2).all(|w| w[0].0 != w[1].0),
                "fixture must present two distinct family names, got {:?}",
                got.1
            );
            assert_eq!(
                got.0.as_deref(),
                Some(got.1[0].0.as_str()),
                "canonical name must be the first ranked family"
            );
            assert_eq!(
                got.1[0].1, 0,
                "face 0 anchors the canonical name (FontData::new(blob, 0) draws it)"
            );
            let faces: Vec<u32> = got.1.iter().map(|(_, f)| *f).collect();
            assert!(
                faces.windows(2).all(|w| w[0] <= w[1]),
                "families rank by ascending face index, got {faces:?}"
            );
            if let Some(want) = &reference {
                assert_eq!(
                    &got, want,
                    "fresh context #{i} disagreed on TTC face selection"
                );
            } else {
                reference = Some(got);
            }
        }
    }

    fn load_font() -> FontData {
        let bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let blob = peniko::Blob::new(Arc::new(bytes.to_vec()));
        FontData::new(blob, 0)
    }

    #[test]
    fn shape_hello_produces_one_glyph_per_char() {
        let font = load_font();
        let glyphs = shape_text("Hello", &font, 12.0);
        assert_eq!(glyphs.len(), 5, "5 glyphs for 5 chars (ligatures off)");
        assert!(
            glyphs.iter().all(|g| g.glyph_id != 0),
            "all glyphs have valid ids"
        );
    }

    #[test]
    fn shape_empty_text_produces_no_glyphs() {
        let font = load_font();
        let glyphs = shape_text("", &font, 12.0);
        assert!(glyphs.is_empty(), "empty text -> no glyphs");
    }

    #[test]
    fn shape_uses_document_font_not_system() {
        // Regression: shape_text MUST shape with the given font, not
        // system fonts. The mechanism is: register the font's bytes with
        // a FontContext, capture the family name, and set it as the
        // default family in the builder. We verify the registration
        // step succeeds (returns a family name) and that shaping is
        // deterministic for the same font + text.
        let font = load_font();
        let mut fcx = parley::FontContext::new();
        let family = register_font(&mut fcx, &font);
        assert!(
            family.is_some(),
            "font registration captured a family name (required for document-font shaping)"
        );

        let glyphs_a = shape_text("Hi", &font, 16.0);
        let glyphs_b = shape_text("Hi", &font, 16.0);
        assert_eq!(
            glyphs_a, glyphs_b,
            "shaping is deterministic for the same font + text"
        );
        assert_eq!(glyphs_a.len(), 2, "2 glyphs for 'Hi'");
    }

    #[test]
    fn shape_garbage_font_returns_no_glyphs_not_system_fonts() {
        // Regression: when font registration fails (family None), shape_text
        // MUST return an empty Vec instead of silently shaping with Parley's
        // default system fonts. Garbage bytes are not a valid font, so
        // register_font returns None -> shape_with_family short-circuits to
        // Vec::new(). On the old code this returned 5 system-font glyphs for
        // "Hello"; with the fix it returns none.
        let garbage_blob = peniko::Blob::new(Arc::new(b"not a font".to_vec()));
        let garbage_font = FontData::new(garbage_blob, 0);
        let glyphs = shape_text("Hello", &garbage_font, 12.0);
        assert!(
            glyphs.is_empty(),
            "garbage font -> no glyphs (not system-font fallback); got {} glyphs",
            glyphs.len()
        );
    }
}
