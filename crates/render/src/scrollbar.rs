//! Persistent scrollbar geometry, hit-testing and painting.
//!
//! Pure functions over [`OfdDocument`] + [`Viewport`]: which axes overflow,
//! track/thumb rectangles in viewport space, chrome hit-testing, and the
//! top-of-scene paint pass. The component owns the drag state machine; this
//! module never reads a clock or touches platform types (AGENTS §4.4/§4.9).

use imaging::kurbo::{Line, Rect, Stroke};
use imaging::peniko::Color;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::OfdDocument;

use crate::viewport::Viewport;

/// Scrollbar track thickness in device pixels (screen-space, zoom-independent).
pub const SCROLLBAR_THICKNESS: f64 = 12.0;
/// Inset between the thumb and the track edges, in device pixels.
pub const THUMB_INSET: f64 = 2.0;
/// Minimum thumb length in device pixels.
pub const THUMB_MIN_LEN: f64 = 24.0;
/// Clicking a track pages by this fraction of the visible content region.
pub const TRACK_PAGE_RATIO: f64 = 0.9;

/// Which scrollbar axis a piece of chrome belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// One scrollbar's geometry in viewport coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarGeom {
    pub axis: Axis,
    pub track: Rect,
    pub thumb: Rect,
}

/// Result of the two-pass overflow analysis: the content region (viewport
/// minus visible scrollbar strips) and the per-axis bar geometry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollbarLayout {
    pub content_size: (f64, f64),
    pub vertical: Option<BarGeom>,
    pub horizontal: Option<BarGeom>,
    /// Bottom-right corner square (only when both bars are visible).
    pub corner: Option<Rect>,
}

/// Total content extent in device pixels: `(widest_page, page_gap + inner_h)`.
/// Mirrors the stacking math in `composite::page_origin` / `clamp_scroll`.
pub fn content_metrics(doc: &OfdDocument, vp: &Viewport) -> (f64, f64) {
    let content_w = doc
        .pages
        .iter()
        .map(|p| p.physical_box.w * vp.zoom)
        .fold(0.0_f64, f64::max);
    let pages_h: f64 = doc.pages.iter().map(|p| p.physical_box.h * vp.zoom).sum();
    let inner_h = pages_h + vp.page_gap * doc.pages.len().saturating_sub(1) as f64;
    (content_w, vp.page_gap + inner_h)
}

/// Maximum legal `scroll.1` for the given content/region heights.
pub fn scroll_y_max(content_h: f64, region_h: f64) -> f64 {
    (content_h - region_h).max(0.0)
}

/// Half the horizontal scroll range (`|scroll.0| <= x_margin`); 0 when the
/// widest page fits the region (the stack stays centered).
pub fn scroll_x_margin(content_w: f64, region_w: f64) -> f64 {
    (content_w - region_w).max(0.0) / 2.0
}

/// Region dimension after reserving a bar strip. Non-positive viewport dims
/// (degenerate test sizes) pin to 0 instead of going negative.
fn region_dim(full: f64, bar_present: bool) -> f64 {
    if full <= 0.0 {
        0.0
    } else {
        (full
            - if bar_present {
                SCROLLBAR_THICKNESS
            } else {
                0.0
            })
        .max(0.0)
    }
}

/// Compute which axes overflow (two-pass: a bar appearing on one axis shrinks
/// the other axis' region, which may itself start overflowing) and the
/// resulting track/thumb rectangles.
pub fn scrollbar_layout(doc: &OfdDocument, vp: &Viewport) -> ScrollbarLayout {
    if doc.pages.is_empty() {
        return ScrollbarLayout::default();
    }
    let (content_w, content_h) = content_metrics(doc, vp);

    let mut need_v = content_h > vp.size.1.max(0.0);
    let mut need_h = content_w > vp.size.0.max(0.0);
    for _ in 0..2 {
        let next_v = content_h > region_dim(vp.size.1, need_h);
        let next_h = content_w > region_dim(vp.size.0, need_v);
        if (next_v, next_h) == (need_v, need_h) {
            break;
        }
        need_v = next_v;
        need_h = next_h;
    }

    let region_w = region_dim(vp.size.0, need_v);
    let region_h = region_dim(vp.size.1, need_h);

    let vertical = need_v.then(|| {
        // Vertical track: full content-region height against the right edge.
        let track = Rect::new(vp.size.0 - SCROLLBAR_THICKNESS, 0.0, vp.size.0, region_h);
        let y_max = scroll_y_max(content_h, region_h);
        let fraction = if y_max > 0.0 {
            (vp.scroll.1 / y_max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.height() > 0.0 {
            (region_h / content_h).clamp(THUMB_MIN_LEN / track.height(), 1.0)
        } else {
            1.0
        };
        bar(Axis::Vertical, track, fraction, len_fraction)
    });
    let horizontal = need_h.then(|| {
        // Horizontal track: full content-region width against the bottom edge.
        let track = Rect::new(0.0, vp.size.1 - SCROLLBAR_THICKNESS, region_w, vp.size.1);
        let x_margin = scroll_x_margin(content_w, region_w);
        let fraction = if x_margin > 0.0 {
            ((vp.scroll.0 + x_margin) / (2.0 * x_margin)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.width() > 0.0 {
            (region_w / content_w).clamp(THUMB_MIN_LEN / track.width(), 1.0)
        } else {
            1.0
        };
        bar(Axis::Horizontal, track, fraction, len_fraction)
    });
    // Suppress the corner for non-positive viewport dims: the raw rect would
    // sit at negative coordinates with POSITIVE area (e.g. (-12,-12,0,0)) and
    // wrongly absorb hit-tests and paints.
    let corner = (need_v && need_h && vp.size.0 > 0.0 && vp.size.1 > 0.0).then(|| {
        Rect::new(
            vp.size.0 - SCROLLBAR_THICKNESS,
            vp.size.1 - SCROLLBAR_THICKNESS,
            vp.size.0,
            vp.size.1,
        )
    });

    ScrollbarLayout {
        content_size: (region_w, region_h),
        vertical,
        horizontal,
        corner,
    }
}

/// Scrollbar chrome under a viewport-space point. Thumbs are tested before
/// their tracks; the corner wins over both tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarHit {
    VerticalThumb,
    HorizontalThumb,
    VerticalTrack { page_up: bool },
    HorizontalTrack { page_left: bool },
    Corner,
}

/// Rect containment that rejects zero/negative-area rects (degenerate
/// viewport sizes produce such tracks, and they must never absorb hits).
fn contains_with_area(r: Rect, x: f64, y: f64) -> bool {
    r.width() > 0.0 && r.height() > 0.0 && x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1
}

pub fn hit_scrollbar(layout: &ScrollbarLayout, point: (f64, f64)) -> Option<ScrollbarHit> {
    let (x, y) = point;
    // Corner is checked first (spec §3.5 ordering): its square abuts the end
    // regions of both tracks (the tracks end exactly at its edges, no
    // overlap), so a point inside it must not page either bar.
    if let Some(c) = layout.corner {
        if contains_with_area(c, x, y) {
            return Some(ScrollbarHit::Corner);
        }
    }
    if let Some(bar) = layout.vertical {
        if contains_with_area(bar.thumb, x, y) {
            return Some(ScrollbarHit::VerticalThumb);
        }
        if contains_with_area(bar.track, x, y) {
            return Some(ScrollbarHit::VerticalTrack {
                page_up: y < bar.thumb.y0,
            });
        }
    }
    if let Some(bar) = layout.horizontal {
        if contains_with_area(bar.thumb, x, y) {
            return Some(ScrollbarHit::HorizontalThumb);
        }
        if contains_with_area(bar.track, x, y) {
            return Some(ScrollbarHit::HorizontalTrack {
                page_left: x < bar.thumb.x0,
            });
        }
    }
    None
}

/// Build one bar along `track` with the thumb placed at `fraction` (0..=1).
/// `len_fraction` is the visible-length ratio (region/content, min-clamped).
fn bar(axis: Axis, track: Rect, fraction: f64, len_fraction: f64) -> BarGeom {
    let (track_len, cross0, cross1) = match axis {
        Axis::Vertical => (track.height(), track.x0, track.x1),
        Axis::Horizontal => (track.width(), track.y0, track.y1),
    };
    let thumb_len = if track_len > 0.0 {
        (len_fraction * track_len).max(THUMB_MIN_LEN).min(track_len)
    } else {
        0.0
    };
    // Thumb thickness leaves THUMB_INSET on both cross-axis sides.
    let thumb_cross0 = cross0 + THUMB_INSET;
    let thumb_cross1 = cross1 - THUMB_INSET;
    let travel = (track_len - thumb_len - 2.0 * THUMB_INSET).max(0.0);
    let start = THUMB_INSET + fraction * travel;
    let thumb = match axis {
        Axis::Vertical => Rect::new(thumb_cross0, start, thumb_cross1, start + thumb_len),
        Axis::Horizontal => Rect::new(start, thumb_cross0, start + thumb_len, thumb_cross1),
    };
    BarGeom { axis, track, thumb }
}

/// Per-axis visual state driving the thumb color. `active` (drag in progress)
/// wins over `hover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollbarVisual {
    pub hover: Option<Axis>,
    pub active: Option<Axis>,
}

const TRACK_COLOR: Color = Color::from_rgba8(0xF1, 0xF1, 0xF1, 0xFF);
const TRACK_BORDER_COLOR: Color = Color::from_rgba8(0xD9, 0xD9, 0xD9, 0xFF);
const THUMB_COLOR: Color = Color::from_rgba8(0xC1, 0xC1, 0xC1, 0xFF);
const THUMB_HOVER_COLOR: Color = Color::from_rgba8(0xA8, 0xA8, 0xA8, 0xFF);
const THUMB_ACTIVE_COLOR: Color = Color::from_rgba8(0x8C, 0x8C, 0x8C, 0xFF);

/// Paint the scrollbar chrome on top of an already-composited scene. Tracks,
/// thumbs and the corner are drawn last so they always cover page content.
/// Zero-area tracks (degenerate viewport sizes) are skipped.
pub fn paint_scrollbars(scene: &mut Scene, layout: &ScrollbarLayout, visual: ScrollbarVisual) {
    let mut painter = Painter::new(scene);

    if let Some(bar) = layout.vertical {
        // Nested guard (not an early return): a degenerate v-track must not
        // skip the horizontal bar below it.
        if bar.track.height() > 0.0 {
            painter.fill_rect(bar.track, TRACK_COLOR);
            // 1px separator on the content-facing (left) edge.
            painter
                .stroke(
                    Line::new((bar.track.x0, bar.track.y0), (bar.track.x0, bar.track.y1)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            painter.fill_rect(bar.thumb, thumb_color(Axis::Vertical, visual));
        }
    }
    if let Some(bar) = layout.horizontal {
        if bar.track.width() > 0.0 {
            painter.fill_rect(bar.track, TRACK_COLOR);
            // 1px separator on the content-facing (top) edge.
            painter
                .stroke(
                    Line::new((bar.track.x0, bar.track.y0), (bar.track.x1, bar.track.y0)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            painter.fill_rect(bar.thumb, thumb_color(Axis::Horizontal, visual));
        }
    }
    // `corner` is already None for non-positive viewport dims (Task 1), so a
    // positive-area check here is belt-and-braces.
    if let Some(corner) = layout.corner {
        if corner.width() > 0.0 && corner.height() > 0.0 {
            painter.fill_rect(corner, TRACK_COLOR);
        }
    }
}

fn thumb_color(axis: Axis, visual: ScrollbarVisual) -> Color {
    if visual.active == Some(axis) {
        THUMB_ACTIVE_COLOR
    } else if visual.hover == Some(axis) {
        THUMB_HOVER_COLOR
    } else {
        THUMB_COLOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn doc_of(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                layers: vec![],
                template: None,
            });
        }
        doc
    }

    fn vp(size: (f64, f64), zoom: f64, gap: f64) -> Viewport {
        Viewport {
            scroll: (0.0, 0.0),
            zoom,
            size,
            page_gap: gap,
        }
    }

    #[test]
    fn no_bars_when_content_fits() {
        // Single 200x300 page in 500x700 viewport: no overflow on either axis.
        let l = scrollbar_layout(&doc_of(&[(200.0, 300.0)]), &vp((500.0, 700.0), 1.0, 20.0));
        assert!(l.vertical.is_none());
        assert!(l.horizontal.is_none());
        assert!(l.corner.is_none());
        assert_eq!(l.content_size, (500.0, 700.0));
    }

    #[test]
    fn vertical_bar_only_geometry() {
        // 180x400 page, 200x200 viewport, gap 0: vertical overflow only.
        // 180 < 200-12 = 188, so the vertical bar's 12px strip does NOT push
        // the horizontal axis into overflow (the two-pass boundary case).
        let l = scrollbar_layout(&doc_of(&[(180.0, 400.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        assert!(l.vertical.is_some());
        assert!(l.horizontal.is_none());
        // Vertical bar consumes 12px of width; no horizontal bar, full height.
        assert_eq!(l.content_size, (188.0, 200.0));
        let v = l.vertical.unwrap();
        assert_eq!(v.track, Rect::new(188.0, 0.0, 200.0, 200.0));
        // thumb_len = 200/400 * 200 = 100; travel = 200 - 100 - 4 = 96;
        // scroll fraction 0 -> thumb y in [2, 102], x insets [190, 198].
        assert_eq!(v.thumb, Rect::new(190.0, 2.0, 198.0, 102.0));
    }

    #[test]
    fn two_pass_recheck_horizontal_bar_after_vertical_appears() {
        // Content 195 wide does NOT overflow a 200 wide viewport initially, but
        // once the 12px vertical bar is deducted (region w = 188) the horizontal
        // bar must appear too; region h then shrinks to 188 as well.
        let l = scrollbar_layout(&doc_of(&[(195.0, 400.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        assert!(l.vertical.is_some());
        assert!(l.horizontal.is_some());
        assert_eq!(l.content_size, (188.0, 188.0));
        // Corner square at the bottom right.
        assert_eq!(l.corner, Some(Rect::new(188.0, 188.0, 200.0, 200.0)));
        // Tracks end where the corner begins.
        assert_eq!(
            l.vertical.unwrap().track,
            Rect::new(188.0, 0.0, 200.0, 188.0)
        );
        assert_eq!(
            l.horizontal.unwrap().track,
            Rect::new(0.0, 188.0, 188.0, 200.0)
        );
    }

    #[test]
    fn thumb_position_tracks_scroll_fraction() {
        // 180x400 page: y_max = 400-200 = 200. Halfway scroll -> halfway thumb.
        let mut v = vp((200.0, 200.0), 1.0, 0.0);
        v.scroll.1 = 100.0;
        let l = scrollbar_layout(&doc_of(&[(180.0, 400.0)]), &v);
        let bar = l.vertical.unwrap();
        // fraction .5 -> thumb y in [2 + 48, 102 + 48] = [50, 150].
        assert_eq!(bar.thumb, Rect::new(190.0, 50.0, 198.0, 150.0));
    }

    #[test]
    fn thumb_never_shorter_than_min() {
        // 200x2000 page in 200x200: two-pass gives region_h 188. The raw ratio
        // 188/2000 would yield ~17.7px; clamped to THUMB_MIN_LEN (24).
        let l = scrollbar_layout(&doc_of(&[(200.0, 2000.0)]), &vp((200.0, 200.0), 1.0, 0.0));
        let bar = l.vertical.unwrap();
        assert!((bar.thumb.height() - THUMB_MIN_LEN).abs() < 1e-9);
    }

    #[test]
    fn horizontal_thumb_fraction_maps_centered_scroll() {
        // 400x100 page, 200x200 viewport: hbar only (100 < 200-12=188).
        // x_margin = 100; scroll.0 = 100 -> fraction 1 (thumb right).
        let mut v = vp((200.0, 200.0), 1.0, 0.0);
        v.scroll.0 = 100.0;
        let l = scrollbar_layout(&doc_of(&[(400.0, 100.0)]), &v);
        let bar = l.horizontal.unwrap();
        // region 200x188; track y [188,200]; thumb y insets [190,198];
        // thumb_len 100, travel 96 -> fraction 1 -> x [98, 198].
        assert_eq!(bar.thumb, Rect::new(98.0, 190.0, 198.0, 198.0));
    }

    #[test]
    fn empty_doc_has_no_bars() {
        let l = scrollbar_layout(&OfdDocument::default(), &vp((500.0, 700.0), 1.0, 20.0));
        assert_eq!(l, ScrollbarLayout::default());
    }

    #[test]
    fn zero_sized_viewport_pins_region_to_zero() {
        // Degenerate host size (component unit tests use size=(0,0)): region
        // dims pin to 0 instead of going negative; bars are still reported so
        // clamp_scroll gets zero-sized region semantics.
        let l = scrollbar_layout(&doc_of(&[(200.0, 200.0)]), &vp((0.0, 0.0), 1.0, 0.0));
        assert_eq!(l.content_size, (0.0, 0.0));
        assert!(l.vertical.is_some() && l.horizontal.is_some());
        // Zero-area tracks rather than negative rectangles.
        assert_eq!(l.vertical.unwrap().track, Rect::new(-12.0, 0.0, 0.0, 0.0));
        // The raw corner formula would yield (-12,-12,0,0): a positive-area
        // 12x12 square that wrongly absorbs hits/paints. It must be suppressed.
        assert!(l.corner.is_none());
    }

    #[test]
    fn bounds_helpers() {
        assert_eq!(scroll_y_max(400.0, 188.0), 212.0);
        assert_eq!(scroll_y_max(100.0, 188.0), 0.0);
        assert_eq!(scroll_x_margin(400.0, 188.0), 106.0);
        assert_eq!(scroll_x_margin(100.0, 188.0), 0.0);
    }
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn layout_for(size: (f64, f64), pages: &[(f64, f64)], scroll: (f64, f64)) -> ScrollbarLayout {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                layers: vec![],
                template: None,
            });
        }
        let vp = Viewport {
            scroll,
            zoom: 1.0,
            size,
            page_gap: 0.0,
        };
        scrollbar_layout(&doc, &vp)
    }

    #[test]
    fn hits_vertical_thumb_before_track() {
        // 180x400 page in 200x200: vbar only; thumb y [2,102], x [190,198].
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)], (0.0, 0.0));
        assert_eq!(
            hit_scrollbar(&l, (194.0, 50.0)),
            Some(ScrollbarHit::VerticalThumb)
        );
        // Below the thumb but inside the track -> page-down zone.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 150.0)),
            Some(ScrollbarHit::VerticalTrack { page_up: false })
        );
        // Above the thumb -> page-up zone.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 1.0)),
            Some(ScrollbarHit::VerticalTrack { page_up: true })
        );
    }

    #[test]
    fn hits_horizontal_thumb_and_corner() {
        // Two-pass case: 195x400 in 200x200 -> both bars + corner [188,200]^2.
        let l = layout_for((200.0, 200.0), &[(195.0, 400.0)], (0.0, 0.0));
        // hbar thumb: track y [188,200], thumb y [190,198]. At scroll 0 the
        // centered x-margin gives fraction 0.5 (x_margin = (195-188)/2 = 3.5),
        // so the thumb starts at 2 + 0.5*travel ~= 3.37; point x=50 is well
        // inside it.
        assert_eq!(
            hit_scrollbar(&l, (50.0, 194.0)),
            Some(ScrollbarHit::HorizontalThumb)
        );
        assert_eq!(
            hit_scrollbar(&l, (194.0, 194.0)),
            Some(ScrollbarHit::Corner)
        );
    }

    #[test]
    fn no_hit_without_bars_or_outside() {
        let l = layout_for((500.0, 700.0), &[(200.0, 300.0)], (0.0, 0.0));
        assert_eq!(hit_scrollbar(&l, (499.0, 699.0)), None);
        // Degenerate zero-size tracks never hit.
        let l0 = layout_for((0.0, 0.0), &[(200.0, 200.0)], (0.0, 0.0));
        assert_eq!(hit_scrollbar(&l0, (0.0, 0.0)), None);
    }
}

#[cfg(test)]
mod paint_tests {
    use super::*;
    use imaging::record::{Command, Draw, Scene};
    use rofd_dom::{OfdDocument, Page, PageId, Rect as RofdRect};

    fn count_fills(scene: &Scene) -> usize {
        scene
            .commands()
            .iter()
            .filter(|cmd| {
                matches!(cmd, Command::Draw(id) if matches!(scene.draw_op(*id), Draw::Fill { .. }))
            })
            .count()
    }

    fn layout_for(size: (f64, f64), pages: &[(f64, f64)]) -> ScrollbarLayout {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: RofdRect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                layers: vec![],
                template: None,
            });
        }
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size,
            page_gap: 0.0,
        };
        scrollbar_layout(&doc, &vp)
    }

    #[test]
    fn paints_nothing_without_bars() {
        let mut scene = Scene::new();
        let l = layout_for((500.0, 700.0), &[(200.0, 300.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 0);
    }

    #[test]
    fn paints_both_bars_and_corner() {
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(195.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        // 2 tracks + 2 thumbs + 1 corner = 5 fills.
        assert_eq!(count_fills(&scene), 5);
    }

    #[test]
    fn paints_single_bar_without_corner() {
        // 180-wide page: vertical bar only (no corner, no horizontal track).
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 2);
    }

    #[test]
    fn skips_degenerate_zero_area_tracks() {
        let mut scene = Scene::new();
        let l = layout_for((0.0, 0.0), &[(200.0, 200.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 0, "zero-area chrome is never painted");
    }
}
