//! Fit-zoom computation (适应宽度/高度/页面): the pure geometry that maps a
//! reference page + viewport to the zoom that fits it.
//!
//! Mirrors [`crate::scrollbar::scrollbar_layout`]'s two-pass overflow
//! analysis: the available content region shrinks by visible scrollbar
//! strips, and strip visibility itself depends on the content size at the
//! candidate zoom — a joint fixed point solved by iteration with the SAME
//! predicates, so the bars predicted here agree with the ones
//! `scrollbar_layout` renders after the zoom is applied (spec
//! 2026-10-10-fit-zoom-modes §3.1).

use rofd_dom::{OfdDocument, Rect};

use crate::scrollbar::region_dim;
use crate::viewport::{Viewport, FIT_MIN_ZOOM, MAX_ZOOM};

/// Which axis(es) the fit zoom must satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitTarget {
    /// The reference page's width fills the content region.
    Width,
    /// The reference page's height fills the content region.
    Height,
    /// The whole reference page is visible: the smaller of Width/Height.
    Page,
}

/// Compute the zoom that fits `page` (mm, the caller-resolved reference
/// page) into the viewport's content region. Returns `None` when not
/// ready: no pages, a degenerate (non-finite / non-positive) page box, or
/// an unlayouted (non-positive / non-finite) viewport.
///
/// `doc` supplies the stack geometry for scrollbar-presence estimation (the
/// widest page and the total stacked height decide which strips appear);
/// only `page` decides the fit zoom itself (current-page semantics, spec
/// D1). The result is clamped to `[FIT_MIN_ZOOM, MAX_ZOOM]`.
pub fn fit_zoom(doc: &OfdDocument, vp: &Viewport, page: &Rect, target: FitTarget) -> Option<f64> {
    if doc.pages.is_empty() {
        return None;
    }
    if !page.w.is_finite() || !page.h.is_finite() || page.w <= 0.0 || page.h <= 0.0 {
        return None;
    }
    if !vp.size.0.is_finite() || !vp.size.1.is_finite() || vp.size.0 <= 0.0 || vp.size.1 <= 0.0 {
        return None;
    }

    // Content extents as functions of zoom (mirrors scrollbar::content_metrics:
    // content_w = widest page × zoom; content_h = gap·(n+1) + Σ(page_h)·zoom —
    // one gap above the first page, one between consecutive pages AND one
    // below the last page).
    let n = doc.pages.len();
    let widest_mm = doc
        .pages
        .iter()
        .map(|p| p.physical_box.w)
        .fold(0.0_f64, f64::max);
    let sum_h_mm: f64 = doc.pages.iter().map(|p| p.physical_box.h).sum();
    let gap = vp.page_gap;
    let content_w = |z: f64| widest_mm * z;
    let content_h = |z: f64| gap * (n + 1) as f64 + sum_h_mm * z;

    // Joint fixed point of (bar presence, candidate zoom), seeded from the
    // current zoom. Predicates are scrollbar_layout's own, so the bars
    // predicted here are the ones rendered after the zoom is applied.
    let mut need_v = content_h(vp.zoom) > vp.size.1;
    let mut need_h = content_w(vp.zoom) > vp.size.0;
    let mut z = vp.zoom;
    for _ in 0..3 {
        let region_w = region_dim(vp.size.0, need_v);
        let region_h = region_dim(vp.size.1, need_h);
        z = match target {
            FitTarget::Width => region_w / page.w,
            FitTarget::Height => region_h / page.h,
            FitTarget::Page => (region_w / page.w).min(region_h / page.h),
        };
        let next_v = content_h(z) > region_dim(vp.size.1, need_h);
        let next_h = content_w(z) > region_dim(vp.size.0, need_v);
        if (next_v, next_h) == (need_v, need_h) {
            break;
        }
        need_v = next_v;
        need_h = next_h;
    }
    Some(z.clamp(FIT_MIN_ZOOM, MAX_ZOOM))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollbar::scrollbar_layout;
    use crate::viewport::{FIT_MIN_ZOOM, MAX_ZOOM, MIN_ZOOM};

    fn doc_of(pages: &[(f64, f64)]) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for (i, &(w, h)) in pages.iter().enumerate() {
            doc.pages.push(rofd_dom::Page {
                id: rofd_dom::PageId::new(format!("P{i}")),
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

    fn vp(size: (f64, f64), zoom: f64) -> Viewport {
        Viewport {
            scroll: (0.0, 0.0),
            zoom,
            size,
            page_gap: 20.0,
        }
    }

    fn feq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fit_width_without_scrollbars() {
        // 单 A4，视口 780x2000：无滚动条，z = 780/210。
        let doc = doc_of(&[(210.0, 297.0)]);
        let z = fit_zoom(
            &doc,
            &vp((780.0, 2000.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Width,
        )
        .unwrap();
        assert!(feq(z, 780.0 / 210.0), "z={z}");
    }

    #[test]
    fn fit_width_reserves_vertical_bar() {
        // 两页 A4，视口 780x500：竖条出现，z = (780-12)/210。
        let doc = doc_of(&[(210.0, 297.0), (210.0, 297.0)]);
        let z = fit_zoom(
            &doc,
            &vp((780.0, 500.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Width,
        )
        .unwrap();
        assert!(feq(z, 768.0 / 210.0), "z={z}");
        assert!(z < 780.0 / 210.0, "vertical bar must shrink the region");
    }

    #[test]
    fn fit_height_reserves_horizontal_bar() {
        // 场上有更宽页（400mm），视口 500x400：横条出现，z = (400-12)/297。
        let doc = doc_of(&[(210.0, 297.0), (400.0, 297.0)]);
        let z = fit_zoom(
            &doc,
            &vp((500.0, 400.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Height,
        )
        .unwrap();
        assert!(feq(z, 388.0 / 297.0), "z={z}");
    }

    #[test]
    fn fit_page_takes_min() {
        // 单 A4，视口 780x1123：收敛后竖条在场，z = min(768/210, 1123/297)。
        let doc = doc_of(&[(210.0, 297.0)]);
        let v = vp((780.0, 1123.0), 96.0 / 25.4);
        let z = fit_zoom(&doc, &v, &doc.pages[0].physical_box, FitTarget::Page).unwrap();
        assert!(feq(z, (768.0_f64 / 210.0).min(1123.0 / 297.0)), "z={z}");
    }

    #[test]
    fn fit_breaks_below_manual_floor() {
        // A0 图纸 (841x1189mm)，小视口 400x300：fit 落到 25% 以下但高于 5%。
        let doc = doc_of(&[(841.0, 1189.0)]);
        let z = fit_zoom(
            &doc,
            &vp((400.0, 300.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Width,
        )
        .unwrap();
        assert!(feq(z, 388.0 / 841.0), "z={z}");
        assert!(z < MIN_ZOOM && z > FIT_MIN_ZOOM);
    }

    #[test]
    fn fit_clamps_at_floor() {
        // 巨页 2000x2000mm，视口 300x300：理论值 0.144 低于 5% 下限。
        let doc = doc_of(&[(2000.0, 2000.0)]);
        let z = fit_zoom(
            &doc,
            &vp((300.0, 300.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Page,
        )
        .unwrap();
        assert_eq!(z, FIT_MIN_ZOOM);
    }

    #[test]
    fn fit_clamps_at_max() {
        // 小页（印章页 50mm），大视口：z 超过 300% 上限。
        let doc = doc_of(&[(50.0, 50.0)]);
        let z = fit_zoom(
            &doc,
            &vp((780.0, 2000.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Page,
        )
        .unwrap();
        assert_eq!(z, MAX_ZOOM);
    }

    #[test]
    fn fit_returns_none_when_not_ready() {
        let doc = doc_of(&[(210.0, 297.0)]);
        let empty = OfdDocument::default();
        let good = vp((780.0, 500.0), 96.0 / 25.4);
        assert!(fit_zoom(&empty, &good, &doc.pages[0].physical_box, FitTarget::Width).is_none());
        let degenerate = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 100.0,
        };
        assert!(fit_zoom(&doc, &good, &degenerate, FitTarget::Width).is_none());
        assert!(fit_zoom(
            &doc,
            &vp((0.0, 0.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Width
        )
        .is_none());
    }

    #[test]
    fn fit_agrees_with_scrollbar_layout() {
        // 一致性锚定：应用算出的 z 后，scrollbar_layout 的条出现性与预估一致。
        let doc = doc_of(&[(210.0, 297.0), (210.0, 297.0)]);
        let z = fit_zoom(
            &doc,
            &vp((780.0, 500.0), 96.0 / 25.4),
            &doc.pages[0].physical_box,
            FitTarget::Width,
        )
        .unwrap();
        let mut after = vp((780.0, 500.0), z);
        after.zoom = z;
        let layout = scrollbar_layout(&doc, &after);
        assert!(layout.vertical.is_some(), "vertical bar predicted");
        assert!(
            layout.horizontal.is_none(),
            "widest page exactly fills region"
        );
        assert!(feq(layout.content_size.0, 768.0));
    }
}
