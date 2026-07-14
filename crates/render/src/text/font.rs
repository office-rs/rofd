use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parley::fontique::FamilyId;
use parley::style::{FontFamily, FontFamilyName, GenericFamily};
use peniko::FontData;
use rofd_dom::{FontId, Resources};

use super::shape::{register_font_with_ids, shape_with_family, ShapedGlyph};

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
    /// Shaped-glyph cache: `font_id -> (size_bits -> (text -> (font, glyphs)))`.
    /// OFD body text is static, so shaping happens once per unique (font, size,
    /// text); subsequent composites hit the cache and skip parley re-shaping
    /// (which dominated render time - ~500ms for a text-heavy doc per frame).
    /// `size.to_bits()` keys avoid `f64` not being `Eq`. Cleared on
    /// `register_font`.
    #[allow(clippy::type_complexity)]
    glyph_cache: RefCell<
        HashMap<FontId, HashMap<u64, HashMap<String, (Option<FontData>, Rc<Vec<ShapedGlyph>>)>>>,
    >,
}

impl FontStore {
    /// Build from document resources + a default font (raw bytes).
    pub fn from_resources(res: &Resources, default_bytes: Arc<Vec<u8>>) -> Self {
        let mut fonts = HashMap::new();
        let mut families = HashMap::new();
        let mut font_cx = parley::FontContext::new();

        for (id, bytes) in &res.font_data {
            if let Some(font) = make_font(bytes.clone()) {
                let (family, ids) = register_font_with_ids(&mut font_cx, &font);
                if let Some(name) = family {
                    families.insert(id.clone(), name);
                }
                append_sansserif(&mut font_cx, ids);
                fonts.insert(id.clone(), font);
            }
        }

        let default = make_font(default_bytes);
        let (default_family, default_ids) = match &default {
            Some(font) => register_font_with_ids(&mut font_cx, font),
            None => (None, Vec::new()),
        };
        append_sansserif(&mut font_cx, default_ids);

        Self {
            fonts,
            default,
            font_cx: Rc::new(RefCell::new(font_cx)),
            families,
            default_family,
            glyph_cache: RefCell::new(HashMap::new()),
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

    /// Register an additional font (raw bytes) at runtime - e.g. a CJK font
    /// loaded by the web SDK after construction. The font is added to the shared
    /// `FontContext` so parley can select it: via the default family (the first
    /// registered font's family becomes the default when none is set) or via
    /// fontique's script fallback when a glyph is missing in the selected font.
    ///
    /// Returns `false` for empty/invalid bytes (no font registered).
    pub fn register_font(&mut self, bytes: Arc<Vec<u8>>) -> bool {
        let font = match make_font(bytes) {
            Some(f) => f,
            None => return false,
        };
        let family = {
            let mut fcx = self.font_cx.borrow_mut();
            let (name, ids) = register_font_with_ids(&mut fcx, &font);
            append_sansserif(&mut fcx, ids);
            name
        };
        let ok = family.is_some();
        if ok && self.default_family.is_none() {
            self.default_family = family;
        }
        // Font set changed -> cached shapes (which may reference the old font
        // selection) are invalid.
        self.glyph_cache.borrow_mut().clear();
        ok
    }

    /// Shape `text` with the document font for `font_id` (falling back to the
    /// default font's family if `font_id` is unknown), at `size`.
    ///
    /// Results are cached by `(font_id, size, text)` - OFD body text is static,
    /// so shaping happens once per unique triple and subsequent composites hit
    /// the cache (parley re-shaping dominated render time otherwise). Cleared on
    /// `register_font`.
    ///
    /// Ligatures OFF (1:1 char<->glyph). If neither a document font nor a
    /// default font is resolved, falls back to a generic system family
    /// (SansSerif) so text still renders - parley's `FontContext` discovers
    /// system fonts (via the `system` feature), and fontique's script fallback
    /// covers characters the default lacks (e.g. CJK on a Latin default).
    ///
    /// Returns the `FontData` that actually shaped the glyphs (captured from the
    /// parley run) alongside the glyph ids + positions (`Rc`-shared from the
    /// cache, so cache hits don't copy the glyph vec). The caller MUST draw with
    /// this font - it is the system font when the fallback was used, so using a
    /// different font would mismatch the glyph ids and panic in vello/skrifa.
    /// `None` only when no glyphs were produced.
    pub fn shape(
        &self,
        font_id: &FontId,
        text: &str,
        size: f64,
    ) -> (Option<FontData>, Rc<Vec<ShapedGlyph>>) {
        let size_key = size.to_bits();
        // Cache hit (no clones for the lookup: font_id borrowed, size_key Copy,
        // text borrowed via String: Borrow<str>).
        if let Some(cached) = self
            .glyph_cache
            .borrow()
            .get(font_id)
            .and_then(|m1| m1.get(&size_key))
            .and_then(|m2| m2.get(text))
            .cloned()
        {
            return cached;
        }
        // Cache miss: shape, cache, return.
        let family_name = self
            .families
            .get(font_id)
            .map(String::as_str)
            .or(self.default_family.as_deref());
        // Build a family list: the named document/default font first, then
        // `SansSerif` as a glyph-coverage fallback. `SansSerif` resolves to
        // every registered font (appended in `from_resources`/`register_font`)
        // plus system fonts (native), so a CJK char in a Latin-only named
        // family falls back to a registered CJK font. On wasm, system fonts
        // are unavailable, so without this list CJK renders as .notdef (tofu).
        let family = match family_name {
            Some(name) => FontFamily::List(Cow::Owned(vec![
                FontFamilyName::Named(Cow::Owned(name.to_string())),
                FontFamilyName::Generic(GenericFamily::SansSerif),
            ])),
            // No document/default font resolved -> generic family.
            None => FontFamily::from(GenericFamily::SansSerif),
        };
        let result = {
            let mut fcx = self.font_cx.borrow_mut();
            shape_with_family(&mut fcx, text, size, family)
        };
        let cached = (result.0, Rc::new(result.1));
        self.glyph_cache
            .borrow_mut()
            .entry(font_id.clone())
            .or_default()
            .entry(size_key)
            .or_default()
            .insert(text.to_string(), cached.clone());
        cached
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

/// Append `ids` to the `SansSerif` generic family so the default `sans-serif`
/// resolves to registered fonts. `append` (not `set`) accumulates across calls
/// so a CJK font registered earlier survives a later Latin-only registration
/// (mirrors reditor's `parley_font.rs`). No-op for empty `ids`.
///
/// This is the wasm CJK fallback path: parley's script fallback is empty on
/// wasm (fontique's `System` backend is a dummy there), so registered fonts
/// must be reachable via the `SansSerif` generic used as the list fallback in
/// [`FontStore::shape`].
fn append_sansserif(fcx: &mut parley::FontContext, ids: Vec<FamilyId>) {
    if ids.is_empty() {
        return;
    }
    fcx.collection
        .append_generic_families(GenericFamily::SansSerif, ids.into_iter());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_store_resolves_registered_document_font() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut res = Resources::default();
        res.font_data
            .insert(FontId::new("F1"), Arc::new(font_bytes.to_vec()));
        let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));
        assert!(
            store.resolve(&FontId::new("F1")).is_some(),
            "document font resolves"
        );
    }

    #[test]
    fn font_store_falls_back_to_default_when_font_absent() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
        assert!(store.default_font().is_some(), "default font available");
        assert!(
            store.resolve(&FontId::new("missing")).is_none(),
            "no document font for unknown id"
        );
    }

    #[test]
    fn font_store_shape_hello_with_document_font() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
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
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
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
        assert!(
            font.is_some(),
            "system font captured (not the empty default)"
        );
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

    #[test]
    fn font_store_register_font_then_shapes() {
        // Register a font at runtime (via register_font, like the web SDK loads
        // fonts after construction). The first registered font's family becomes
        // the default, so shaping an unknown font id uses it.
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        assert!(
            store.register_font(Arc::new(font_bytes.to_vec())),
            "font registered"
        );
        let (font, glyphs) = store.shape(&FontId::new("missing"), "Hi", 16.0);
        assert_eq!(glyphs.len(), 2, "2 glyphs via registered font");
        assert!(font.is_some(), "registered font captured");
    }

    #[test]
    fn font_store_shape_cache_hits() {
        // Shaping the same (font, size, text) twice: the second call hits the
        // glyph cache (no parley re-shaping). Verifies the cache that keeps
        // multi-page documents from re-shaping every frame.
        let store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let text = "The quick brown fox";
        let t0 = std::time::Instant::now();
        let (_f1, g1) = store.shape(&FontId::new("missing"), text, 12.0);
        let miss = t0.elapsed();
        let t1 = std::time::Instant::now();
        let (_f2, g2) = store.shape(&FontId::new("missing"), text, 12.0);
        let hit = t1.elapsed();
        assert_eq!(g1.len(), g2.len(), "cached result matches fresh");
        eprintln!("[cache] miss={:.2?} hit={:.2?}", miss, hit);
        assert!(
            hit <= miss,
            "cache hit ({:.2?}) not slower than miss ({:.2?})",
            hit,
            miss
        );
    }

    #[test]
    fn register_font_appends_to_sansserif_generic() {
        // Regression (web CJK tofu): registering a font at runtime (the web SDK
        // path) must append it to the SansSerif generic family. On wasm, system
        // fonts are unavailable (fontique's System backend is a dummy), so
        // registered fonts are the only fallback source - without this append,
        // a CJK char in a Latin-only named family has no fallback and renders
        // as .notdef (tofu). `shape` lists SansSerif after the named family, so
        // parley picks up the registered CJK font via SansSerif.
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        assert!(
            store.register_font(Arc::new(font_bytes.to_vec())),
            "font registered"
        );

        let registered_name = store
            .default_family
            .as_deref()
            .expect("default family set after register_font");
        let sansserif_ids: Vec<FamilyId> = {
            let mut fcx = store.font_cx.borrow_mut();
            fcx.collection
                .generic_families(GenericFamily::SansSerif)
                .collect()
        };
        let registered_id = {
            let mut fcx = store.font_cx.borrow_mut();
            fcx.collection
                .family_id(registered_name)
                .expect("family id resolves")
        };
        assert!(
            sansserif_ids.contains(&registered_id),
            "SansSerif generic must contain the runtime-registered font's family id"
        );
    }

    #[test]
    fn from_resources_appends_document_font_to_sansserif() {
        // Regression (web CJK tofu): document fonts (from Resources.font_data)
        // must also be appended to SansSerif so they participate in glyph-
        // coverage fallback alongside runtime-registered fonts.
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut res = Resources::default();
        res.font_data
            .insert(FontId::new("F1"), Arc::new(font_bytes.to_vec()));
        let store = FontStore::from_resources(&res, Arc::new(vec![]));

        let family_name = store
            .families
            .get(&FontId::new("F1"))
            .expect("document family registered");
        let sansserif_ids: Vec<FamilyId> = {
            let mut fcx = store.font_cx.borrow_mut();
            fcx.collection
                .generic_families(GenericFamily::SansSerif)
                .collect()
        };
        let family_id = {
            let mut fcx = store.font_cx.borrow_mut();
            fcx.collection
                .family_id(family_name)
                .expect("family id resolves")
        };
        assert!(
            sansserif_ids.contains(&family_id),
            "SansSerif generic must contain the document font's family id"
        );
    }
}
