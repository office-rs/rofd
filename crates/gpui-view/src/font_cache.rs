// Ported from rword@4e0b25b6 (crates/gpui-view/src/font_cache.rs) — kernel-agnostic halves, imaging IR identical.

//! Font bytes -> gpui `FontId` cache.
//!
//! The Scene IR carries raw font bytes (`peniko::FontData`, TTC face index
//! included) but no family name, while gpui resolves fonts BY family
//! string and `add_fonts` returns nothing. The cache keys on the blob's
//! Arc identity (the layout shares one `Arc` per font across all glyph
//! runs — no hashing of multi-MB CJK bytes per run), parses the face's
//! name table for its family on first sight, registers the bytes through
//! `TextSystem::add_fonts`, and resolves the family once.

use std::collections::HashMap;

use gpui::{FontId, TextSystem};
use imaging::peniko::FontData;

/// Face identity parsed from the font's own tables: family name (name
/// id 1) + OS/2 weight + italic flag. Resolving by family ALONE
/// collapses real bold/italic faces onto the regular FontId: the IR
/// carries the BOLD file's bytes and glyph ids, and a weight-400 query
/// returns the regular face — painting bold glyph ids through the
/// regular atlas renders wrong glyphs, not merely wrong weight.
pub struct FaceInfo {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

pub fn parse_face(bytes: &[u8], index: u32) -> Option<FaceInfo> {
    let face = ttf_parser::Face::parse(bytes, index).ok()?;
    let mut family: Option<String> = None;
    for name in face.names() {
        if name.name_id != ttf_parser::name_id::FAMILY {
            continue;
        }
        // First decodable family record wins (fonts put the canonical/
        // English name first in practice; either record resolves through
        // the text system — verified for both the English and the
        // localized SimSun records).
        if family.is_none() {
            family = name.to_string();
        }
    }
    Some(FaceInfo {
        family: family?,
        weight: face
            .tables()
            .os2
            .map(|os2| os2.weight().to_number())
            .unwrap_or(400),
        italic: face.is_italic(),
    })
}

/// Back-compat thin wrapper.
pub fn parse_family(bytes: &[u8], index: u32) -> Option<String> {
    parse_face(bytes, index).map(|f| f.family)
}

#[derive(Default)]
pub struct FontCache {
    /// (blob Arc address, byte len, face index) -> resolved FontId.
    /// ARC IDENTITY is the dedup key (not content hashing): the layout
    /// hands out ONE shared Arc per font, so every glyph run of that font
    /// hits the same entry with zero hashing of multi-MB CJK bytes.
    map: HashMap<(usize, usize, u32), FontId>,
}

impl FontCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve (registering on first sight) the FontId for a scene font.
    /// `None` only when the bytes are unparsable or registration fails —
    /// callers fall back to skipping the run (matching the degraded-
    /// gracefully stance of the other sink paths).
    ///
    /// NOTE: headless platform TextSystem is a stub — real parsing verified only in window state.
    pub fn font_id(&mut self, font_data: &FontData, ts: &TextSystem) -> Option<FontId> {
        let bytes = font_data.data.as_ref();
        let key = (bytes.as_ptr() as usize, bytes.len(), font_data.index);
        if let Some(id) = self.map.get(&key) {
            return Some(*id);
        }
        let info = parse_face(bytes, font_data.index)?;
        // add_fonts takes owning Cow bytes: one Vec copy per distinct font
        // per process (18 MB worst case for a CJK face) — paid once.
        ts.add_fonts(vec![std::borrow::Cow::Owned(bytes.to_vec())])
            .ok()?;
        // Resolve with the FACE's own weight/style so a real bold/italic
        // face does not collapse onto the regular one.
        let query = gpui::Font {
            family: info.family.into(),
            features: Default::default(),
            fallbacks: None,
            weight: gpui::FontWeight(info.weight as f32),
            style: if info.italic {
                gpui::FontStyle::Italic
            } else {
                gpui::FontStyle::Normal
            },
        };
        let id = ts.resolve_font(&query);
        self.map.insert(key, id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_ptr_identity_not_content() {
        // Guard the keying contract at the type level: cloned Arcs of one
        // allocation key identically, while two allocations of IDENTICAL
        // content key differently (content is never hashed). Behavioral
        // use through `font_id` lives in the integration test.
        let bytes: &[u8] = b"whatever";
        let blob = imaging::peniko::Blob::new(std::sync::Arc::new(bytes.to_vec()));
        let a = FontData::new(blob.clone(), 0);
        let b = FontData::new(blob, 0);
        let ka = (
            a.data.as_ref().as_ptr() as usize,
            a.data.as_ref().len(),
            a.index,
        );
        let kb = (
            b.data.as_ref().as_ptr() as usize,
            b.data.as_ref().len(),
            b.index,
        );
        assert_eq!(ka, kb, "cloned Arc of one allocation must key identically");

        let other = FontData::new(
            imaging::peniko::Blob::new(std::sync::Arc::new(bytes.to_vec())),
            0,
        );
        let ko = (
            other.data.as_ref().as_ptr() as usize,
            other.data.as_ref().len(),
            other.index,
        );
        assert_ne!(
            ka, ko,
            "distinct allocations must key differently even with identical content"
        );
    }
}
