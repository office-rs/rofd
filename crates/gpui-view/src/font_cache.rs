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

/// Bytes to register + the face info to resolve by, ensuring the font
/// carries a decodable family name.
///
/// OFD embedded fonts are frequently shipped as stripped subsets with the
/// WHOLE `name` table removed (e.g. test/sample-content.ofd's four faces).
/// The kernel shapes them fine (fontique keys on bytes, not names), but a
/// gpui `TextSystem` resolves strictly by family string and silently
/// drops nameless fonts — `add_fonts` reports Ok while `all_font_names`
/// never grows (verified against DirectWrite). Nameless faces therefore
/// get a synthetic unique family injected via write-fonts before
/// registration, so their glyph ids resolve back to their own outlines.
///
/// `None` only when the bytes are unparsable, or the face is nameless
/// inside a collection (v1 scope: single-face surgery only — nameless TTC
/// faces have not been seen in the wild).
pub fn named_font_bytes(
    bytes: &[u8],
    index: u32,
) -> Option<(std::borrow::Cow<'_, [u8]>, FaceInfo)> {
    if let Some(info) = parse_face(bytes, index) {
        return Some((std::borrow::Cow::Borrowed(bytes), info));
    }
    if index != 0 || ttf_parser::fonts_in_collection(bytes).unwrap_or(1) > 1 {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let family = format!("__rofd_font_{:016x}", hasher.finish());

    let font_ref = read_fonts::FontRef::new(bytes).ok()?;
    let mut builder = write_fonts::FontBuilder::new();
    builder.add_table(&synthetic_name_table(&family)).ok()?;
    builder.copy_missing_tables(font_ref.clone());
    let rebuilt = builder.build();
    let info = parse_face(&rebuilt, 0)?;
    Some((std::borrow::Cow::Owned(rebuilt), info))
}

/// Minimal Windows-Unicode name table carrying the family (ids 1 + 16)
/// and a matching PostScript name (id 6): enough for every text system
/// to enumerate and resolve the family. Records must stay sorted or
/// write-fonts refuses to dump the table.
fn synthetic_name_table(family: &str) -> write_fonts::tables::name::Name {
    use write_fonts::tables::name::NameRecord;

    let record = |id: u16| NameRecord {
        platform_id: 3, // Windows
        encoding_id: 1, // Unicode BMP (UTF-16BE)
        language_id: 0x0409,
        name_id: write_fonts::types::NameId::new(id),
        string: family.to_string().into(),
    };
    // 1 = family, 16 = typographic family, 6 = PostScript name.
    write_fonts::tables::name::Name {
        name_record: vec![record(1), record(6), record(16)],
        ..Default::default()
    }
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
        // Nameless subset fonts (see `named_font_bytes`) get a synthetic
        // family injected before registration; the cache key stays the
        // ORIGINAL blob identity (the layout shares one Arc per font).
        let (register, info) = named_font_bytes(bytes, font_data.index)?;
        // add_fonts takes owning Cow bytes: one Vec copy per distinct font
        // per process (18 MB worst case for a CJK face) — paid once.
        ts.add_fonts(vec![std::borrow::Cow::Owned(register.to_vec())])
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

    /// The render suite's tracked TrueType fixture, used as a base for
    /// building a nameless-font case (the real-world trigger: OFD embedded
    /// subset fonts ship with the whole `name` table stripped).
    const TEST_FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

    /// Strip the fixture's name table via write-fonts, producing the
    /// same shape the failing documents embed.
    fn strip_name_table() -> Vec<u8> {
        let font_ref = read_fonts::FontRef::new(TEST_FONT).expect("fixture parses");
        let mut builder = write_fonts::FontBuilder::new();
        for record in font_ref.table_directory().table_records() {
            let tag = record.tag();
            if tag == read_fonts::types::Tag::new(b"name") {
                continue;
            }
            let data = font_ref.table_data(tag).expect("directory record has data");
            builder.add_raw(tag, data);
        }
        builder.build()
    }

    #[test]
    fn named_font_bytes_passthrough_when_family_exists() {
        let (cow, info) = named_font_bytes(TEST_FONT, 0).expect("named font resolves");
        assert!(matches!(cow, std::borrow::Cow::Borrowed(_)));
        assert!(!info.family.is_empty());
    }

    #[test]
    fn named_font_bytes_synthesizes_family_for_nameless_font() {
        let nameless = strip_name_table();
        // Fixture sanity: the stripped blob really has no name table.
        let face = ttf_parser::Face::parse(&nameless, 0).expect("stripped font still parses");
        assert!(face.names().into_iter().count() == 0);

        let (cow, info) =
            named_font_bytes(&nameless, 0).expect("nameless font gets a synthetic name");
        assert!(matches!(cow, std::borrow::Cow::Owned(_)));

        // The synthetic family is unique-stable and parse_face finds it.
        assert!(info.family.starts_with("__rofd_font_"), "{}", info.family);
        let reparsed = parse_face(&cow, 0).expect("synthetic font reparses");
        assert_eq!(reparsed.family, info.family);

        // Deterministic: same bytes -> same synthetic name.
        let (_, info2) = named_font_bytes(&nameless, 0).unwrap();
        assert_eq!(info.family, info2.family);

        // Glyph data survives the rewrite intact.
        let before = ttf_parser::Face::parse(TEST_FONT, 0)
            .unwrap()
            .number_of_glyphs();
        let after = ttf_parser::Face::parse(&cow, 0).unwrap().number_of_glyphs();
        assert_eq!(
            before, after,
            "glyph count preserved through name injection"
        );
    }

    #[test]
    fn named_font_bytes_rejects_garbage() {
        assert!(named_font_bytes(b"not a font", 0).is_none());
    }

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
