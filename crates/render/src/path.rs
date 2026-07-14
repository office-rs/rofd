use kurbo::BezPath;
use rofd_dom::{PathCommand, PathData};

/// Convert OFD PathData (AbbreviatedData commands) to a kurbo BezPath.
/// Coordinates are in the object's local space (CTM applied separately by the caller).
pub fn path_to_bezpath(data: &PathData) -> BezPath {
    let mut path = BezPath::new();
    for cmd in &data.commands {
        match *cmd {
            PathCommand::M(x, y) => path.move_to((x, y)),
            PathCommand::L(x, y) => path.line_to((x, y)),
            PathCommand::C(x1, y1, x2, y2, x, y) => path.curve_to((x1, y1), (x2, y2), (x, y)),
            PathCommand::Q(x1, y1, x, y) => path.quad_to((x1, y1), (x, y)),
            PathCommand::A(_a, _b, _c, _d, x, y) => {
                // OFD `A` (arc) carries 6 params (see PathCommand::A in rofd-dom).
                // A faithful conversion to kurbo::Arc would need center/radii/angles,
                // which requires endpoint->center conversion. Arcs are rare in common
                // OFD paths; v1 approximates via a line to the endpoint (x, y).
                // Full arc conversion is deferred to a later task.
                path.line_to((x, y));
            }
            PathCommand::Z => path.close_path(),
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{ParamCurve, PathSeg, Point};

    #[test]
    fn converts_m_l_z() {
        let pd = PathData {
            commands: vec![
                PathCommand::M(0.0, 0.0),
                PathCommand::L(100.0, 0.0),
                PathCommand::L(100.0, 10.0),
                PathCommand::Z,
            ],
        };
        let bez = path_to_bezpath(&pd);
        let segs: Vec<_> = bez.segments().collect();
        // kurbo 0.13: move_to is not a segment; each line_to is a Line; close_path
        // emits a Line back to the subpath start. So M + 2 L + Z -> 3 Line segments.
        assert_eq!(
            segs.len(),
            3,
            "M + 2 L + Z -> 3 segments (move is not a segment)"
        );
        assert!(segs.iter().all(|s| matches!(s, PathSeg::Line(_))));
        // Close-path segment returns to the start point (0, 0).
        let close_end = segs[2].end();
        assert_eq!(
            close_end,
            Point::new(0.0, 0.0),
            "close_path returns to start"
        );
    }

    #[test]
    fn empty_pathdata_yields_empty_bezpath() {
        let bez = path_to_bezpath(&PathData::default());
        assert_eq!(bez.segments().count(), 0);
    }

    #[test]
    fn converts_cubic_and_quad() {
        // M + C (cubic) + Q (quad) -> 1 cubic segment + 1 quad segment.
        let pd = PathData {
            commands: vec![
                PathCommand::M(0.0, 0.0),
                PathCommand::C(10.0, 10.0, 20.0, 10.0, 30.0, 0.0),
                PathCommand::Q(40.0, 10.0, 50.0, 0.0),
            ],
        };
        let bez = path_to_bezpath(&pd);
        let segs: Vec<_> = bez.segments().collect();
        assert_eq!(segs.len(), 2);
        assert!(matches!(segs[0], PathSeg::Cubic(_)), "C -> cubic segment");
        assert!(matches!(segs[1], PathSeg::Quad(_)), "Q -> quad segment");
    }

    #[test]
    fn converts_arc_as_line_to_endpoint() {
        // v1 approximation: A -> line_to(endpoint). Endpoint is the last two fields.
        let pd = PathData {
            commands: vec![
                PathCommand::M(0.0, 0.0),
                PathCommand::A(5.0, 5.0, 0.0, 0.0, 40.0, 0.0),
            ],
        };
        let bez = path_to_bezpath(&pd);
        let segs: Vec<_> = bez.segments().collect();
        assert_eq!(
            segs.len(),
            1,
            "arc approximated as a single line to endpoint"
        );
        let end = segs[0].end();
        assert_eq!(
            end,
            Point::new(40.0, 0.0),
            "arc endpoint is (x, y) = last two fields"
        );
    }
}
