//! rofd io Appearance geometry helpers. io does not depend on render
//! (AGENTS.md §4.1), so path-generation for `<PathObject AbbreviatedData>`
//! lives here. These produce [`PathData`] from high-level shapes.
//!
//! All coordinates are object-local (origin at the PathObject's Boundary
//! top-left), matching OFD §8.1 where AbbreviatedData is relative to the
//! object boundary.

use rofd_dom::{PathCommand, PathData, Point, Rect};

/// Translate every command of a PathData by (dx, dy). Shared by serialize
/// (page-local -> object-local) and parse (object-local -> page-local).
pub fn translate_path(p: &PathData, dx: f64, dy: f64) -> PathData {
    let commands = p
        .commands
        .iter()
        .map(|c| match *c {
            PathCommand::M(x, y) => PathCommand::M(x + dx, y + dy),
            PathCommand::L(x, y) => PathCommand::L(x + dx, y + dy),
            PathCommand::C(a, b, x, y, e, f) => {
                PathCommand::C(a + dx, b + dy, x + dx, y + dy, e + dx, f + dy)
            }
            PathCommand::Q(a, b, x, y) => PathCommand::Q(a + dx, b + dy, x + dx, y + dy),
            PathCommand::Z => PathCommand::Z,
            PathCommand::A(a, b, c, d, x, y) => PathCommand::A(a, b, c, d, x + dx, y + dy),
        })
        .collect();
    PathData { commands }
}

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

/// Stroked-rectangle path inset by `inset` (typically half the stroke width)
/// from the box on all four sides. Strict readers clip a PathObject to its
/// Boundary, so a stroke centered ON the edge loses its outer half to
/// clipping on every side. Reference authoring tools inset the path by
/// LineWidth/2 so the whole stroke sits inside the boundary (measured from
/// test/sample.ofd: LineWidth 0.3528, rect path inset exactly 0.1764 on all
/// four sides). Reads only w/h; degenerate boxes collapse onto the inset.
pub fn inset_rect_path(r: &Rect, inset: f64) -> PathData {
    let x1 = (r.w - inset).max(inset);
    let y1 = (r.h - inset).max(inset);
    PathData {
        commands: vec![
            PathCommand::M(inset, inset),
            PathCommand::L(x1, inset),
            PathCommand::L(x1, y1),
            PathCommand::L(inset, y1),
            PathCommand::Z,
        ],
    }
}

/// Ellipse path (4-segment arc) filling the whole rect. See
/// [`ellipse_path_inset`] for the stroked form.
pub fn ellipse_path(r: &Rect) -> PathData {
    ellipse_path_inset(r, 0.0)
}

/// Stroked-ellipse path: center stays at the box center but both radii shrink
/// by `inset` (typically half the stroke width). Strict readers clip a
/// PathObject to its Boundary, so an ellipse touching the box edges loses the
/// stroke's outer half at its top/bottom/left/right extremes. Reference
/// authoring tools inset the radii by LineWidth/2 (measured from
/// test/sample.ofd ID=106: Boundary 23.8517x6.4919, rx = w/2 - 0.1764,
/// ry = h/2 - 0.1764 at LineWidth 0.3528).
///
/// Note: `PathCommand::A` carries 6 params `(rx, ry, rot, sweep, x, y)` (the
/// OFD/dom convention drops SVG's `large-arc-flag`; quarter arcs are always
/// small-arc). See `docs/superpowers/specs/2026-07-14-c1.5-*.md` §A.
pub fn ellipse_path_inset(r: &Rect, inset: f64) -> PathData {
    let (cx, cy) = (r.w / 2.0, r.h / 2.0);
    let rx = (r.w / 2.0 - inset).max(0.0);
    let ry = (r.h / 2.0 - inset).max(0.0);
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
/// Matches the reference arrow in `test/sample.ofd`
/// (side 1.7639mm at LineWidth 0.3528mm => exactly 5x).
const ARROW_HEAD_SIDE_PER_WIDTH: f64 = 5.0;

/// Arrowhead half-angle between the shaft axis and each tip->corner edge
/// (25 degrees, measured from the reference arrow in `test/sample.ofd`).
const ARROW_HEAD_HALF_ANGLE: f64 = 25.0 * std::f64::consts::PI / 180.0;

/// Base-corner points of the filled arrowhead triangle at `tip`, oriented
/// along the shaft direction `angle`. Corners sit `5 x width` from the tip at
/// +/-25 degrees off the shaft axis - the head geometry of the reference
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
/// reference arrow in `test/sample.ofd`). Emits M-L for the shaft, then M-L-L-Z for
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

/// Squiggly wave amplitude (page-local mm): the Q control point alternates
/// +/- this value around the baseline (visual curve peak = half of this).
/// 0.5625mm measured from reference-authoring-tool squiggles (test/sample.ofd
/// ID=86: controls at baseline +/- 0.5625). Must equal
/// `render::annotation_scene::SQUIGGLY_AMPLITUDE` so the serialized wave
/// shape matches the rendered wave.
pub const SQUIGGLY_AMPLITUDE: f64 = 0.5625;

/// X-span (page-local mm) of one Q-curve half-wave. Fixed so wave density is
/// independent of quad width (text length). 0.5925mm measured from reference
/// squiggles (nodes spaced 0.5925mm), ~3.4x denser than the old 2.0mm wave.
/// Must equal `render::annotation_scene::SQUIGGLY_HALF_WAVE` for the same
/// reason.
pub const SQUIGGLY_HALF_WAVE: f64 = 0.5925;

/// Squiggly (wavy) path between two quad_points, using Q quadratic curves that
/// alternate above and below the baseline. Half-waves have a FIXED 0.5925mm
/// x-span: the number of arcs grows with the quad width while their density
/// stays constant (a 4-char and a whole-line squiggly share the same
/// wavelength). A span that is not a multiple of the half-wave ends with one
/// partial arc landing exactly on `p1.x`. Used by the Squiggly Markup
/// appearance (GB/T 33190 §15.2.3.4).
pub fn squiggly_path(p0: Point, p1: Point) -> PathData {
    let mut cmds = vec![PathCommand::M(p0.x, p0.y)];
    let span = p1.x - p0.x;
    if span.abs() < 1e-6 {
        return PathData { commands: cmds };
    }
    let dir = span.signum();
    let full = (span.abs() / SQUIGGLY_HALF_WAVE).floor();
    let mut x = p0.x;
    let mut up = true;
    for _ in 0..full as usize {
        let x_next = x + dir * SQUIGGLY_HALF_WAVE;
        let y_mid = if up {
            p0.y - SQUIGGLY_AMPLITUDE
        } else {
            p0.y + SQUIGGLY_AMPLITUDE
        };
        cmds.push(PathCommand::Q((x + x_next) / 2.0, y_mid, x_next, p0.y));
        x = x_next;
        up = !up;
    }
    // Remainder (< one half-wave): a final partial arc ending exactly at
    // p1.x so the wave stays flush with the text end.
    if (p1.x - x).abs() > 1e-6 {
        let y_mid = if up {
            p0.y - SQUIGGLY_AMPLITUDE
        } else {
            p0.y + SQUIGGLY_AMPLITUDE
        };
        cmds.push(PathCommand::Q((x + p1.x) / 2.0, y_mid, p1.x, p0.y));
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
    fn inset_rect_path_pulls_all_four_sides_inside() {
        // Reference convention (sample.ofd ID=103): LineWidth 0.3528 rect path
        // sits 0.1764 inside the Boundary on every side, so the stroke's
        // outer half is not clipped. Degenerate boxes collapse onto the
        // inset instead of inverting.
        let p = inset_rect_path(
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 30.6262,
                h: 7.762,
            },
            0.1764,
        );
        match &p.commands[..] {
            [PathCommand::M(x0, y0), PathCommand::L(x1, y1), PathCommand::L(x2, y2), PathCommand::L(x3, y3), PathCommand::Z] =>
            {
                assert!((*x0 - 0.1764).abs() < 1e-9 && (*y0 - 0.1764).abs() < 1e-9);
                assert!((x1 - (30.6262 - 0.1764)).abs() < 1e-9 && (*y1 - 0.1764).abs() < 1e-9);
                assert!(
                    (x2 - (30.6262 - 0.1764)).abs() < 1e-9 && (y2 - (7.762 - 0.1764)).abs() < 1e-9
                );
                assert!((*x3 - 0.1764).abs() < 1e-9 && (y3 - (7.762 - 0.1764)).abs() < 1e-9);
            }
            other => panic!("expected M-L-L-L-Z, got {other:?}"),
        }
        // Inset larger than half the box: collapses, never inverts.
        let degenerate = inset_rect_path(
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 0.2,
                h: 0.2,
            },
            0.5,
        );
        match &degenerate.commands[..] {
            [PathCommand::M(x0, _), PathCommand::L(x1, _), PathCommand::L(x2, _), PathCommand::L(x3, _), PathCommand::Z] => {
                for x in [*x0, *x1, *x2, *x3] {
                    assert!((x - 0.5).abs() < 1e-9, "collapsed onto the inset, got {x}");
                }
            }
            other => panic!("expected M-L-L-L-Z, got {other:?}"),
        }
    }

    #[test]
    fn ellipse_path_inset_shrinks_radii_keeps_center() {
        // Reference convention (sample.ofd ID=106): Boundary 23.8517x6.4919 at
        // LineWidth 0.3528 -> rx = w/2 - 0.1764, ry = h/2 - 0.1764, center
        // unchanged at (w/2, h/2). The path stays strictly inside the box.
        let p = ellipse_path_inset(
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 23.8517,
                h: 6.4919,
            },
            0.1764,
        );
        match &p.commands[..] {
            [PathCommand::M(mx, my), PathCommand::A(rx0, ry0, _, _, ax0, ay0), PathCommand::A(rx1, ry1, _, _, ax1, ay1), PathCommand::A(rx2, ry2, _, _, ax2, ay2), PathCommand::A(rx3, ry3, _, _, ax3, ay3), PathCommand::Z] =>
            {
                let (cx, cy) = (23.8517 / 2.0, 6.4919 / 2.0);
                let (erx, ery) = (cx - 0.1764, cy - 0.1764);
                assert!((*mx - cx - erx).abs() < 1e-9 && (*my - cy).abs() < 1e-9);
                for (rx, ry) in [(*rx0, *ry0), (*rx1, *ry1), (*rx2, *ry2), (*rx3, *ry3)] {
                    assert!((rx - erx).abs() < 1e-9, "rx = w/2 - inset, got {rx}");
                    assert!((ry - ery).abs() < 1e-9, "ry = h/2 - inset, got {ry}");
                }
                // Extreme points: (cx +/- rx, cy) and (cx, cy +/- ry) - all
                // inside the box with the full stroke width to spare.
                for (x, y) in [
                    (cx + erx, cy),
                    (cx, cy + ery),
                    (cx - erx, cy),
                    (cx, cy - ery),
                ] {
                    let matched = [
                        (*mx, *my),
                        (*ax0, *ay0),
                        (*ax1, *ay1),
                        (*ax2, *ay2),
                        (*ax3, *ay3),
                    ]
                    .iter()
                    .any(|(px, py)| (px - x).abs() < 1e-9 && (py - y).abs() < 1e-9);
                    assert!(matched, "missing extreme point ({x}, {y})");
                }
            }
            other => panic!("expected M-A-A-A-A-Z, got {other:?}"),
        }
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
    fn arrow_head_matches_sample_geometry() {
        // Reproduces the reference arrow from `test/sample.ofd`
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
        let p = arrow_path_points(Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }, 0.0);
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
        // 40mm = 67 full half-waves + 1 partial: M + 68 Q = 69
        assert_eq!(p.commands.len(), 69);
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
        // SQUIGGLY_AMPLITUDE (0.5625, measured from reference squiggles) so
        // the serialized wave matches the rendered wave. The first Q's
        // control point (y_mid) should be baseline - 0.5625 (i=0 is even ->
        // above).
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
                    (*y_mid - (baseline_y - 0.5625)).abs() < 1e-10,
                    "expected amp=0.5625 (y_mid={}), got {}",
                    baseline_y - 0.5625,
                    y_mid
                );
            }
            _ => panic!("expected Q as second command"),
        }
    }

    #[test]
    fn squiggly_wave_density_is_fixed_not_length_scaled() {
        // 每个 Q 段 (半波) 的 x 跨度恒为 0.5925 页面局部 mm (参考工具实测
        // 波距): 波浪密度不随 quad 宽度 (文字长度) 变化. 40mm 与 400mm 的
        // 每段跨度相同.
        const HALF_WAVE: f64 = 0.5925;
        for width in [40.0f64, 400.0] {
            let p = squiggly_path(Point { x: 0.0, y: 4.0 }, Point { x: width, y: 8.0 });
            let full = (width / HALF_WAVE).floor() as usize;
            let tail = width - full as f64 * HALF_WAVE > 1e-6;
            assert_eq!(
                p.commands.len(),
                full + usize::from(tail) + 1,
                "width={width}"
            );
            for (i, c) in p.commands[1..].iter().enumerate() {
                match c {
                    PathCommand::Q(cx, _, x_end, _) => {
                        if i < full {
                            let expected_end = (i as f64 + 1.0) * HALF_WAVE;
                            assert!(
                                (*x_end - expected_end).abs() < 1e-9,
                                "width={width} segment {i} must span {HALF_WAVE} (end {expected_end}), got {x_end}"
                            );
                            let mid = (i as f64 + 0.5) * HALF_WAVE;
                            assert!(
                                (*cx - mid).abs() < 1e-9,
                                "width={width} segment {i} control at {mid}, got {cx}"
                            );
                        } else {
                            let mid = (i as f64 * HALF_WAVE + width) / 2.0;
                            assert!(
                                (*cx - mid).abs() < 1e-9,
                                "width={width} tail control at {mid}, got {cx}"
                            );
                            assert!(
                                (*x_end - width).abs() < 1e-9,
                                "width={width} tail ends at {width}, got {x_end}"
                            );
                        }
                    }
                    _ => panic!("expected Q, got {c:?}"),
                }
            }
        }
    }

    #[test]
    fn squiggly_control_x_at_segment_midpoint_matching_render() {
        // render 版控制点在段中点; io 序列化的波形必须同形, 否则保存文件
        // 在其他阅读器里的波峰位置与 rofd 屏显不一致.
        let p = squiggly_path(Point { x: 0.0, y: 4.0 }, Point { x: 40.0, y: 8.0 });
        match &p.commands[1] {
            PathCommand::Q(cx, _, x_end, _) => {
                assert!(
                    (*cx - 0.29625).abs() < 1e-9,
                    "control at midpoint 0.29625, got {cx}"
                );
                assert!((*x_end - 0.5925).abs() < 1e-9);
            }
            _ => panic!("expected Q as second command"),
        }
    }

    #[test]
    fn squiggly_partial_final_wave_ends_exactly_at_x1() {
        // 宽度非半波整数倍时: 完整半波保持 0.5925 跨度不变 (41mm = 69 个完整
        // 半波 = 40.8825mm), 末尾补一段部分波, 终点精确落在 p1.x (与文字末尾
        // 对齐, 不留缺口).
        let p = squiggly_path(Point { x: 0.0, y: 4.0 }, Point { x: 41.0, y: 8.0 });
        assert_eq!(p.commands.len(), 71); // M + 69 full + 1 partial
        match &p.commands[70] {
            PathCommand::Q(cx, _, x_end, _) => {
                assert!(
                    (*cx - 40.94125).abs() < 1e-9,
                    "control at partial midpoint 40.94125, got {cx}"
                );
                assert!(
                    (*x_end - 41.0).abs() < 1e-9,
                    "ends exactly at x1, got {x_end}"
                );
            }
            _ => panic!("expected Q as last command"),
        }
    }

    #[test]
    fn squiggly_span_shorter_than_half_wave_draws_single_partial_arc() {
        // 不足一个半波时也画一段弧 (而非空路径), 终点仍落在 p1.x.
        let p = squiggly_path(Point { x: 3.0, y: 4.0 }, Point { x: 3.4, y: 8.0 });
        assert_eq!(p.commands.len(), 2); // M + one partial Q
        match (&p.commands[0], &p.commands[1]) {
            (PathCommand::M(mx, _), PathCommand::Q(cx, _, x_end, _)) => {
                assert!((*mx - 3.0).abs() < 1e-9);
                assert!(
                    (*cx - 3.2).abs() < 1e-9,
                    "control at midpoint 3.2, got {cx}"
                );
                assert!((*x_end - 3.4).abs() < 1e-9);
            }
            _ => panic!("expected M + Q"),
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
