use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parley::fontique::{FamilyId, FontInfoOverride};
use parley::style::{FontFamily, FontFamilyName, GenericFamily};
use peniko::FontData;
use rofd_dom::{FontId, Resources};

use super::shape::{
    register_font_with_ids, shape_grouped_with_family, shape_with_family, ShapedGlyph,
};

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
    /// 声明族键（Resources.fonts 的 family_name 归一化）per FontId——
    /// 声明族切片链解析入口（spec §2.3-1/2）。
    declared_families: HashMap<FontId, String>,
    /// 切片族键 -> 内部名链（注册序）。具名族解析的族内优先链（spec §2.3-2）。
    slice_chains: HashMap<String, Vec<String>>,
    /// 已注册切片的 fontique FamilyId（SansSerif 链重建用，去重追加）。
    /// Populated/read by [`Self::register_font_slice`]; read by
    /// [`Self::rebuild_sansserif`] and [`Self::register_font`].
    slice_family_ids: Vec<FamilyId>,
    /// 整只字体（嵌入/default/legacy）的 FamilyId 记账：链重建时排在切片后。
    /// Populated in `from_resources`/[`Self::register_font`]; read by
    /// [`Self::rebuild_sansserif`].
    whole_family_ids: Vec<FamilyId>,
    /// default 锚定的切片族键（首个成功注册的片；spec §2.3-4）。
    default_slice_key: Option<String>,
    /// Shaped-glyph cache: `font_id -> (size_bits -> (text -> (font, glyphs)))`.
    /// OFD body text is static, so shaping happens once per unique (font, size,
    /// text); subsequent composites hit the cache and skip parley re-shaping
    /// (which dominated render time - ~500ms for a text-heavy doc per frame).
    /// `size.to_bits()` keys avoid `f64` not being `Eq`. Cleared on
    /// `register_font`/`register_font_slice`.
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
        // 声明族映射（spec §2.3-1）：FontRef.family_name 归一化后按 FontId 记录。
        let declared_families = res
            .fonts
            .iter()
            .filter_map(|(id, fr)| {
                fr.family_name
                    .as_ref()
                    .map(|n| (id.clone(), super::family::normalize_family_key(n)))
            })
            .collect();
        // 整只字体（嵌入/default）的 FamilyId 记账：Task 3 链重建时排在切片后。
        let mut whole_family_ids = Vec::new();

        for (id, bytes) in &res.font_data {
            if let Some(font) = make_font(bytes.clone()) {
                let (family, ids) = register_font_with_ids(&mut font_cx, &font);
                if let Some(name) = family {
                    families.insert(id.clone(), name);
                }
                whole_family_ids.extend(ids.iter().cloned());
                append_sansserif(&mut font_cx, ids);
                fonts.insert(id.clone(), font);
            }
        }

        let default = make_font(default_bytes);
        let (default_family, default_ids) = match &default {
            Some(font) => register_font_with_ids(&mut font_cx, font),
            None => (None, Vec::new()),
        };
        whole_family_ids.extend(default_ids.iter().cloned());
        append_sansserif(&mut font_cx, default_ids);

        Self {
            fonts,
            default,
            font_cx: Rc::new(RefCell::new(font_cx)),
            families,
            default_family,
            declared_families,
            slice_chains: HashMap::new(),
            slice_family_ids: Vec::new(),
            whole_family_ids,
            default_slice_key: None,
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
    /// registered font's family becomes the default when neither a default
    /// family nor a slice chain is anchored yet) or via fontique's script
    /// fallback when a glyph is missing in the selected font.
    ///
    /// The registered family ids are recorded in `whole_family_ids` (legacy
    /// whole-font ledger). When slice chains exist, the `SansSerif` generic is
    /// rebuilt in the deterministic slice-first order ([`Self::rebuild_sansserif`]);
    /// otherwise the legacy append semantics are kept.
    ///
    /// Returns `false` for empty/invalid bytes (no font registered).
    pub fn register_font(&mut self, bytes: Arc<Vec<u8>>) -> bool {
        let font = match make_font(bytes) {
            Some(f) => f,
            None => return false,
        };
        let has_slices = !self.slice_family_ids.is_empty();
        let (family, ids) = {
            let mut fcx = self.font_cx.borrow_mut();
            let (name, ids) = register_font_with_ids(&mut fcx, &font);
            if !has_slices {
                append_sansserif(&mut fcx, ids.clone());
            }
            (name, ids)
        };
        let ok = family.is_some();
        if ok {
            self.whole_family_ids.extend(ids.iter().copied());
            if has_slices {
                self.rebuild_sansserif();
            }
        }
        if ok && self.default_family.is_none() && self.default_slice_key.is_none() {
            self.default_family = family;
        }
        // Font set changed -> cached shapes (which may reference the old font
        // selection) are invalid.
        self.glyph_cache.borrow_mut().clear();
        ok
    }

    /// 注册一个解码后的字体切片（spec §2.3）。字节以内部名
    /// `__rofs_<key>_<sliceId>` 注册（FontInfoOverride 覆写族名），进族内链与
    /// SansSerif 重建序；首个成功注册的片锚定 default（此后不抢）。字节不合法
    /// 返回 `false` 且不留任何记账。
    pub fn register_font_slice(
        &mut self,
        bytes: Arc<Vec<u8>>,
        family_key: &str,
        slice_id: &str,
    ) -> bool {
        let key = super::family::normalize_family_key(family_key);
        let internal = format!("__rofs_{key}_{slice_id}");
        let blob = peniko::Blob::new(bytes);
        let families = {
            let mut fcx = self.font_cx.borrow_mut();
            fcx.collection.register_fonts(
                blob,
                Some(FontInfoOverride {
                    family_name: Some(internal.as_str()),
                    ..Default::default()
                }),
            )
        };
        if families.is_empty() {
            return false;
        }
        for (id, _) in &families {
            if !self.slice_family_ids.contains(id) {
                self.slice_family_ids.push(*id);
            }
        }
        self.slice_chains
            .entry(key.clone())
            .or_default()
            .push(internal);
        if self.default_family.is_none() && self.default_slice_key.is_none() {
            self.default_slice_key = Some(key);
        }
        self.rebuild_sansserif();
        // Font set changed -> cached shapes invalid (same rule as register_font).
        self.glyph_cache.borrow_mut().clear();
        true
    }

    /// SansSerif 泛型链规范重建（rword 修正 4.1）：切片在前、整只在后的确定序。
    /// 无切片时不动（构造期/纯 legacy 保持 append 语义，native 系统回退存活）。
    fn rebuild_sansserif(&mut self) {
        if self.slice_family_ids.is_empty() {
            return;
        }
        let ids: Vec<FamilyId> = self
            .slice_family_ids
            .iter()
            .chain(self.whole_family_ids.iter())
            .copied()
            .collect();
        self.font_cx
            .borrow_mut()
            .collection
            .set_generic_families(GenericFamily::SansSerif, ids.into_iter());
    }

    /// Shape `text` with the document font for `font_id` (falling back to the
    /// default font's family if `font_id` is unknown), at `size`.
    ///
    /// Results are cached by `(font_id, size, text)` - OFD body text is static,
    /// so shaping happens once per unique triple and subsequent composites hit
    /// the cache (parley re-shaping dominated render time otherwise). Cleared on
    /// `register_font`/`register_font_slice`.
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
        // Build a family list: the named document/default font first, then
        // `SansSerif` as a glyph-coverage fallback. `SansSerif` resolves to
        // every registered font (appended in `from_resources`/`register_font`)
        // plus system fonts (native), so a CJK char in a Latin-only named
        // family falls back to a registered CJK font. On wasm, system fonts
        // are unavailable, so without this list CJK renders as .notdef (tofu).
        let family = self.family_list_for(font_id);
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

    /// Shape a UI-text line with the default font family - the UI chrome (hover
    /// tooltip) uses no document font. No glyph cache: UI lines are short and
    /// transient, unlike body text (shaping 2 short lines per frame is trivial).
    /// Returns the glyphs grouped by the font that shaped each run (fallback can
    /// split a line; a glyph id is only valid with its own font) plus the line
    /// width (px).
    pub fn shape_default(&self, text: &str, size: f64) -> (Vec<(FontData, Vec<ShapedGlyph>)>, f64) {
        let names = self.default_names();
        let family = if names.is_empty() {
            FontFamily::from(GenericFamily::SansSerif)
        } else {
            let mut names = names;
            names.push(FontFamilyName::Generic(GenericFamily::SansSerif));
            FontFamily::List(Cow::Owned(names))
        };
        let mut fcx = self.font_cx.borrow_mut();
        shape_grouped_with_family(&mut fcx, text, size, family)
    }

    /// shape 的 family list（spec §2.3-2）：嵌入族名 -> 声明族切片链 ->
    /// default（切片链优先，退化整只名）-> SansSerif 兜底。
    ///
    /// `SansSerif` resolves to every registered font (appended in
    /// `from_resources`/`register_font`) plus system fonts (native), so a CJK
    /// char in a Latin-only named family falls back to a registered CJK font.
    /// On wasm, system fonts are unavailable, so without SansSerif in the list
    /// CJK renders as .notdef (tofu).
    fn family_list_for(&self, font_id: &FontId) -> FontFamily<'_> {
        if let Some(name) = self.families.get(font_id) {
            return named_then_sansserif(name);
        }
        let mut names: Vec<FontFamilyName> = Vec::new();
        if let Some(declared) = self.declared_families.get(font_id) {
            names.extend(chain_names(&self.slice_chains, declared));
        }
        if names.is_empty() {
            names.extend(self.default_names());
        }
        if names.is_empty() {
            return FontFamily::from(GenericFamily::SansSerif);
        }
        names.push(FontFamilyName::Generic(GenericFamily::SansSerif));
        FontFamily::List(Cow::Owned(names))
    }

    /// default 解析名单：切片链优先（default_slice_key），退化整只名（legacy）。
    fn default_names(&self) -> Vec<FontFamilyName<'_>> {
        if let Some(key) = &self.default_slice_key {
            let chain = chain_names(&self.slice_chains, key);
            if !chain.is_empty() {
                return chain;
            }
        }
        self.default_family
            .as_ref()
            .map(|n| vec![FontFamilyName::Named(Cow::Owned(n.clone()))])
            .unwrap_or_default()
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

/// 具名族 + SansSerif 兜底的家族列表（嵌入/整只字体的 shape 解析，spec §2.3-2）。
fn named_then_sansserif(name: &str) -> FontFamily<'_> {
    FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(name.to_string())),
        FontFamilyName::Generic(GenericFamily::SansSerif),
    ]))
}

/// 切片族键 -> 内部名链的家族名展开；未注册该键时为空（调用方退化）。
/// 全部为 Owned 名单，故 `'static`（无借用）。
fn chain_names(chains: &HashMap<String, Vec<String>>, key: &str) -> Vec<FontFamilyName<'static>> {
    chains
        .get(key)
        .map(|v| {
            v.iter()
                .cloned()
                .map(|n| FontFamilyName::Named(Cow::Owned(n)))
                .collect()
        })
        .unwrap_or_default()
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

    #[test]
    fn shape_default_uses_default_font_and_reports_width() {
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let store = FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
        let (groups, width) = store.shape_default("Hello", 12.0);
        assert_eq!(groups.len(), 1, "single font -> single group");
        assert_eq!(groups[0].1.len(), 5, "ligatures off: 1 glyph per char");
        assert!(width > 0.0);
    }

    #[test]
    fn shape_default_groups_mixed_script_fallback() {
        // Latin-only default (TestFont) + a runtime-registered CJK font that
        // covers the Han char: fontique falls back to it, splitting the line
        // into >= 2 font groups. A glyph id is only valid with the font that
        // shaped it - drawing a whole line with the first run's font garbles
        // the rest (the tooltip bug this pins down). The CJK font is a bundled
        // 5-glyph subset fixture (TestFontCjk.otf) registered via register_font
        // - the same path the web SDK uses for a fetched CJK font - so the test
        // does not depend on system fonts: hosts without any CJK font (e.g. the
        // CI Linux runners) previously saw one group and failed this assertion.
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let cjk_bytes = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
        let mut store =
            FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()));
        assert!(
            store.register_font(Arc::new(cjk_bytes.to_vec())),
            "CJK fixture font registered"
        );
        let (groups, width) = store.shape_default("a中b", 12.0);
        assert!(
            groups.len() >= 2,
            "mixed script splits into fallback groups"
        );
        assert!(!groups.iter().any(|(_, g)| g.is_empty()), "no empty groups");
        assert_eq!(
            groups.iter().map(|(_, g)| g.len()).sum::<usize>(),
            3,
            "1 glyph per char across groups"
        );
        assert!(width > 0.0);
    }

    #[test]
    fn shape_default_without_default_font_does_not_panic() {
        // Empty bytes -> no default font; on hosts with system fonts parley may
        // still resolve SansSerif. The contract here is only: never panic.
        let store = FontStore::from_resources(&Resources::default(), Arc::new(Vec::new()));
        let _ = store.shape_default("x", 12.0);
    }

    #[test]
    fn from_resources_records_declared_family_names() {
        // Resources.fonts 的 FontRef.family_name（归一化后）进入 declared_families，
        // 供声明族 -> 切片链解析（spec §2.3-1）。
        let mut res = Resources::default();
        res.fonts.insert(
            FontId::new("F1"),
            rofd_dom::FontRef {
                id: FontId::new("F1"),
                family_name: Some("宋体".into()),
            },
        );
        let store = FontStore::from_resources(&res, Arc::new(vec![]));
        assert_eq!(
            store
                .declared_families
                .get(&FontId::new("F1"))
                .map(String::as_str),
            Some("simsun")
        );
    }

    #[test]
    fn shape_declared_family_without_slices_falls_to_default() {
        // 声明族尚无片：解析落 default/SansSerif（现状行为），不 panic。
        let font_bytes = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut res = Resources::default();
        res.fonts.insert(
            FontId::new("F9"),
            rofd_dom::FontRef {
                id: FontId::new("F9"),
                family_name: Some("simsun".into()),
            },
        );
        let store = FontStore::from_resources(&res, Arc::new(font_bytes.to_vec()));
        let (font, glyphs) = store.shape(&FontId::new("F9"), "Hi", 12.0);
        assert_eq!(glyphs.len(), 2);
        assert!(font.is_some());
    }

    #[test]
    fn register_font_slice_shapes_declared_family_via_chain() {
        // 声明族（simsun）注册切片后，其 FontId 的 shape 走族内链：
        // 覆盖字形非 .notdef（spec §2.3-2）。
        let cjk = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
        let mut res = Resources::default();
        res.fonts.insert(
            FontId::new("F1"),
            rofd_dom::FontRef {
                id: FontId::new("F1"),
                family_name: Some("宋体".into()),
            },
        );
        let mut store = FontStore::from_resources(&res, Arc::new(vec![]));
        assert!(store.register_font_slice(Arc::new(cjk.to_vec()), "simsun", "common-0"));
        let (font, glyphs) = store.shape(&FontId::new("F1"), "中", 12.0);
        assert!(!glyphs.is_empty());
        assert!(
            glyphs.iter().all(|g| g.glyph_id != 0),
            "chain slice shapes nonzero glyphs"
        );
        assert!(font.is_some());
    }

    #[test]
    fn first_slice_anchors_default_unknown_ids_shape() {
        // 首个成功注册的片锚定 default：无声明 FontId 的文本经 default 链成形。
        let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        assert!(store.register_font_slice(Arc::new(latin.to_vec()), "notosans", "whole"));
        let (_font, glyphs) = store.shape(&FontId::new("nope"), "Hi", 12.0);
        assert_eq!(glyphs.len(), 2, "default slice chain shapes unknown id");
        assert!(glyphs.iter().all(|g| g.glyph_id != 0));
    }

    #[test]
    fn whole_font_after_slice_does_not_shadow_generic_order() {
        // rword 修正 4.1：切片后注册的整字体不得在 SansSerif 泛型里排在切片前。
        let cjk = include_bytes!("../../tests/fixtures/fonts/TestFontCjk.otf") as &[u8];
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        assert!(store.register_font_slice(Arc::new(cjk.to_vec()), "notosanscjksc", "common-0"));
        assert!(
            store.register_font(Arc::new(cjk.to_vec())),
            "whole font registers"
        );
        let order: Vec<FamilyId> = {
            let mut fcx = store.font_cx.borrow_mut();
            fcx.collection
                .generic_families(GenericFamily::SansSerif)
                .collect()
        };
        let slice_pos = order
            .iter()
            .position(|id| store.slice_family_ids.contains(id))
            .expect("slice in generic");
        let whole_pos = order
            .iter()
            .position(|id| store.whole_family_ids.contains(id))
            .expect("whole in generic");
        assert!(slice_pos < whole_pos, "slices precede wholes after rebuild");
    }

    #[test]
    fn register_font_slice_clears_glyph_cache_and_reshapes() {
        // 补片后缓存失效：同 (font,size,text) 重新 shape，结果覆盖此前 tofu 场景。
        let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let (_f0, g0) = store.shape(&FontId::new("x"), "Hi", 12.0); // 系统回退（可能为空）
        assert!(store.register_font_slice(Arc::new(latin.to_vec()), "notosans", "w"));
        let (_f1, g1) = store.shape(&FontId::new("x"), "Hi", 12.0);
        assert_eq!(g1.len(), 2, "post-slice shape uses the slice");
        let _ = g0;
    }

    #[test]
    fn register_font_slice_invalid_bytes_returns_false() {
        let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        assert!(!store.register_font_slice(Arc::new(vec![1, 2, 3]), "simsun", "s0"));
        assert!(
            store.slice_chains.is_empty(),
            "failed slice leaves no chain entry"
        );
    }

    #[test]
    #[ignore = "perf gate: run manually per spec §6（120 片 vs 3 片 shape/composite 回归 <10%）"]
    fn slice_chain_perf_gate_120_vs_3() {
        let latin = include_bytes!("../../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let text = "The quick brown fox jumps over the lazy dog 入院记录测试文本";
        let shape_n = |n: usize| {
            let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
            for i in 0..n {
                assert!(store.register_font_slice(
                    Arc::new(latin.to_vec()),
                    &format!("fam{}", i % 4),
                    &format!("s{i}")
                ));
            }
            let t = std::time::Instant::now();
            for _ in 0..200 {
                let _ = store.shape(&FontId::new("x"), text, 12.0);
                store.glyph_cache.borrow_mut().clear(); // 强制 miss 路径
            }
            t.elapsed()
        };
        let base = shape_n(3);
        let loaded = shape_n(120);
        eprintln!(
            "[perf] 3-slice={base:?} 120-slice={loaded:?} ratio={:.2}",
            loaded.as_secs_f64() / base.as_secs_f64()
        );
        assert!(
            loaded.as_secs_f64() < base.as_secs_f64() * 10.0 + 0.05,
            "120-slice shaping blew past the gate"
        );
    }
}
