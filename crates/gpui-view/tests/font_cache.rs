// Ported from rword@4e0b25b6 (crates/gpui-view/tests/font_cache.rs) — kernel-agnostic halves, imaging IR identical.

//! FontData blob -> gpui FontId cache integration.
//!
//! The Scene IR carries raw font bytes (`peniko::FontData`, TTC index
//! included) but no family name, while gpui resolves fonts BY family
//! string (`add_fonts` returns nothing). The cache therefore:
//! 1. keys on (blob Arc identity, face index);
//! 2. on a miss, parses the face's name table for its family, registers
//!    the bytes via `TextSystem::add_fonts`, resolves `font(family)`;
//! 3. returns the cached FontId forever after (registration is
//!    idempotent at the text-system level, but we skip the repeat cost).
//!
//! Fixture: the tracked Noto Sans CJK SC subset under
//! crates/render/tests/fixtures/fonts (2.5 KB, present in every checkout).
//! The full 16 MB font under crates/web-app/public/fonts is gitignored and
//! absent in CI - reading it made this suite fail there.

use gpui::TestAppContext;
use rofd_gpui_view::font_cache::{self, FontCache};

const FIXTURE_A: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFontCjk.otf");

fn fixture_bytes() -> Vec<u8> {
    FIXTURE_A.to_vec()
}

#[test]
fn parse_family_reads_name_table() {
    let family = font_cache::parse_family(&fixture_bytes(), 0).expect("family from name table");
    assert!(
        family.to_lowercase().contains("noto sans cjk"),
        "unexpected family: {family}"
    );
}

#[test]
fn parse_family_bad_bytes_is_none() {
    assert!(font_cache::parse_family(b"not a font at all", 0).is_none());
    // Out-of-range TTC index degrades to None rather than panicking.
    assert!(font_cache::parse_family(&fixture_bytes(), 99).is_none());
}

#[gpui::test]
fn cache_resolves_via_text_system(cx: &mut TestAppContext) {
    let bytes = fixture_bytes();
    let font_data =
        imaging::peniko::FontData::new(imaging::peniko::Blob::new(std::sync::Arc::new(bytes)), 0);
    let ts = cx.update(|cx| cx.text_system().clone());
    let mut cache = FontCache::new();
    let first = cache
        .font_id(&font_data, &ts)
        .expect("first resolution succeeds");
    let second = cache
        .font_id(&font_data, &ts)
        .expect("second resolution succeeds");
    assert_eq!(first, second, "same blob must resolve to one FontId");

    // NOTE: the HEADLESS test platform's text system is a stub — every
    // family resolves to the same FontId, so a fallback-inequality
    // assertion is unprovable here. Real resolution was verified
    // windowed (SimSun vs a bogus family gave distinct ids); this test
    // proves the cache contract itself: a stable id, dedup, and a Some
    // over the parse+register path.
}
