use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parley::style::{FontFamily, GenericFamily};
use peniko::FontData;
use rofd_dom::{FontId, Resources};

use super::shape::{register_font, shape_with_family, ShapedGlyph};

/// Holds document fonts (from `Resources.font_data`) + a default fallback font,
/// resolved to `peniko::FontData` for Vello `draw_glyphs`.
///
/// On peniko 0.6 the font handle is `FontData` (re-exported from
/// `linebender_resource_handle`); `Scene::draw_glyphs` takes `&FontData`.
/// `Blob::new` wraps an `Arc<dyn AsRef<[u8]> + Send + Sync>` - an `Arc<Vec<u8>>`
/// coerces without copying the font bytes.
///
/// In addition to the `FontData` handles (for Vello), the store also owns a
/// `parley::FontContext` (font database for shaping) with every document font +
/// the default font registered. [`Self::shape`] reuses this single `FontContext`
/// across calls (preferred over [`shape_text`](super::shape_text), which creates
/// a fresh one per call).
///
/// The `FontContext` is shared via `Rc<RefCell<...>>` because Parley's
/// `ranged_builder` requires `&mut FontContext` while `shape` exposes `&self`.
/// `Rc<RefCell>` is single-threaded; the render crate builds scenes
/// single-threaded.
#[derive(Clone)]
pub struct FontStore {
    fonts: HashMap<FontId, FontData>,
    default: Option<FontData>,
    /// Shared Parley font database (all document + default fonts registered).
    font_cx: Rc<RefCell<parley::FontContext>>,
    /// Registered family name per document font id (for `shape`).
    families: HashMap<FontId, String>,
    /// Registered family name for the default font.
    default_family: Option<String>,
}

impl FontStore {
    /// Build from document resources + a default font (raw bytes).
    pub fn from_resources(res: &Resources, default_bytes: Arc<Vec<u8>>) -> Self {
        let mut fonts = HashMap::new();
        let mut families = HashMap::new();
        let mut font_cx = parley::FontContext::new();

        for (id, bytes) in &res.font_data {
            if let Some(font) = make_font(bytes.clone()) {
                if let Some(family) = register_font(&mut font_cx, &font) {
                    families.insert(id.clone(), family);
                }
                fonts.insert(id.clone(), font);
            }
        }

        let default = make_font(default_bytes);
        let default_family = default
            .as_ref()
            .and_then(|font| register_font(&mut font_cx, font));

        Self {
            fonts,
            default,
            font_cx: Rc::new(RefCell::new(font_cx)),
            families,
            default_family,
        }
    }

    /// Resolve a document font by id. `None` if the id has no font bytes.
    pub fn resolve(&self, id: &FontId) -> Option<&FontData> {
        self.fonts.get(id)
    }

    /// The default fallback font (for body text whose `FontId` has no bytes).
    pub fn default_font(&self) -> Option<&FontData> {
        self.default.as_ref()
    }

    /// Resolve a font by id, falling back to the default.
    pub fn resolve_or_default(&self, id: &FontId) -> Option<&FontData> {
        self.fonts.get(id).or(self.default.as_ref())
    }

    /// Shape `text` with the document font for `font_id` (falling back to the
    /// default font's family if `font_id` is unknown), at `size`.
    ///
    /// Reuses the store's shared `FontContext` (registered in
    /// [`Self::from_resources`]). Ligatures OFF (1:1 char<->glyph).
    ///
    /// If neither a document font nor a default font is resolved, falls back to
    /// a generic system family (SansSerif) so text still renders - parley's
    /// `FontContext` discovers system fonts (via the `system` feature), and
    /// fontique's script fallback covers characters the default lacks (e.g. CJK
    /// on a Latin default).
    ///
    /// Returns the `FontData` that actually shaped the glyphs (captured from the
    /// parley run) alongside the glyph ids + positions. The caller MUST draw
    /// with this font - it is the system font when the fallback was used, so
    /// using a different font (e.g. an empty default) would mismatch the glyph
    /// ids and panic in vello/skrifa. `None` only when no glyphs were produced.
    pub fn shape(&self, font_id: &FontId, text: &str, size: f64) -> (Option<FontData>, Vec<ShapedGlyph>) {
        let family_name = self
            .families
            .get(font_id)
            .map(String::as_str)
            .or(self.default_family.as_deref());
        let family = match family_name {
            Some(name) => FontFamily::named(name),
            // No document/default font resolved -> generic system family.
            None => FontFamily::from(GenericFamily::SansSerif),
        };
        let mut fcx = self.font_cx.borrow_mut();
        shape_with_family(&mut fcx, text, size, family)
    }
}

/// Wrap raw font bytes into a shareable `peniko::FontData` (collection index 0).
/// `Blob::new` takes `Arc<dyn AsRef<[u8]> + Send + Sync>`; an `Arc<Vec<u8>>`
/// coerces, so the bytes are shared (not copied). Returns `None` for empty
/// bytes (no valid font) so the caller falls back to a system family.
fn make_font(bytes: Arc<Vec<u8>>) -> Option<FontData> {
    if bytes.is_empty() {
        return None;
    }
    let blob = peniko::Blob::new(bytes);
    Some(FontData::new(blob, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_store_resolves_registered_document_font() {
        let font_bytes =
            include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut res = Resources::default();
        res.font_data
            .insert(FontId::new("F1"), Arc::new(font_bytes.to_vec()));
        let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));
        assert!(store.resolve(&FontId::new("F1")).is_some(), "document font resolves");
    }

    #[test]
    fn font_store_falls_back_to_default_when_font_absent() {
        let font_bytes =
            include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
        assert!(store.default_font().is_some(), "default font available");
        assert!(
            store.resolve(&FontId::new("missing")).is_none(),
            "no document font for unknown id"
        );
    }

    #[test]
    fn font_store_shape_hello_with_document_font() {
        let font_bytes =
            include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut res = Resources::default();
        res.font_data
            .insert(FontId::new("F1"), Arc::new(font_bytes.to_vec()));
        let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));

        let (font, glyphs) = store.shape(&FontId::new("F1"), "Hello", 12.0);
        assert_eq!(glyphs.len(), 5, "5 glyphs for 5 chars (ligatures off)");
        assert!(font.is_some(), "font captured from the run");
        assert!(
            glyphs.iter().all(|g| g.glyph_id != 0),
            "all glyphs have valid ids"
        );
    }

    #[test]
    fn font_store_shape_falls_back_to_default_family() {
        // An unknown font id should still shape using the default font's family.
        let font_bytes =
            include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));

        let (font, glyphs) = store.shape(&FontId::new("missing"), "Hi", 16.0);
        assert_eq!(glyphs.len(), 2, "2 glyphs for 'Hi' via default font");
        assert!(font.is_some(), "font captured from the run");
        assert!(
            glyphs.iter().all(|g| g.glyph_id != 0),
            "all glyphs have valid ids"
        );
    }

    #[test]
    fn font_store_shape_falls_back_to_system_when_no_font() {
        // No document fonts + empty default -> FontStore::shape falls back to a
        // generic system family (SansSerif). On a system with fonts installed,
        // "Hello" shapes to 5 glyphs (1:1, ligatures off). The returned font is
        // the system font parley selected (not the empty default).
        let store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let (font, glyphs) = store.shape(&FontId::new("missing"), "Hello", 12.0);
        assert_eq!(
            glyphs.len(),
            5,
            "system fallback shapes 5 glyphs for 'Hello'"
        );
        assert!(font.is_some(), "system font captured (not the empty default)");
        assert!(
            glyphs.iter().all(|g| g.glyph_id != 0),
            "all glyphs have valid ids"
        );
    }

    #[test]
    fn font_store_shape_system_fallback_covers_cjk() {
        // The system fallback (generic SansSerif) + fontique's script fallback
        // should cover CJK even when the system's default sans-serif is
        // Latin-only. Requires system fonts to be installed.
        let store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let (font, glyphs) = store.shape(&FontId::new("missing"), "入院记录", 12.0);
        assert!(
            !glyphs.is_empty(),
            "system fallback covers CJK; got {} glyphs",
            glyphs.len()
        );
        assert!(font.is_some(), "system CJK font captured");
    }
}
