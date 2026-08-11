use rofd_dom::{AnnotationPayload, Color, PathCommand, Rect};

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
    use rofd_dom::{Color, PathData};

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
}
