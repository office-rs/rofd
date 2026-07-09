use std::collections::HashMap;
use std::sync::Arc;

use peniko::FontData;
use rofd_dom::{FontId, Resources};

/// Holds document fonts (from `Resources.font_data`) + a default fallback font,
/// resolved to `peniko::FontData` for Vello `draw_glyphs`.
///
/// On peniko 0.6 the font handle is `FontData` (re-exported from
/// `linebender_resource_handle`); `Scene::draw_glyphs` takes `&FontData`.
/// `Blob::new` wraps an `Arc<dyn AsRef<[u8]> + Send + Sync>` - an `Arc<Vec<u8>>`
/// coerces without copying the font bytes.
#[derive(Clone)]
pub struct FontStore {
    fonts: HashMap<FontId, FontData>,
    default: Option<FontData>,
}

impl FontStore {
    /// Build from document resources + a default font (raw bytes).
    pub fn from_resources(res: &Resources, default_bytes: Arc<Vec<u8>>) -> Self {
        let mut fonts = HashMap::new();
        for (id, bytes) in &res.font_data {
            if let Some(font) = make_font(bytes.clone()) {
                fonts.insert(id.clone(), font);
            }
        }
        let default = make_font(default_bytes);
        Self { fonts, default }
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
}

/// Wrap raw font bytes into a shareable `peniko::FontData` (collection index 0).
/// `Blob::new` takes `Arc<dyn AsRef<[u8]> + Send + Sync>`; an `Arc<Vec<u8>>`
/// coerces, so the bytes are shared (not copied).
fn make_font(bytes: Arc<Vec<u8>>) -> Option<FontData> {
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
}
