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

/// Ellipse path (4-segment cubic Bezier approximation), centered at (w/2, h/2).
pub fn ellipse_path(r: &Rect) -> PathData {
    let (w, h) = (r.w, r.h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    let k = 0.5522847498; // circle Bezier magic constant
    PathData {
        commands: vec![
            PathCommand::M(cx + rx, cy),
            PathCommand::C(cx + rx, cy + ry * k, cx + rx * k, cy + ry, cx, cy + ry),
            PathCommand::C(cx - rx * k, cy + ry, cx - rx, cy + ry * k, cx - rx, cy),
            PathCommand::C(cx - rx, cy - ry * k, cx - rx * k, cy - ry, cx, cy - ry),
            PathCommand::C(cx + rx * k, cy - ry, cx + rx, cy - ry * k, cx + rx, cy),
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

/// Arrow path: main diagonal line + two short head segments.
pub fn arrow_path(r: &Rect) -> PathData {
    let (w, h) = (r.w, r.h);
    let head = w.min(h).max(1.0) * 0.25;
    PathData {
        commands: vec![
            PathCommand::M(0.0, 0.0),
            PathCommand::L(w, h),
            PathCommand::M(w, h),
            PathCommand::L(w - head, h),
            PathCommand::M(w, h),
            PathCommand::L(w, h - head),
        ],
    }
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
        assert_eq!(p.commands.len(), 6); // M + 4*C + Z
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
        let p = arrow_path(&Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        });
        assert_eq!(p.commands.len(), 6);
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
