//! WPS-aligned selection handle strategy (spec §4.1). Single source of truth
//! for which handles a selected annotation exposes and where they sit:
//! `hit_test` (hit priority) and `composite::draw_selection_overlay`
//! (drawing) both resolve through this module - never hard-code a second copy.
//!
//! | payload                        | handles                     |
//! |--------------------------------|-----------------------------|
//! | Shape(Rect), Note/TextBox/...  | 8: 4 corners + 4 edges      |
//! | Shape(Ellipse)                 | 4 edge midpoints (N/S/E/W)  |
//! | Shape(Line/Arrow)              | 2 endpoints (Vertex 0/1)    |
//! | Shape(Polygon/PolyLine)        | one Vertex per point        |
//! | Freehand, Markup               | none (bbox frame only)      |

use rofd_dom::{Annotation, AnnotationPayload, OfdDocument, ShapeKind};

use crate::hit_test::{annotation_local_rect, HandlePos};
use crate::viewport::Viewport;

/// The selection handles an annotation exposes, in hit-priority order
/// (corners before edges for rect-like sets).
pub fn annotation_handles(ann: &Annotation) -> Vec<HandlePos> {
    match &ann.payload {
        AnnotationPayload::Shape { kind, .. } => match kind {
            ShapeKind::Rect => eight(),
            ShapeKind::Ellipse => vec![HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W],
            // Endpoints even when `points` is short: centers resolve via the
            // rect-diagonal fallback (mirrors render's line_endpoints).
            ShapeKind::Line | ShapeKind::Arrow => vec![HandlePos::Vertex(0), HandlePos::Vertex(1)],
            ShapeKind::Polygon | ShapeKind::PolyLine => {
                (0..kind_points(ann).len()).map(HandlePos::Vertex).collect()
            }
        },
        AnnotationPayload::Note { .. }
        | AnnotationPayload::TextBox { .. }
        | AnnotationPayload::Stamp { .. }
        | AnnotationPayload::Watermark { .. } => eight(),
        AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. } => Vec::new(),
    }
}

fn eight() -> Vec<HandlePos> {
    vec![
        HandlePos::Nw,
        HandlePos::Ne,
        HandlePos::Sw,
        HandlePos::Se,
        HandlePos::N,
        HandlePos::S,
        HandlePos::E,
        HandlePos::W,
    ]
}

fn kind_points(ann: &Annotation) -> &[rofd_dom::Point] {
    match &ann.payload {
        AnnotationPayload::Shape { points, .. } => points,
        _ => &[],
    }
}

/// Resolve a handle's center in page-local coordinates. `None` when the
/// position does not exist for this payload (out-of-range Vertex, Vertex on
/// a rect-based kind, or a degenerate no-rect payload for standard handles).
pub fn handle_center_local(ann: &Annotation, pos: HandlePos) -> Option<(f64, f64)> {
    match pos {
        HandlePos::Vertex(i) => {
            let AnnotationPayload::Shape {
                kind, rect, points, ..
            } = &ann.payload
            else {
                return None;
            };
            match kind {
                ShapeKind::Line | ShapeKind::Arrow => {
                    // Prefer explicit endpoints; fall back to the rect's
                    // TL->BR diagonal (same fallback as annotation_scene::
                    // line_endpoints, so handle and drawing agree).
                    match points.get(i) {
                        Some(p) => Some((p.x, p.y)),
                        None if i < 2 => Some((
                            if i == 0 { rect.x } else { rect.x + rect.w },
                            if i == 0 { rect.y } else { rect.y + rect.h },
                        )),
                        None => None,
                    }
                }
                ShapeKind::Polygon | ShapeKind::PolyLine => points.get(i).map(|p| (p.x, p.y)),
                _ => None,
            }
        }
        _ => {
            let r = annotation_local_rect(ann)?;
            let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            Some(match pos {
                HandlePos::Nw => (x0, y0),
                HandlePos::Ne => (x1, y0),
                HandlePos::Sw => (x0, y1),
                HandlePos::Se => (x1, y1),
                HandlePos::N => (cx, y0),
                HandlePos::S => (cx, y1),
                HandlePos::E => (x1, cy),
                HandlePos::W => (x0, cy),
                HandlePos::Vertex(_) => unreachable!("handled above"),
            })
        }
    }
}

/// Handle positions in viewport (screen) space: `(HandlePos, center)`.
/// Screen-space like the old rect-based hit: `origin + local * zoom`.
pub fn annotation_handle_positions(
    doc: &OfdDocument,
    ann: &Annotation,
    vp: &Viewport,
) -> Vec<(HandlePos, (f64, f64))> {
    let Some(idx) = doc.pages.iter().position(|p| p.id == ann.page) else {
        return Vec::new();
    };
    let Some((ox, oy)) = crate::composite::page_origin(doc, vp, idx) else {
        return Vec::new();
    };
    annotation_handles(ann)
        .into_iter()
        .filter_map(|pos| {
            handle_center_local(ann, pos)
                .map(|(lx, ly)| (pos, (ox + lx * vp.zoom, oy + ly * vp.zoom)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, PageId, PathCommand,
        PathData, Point, Rect, ShapeKind,
    };

    fn shape_ann(kind: ShapeKind, rect: Rect, points: Vec<Point>) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Shape(kind),
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Shape {
                kind,
                rect,
                stroke: Color::Rgb(255, 0, 0),
                fill: None,
                width: 2.0,
                points,
            },
        }
    }

    const EIGHT: [HandlePos; 8] = [
        HandlePos::Nw,
        HandlePos::Ne,
        HandlePos::Sw,
        HandlePos::Se,
        HandlePos::N,
        HandlePos::S,
        HandlePos::E,
        HandlePos::W,
    ];

    #[test]
    fn handle_sets_follow_wps_strategy() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        // Rect + rect-bearing（Note 等）= 8；Ellipse = 4 边中点。
        assert_eq!(
            annotation_handles(&shape_ann(ShapeKind::Rect, r, vec![])),
            EIGHT.to_vec()
        );
        assert_eq!(
            annotation_handles(&shape_ann(ShapeKind::Ellipse, r, vec![])),
            vec![HandlePos::N, HandlePos::S, HandlePos::E, HandlePos::W]
        );
        // Line/Arrow = 2 端点（即使 points 为空也暴露，坐标走 rect 对角线回退）。
        for kind in [ShapeKind::Line, ShapeKind::Arrow] {
            assert_eq!(
                annotation_handles(&shape_ann(kind, r, vec![])),
                vec![HandlePos::Vertex(0), HandlePos::Vertex(1)]
            );
        }
        // Polygon/PolyLine = 每顶点一个。
        let pts = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 5.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
        ];
        for kind in [ShapeKind::Polygon, ShapeKind::PolyLine] {
            assert_eq!(
                annotation_handles(&shape_ann(kind, r, pts.clone())),
                vec![
                    HandlePos::Vertex(0),
                    HandlePos::Vertex(1),
                    HandlePos::Vertex(2)
                ]
            );
        }
    }

    #[test]
    fn markup_and_freehand_have_no_handles() {
        let markup = Annotation {
            payload: AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                color: Color::Rgb(255, 255, 0),
            },
            ..shape_ann(ShapeKind::Rect, Rect::default(), vec![])
        };
        assert!(annotation_handles(&markup).is_empty());
        let freehand = Annotation {
            payload: AnnotationPayload::Freehand {
                path: PathData {
                    commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)],
                },
                color: Color::Rgb(0, 0, 255),
                width: 1.5,
            },
            ..shape_ann(ShapeKind::Rect, Rect::default(), vec![])
        };
        assert!(annotation_handles(&freehand).is_empty());
    }

    #[test]
    fn vertex_centers_resolve_from_points_or_rect_fallback() {
        let r = Rect {
            x: 10.0,
            y: 10.0,
            w: 90.0,
            h: 50.0,
        };
        let a = shape_ann(
            ShapeKind::Line,
            r,
            vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }],
        );
        assert_eq!(
            handle_center_local(&a, HandlePos::Vertex(0)),
            Some((10.0, 10.0))
        );
        assert_eq!(
            handle_center_local(&a, HandlePos::Vertex(1)),
            Some((100.0, 60.0))
        );
        // 空 points：回退 rect 对角线（TL / BR）。
        let b = shape_ann(ShapeKind::Arrow, r, vec![]);
        assert_eq!(
            handle_center_local(&b, HandlePos::Vertex(0)),
            Some((10.0, 10.0))
        );
        assert_eq!(
            handle_center_local(&b, HandlePos::Vertex(1)),
            Some((100.0, 60.0))
        );
        // 越界 / Rect 类型的 Vertex -> None。
        assert_eq!(handle_center_local(&a, HandlePos::Vertex(2)), None);
        let c = shape_ann(ShapeKind::Rect, r, vec![]);
        assert_eq!(handle_center_local(&c, HandlePos::Vertex(0)), None);
        // 标准 8 句柄仍从 rect 解析。
        assert_eq!(handle_center_local(&c, HandlePos::Nw), Some((10.0, 10.0)));
        assert_eq!(handle_center_local(&c, HandlePos::E), Some((100.0, 35.0)));
    }
}
