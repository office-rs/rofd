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
use parley::{
    FontContext, LayoutContext, PositionedLayoutItem, StyleProperty,
};
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
/// registered family name (the first family reported by the font).
///
/// The bytes are shared (not copied): `font.data` is a `Blob<u8>`
/// which clones cheaply (Arc internally), and `register_fonts` takes
/// ownership of a `Blob<u8>`.
pub(crate) fn register_font(fcx: &mut FontContext, font: &FontData) -> Option<String> {
    let blob = font.data.clone();
    let families = fcx.collection.register_fonts(blob, None);
    let (family_id, _fonts) = families.first()?;
    fcx.collection.family(*family_id).map(|info| info.name().to_owned())
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
    let mut lcx: LayoutContext = LayoutContext::new();
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(size as f32));
    builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
    builder.push_default(StyleProperty::FontStyle(ParleyFontStyle::Normal));
    builder.push_default(StyleProperty::FontFamily(family));
    // Disable ligatures: liga=0 (keeps 1:1 char<->glyph for delta alignment).
    let liga_off = [FontFeature::new(Tag::from_bytes(*b"liga"), 0)];
    builder.push_default(StyleProperty::FontFeatures(liga_off.as_slice().into()));

    let mut layout = builder.build(text);
    layout.break_all_lines(None);

    let mut font = None;
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                if font.is_none() {
                    font = Some(run.run().font().clone());
                }
                for g in run.positioned_glyphs() {
                    out.push(ShapedGlyph { glyph_id: g.id, x: g.x, y: g.y });
                }
            }
        }
    }
    (font, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
