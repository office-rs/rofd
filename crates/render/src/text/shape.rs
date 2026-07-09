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
/// Returns glyph IDs + the shaper's natural positions.
pub fn shape_text(text: &str, font: &FontData, size: f64) -> Vec<ShapedGlyph> {
    let mut fcx = FontContext::new();
    let family = register_font(&mut fcx, font);
    shape_with_family(&mut fcx, text, size, family.as_deref())
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

/// Shape `text` at `size` using `family_name` as the default family.
///
/// If `family_name` is `None` (font registration failed), Parley
/// falls back to its default (`sans-serif`); this should not happen
/// for valid document fonts.
pub(crate) fn shape_with_family(
    fcx: &mut FontContext,
    text: &str,
    size: f64,
    family_name: Option<&str>,
) -> Vec<ShapedGlyph> {
    let mut lcx: LayoutContext = LayoutContext::new();
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(size as f32));
    builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
    builder.push_default(StyleProperty::FontStyle(ParleyFontStyle::Normal));
    if let Some(name) = family_name {
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(name)));
    }
    // Disable ligatures: liga=0 (keeps 1:1 char<->glyph for delta alignment).
    let liga_off = [FontFeature::new(Tag::from_bytes(*b"liga"), 0)];
    builder.push_default(StyleProperty::FontFeatures(liga_off.as_slice().into()));

    let mut layout = builder.build(text);
    layout.break_all_lines(None);

    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                for g in run.positioned_glyphs() {
                    out.push(ShapedGlyph { glyph_id: g.id, x: g.x, y: g.y });
                }
            }
        }
    }
    out
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
}
