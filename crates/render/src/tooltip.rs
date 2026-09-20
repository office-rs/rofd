//! Hover tooltip chrome (spec 2026-09-18-annotation-hover-tooltip §3.3).
//! Viewport-space UI: fixed logical-pixel sizes, never zoom-scaled, painted
//! last (above scrollbars) by `EditorComponent::build_scene`.

use imaging::kurbo::{Affine, RoundedRect, Stroke};
use imaging::record::{Glyph, Scene};
use imaging::Painter;
use peniko::{Color, Fill, FontData, Style};

use crate::text::FontStore;

/// Card offset from the cursor (bottom-right of the pointer, spec §2.1).
pub const CURSOR_OFFSET: f64 = 16.0;
pub const PADDING: f64 = 8.0;
pub const LINE_GAP: f64 = 4.0;
pub const FONT_SIZE: f64 = 12.0;
/// Line box height (approximate: size * 1.35 covers ascent + descent).
pub const LINE_HEIGHT: f64 = FONT_SIZE * 1.35;
pub const RADIUS: f64 = 4.0;

const BG: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xF0); // ~94% opaque
const BORDER: Color = Color::from_rgba8(0xC9, 0xCD, 0xD4, 0xFF);
const TEXT: Color = Color::from_rgba8(0x33, 0x38, 0x40, 0xFF);

/// One shaped tooltip line: per-font glyph groups (font fallback splits
/// mixed-script lines) plus the line width (px).
type ShapedLine = (Vec<(FontData, Vec<Glyph>)>, f64);

/// Card top-left for a `card_w x card_h` tooltip near `cursor`: cursor's
/// bottom-right, flipping to left/top when the card would overflow the
/// right/bottom viewport edge, then clamped into the viewport. Pure.
pub fn tooltip_anchor(
    card_w: f64,
    card_h: f64,
    cursor: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let mut x = cursor.0 + CURSOR_OFFSET;
    let mut y = cursor.1 + CURSOR_OFFSET;
    if x + card_w > viewport.0 {
        x = cursor.0 - CURSOR_OFFSET - card_w;
    }
    if y + card_h > viewport.1 {
        y = cursor.1 - CURSOR_OFFSET - card_h;
    }
    (
        x.clamp(0.0, (viewport.0 - card_w).max(0.0)),
        y.clamp(0.0, (viewport.1 - card_h).max(0.0)),
    )
}

/// Paint the hover tooltip card. Skips silently when no line shapes to
/// glyphs (no font) - pure UI degradation, never fatal (AGENTS §4.6).
pub fn paint_tooltip(
    scene: &mut Scene,
    lines: &[String],
    cursor: (f64, f64),
    viewport: (f64, f64),
    fonts: &FontStore,
) {
    if lines.is_empty() {
        return;
    }
    // Shape every line once; drop blank lines (spec §4: all-blank -> no card)
    // and lines that yield no glyphs (no resolvable font). Nothing drawable ->
    // nothing to show. A line can shape into SEVERAL font groups (fallback
    // splits e.g. a CJK label from Latin digits); every group keeps the shared
    // layout-relative positions, so all groups of a line draw at the same
    // transform - a glyph id is only valid with the font that shaped it.
    let shaped: Vec<ShapedLine> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let (groups, width) = fonts.shape_default(line, FONT_SIZE);
            let positioned: Vec<(FontData, Vec<Glyph>)> = groups
                .into_iter()
                .filter(|(_, glyphs)| !glyphs.is_empty())
                .map(|(font, glyphs)| {
                    let run: Vec<Glyph> = glyphs
                        .iter()
                        .map(|g| Glyph {
                            id: g.glyph_id,
                            x: g.x,
                            y: g.y,
                        })
                        .collect();
                    (font, run)
                })
                .collect();
            if positioned.is_empty() {
                return None;
            }
            Some((positioned, width))
        })
        .collect();
    if shaped.is_empty() {
        return;
    }
    let card_w = shaped.iter().map(|(_, w)| *w).fold(0.0_f64, f64::max) + 2.0 * PADDING;
    let card_h =
        shaped.len() as f64 * LINE_HEIGHT + (shaped.len() as f64 - 1.0) * LINE_GAP + 2.0 * PADDING;
    let (x, y) = tooltip_anchor(card_w, card_h, cursor, viewport);

    let mut painter = Painter::new(scene);
    let card = RoundedRect::new(x, y, x + card_w, y + card_h, RADIUS);
    painter.fill(card, BG).draw();
    painter.stroke(card, &Stroke::new(1.0), BORDER).draw();
    for (i, (groups, _)) in shaped.iter().enumerate() {
        let line_y = y + PADDING + i as f64 * (LINE_HEIGHT + LINE_GAP);
        for (font, glyphs) in groups {
            // Parley glyph y is layout-relative (first baseline at the ascent
            // from the layout top) - same convention as annotation text: the
            // line's ink top lands on `line_y`.
            painter
                .glyphs(font, TEXT)
                .font_size(FONT_SIZE as f32)
                .transform(Affine::translate((x + PADDING, line_y)))
                .draw(&Style::Fill(Fill::NonZero), glyphs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imaging::record::{Command, Draw};
    use rofd_dom::Resources;
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(bytes.to_vec()))
    }

    fn draws(scene: &Scene) -> Vec<&Draw> {
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => Some(scene.draw_op(*id)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn anchor_bottom_right_of_cursor() {
        let (x, y) = tooltip_anchor(100.0, 50.0, (200.0, 200.0), (800.0, 600.0));
        assert_eq!((x, y), (216.0, 216.0));
    }

    #[test]
    fn anchor_flips_at_right_and_bottom_edges() {
        let (x, y) = tooltip_anchor(100.0, 50.0, (790.0, 580.0), (800.0, 600.0));
        // 790+16+100 > 800 -> flip left: 790-16-100 = 674; y: 580+16+50 > 600 -> 580-16-50 = 514.
        assert_eq!((x, y), (674.0, 514.0));
    }

    #[test]
    fn anchor_flip_past_edge_clamps_to_zero() {
        // Small viewport: both flips would land negative -> clamped to 0.
        let (x, y) = tooltip_anchor(60.0, 30.0, (4.0, 4.0), (50.0, 40.0));
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn empty_lines_paint_nothing() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        paint_tooltip(&mut scene, &[], (100.0, 100.0), (800.0, 600.0), &fonts);
        assert!(draws(&scene).is_empty());
    }

    #[test]
    fn two_lines_paint_card_border_and_glyph_runs() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        let lines = vec!["author".to_string(), "2026-07-10 00:00".to_string()];
        paint_tooltip(&mut scene, &lines, (100.0, 100.0), (800.0, 600.0), &fonts);
        let d = draws(&scene);
        assert_eq!(
            d.len(),
            4,
            "card fill + 1px border + one glyph run per line"
        );
        assert!(matches!(d[0], Draw::Fill { .. }), "card background");
        assert!(matches!(d[1], Draw::Stroke { .. }), "card border");
        assert!(matches!(d[2], Draw::GlyphRun(_)), "line 1 glyphs");
        assert!(matches!(d[3], Draw::GlyphRun(_)), "line 2 glyphs");
    }

    #[test]
    fn mixed_script_line_draws_one_run_per_font_group() {
        // Whatever font groups the shaper produces (system fallback splits a
        // CJK+Latin line when the default font is Latin-only), paint_tooltip
        // draws exactly one glyph run per group - a glyph id is only valid
        // with the font that shaped it.
        let mut scene = Scene::new();
        let fonts = test_font_store();
        let lines = vec!["时间：2026-07-10 08:00".to_string()];
        let (groups, _) = fonts.shape_default(&lines[0], FONT_SIZE);
        assert!(
            !groups.is_empty(),
            "line shapes (system fallback covers CJK)"
        );
        paint_tooltip(&mut scene, &lines, (50.0, 50.0), (800.0, 600.0), &fonts);
        let run_count = draws(&scene)
            .iter()
            .filter(|d| matches!(d, Draw::GlyphRun(_)))
            .count();
        assert_eq!(
            run_count,
            groups.len(),
            "one glyph run per font group (glyph ids are font-specific)"
        );
    }

    #[test]
    fn blank_lines_paint_nothing() {
        let mut scene = Scene::new();
        let fonts = test_font_store();
        let lines = vec![String::new(), "   ".to_string()];
        paint_tooltip(&mut scene, &lines, (100.0, 100.0), (800.0, 600.0), &fonts);
        assert!(draws(&scene).is_empty(), "no drawable line -> no card");
    }
}
