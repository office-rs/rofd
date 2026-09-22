//! Persistent scrollbar geometry, hit-testing and painting.
//!
//! Pure functions over [`OfdDocument`] + [`Viewport`]: which axes overflow,
//! track/thumb rectangles in viewport space, chrome hit-testing, and the
//! top-of-scene paint pass. The component owns the drag state machine; this
//! module never reads a clock or touches platform types (AGENTS §4.4/§4.9).

use imaging::kurbo::{BezPath, Line, Rect, Stroke};
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
/// Length of the square arrow button at each end of a bar strip, in device
/// pixels.
pub const ARROW_LEN: f64 = 12.0;
/// Clicking an arrow button scrolls by this fraction of the visible content
/// region (spec §3.5: one click = one step; press-and-hold repeat is a
/// documented non-goal).
pub const ARROW_STEP_RATIO: f64 = 0.1;

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
    /// Paging trough BETWEEN the two arrow buttons; the thumb also travels
    /// inside it.
    pub track: Rect,
    pub thumb: Rect,
    /// Square step-scroll buttons at the strip ends. `arrow_start` points
    /// toward the scroll origin (top for vertical, left for horizontal).
    pub arrow_start: Rect,
    pub arrow_end: Rect,
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

/// Total content extent in device pixels:
/// `(widest_page, page_gap + inner_h + page_gap)`.
/// Mirrors the stacking math in `composite::page_origin` / `clamp_scroll`.
pub fn content_metrics(doc: &OfdDocument, vp: &Viewport) -> (f64, f64) {
    let content_w = doc
        .pages
        .iter()
        .map(|p| p.physical_box.w * vp.zoom)
        .fold(0.0_f64, f64::max);
    let pages_h: f64 = doc.pages.iter().map(|p| p.physical_box.h * vp.zoom).sum();
    let inner_h = pages_h + vp.page_gap * doc.pages.len().saturating_sub(1) as f64;
    // One gap above the first page, one between consecutive pages AND one
    // below the last page (top/bottom symmetric, spec §3.1): fully scrolled
    // down, the last page's bottom edge keeps a `page_gap` margin visible.
    (content_w, vp.page_gap + inner_h + vp.page_gap)
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

/// Split a full-length bar strip into the paging track (between the arrows)
/// and the two square [`ARROW_LEN`] arrow buttons at its ends. `arrow_start`
/// points toward the scroll origin (up / left). Short strips collapse the
/// arrows instead of overlapping them; the track may end up zero-area.
fn split_strip(axis: Axis, strip: Rect) -> (Rect, Rect, Rect) {
    let (a0, a1) = match axis {
        Axis::Vertical => (strip.y0, strip.y1),
        Axis::Horizontal => (strip.x0, strip.x1),
    };
    let start_btn_end = (a0 + ARROW_LEN).min(a1);
    let end_btn_start = (a1 - ARROW_LEN).max(start_btn_end);
    match axis {
        Axis::Vertical => (
            Rect::new(strip.x0, start_btn_end, strip.x1, end_btn_start),
            Rect::new(strip.x0, a0, strip.x1, start_btn_end),
            Rect::new(strip.x0, end_btn_start, strip.x1, a1),
        ),
        Axis::Horizontal => (
            Rect::new(start_btn_end, strip.y0, end_btn_start, strip.y1),
            Rect::new(a0, strip.y0, start_btn_end, strip.y1),
            Rect::new(end_btn_start, strip.y0, a1, strip.y1),
        ),
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
        // Vertical strip: full content-region height against the right edge,
        // split into the two arrow buttons and the paging track between them.
        let strip = Rect::new(vp.size.0 - SCROLLBAR_THICKNESS, 0.0, vp.size.0, region_h);
        let (track, arrow_start, arrow_end) = split_strip(Axis::Vertical, strip);
        let y_max = scroll_y_max(content_h, region_h);
        let fraction = if y_max > 0.0 {
            (vp.scroll.1 / y_max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.height() > 0.0 {
            (region_h / content_h).clamp((THUMB_MIN_LEN / track.height()).min(1.0), 1.0)
        } else {
            1.0
        };
        bar(
            Axis::Vertical,
            track,
            arrow_start,
            arrow_end,
            fraction,
            len_fraction,
        )
    });
    let horizontal = need_h.then(|| {
        // Horizontal strip: full content-region width against the bottom
        // edge, split into the two arrow buttons and the paging track.
        let strip = Rect::new(0.0, vp.size.1 - SCROLLBAR_THICKNESS, region_w, vp.size.1);
        let (track, arrow_start, arrow_end) = split_strip(Axis::Horizontal, strip);
        let x_margin = scroll_x_margin(content_w, region_w);
        let fraction = if x_margin > 0.0 {
            ((vp.scroll.0 + x_margin) / (2.0 * x_margin)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let len_fraction = if track.width() > 0.0 {
            (region_w / content_w).clamp((THUMB_MIN_LEN / track.width()).min(1.0), 1.0)
        } else {
            1.0
        };
        bar(
            Axis::Horizontal,
            track,
            arrow_start,
            arrow_end,
            fraction,
            len_fraction,
        )
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
    /// Square step button at a strip end. `negative` points toward the scroll
    /// origin (up for vertical, left for horizontal); one click = one
    /// [`ARROW_STEP_RATIO`] step of the content region.
    Arrow {
        axis: Axis,
        negative: bool,
    },
    VerticalTrack {
        page_up: bool,
    },
    HorizontalTrack {
        page_left: bool,
    },
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
        // Arrows flank the track inside the same strip, so they must be
        // tested before the track (a track hit would page instead of step).
        if contains_with_area(bar.arrow_start, x, y) {
            return Some(ScrollbarHit::Arrow {
                axis: Axis::Vertical,
                negative: true,
            });
        }
        if contains_with_area(bar.arrow_end, x, y) {
            return Some(ScrollbarHit::Arrow {
                axis: Axis::Vertical,
                negative: false,
            });
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
        if contains_with_area(bar.arrow_start, x, y) {
            return Some(ScrollbarHit::Arrow {
                axis: Axis::Horizontal,
                negative: true,
            });
        }
        if contains_with_area(bar.arrow_end, x, y) {
            return Some(ScrollbarHit::Arrow {
                axis: Axis::Horizontal,
                negative: false,
            });
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
fn bar(
    axis: Axis,
    track: Rect,
    arrow_start: Rect,
    arrow_end: Rect,
    fraction: f64,
    len_fraction: f64,
) -> BarGeom {
    let (track_len, cross0, cross1, origin) = match axis {
        Axis::Vertical => (track.height(), track.x0, track.x1, track.y0),
        Axis::Horizontal => (track.width(), track.y0, track.y1, track.x0),
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
    // `start` is absolute in viewport space: the track no longer begins at 0
    // now that the arrow buttons flank it.
    let start = origin + THUMB_INSET + fraction * travel;
    let thumb = match axis {
        Axis::Vertical => Rect::new(thumb_cross0, start, thumb_cross1, start + thumb_len),
        Axis::Horizontal => Rect::new(start, thumb_cross0, start + thumb_len, thumb_cross1),
    };
    BarGeom {
        axis,
        track,
        thumb,
        arrow_start,
        arrow_end,
    }
}

/// Which scrollbar element the pointer hovers, driving hover colors.
/// `Thumb` also requests the resize cursor; `Arrow` keeps the default
/// pointer (spec §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarHover {
    Thumb(Axis),
    Arrow { axis: Axis, negative: bool },
}

/// Per-element visual state driving the thumb/glyph colors. `active` (drag
/// in progress) wins over `hover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollbarVisual {
    pub hover: Option<ScrollbarHover>,
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
        // Full strip = arrow buttons + paging track, filled as one trough.
        let strip = Rect::new(
            bar.track.x0,
            bar.arrow_start.y0,
            bar.track.x1,
            bar.arrow_end.y1,
        );
        // Nested guard (not an early return): a degenerate strip must not
        // skip the horizontal bar below it.
        if strip.height() > 0.0 {
            painter.fill_rect(strip, TRACK_COLOR);
            // 1px separator on the content-facing (left) edge.
            painter
                .stroke(
                    Line::new((strip.x0, strip.y0), (strip.x0, strip.y1)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            // Separators between the arrows and the paging track (a strip too
            // short to hold a track has none).
            if bar.arrow_start.y1 < bar.arrow_end.y0 {
                for y in [bar.arrow_start.y1, bar.arrow_end.y0] {
                    painter
                        .stroke(
                            Line::new((strip.x0, y), (strip.x1, y)),
                            &Stroke::new(1.0_f64),
                            TRACK_BORDER_COLOR,
                        )
                        .draw();
                }
            }
            painter.fill_rect(bar.thumb, thumb_color(Axis::Vertical, visual));
            for (button, negative) in [(bar.arrow_start, true), (bar.arrow_end, false)] {
                if let Some(glyph) = arrow_glyph(Axis::Vertical, button, negative) {
                    painter
                        .fill(&glyph, arrow_color(Axis::Vertical, negative, visual))
                        .draw();
                }
            }
        }
    }
    if let Some(bar) = layout.horizontal {
        // Full strip = arrow buttons + paging track, filled as one trough.
        let strip = Rect::new(
            bar.arrow_start.x0,
            bar.track.y0,
            bar.arrow_end.x1,
            bar.track.y1,
        );
        if strip.width() > 0.0 {
            painter.fill_rect(strip, TRACK_COLOR);
            // 1px separator on the content-facing (top) edge.
            painter
                .stroke(
                    Line::new((strip.x0, strip.y0), (strip.x1, strip.y0)),
                    &Stroke::new(1.0_f64),
                    TRACK_BORDER_COLOR,
                )
                .draw();
            if bar.arrow_start.x1 < bar.arrow_end.x0 {
                for x in [bar.arrow_start.x1, bar.arrow_end.x0] {
                    painter
                        .stroke(
                            Line::new((x, strip.y0), (x, strip.y1)),
                            &Stroke::new(1.0_f64),
                            TRACK_BORDER_COLOR,
                        )
                        .draw();
                }
            }
            painter.fill_rect(bar.thumb, thumb_color(Axis::Horizontal, visual));
            for (button, negative) in [(bar.arrow_start, true), (bar.arrow_end, false)] {
                if let Some(glyph) = arrow_glyph(Axis::Horizontal, button, negative) {
                    painter
                        .fill(&glyph, arrow_color(Axis::Horizontal, negative, visual))
                        .draw();
                }
            }
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

/// Filled triangle glyph for one arrow button, pointing outward (`negative` =
/// up for vertical / left for horizontal). Returns `None` for zero-area
/// buttons (degenerate strips never paint a glyph).
fn arrow_glyph(axis: Axis, button: Rect, negative: bool) -> Option<BezPath> {
    if button.width() <= 0.0 || button.height() <= 0.0 {
        return None;
    }
    // Glyph inset inside the 12x12 button: tip and base corners sit `g` from
    // the edges, keeping the triangle clear of the 1px separators.
    let g = 3.0;
    let (x0, y0, x1, y1) = (button.x0, button.y0, button.x1, button.y1);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut path = BezPath::new();
    match (axis, negative) {
        (Axis::Vertical, true) => {
            path.move_to((cx, y0 + g));
            path.line_to((x1 - g, y1 - g));
            path.line_to((x0 + g, y1 - g));
        }
        (Axis::Vertical, false) => {
            path.move_to((cx, y1 - g));
            path.line_to((x1 - g, y0 + g));
            path.line_to((x0 + g, y0 + g));
        }
        (Axis::Horizontal, true) => {
            path.move_to((x0 + g, cy));
            path.line_to((x1 - g, y1 - g));
            path.line_to((x1 - g, y0 + g));
        }
        (Axis::Horizontal, false) => {
            path.move_to((x1 - g, cy));
            path.line_to((x0 + g, y1 - g));
            path.line_to((x0 + g, y0 + g));
        }
    }
    path.close_path();
    Some(path)
}

fn thumb_color(axis: Axis, visual: ScrollbarVisual) -> Color {
    if visual.active == Some(axis) {
        THUMB_ACTIVE_COLOR
    } else if visual.hover == Some(ScrollbarHover::Thumb(axis)) {
        THUMB_HOVER_COLOR
    } else {
        THUMB_COLOR
    }
}

/// Arrow glyphs darken on hover exactly like the thumb does (spec §3.4).
fn arrow_color(axis: Axis, negative: bool, visual: ScrollbarVisual) -> Color {
    if visual.hover == Some(ScrollbarHover::Arrow { axis, negative }) {
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
    fn tiny_viewport_thumb_clamp_does_not_panic() {
        // 50x50 viewport, 200x200 page: both bars appear and tracks are only
        // 14px long (< THUMB_MIN_LEN), so the old clamp with min > max
        // panicked. The thumb fills the full track length on both axes.
        let l = scrollbar_layout(&doc_of(&[(200.0, 200.0)]), &vp((50.0, 50.0), 1.0, 0.0));
        let v = l.vertical.expect("vertical bar");
        assert!((v.thumb.height() - v.track.height()).abs() < 1e-9);
        let h = l.horizontal.expect("horizontal bar");
        assert!((h.thumb.width() - h.track.width()).abs() < 1e-9);
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
    fn content_metrics_reserve_symmetric_top_and_bottom_gaps() {
        // Two 100x200 pages, zoom 1, gap 20: the content height counts one
        // gap above the first page, one between pages AND one below the last
        // page (top/bottom symmetric): 20 + 200 + 20 + 200 + 20 = 460.
        let (w, h) = content_metrics(
            &doc_of(&[(100.0, 200.0), (100.0, 200.0)]),
            &vp((500.0, 700.0), 1.0, 20.0),
        );
        assert_eq!(w, 100.0);
        assert_eq!(h, 460.0);
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
        // The full-height strip splits into square arrow buttons at both ends
        // and a paging track between them.
        assert_eq!(v.arrow_start, Rect::new(188.0, 0.0, 200.0, 12.0));
        assert_eq!(v.arrow_end, Rect::new(188.0, 188.0, 200.0, 200.0));
        assert_eq!(v.track, Rect::new(188.0, 12.0, 200.0, 188.0));
        // thumb_len = 176 * (200/400) = 88; travel = 176 - 88 - 4 = 84;
        // scroll fraction 0 -> thumb y [14, 102], x insets [190, 198].
        assert_eq!(v.thumb, Rect::new(190.0, 14.0, 198.0, 102.0));
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
        // Tracks end where the corner begins; the arrows flank each track.
        assert_eq!(
            l.vertical.unwrap().track,
            Rect::new(188.0, 12.0, 200.0, 176.0)
        );
        assert_eq!(
            l.horizontal.unwrap().track,
            Rect::new(12.0, 188.0, 176.0, 200.0)
        );
    }

    #[test]
    fn thumb_position_tracks_scroll_fraction() {
        // 180x400 page: y_max = 400-200 = 200. Halfway scroll -> halfway thumb.
        let mut v = vp((200.0, 200.0), 1.0, 0.0);
        v.scroll.1 = 100.0;
        let l = scrollbar_layout(&doc_of(&[(180.0, 400.0)]), &v);
        let bar = l.vertical.unwrap();
        // Track starts at y=12 (below the top arrow); fraction .5 ->
        // thumb y = 12 + 2 + .5*84 = 56 -> [56, 144].
        assert_eq!(bar.thumb, Rect::new(190.0, 56.0, 198.0, 144.0));
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
        // region 200x188; h-strip arrows x [0,12] & [176,188], track x [12,188]
        // (len 176); thumb y insets [190,198]; thumb_len = 176 * (200/400) = 88,
        // travel 84 -> fraction 1 -> x [98, 186].
        assert_eq!(bar.thumb, Rect::new(98.0, 190.0, 186.0, 198.0));
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
        // 180x400 page in 200x200: vbar only; thumb y [14,102], x [190,198].
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
        // Inside the TOP ARROW button (strip end, above the track): an arrow
        // step, not a track page.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 1.0)),
            Some(ScrollbarHit::Arrow {
                axis: Axis::Vertical,
                negative: true
            })
        );
        // Just below the top arrow, above the thumb -> page-up zone.
        assert_eq!(
            hit_scrollbar(&l, (194.0, 13.0)),
            Some(ScrollbarHit::VerticalTrack { page_up: true })
        );
    }

    #[test]
    fn hits_arrow_buttons_in_strip_ends() {
        // Wide page 400x100 in 200x200: hbar only; arrows flank the track.
        let l = layout_for((200.0, 200.0), &[(400.0, 100.0)], (0.0, 0.0));
        assert_eq!(
            hit_scrollbar(&l, (6.0, 194.0)),
            Some(ScrollbarHit::Arrow {
                axis: Axis::Horizontal,
                negative: true
            })
        );
        assert_eq!(
            hit_scrollbar(&l, (194.0, 194.0)),
            Some(ScrollbarHit::Arrow {
                axis: Axis::Horizontal,
                negative: false
            })
        );
        // Vertical bottom arrow on the tall fixture (above the corner-free
        // single-bar layout, the strip ends at y=200).
        let v = layout_for((200.0, 200.0), &[(180.0, 400.0)], (0.0, 0.0));
        assert_eq!(
            hit_scrollbar(&v, (194.0, 190.0)),
            Some(ScrollbarHit::Arrow {
                axis: Axis::Vertical,
                negative: false
            })
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

    /// Solid brush colors of the Fill draws, in paint order.
    fn fill_colors(scene: &Scene) -> Vec<Color> {
        use imaging::peniko::Brush;
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => Some(scene.draw_op(*id)),
                _ => None,
            })
            .filter_map(|draw| match draw {
                Draw::Fill {
                    brush: Brush::Solid(c),
                    ..
                } => Some(*c),
                _ => None,
            })
            .collect()
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
        // Per bar: strip fill + thumb fill + 2 arrow glyphs = 4; both bars
        // plus the corner = 4 + 4 + 1 = 9 fills.
        assert_eq!(count_fills(&scene), 9);
    }

    #[test]
    fn paints_single_bar_without_corner() {
        // 180-wide page: vertical bar only (no corner, no horizontal strip).
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 4);
    }

    #[test]
    fn skips_degenerate_zero_area_tracks() {
        let mut scene = Scene::new();
        let l = layout_for((0.0, 0.0), &[(200.0, 200.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(count_fills(&scene), 0, "zero-area chrome is never painted");
    }

    #[test]
    fn paint_appends_chrome_without_clearing_scene() {
        // Append-not-clear contract relied on by the component's scene
        // composition: the composited page scene is passed in already
        // populated, and the chrome must be appended AFTER it with the
        // thumb last, never replacing it.
        let mut scene = Scene::new();
        // Pre-existing content authored with the same Painter call shape the
        // track fill uses.
        Painter::new(&mut scene).fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), TRACK_COLOR);
        assert_eq!(count_fills(&scene), 1);
        // Vertical-bar-only fixture: strip fill + outer separator stroke + 2
        // arrow-separator strokes + thumb fill + 2 arrow glyph fills.
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(&mut scene, &l, ScrollbarVisual::default());
        assert_eq!(
            count_fills(&scene),
            5,
            "chrome adds four fills, clears none"
        );
        // Full draw order: the pre-existing fill stays first; the chrome block
        // is appended after it (strip fill, three strokes, thumb fill, then
        // the two arrow glyph fills) so the chrome paints on top of the page.
        let draws: Vec<&Draw> = scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => Some(scene.draw_op(*id)),
                _ => None,
            })
            .collect();
        assert_eq!(
            draws.len(),
            8,
            "pre fill + strip + 3 strokes + thumb + 2 glyphs"
        );
        assert!(matches!(draws[0], Draw::Fill { .. }), "content kept first");
        assert!(matches!(draws[1], Draw::Fill { .. }), "strip appended");
        assert!(matches!(draws[2], Draw::Stroke { .. }), "outer edge stroke");
        assert!(matches!(draws[3], Draw::Stroke { .. }), "arrow separator");
        assert!(matches!(draws[4], Draw::Stroke { .. }), "arrow separator");
        assert!(
            matches!(draws[5], Draw::Fill { .. }),
            "thumb on top of track"
        );
        assert!(matches!(draws[6], Draw::Fill { .. }), "arrow glyph");
        assert!(matches!(draws[7], Draw::Fill { .. }), "arrow glyph");
    }

    #[test]
    fn hovered_arrow_darkens_only_its_glyph() {
        // Tall fixture paint order: strip, thumb, up glyph, down glyph.
        // Hovering the up arrow recolors only that glyph.
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(
            &mut scene,
            &l,
            ScrollbarVisual {
                hover: Some(ScrollbarHover::Arrow {
                    axis: Axis::Vertical,
                    negative: true,
                }),
                active: None,
            },
        );
        let colors = fill_colors(&scene);
        assert_eq!(colors.len(), 4);
        assert_eq!(colors[0], TRACK_COLOR, "strip fill");
        assert_eq!(colors[1], THUMB_COLOR, "thumb stays unhovered");
        assert_eq!(colors[2], THUMB_HOVER_COLOR, "hovered up glyph darkens");
        assert_eq!(colors[3], THUMB_COLOR, "other glyph unchanged");
    }

    #[test]
    fn hovered_thumb_still_darkens_via_hover_enum() {
        // The thumb hover color keyed on the new ScrollbarHover::Thumb variant
        // (and an arrow hover on the same bar must NOT darken the thumb).
        let mut scene = Scene::new();
        let l = layout_for((200.0, 200.0), &[(180.0, 400.0)]);
        paint_scrollbars(
            &mut scene,
            &l,
            ScrollbarVisual {
                hover: Some(ScrollbarHover::Thumb(Axis::Vertical)),
                active: None,
            },
        );
        let colors = fill_colors(&scene);
        assert_eq!(colors[1], THUMB_HOVER_COLOR, "hovered thumb darkens");
        assert_eq!(colors[2], THUMB_COLOR, "glyphs stay unhovered");
    }
}
