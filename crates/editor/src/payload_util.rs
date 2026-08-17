use rofd_dom::{AnnotationPayload, Color, PathCommand, Point, Rect, ShapeKind};

/// Shift an annotation's geometry by (dx, dy).
pub fn move_payload(p: &mut AnnotationPayload, dx: f64, dy: f64) {
    match p {
        AnnotationPayload::Markup { quad_points, .. } => {
            for pt in quad_points {
                pt.x += dx;
                pt.y += dy;
            }
        }
        AnnotationPayload::Freehand { path, .. } => {
            for cmd in &mut path.commands {
                shift_cmd(cmd, dx, dy);
            }
        }
        AnnotationPayload::Shape { rect, points, .. } => {
            rect.x += dx;
            rect.y += dy;
            // Line/Arrow/Polygon/PolyLine endpoints must move with the bbox,
            // else the geometry stays put while the bbox slides away.
            for pt in points {
                pt.x += dx;
                pt.y += dy;
            }
        }
        AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. }
        | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => {
            rect.x += dx;
            rect.y += dy;
        }
    }
}

/// Set the rect (for resize). No-op for Markup/Freehand (no single rect).
///
/// For `Shape`, the endpoint `points` are rescaled from the old rect to the
/// new rect (proportional, per-axis), preserving each point's relative
/// position within the bbox. This keeps a Line/Arrow's direction and a
/// Polygon/PolyLine's shape intact across resize. A degenerate old rect
/// (zero size on an axis) degrades to translating points by the origin delta.
pub fn resize_payload(p: &mut AnnotationPayload, new_rect: Rect) {
    match p {
        AnnotationPayload::Shape { rect, points, .. } => {
            let old = *rect;
            if !points.is_empty() {
                let sx = if old.w != 0.0 {
                    new_rect.w / old.w
                } else {
                    1.0
                };
                let sy = if old.h != 0.0 {
                    new_rect.h / old.h
                } else {
                    1.0
                };
                for pt in points {
                    pt.x = new_rect.x + (pt.x - old.x) * sx;
                    pt.y = new_rect.y + (pt.y - old.y) * sy;
                }
            }
            *rect = new_rect;
        }
        AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. }
        | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => {
            *rect = new_rect;
        }
        AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. } => { /* no-op v1 */ }
    }
}

/// Move vertex `index` of a point-based Shape payload to `new_point` and
/// recompute `rect` as the points' bounding box (hit-test / selection frame /
/// resize all read `rect`). Returns false -- payload untouched -- for
/// rect-based Shape kinds (Rect/Ellipse), non-Shape payloads, or an
/// out-of-range index.
///
/// Line/Arrow with fewer than 2 points first seeds the endpoints from the
/// rect's TL->BR diagonal (mirroring render's `line_endpoints` fallback) so
/// external OFD that carries only a boundary is still editable. Seeding only
/// happens when `index` is reachable (< 2), so a false return never leaves a
/// partially seeded payload behind.
pub fn move_vertex_payload(p: &mut AnnotationPayload, index: usize, new_point: (f64, f64)) -> bool {
    let AnnotationPayload::Shape {
        kind, rect, points, ..
    } = p
    else {
        return false;
    };
    if !matches!(
        kind,
        ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Polygon | ShapeKind::PolyLine
    ) {
        return false;
    }
    if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) && points.len() < 2 && index < 2 {
        points.push(Point {
            x: rect.x,
            y: rect.y,
        });
        points.push(Point {
            x: rect.x + rect.w,
            y: rect.y + rect.h,
        });
    }
    let Some(pt) = points.get_mut(index) else {
        return false;
    };
    pt.x = new_point.0;
    pt.y = new_point.1;
    let (mut minx, mut miny, mut maxx, mut maxy) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in points.iter() {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    *rect = Rect {
        x: minx,
        y: miny,
        w: maxx - minx,
        h: maxy - miny,
    };
    true
}

fn shift_cmd(cmd: &mut PathCommand, dx: f64, dy: f64) {
    match cmd {
        PathCommand::M(x, y) | PathCommand::L(x, y) => {
            *x += dx;
            *y += dy;
        }
        PathCommand::C(x1, y1, x2, y2, x, y) => {
            *x1 += dx;
            *y1 += dy;
            *x2 += dx;
            *y2 += dy;
            *x += dx;
            *y += dy;
        }
        PathCommand::Q(x1, y1, x, y) => {
            *x1 += dx;
            *y1 += dy;
            *x += dx;
            *y += dy;
        }
        PathCommand::A(_a, _b, _c, _d, x, y) => {
            *x += dx;
            *y += dy;
        }
        PathCommand::Z => {}
    }
}

/// Set the primary color of an annotation (no-op for Stamp which has no color).
pub fn set_color(p: &mut AnnotationPayload, color: Color) {
    match p {
        AnnotationPayload::Markup { color: c, .. } => *c = color,
        AnnotationPayload::Freehand { color: c, .. } => *c = color,
        AnnotationPayload::Shape { stroke: c, .. } => *c = color,
        AnnotationPayload::Note { color: c, .. } => *c = color,
        AnnotationPayload::TextBox { color: c, .. } => *c = color,
        AnnotationPayload::Watermark { color: c, .. } => *c = color,
        AnnotationPayload::Stamp { .. } => { /* no color */ }
    }
}

/// Set the stroke width (Freehand/Shape only; no-op otherwise).
pub fn set_width(p: &mut AnnotationPayload, width: f64) {
    match p {
        AnnotationPayload::Freehand { width: w, .. } => *w = width,
        AnnotationPayload::Shape { width: w, .. } => *w = width,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{Color, PathData, Point, ShapeKind};

    #[test]
    fn move_note_shifts_rect() {
        let mut p = AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 5.0,
                h: 5.0,
            },
            color: Color::Rgb(0, 0, 0),
            content: "".into(),
            icon: rofd_dom::NoteIcon::Note,
        };
        move_payload(&mut p, 3.0, 4.0);
        assert!(matches!(
            p,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 13.0,
                    y: 14.0,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn resize_shape_sets_rect() {
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Rect,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        };
        resize_payload(
            &mut p,
            Rect {
                x: 5.0,
                y: 5.0,
                w: 2.0,
                h: 2.0,
            },
        );
        assert!(matches!(
            p,
            AnnotationPayload::Shape {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    w: 2.0,
                    h: 2.0
                },
                ..
            }
        ));
    }

    #[test]
    fn move_freehand_shifts_path_points() {
        let mut p = AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0)],
            },
            color: Color::Rgb(0, 0, 0),
            width: 1.0,
        };
        move_payload(&mut p, 10.0, 20.0);
        if let AnnotationPayload::Freehand { path, .. } = &p {
            assert!(matches!(path.commands[0], PathCommand::M(11.0, 22.0)));
            assert!(matches!(path.commands[1], PathCommand::L(13.0, 24.0)));
        } else {
            panic!("expected Freehand");
        }
    }

    #[test]
    fn move_shape_translates_points_with_rect() {
        // A Line's `points` must move with `rect` (else the line stays put
        // while its bbox moves -> arrow renders at the wrong place).
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Line,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 50.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![
                rofd_dom::Point { x: 10.0, y: 10.0 },
                rofd_dom::Point { x: 50.0, y: 60.0 },
            ],
        };
        move_payload(&mut p, 3.0, 4.0);
        match p {
            AnnotationPayload::Shape { rect, points, .. } => {
                assert_eq!(rect.x, 13.0);
                assert_eq!(rect.y, 14.0);
                assert_eq!(
                    points,
                    vec![
                        rofd_dom::Point { x: 13.0, y: 14.0 },
                        rofd_dom::Point { x: 53.0, y: 64.0 },
                    ],
                    "points must translate with rect"
                );
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn move_polygon_translates_points() {
        // Polygon `points` must move with `rect` (latent bug: previously only
        // rect moved, leaving the polygon's vertices behind).
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Polygon,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![
                rofd_dom::Point { x: 0.0, y: 0.0 },
                rofd_dom::Point { x: 5.0, y: 10.0 },
                rofd_dom::Point { x: 10.0, y: 0.0 },
            ],
        };
        move_payload(&mut p, 1.0, 2.0);
        if let AnnotationPayload::Shape { points, rect, .. } = &p {
            assert_eq!(rect.x, 1.0);
            assert_eq!(rect.y, 2.0);
            assert_eq!(
                points,
                &vec![
                    rofd_dom::Point { x: 1.0, y: 2.0 },
                    rofd_dom::Point { x: 6.0, y: 12.0 },
                    rofd_dom::Point { x: 11.0, y: 2.0 },
                ]
            );
        }
    }

    #[test]
    fn resize_shape_scales_points_proportionally() {
        // Resizing the bbox must rescale the endpoints proportionally so the
        // line keeps its relative position/direction within the bbox. A Line
        // from rect TL->BR (0,0,100,50) resized to (10,20,200,100) maps the
        // endpoints (0,0)->(10,20) and (100,50)->(210,120).
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Line,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![
                rofd_dom::Point { x: 0.0, y: 0.0 },
                rofd_dom::Point { x: 100.0, y: 50.0 },
            ],
        };
        resize_payload(
            &mut p,
            Rect {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
        );
        if let AnnotationPayload::Shape { rect, points, .. } = &p {
            assert_eq!(
                *rect,
                Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 200.0,
                    h: 100.0
                }
            );
            assert_eq!(
                points,
                &vec![
                    rofd_dom::Point { x: 10.0, y: 20.0 },
                    rofd_dom::Point { x: 210.0, y: 120.0 },
                ],
                "points must rescale from old rect to new rect"
            );
        }
    }

    #[test]
    fn resize_shape_preserves_reverse_direction() {
        // A reverse Line (BR->TL) must keep its direction after resize: the
        // endpoint that was at BR maps to the new BR, the TL endpoint to new TL.
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Arrow,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![
                rofd_dom::Point { x: 100.0, y: 50.0 },
                rofd_dom::Point { x: 0.0, y: 0.0 },
            ],
        };
        resize_payload(
            &mut p,
            Rect {
                x: 10.0,
                y: 20.0,
                w: 200.0,
                h: 100.0,
            },
        );
        if let AnnotationPayload::Shape { points, .. } = &p {
            assert_eq!(
                points,
                &vec![
                    rofd_dom::Point { x: 210.0, y: 120.0 },
                    rofd_dom::Point { x: 10.0, y: 20.0 },
                ],
                "BR->TL direction preserved after resize"
            );
        }
    }

    #[test]
    fn move_vertex_line_updates_points_and_rect() {
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::Line,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 90.0,
                h: 50.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 60.0 }],
        };
        assert!(move_vertex_payload(&mut p, 1, (30.0, 20.0)));
        match &p {
            AnnotationPayload::Shape { rect, points, .. } => {
                assert_eq!(points[1], Point { x: 30.0, y: 20.0 });
                // rect 重算为 points 的 bbox：(10,10)-(30,20)
                assert_eq!(
                    *rect,
                    Rect {
                        x: 10.0,
                        y: 10.0,
                        w: 20.0,
                        h: 10.0
                    }
                );
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn move_vertex_polygon_updates_only_that_vertex() {
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::Polygon,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            stroke: Color::Rgb(0, 0, 255),
            fill: None,
            width: 1.0,
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 5.0, y: 10.0 },
                Point { x: 10.0, y: 0.0 },
            ],
        };
        assert!(move_vertex_payload(&mut p, 1, (5.0, 4.0)));
        match &p {
            AnnotationPayload::Shape { rect, points, .. } => {
                assert_eq!(points[0], Point { x: 0.0, y: 0.0 });
                assert_eq!(points[2], Point { x: 10.0, y: 0.0 });
                assert_eq!(
                    *rect,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 4.0
                    }
                );
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn move_vertex_line_with_empty_points_seeds_diagonal() {
        // 外部 OFD 只带边界无顶点：先播种 rect 对角线（镜像渲染的
        // line_endpoints 回退），再把 Vertex(1) 挪到新位置。
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::Arrow,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: 30.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![],
        };
        assert!(move_vertex_payload(&mut p, 1, (50.0, 10.0)));
        match &p {
            AnnotationPayload::Shape { points, .. } => {
                assert_eq!(points[0], Point { x: 0.0, y: 0.0 }, "seeded from rect TL");
                assert_eq!(points[1], Point { x: 50.0, y: 10.0 });
            }
            _ => panic!("expected Shape"),
        }
    }

    #[test]
    fn move_vertex_rect_kind_is_noop() {
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        };
        assert!(
            !move_vertex_payload(&mut p, 0, (5.0, 5.0)),
            "Rect has no vertices"
        );
    }

    #[test]
    fn move_vertex_out_of_range_is_noop() {
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::PolyLine,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
        };
        let before = p.clone();
        assert!(!move_vertex_payload(&mut p, 2, (5.0, 5.0)));
        assert_eq!(p, before, "payload untouched on bad index");
    }

    #[test]
    fn move_vertex_freehand_is_noop() {
        let mut p = AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)],
            },
            color: Color::Rgb(0, 0, 255),
            width: 1.5,
        };
        assert!(!move_vertex_payload(&mut p, 0, (1.0, 1.0)));
    }

    #[test]
    fn move_vertex_line_unreachable_index_does_not_seed() {
        // Line with empty points + index >= 2: seeding must NOT run (the
        // diagonal seed only applies when the index is reachable), so a
        // false return leaves the payload completely untouched.
        let mut p = AnnotationPayload::Shape {
            kind: ShapeKind::Line,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: 30.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![],
        };
        let before = p.clone();
        assert!(!move_vertex_payload(&mut p, 2, (50.0, 10.0)));
        assert_eq!(p, before, "unreachable index must not seed the diagonal");
        if let AnnotationPayload::Shape { points, .. } = &p {
            assert!(points.is_empty(), "points still empty after no-op");
        }
    }
}
