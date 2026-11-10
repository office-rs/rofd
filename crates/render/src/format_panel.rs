//! Geometry for the floating annotation format panel.
//!
//! The panel is rendered by each SDK adapter; only its position is kernel
//! knowledge (the annotation rect lives in viewport coordinates that only the
//! component can compute). The rule is: above the box by `margin`; flip below
//! when the top has no room; clamp into the viewport on both axes.

use rofd_dom::Rect;

/// Top-left corner for a `panel`-sized format panel anchored to `ann`
/// (viewport coordinates). `viewport` is the widget's visible size.
pub fn format_panel_anchor(
    ann: Rect,
    panel: (f64, f64),
    viewport: (f64, f64),
    margin: f64,
) -> (f64, f64) {
    let max_x = (viewport.0 - panel.0 - margin).max(margin);
    let x = ann.x.clamp(margin, max_x);
    let mut y = ann.y - panel.1 - margin;
    if y < margin {
        y = ann.y + ann.h + margin;
    }
    let max_y = (viewport.1 - panel.1 - margin).max(margin);
    (x, y.clamp(margin, max_y))
}

#[cfg(test)]
mod tests {
    use super::format_panel_anchor;
    use rofd_dom::Rect;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn viewport() -> (f64, f64) {
        (800.0, 600.0)
    }

    #[test]
    fn sits_above_the_box() {
        let (x, y) = format_panel_anchor(
            rect(100.0, 100.0, 200.0, 50.0),
            (240.0, 28.0),
            viewport(),
            8.0,
        );
        assert_eq!((x, y), (100.0, 64.0));
    }

    #[test]
    fn flips_below_when_the_top_has_no_room() {
        let (x, y) = format_panel_anchor(
            rect(10.0, 10.0, 200.0, 50.0),
            (240.0, 28.0),
            viewport(),
            8.0,
        );
        assert_eq!((x, y), (10.0, 68.0));
    }

    #[test]
    fn clamps_horizontally_into_the_viewport() {
        let (x, _) = format_panel_anchor(
            rect(700.0, 300.0, 60.0, 20.0),
            (240.0, 28.0),
            viewport(),
            8.0,
        );
        assert_eq!(
            x, 552.0,
            "right edge is clamped to viewport - panel - margin"
        );
        let (x, _) =
            format_panel_anchor(rect(0.0, 300.0, 60.0, 20.0), (240.0, 28.0), viewport(), 8.0);
        assert_eq!(x, 8.0, "left edge is clamped to the margin");
    }

    #[test]
    fn viewport_smaller_than_panel_still_returns_a_visible_origin() {
        let (x, y) = format_panel_anchor(
            rect(10.0, 10.0, 20.0, 20.0),
            (240.0, 28.0),
            (200.0, 120.0),
            8.0,
        );
        assert_eq!((x, y), (8.0, 38.0));
    }

    #[test]
    fn flips_below_and_clamps_vertically_near_the_bottom() {
        let (_, y) = format_panel_anchor(
            rect(10.0, 10.0, 200.0, 80.0),
            (240.0, 28.0),
            (800.0, 120.0),
            8.0,
        );
        assert_eq!(y, 84.0, "below, then clamped to viewport - panel - margin");
    }
}
