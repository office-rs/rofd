//! rofd io Appearance geometry helpers. io does not depend on render
//! (AGENTS.md §4.1), so path-generation for `<PathObject AbbreviatedData>`
//! lives here. These produce [`PathData`] from high-level shapes.
//!
//! All coordinates are object-local (origin at the PathObject's Boundary
//! top-left), matching OFD §8.1 where AbbreviatedData is relative to the
//! object boundary.

use rofd_dom::{PathCommand, PathData, Point, Rect};

/// Rectangle stroke path (M-L-L-L-Z), from (0,0) to (w,h).
pub fn rect_path(r: &Rect) -> PathData {
    PathData {
        commands: vec![
            PathCommand::M(0.0, 0.0),
            PathCommand::L(r.w, 0.0),
            PathCommand::L(r.w, r.h),
            PathCommand::L(0.0, r.h),
            PathCommand::Z,
        ],
    }
}

/// Ellipse path (4-segment arc), centered at (w/2, h/2). Matches the real OFD
/// sample form `M cx+rx cy A rx ry 0 0 1 ...` (GB/T 33190 §8.2 AbbreviatedData
/// `A` arc operator), rather than a cubic-Bezier approximation.
///
/// Note: `PathCommand::A` carries 6 params `(rx, ry, rot, sweep, x, y)` (the
/// OFD/dom convention drops SVG's `large-arc-flag`; quarter arcs are always
/// small-arc). See `docs/superpowers/specs/2026-07-14-c1.5-*.md` §A.
pub fn ellipse_path(r: &Rect) -> PathData {
    let (cx, cy) = (r.w / 2.0, r.h / 2.0);
    let (rx, ry) = (r.w / 2.0, r.h / 2.0);
    PathData {
        commands: vec![
            PathCommand::M(cx + rx, cy),
            PathCommand::A(rx, ry, 0.0, 1.0, cx, cy + ry),
            PathCommand::A(rx, ry, 0.0, 1.0, cx - rx, cy),
            PathCommand::A(rx, ry, 0.0, 1.0, cx, cy - ry),
            PathCommand::A(rx, ry, 0.0, 1.0, cx + rx, cy),
            PathCommand::Z,
        ],
    }
}

/// Straight line path (M-L), diagonal from (0,0) to (w,h).
pub fn line_path(r: &Rect) -> PathData {
    PathData {
        commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(r.w, r.h)],
    }
}

/// Straight line path between two explicit endpoints `p0 -> p1` (object-local
/// coords). Use this when the line direction is known (Shape Line/Arrow with
/// stored `points`); fall back to [`line_path`] when only the bbox is known.
pub fn line_path_points(p0: Point, p1: Point) -> PathData {
    PathData {
        commands: vec![PathCommand::M(p0.x, p0.y), PathCommand::L(p1.x, p1.y)],
    }
}

/// Arrowhead tip-to-corner side length as a multiple of the stroke width.
/// Matches the WPS-generated arrow in `test/sample.ofd`
/// (side 1.7639mm at LineWidth 0.3528mm => exactly 5x).
const ARROW_HEAD_SIDE_PER_WIDTH: f64 = 5.0;

/// Arrowhead half-angle between the shaft axis and each tip->corner edge
/// (25 degrees, measured from the WPS arrow in `test/sample.ofd`).
const ARROW_HEAD_HALF_ANGLE: f64 = 25.0 * std::f64::consts::PI / 180.0;

/// Base-corner points of the filled arrowhead triangle at `tip`, oriented
/// along the shaft direction `angle`. Corners sit `5 x width` from the tip at
/// +/-25 degrees off the shaft axis - the head geometry of the WPS-generated
/// arrow in `test/sample.ofd`. A degenerate `width` (0.0, e.g. a parsed
/// PathObject without LineWidth) falls back to the default 1pt stroke so the
/// head stays visible.
fn arrow_head_corners(tip: Point, angle: f64, width: f64) -> (Point, Point) {
    let side = width.max(0.3528) * ARROW_HEAD_SIDE_PER_WIDTH;
    let (c, s) = (angle.cos(), angle.sin());
    let back = side * ARROW_HEAD_HALF_ANGLE.cos();
    let perp = side * ARROW_HEAD_HALF_ANGLE.sin();
    (
        Point {
            x: tip.x - c * back - s * perp,
            y: tip.y - s * back + c * perp,
        },
        Point {
            x: tip.x - c * back + s * perp,
            y: tip.y - s * back - c * perp,
        },
    )
}

/// Arrow path: main diagonal line (0,0)->(w,h) plus a filled triangle head
/// at the tip, oriented along the line direction. The head size scales with
/// the stroke `width` (5 x line width, +/-25 degrees half-angle - matches the
/// WPS arrow in `test/sample.ofd`). Emits M-L for the shaft, then M-L-L-Z for
/// the head.
pub fn arrow_path(r: &Rect, width: f64) -> PathData {
    let (w, h) = (r.w, r.h);
    let angle = h.atan2(w);
    let (c1, c2) = arrow_head_corners(Point { x: w, y: h }, angle, width);
    PathData {
        commands: vec![
            PathCommand::M(0.0, 0.0),
            PathCommand::L(w, h),
            PathCommand::M(c1.x, c1.y),
            PathCommand::L(w, h),
            PathCommand::L(c2.x, c2.y),
            PathCommand::Z,
        ],
    }
}

/// Arrow path between two explicit endpoints `p0 -> p1` (object-local coords),
/// with the filled triangle head at `p1` oriented along the shaft direction.
/// Head size scales with the stroke `width` (matching [`arrow_path`]). Use
/// this when the arrow direction is known; fall back to [`arrow_path`] when
/// only the bbox is known.
pub fn arrow_path_points(p0: Point, p1: Point, width: f64) -> PathData {
    let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
    let (c1, c2) = arrow_head_corners(p1, angle, width);
    PathData {
        commands: vec![
            PathCommand::M(p0.x, p0.y),
            PathCommand::L(p1.x, p1.y),
            PathCommand::M(c1.x, c1.y),
            PathCommand::L(p1.x, p1.y),
            PathCommand::L(c2.x, c2.y),
            PathCommand::Z,
        ],
    }
}

/// Polygon path (closed): M-L-...-L-Z from the given points. Empty input
/// yields an empty path (no commands).
pub fn polygon_path(points: &[Point]) -> PathData {
    let mut cmds = Vec::with_capacity(points.len() + 1);
    if let Some(p0) = points.first() {
        cmds.push(PathCommand::M(p0.x, p0.y));
        for p in &points[1..] {
            cmds.push(PathCommand::L(p.x, p.y));
        }
        cmds.push(PathCommand::Z);
    }
    PathData { commands: cmds }
}

/// Polyline path (open): M-L-...-L from the given points. Empty input yields
/// an empty path (no commands).
pub fn polyline_path(points: &[Point]) -> PathData {
    let mut cmds = Vec::with_capacity(points.len());
    if let Some(p0) = points.first() {
        cmds.push(PathCommand::M(p0.x, p0.y));
        for p in &points[1..] {
            cmds.push(PathCommand::L(p.x, p.y));
        }
    }
    PathData { commands: cmds }
}

/// Squiggly (wavy) path between two quad_points, using Q quadratic curves that
/// alternate above and below the baseline. The amplitude is a fixed 1.0
/// page-local unit, matching `render::annotation_scene::SQUIGGLY_AMPLITUDE` so
/// the serialized wave shape matches the rendered wave. 20 steps span the
/// p0->p1 x-range. Used by the Squiggly Markup appearance (GB/T 33190 §15.2.3.4).
pub fn squiggly_path(p0: Point, p1: Point) -> PathData {
    let mut cmds = vec![PathCommand::M(p0.x, p0.y)];
    let steps = 20;
    let dx = (p1.x - p0.x) / steps as f64;
    let amp = 1.0;
    for i in 0..steps {
        let x0 = p0.x + dx * i as f64;
        let x1 = p0.x + dx * (i as f64 + 1.0);
        let y_mid = if i % 2 == 0 { p0.y - amp } else { p0.y + amp };
        cmds.push(PathCommand::Q(x0, y_mid, x1, p0.y));
    }
    PathData { commands: cmds }
}

/// Markup underline/strikeout line path.
///
/// `at_bottom = true` draws at `max(p0.y, p1.y)` (underline); `false` draws
/// at the midpoint (strikeout). The x span goes from `p0.x` to `p1.x`.
pub fn markup_line_path(p0: Point, p1: Point, at_bottom: bool) -> PathData {
    let y = if at_bottom {
        p1.y.max(p0.y)
    } else {
        (p0.y + p1.y) / 2.0
    };
    PathData {
        commands: vec![PathCommand::M(p0.x, y), PathCommand::L(p1.x, y)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_path_has_five_commands() {
        let p = rect_path(&Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 20.0,
        });
        assert_eq!(p.commands.len(), 5);
        assert!(matches!(p.commands[0], PathCommand::M(0.0, 0.0)));
        assert!(matches!(p.commands.last(), Some(PathCommand::Z)));
    }

    #[test]
    fn ellipse_path_has_six_commands() {
        let p = ellipse_path(&Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        });
        assert_eq!(p.commands.len(), 6); // M + 4*A + Z
    }

    #[test]
    fn ellipse_path_uses_arc_commands() {
        // T3: ellipse must emit A (arc) operators, not C (Bezier).
        let p = ellipse_path(&Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        });
        assert!(matches!(p.commands[0], PathCommand::M(_, _)));
        assert!(matches!(p.commands[1], PathCommand::A(_, _, _, _, _, _)));
        assert!(matches!(p.commands[2], PathCommand::A(_, _, _, _, _, _)));
        assert!(matches!(p.commands[3], PathCommand::A(_, _, _, _, _, _)));
        assert!(matches!(p.commands[4], PathCommand::A(_, _, _, _, _, _)));
        assert!(matches!(p.commands[5], PathCommand::Z));
    }

    #[test]
    fn line_path_is_two_commands() {
        let p = line_path(&Rect {
            x: 0.0,
            y: 0.0,
            w: 5.0,
            h: 5.0,
        });
        assert_eq!(p.commands.len(), 2);
    }

    #[test]
    fn arrow_path_has_six_commands() {
        let p = arrow_path(
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            0.3528,
        );
        assert_eq!(p.commands.len(), 6);
    }

    #[test]
    fn arrow_path_starts_with_main_line_and_ends_with_closed_triangle() {
        // T3: M(0,0) L(w,h)  then  M(..) L(w,h) L(..) Z (filled triangle head).
        let p = arrow_path(
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            0.3528,
        );
        assert!(matches!(p.commands[0], PathCommand::M(0.0, 0.0)));
        assert!(matches!(p.commands[1], PathCommand::L(10.0, 10.0)));
        // Triangle head closes with Z (filled), not the old open two-stub form.
        assert!(matches!(p.commands.last(), Some(PathCommand::Z)));
    }

    #[test]
    fn line_path_points_uses_explicit_endpoints() {
        // Direction-aware: M(p0) L(p1) with the given endpoints (not the bbox
        // diagonal). An anti-diagonal TR->BL line keeps TR->BL.
        let p = line_path_points(Point { x: 100.0, y: 0.0 }, Point { x: 0.0, y: 50.0 });
        assert_eq!(p.commands.len(), 2);
        assert!(matches!(p.commands[0], PathCommand::M(100.0, 0.0)));
        assert!(matches!(p.commands[1], PathCommand::L(0.0, 50.0)));
    }

    #[test]
    fn arrow_path_points_head_at_p1() {
        // The shaft runs p0 -> p1 and the closed triangle head sits at p1.
        // For a TL->BR arrow this matches arrow_path(rect); for other
        // directions the head follows p1.
        let p = arrow_path_points(
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 50.0 },
            0.3528,
        );
        // M, L (shaft), M, L, L, Z (head) = 6 commands.
        assert_eq!(p.commands.len(), 6);
        assert!(matches!(p.commands[0], PathCommand::M(0.0, 0.0)));
        assert!(matches!(p.commands[1], PathCommand::L(100.0, 50.0)));
        // Head tip (4th command, the L to p1) lands on p1 = (100, 50).
        assert!(matches!(p.commands[3], PathCommand::L(100.0, 50.0)));
        assert!(matches!(p.commands.last(), Some(PathCommand::Z)));
    }

    #[test]
    fn arrow_head_matches_wps_sample_geometry() {
        // Reproduces the WPS-generated arrow from `test/sample.ofd`
        // (Annot ID=100): shaft (35.9894, 134.8477) -> (67.1096, 127.2268),
        // LineWidth 0.3528; the serialized head corners are
        // (65.7342, 128.3311) and (65.3795, 126.883) - i.e. 5 x LineWidth
        // from the tip at +/-25 degrees off the shaft axis. Tolerance covers
        // the file's 4-decimal coordinate rounding.
        let p = arrow_path_points(
            Point {
                x: 35.9894,
                y: 134.8477,
            },
            Point {
                x: 67.1096,
                y: 127.2268,
            },
            0.3528,
        );
        match (p.commands[2], p.commands[4]) {
            (PathCommand::M(x1, y1), PathCommand::L(x2, y2)) => {
                assert!((x1 - 65.7342).abs() < 0.001, "corner1.x = {x1}");
                assert!((y1 - 128.3311).abs() < 0.001, "corner1.y = {y1}");
                assert!((x2 - 65.3795).abs() < 0.001, "corner2.x = {x2}");
                assert!((y2 - 126.883).abs() < 0.001, "corner2.y = {y2}");
            }
            _ => panic!("expected M(corner1) .. L(corner2) head corners"),
        }
    }

    #[test]
    fn arrow_head_degenerate_width_falls_back_to_1pt() {
        // width = 0.0 (parsed PathObject without LineWidth) still yields a
        // visible head sized against the default 1pt stroke.
        let p = arrow_path_points(
            Point { x: 0.0, y: 0.0 },
            Point { x: 10.0, y: 0.0 },
            0.0,
        );
        // Horizontal shaft: corners sit back 5*0.3528*cos(25deg) from the tip
        // at +/-5*0.3528*sin(25deg) perpendicular.
        match (p.commands[2], p.commands[4]) {
            (PathCommand::M(x1, y1), PathCommand::L(x2, y2)) => {
                let side = 0.3528 * 5.0;
                let back = side * (25.0_f64.to_radians().cos());
                let perp = side * (25.0_f64.to_radians().sin());
                assert!((x1 - (10.0 - back)).abs() < 1e-9 && (y1 - perp).abs() < 1e-9);
                assert!((x2 - (10.0 - back)).abs() < 1e-9 && (y2 + perp).abs() < 1e-9);
            }
            _ => panic!("expected M(corner1) .. L(corner2) head corners"),
        }
    }

    #[test]
    fn polygon_path_closed_with_z() {
        let pts = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
        ];
        let p = polygon_path(&pts);
        // M + 2*L + Z = 4
        assert_eq!(p.commands.len(), 4);
        assert!(matches!(p.commands[0], PathCommand::M(0.0, 0.0)));
        assert!(matches!(p.commands[1], PathCommand::L(5.0, 10.0)));
        assert!(matches!(p.commands[2], PathCommand::L(10.0, 0.0)));
        assert!(matches!(p.commands[3], PathCommand::Z));
    }

    #[test]
    fn polyline_path_open_no_z() {
        let pts = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
        ];
        let p = polyline_path(&pts);
        // M + 2*L = 3 (no Z)
        assert_eq!(p.commands.len(), 3);
        assert!(matches!(p.commands.last(), Some(PathCommand::L(10.0, 0.0))));
    }

    #[test]
    fn polygon_and_polyline_empty_points_yield_no_commands() {
        assert!(polygon_path(&[]).commands.is_empty());
        assert!(polyline_path(&[]).commands.is_empty());
    }

    #[test]
    fn squiggly_path_starts_with_m_and_uses_q_curves() {
        let p = squiggly_path(Point { x: 0.0, y: 4.0 }, Point { x: 40.0, y: 8.0 });
        // M + 20 Q = 21
        assert_eq!(p.commands.len(), 21);
        assert!(matches!(p.commands[0], PathCommand::M(0.0, 4.0)));
        for c in &p.commands[1..] {
            assert!(
                matches!(c, PathCommand::Q(_, _, _, _)),
                "expected Q, got {c:?}"
            );
        }
    }

    #[test]
    fn squiggly_path_uses_fixed_amplitude_matching_render() {
        // io's squiggly_path amplitude must equal render's
        // SQUIGGLY_AMPLITUDE (1.0) so the serialized wave matches the
        // rendered wave. The first Q's control point (y_mid) should be
        // baseline - 1.0 (i=0 is even -> above).
        let baseline_y = 4.0;
        let p = squiggly_path(
            Point {
                x: 0.0,
                y: baseline_y,
            },
            Point { x: 40.0, y: 8.0 },
        );
        match &p.commands[1] {
            PathCommand::Q(_, y_mid, _, _) => {
                assert!(
                    (*y_mid - (baseline_y - 1.0)).abs() < 1e-10,
                    "expected amp=1.0 (y_mid={}), got {}",
                    baseline_y - 1.0,
                    y_mid
                );
            }
            _ => panic!("expected Q as second command"),
        }
    }

    #[test]
    fn markup_line_at_bottom_uses_max_y() {
        let p = markup_line_path(Point { x: 0.0, y: 4.0 }, Point { x: 38.0, y: 4.4 }, true);
        match &p.commands[0] {
            PathCommand::M(x, y) => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 4.4); // max(4.0, 4.4)
            }
            _ => panic!("expected M"),
        }
    }

    #[test]
    fn markup_line_at_midpoint_uses_average_y() {
        let p = markup_line_path(Point { x: 0.0, y: 4.0 }, Point { x: 38.0, y: 8.0 }, false);
        match &p.commands[0] {
            PathCommand::M(_, y) => assert_eq!(*y, 6.0), // (4+8)/2
            _ => panic!("expected M"),
        }
    }
}
