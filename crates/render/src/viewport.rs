//! Viewport: scroll/zoom/size/page-gap describing the desk surface a
//! [`RenderEngine`](crate::RenderEngine) composites into.
//!
//! The viewport is the "desk" the paper pages sit on. `scroll` is the desk
//! offset (in device pixels), `zoom` scales page-local coordinates, `size` is
//! the viewport rectangle in device pixels, and `page_gap` is the vertical
//! spacing between stacked pages.

use rofd_dom::OfdDocument;

/// Device pixels per OFD millimetre at 96 DPI.
///
/// OFD coordinates (page boxes, object boundaries, text origins, path data)
/// are in millimetres. The renderer maps mm -> device pixels via this factor,
/// applied as the default viewport `zoom` so that at 100% a page renders at its
/// physical size (an A4 page ~794×1123 px). User zoom multiplies on top.
pub const PX_PER_MM: f64 = 96.0 / 25.4;

/// Paper-on-desk viewport state. All fields are in device pixels.
///
/// - `scroll`: (x, y) desk offset to apply (added to page positions; positive y
///   scrolls pages downward).
/// - `zoom`: uniform scale applied to every page's physical box. Defaults to
///   [`PX_PER_MM`] so 100% == 96 DPI.
/// - `size`: (width, height) of the viewport in device pixels - the gray desk
///   background fills this rectangle.
/// - `page_gap`: vertical gap between consecutive pages, in device pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct Viewport {
    pub scroll: (f64, f64),
    pub zoom: f64,
    pub size: (f64, f64),
    pub page_gap: f64,
}

/// Clamp a viewport scroll offset so the page stack covers the viewport as
/// far as possible (paper-on-desk: the paper can never be scrolled entirely
/// off the desk).
///
/// X uses the widest page: when every page is narrower than the viewport the
/// stack stays centered and `scroll.0` pins to 0. Y allows scrolling from the
/// initial top position (`scroll.1 == 0`) down to the last page's bottom
/// edge reaching the viewport bottom; when the whole stack is shorter than
/// the viewport, `scroll.1` pins to 0.
///
/// Geometry mirrors [`crate::composite::page_origin`]: `page_x = max(0,
/// (size.0 - page_w) / 2) + scroll.0`, `page_y = page_gap - scroll.1 + ...`.
/// Shared single implementation (spec §3.1) - the hand tool's pan drag calls
/// this; wheel `Scroll` may adopt it later.
pub fn clamp_scroll(doc: &OfdDocument, vp: &Viewport) -> (f64, f64) {
    if doc.pages.is_empty() {
        return (0.0, 0.0);
    }
    let widest = doc
        .pages
        .iter()
        .map(|p| p.physical_box.w * vp.zoom)
        .fold(f64::MIN, f64::max);
    let x_margin = (widest - vp.size.0) / 2.0;
    let x = if x_margin <= 0.0 {
        0.0
    } else {
        vp.scroll.0.clamp(-x_margin, x_margin)
    };
    let inner_h: f64 = doc
        .pages
        .iter()
        .map(|p| p.physical_box.h * vp.zoom)
        .sum::<f64>()
        + vp.page_gap * doc.pages.len().saturating_sub(1) as f64;
    let y_max = (vp.page_gap + inner_h - vp.size.1).max(0.0);
    let y = vp.scroll.1.clamp(0.0, y_max);
    (x, y)
}

#[cfg(test)]
mod clamp_tests {
    use super::*;
    use rofd_dom::{OfdDocument, Page, PageId, Rect};

    fn doc_of(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: Rect {
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

    fn vp(size: (f64, f64), zoom: f64, scroll: (f64, f64)) -> Viewport {
        Viewport {
            scroll,
            zoom,
            size,
            page_gap: 20.0,
        }
    }

    #[test]
    fn clamps_both_axes_when_content_exceeds_viewport() {
        // 两页 400x300mm，zoom=2 -> 每页 800x600px，视口 500x700，gap=20。
        // X: 最宽页 800 > 500 -> 余量 (800-500)/2 = 150 -> x ∈ [-150, 150]。
        // Y: 内容高 = 600*2 + 20 = 1220；y_max = gap + 1220 - 700 = 540 -> y ∈ [0, 540]。
        let doc = doc_of(&[(400.0, 300.0), (400.0, 300.0)]);
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 2.0, (500.0, 1000.0))),
            (150.0, 540.0)
        );
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 2.0, (-500.0, -5.0))),
            (-150.0, 0.0)
        );
    }

    #[test]
    fn within_bounds_scroll_unchanged() {
        let doc = doc_of(&[(400.0, 300.0), (400.0, 300.0)]);
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 2.0, (50.0, 200.0))),
            (50.0, 200.0)
        );
    }

    #[test]
    fn pins_to_center_and_top_when_content_fits() {
        // 单页 200x300，zoom=1，视口 500x700：页比视口窄 -> x 钉 0（居中）；
        // 内容高 300 + 20 < 700 -> y 钉 0（顶部对齐）。
        let doc = doc_of(&[(200.0, 300.0)]);
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 1.0, (300.0, 500.0))),
            (0.0, 0.0)
        );
    }

    #[test]
    fn empty_doc_pins_to_zero() {
        let doc = OfdDocument::default();
        assert_eq!(
            clamp_scroll(&doc, &vp((500.0, 700.0), 1.0, (100.0, 100.0))),
            (0.0, 0.0)
        );
    }
}
