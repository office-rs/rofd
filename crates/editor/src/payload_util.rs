use rofd_dom::{AnnotationPayload, PathCommand, Rect};

/// Shift an annotation's geometry by (dx, dy).
pub fn move_payload(p: &mut AnnotationPayload, dx: f64, dy: f64) {
    match p {
        AnnotationPayload::Markup { quad_points, .. } => {
            for pt in quad_points { pt.x += dx; pt.y += dy; }
        }
        AnnotationPayload::Freehand { path, .. } => {
            for cmd in &mut path.commands { shift_cmd(cmd, dx, dy); }
        }
        AnnotationPayload::Shape { rect, .. } | AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. } | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => {
            rect.x += dx; rect.y += dy;
        }
    }
}

/// Set the rect (for resize). No-op for Markup/Freehand (no single rect).
pub fn resize_payload(p: &mut AnnotationPayload, new_rect: Rect) {
    match p {
        AnnotationPayload::Shape { rect, .. } | AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. } | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => { *rect = new_rect; }
        AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. } => { /* no-op v1 */ }
    }
}

fn shift_cmd(cmd: &mut PathCommand, dx: f64, dy: f64) {
    match cmd {
        PathCommand::M(x, y) | PathCommand::L(x, y) => { *x += dx; *y += dy; }
        PathCommand::C(x1, y1, x2, y2, x, y) => { *x1 += dx; *y1 += dy; *x2 += dx; *y2 += dy; *x += dx; *y += dy; }
        PathCommand::Q(x1, y1, x, y) => { *x1 += dx; *y1 += dy; *x += dx; *y += dy; }
        PathCommand::A(_a, _b, _c, _d, x, y) => { *x += dx; *y += dy; }
        PathCommand::Z => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{Color, PathData};

    #[test]
    fn move_note_shifts_rect() {
        let mut p = AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 5.0, h: 5.0 }, color: Color::Rgb(0,0,0),
            content: "".into(), icon: rofd_dom::NoteIcon::Note,
        };
        move_payload(&mut p, 3.0, 4.0);
        assert!(matches!(p, AnnotationPayload::Note { rect: Rect { x: 13.0, y: 14.0, .. }, .. }));
    }

    #[test]
    fn resize_shape_sets_rect() {
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Rect, rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            stroke: Color::Rgb(0,0,0), fill: None, width: 1.0,
        };
        resize_payload(&mut p, Rect { x: 5.0, y: 5.0, w: 2.0, h: 2.0 });
        assert!(matches!(p, AnnotationPayload::Shape { rect: Rect { x: 5.0, y: 5.0, w: 2.0, h: 2.0 }, .. }));
    }

    #[test]
    fn move_freehand_shifts_path_points() {
        let mut p = AnnotationPayload::Freehand {
            path: PathData { commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0)] },
            color: Color::Rgb(0,0,0), width: 1.0,
        };
        move_payload(&mut p, 10.0, 20.0);
        if let AnnotationPayload::Freehand { path, .. } = &p {
            assert!(matches!(path.commands[0], PathCommand::M(11.0, 22.0)));
            assert!(matches!(path.commands[1], PathCommand::L(13.0, 24.0)));
        } else { panic!("expected Freehand"); }
    }
}
