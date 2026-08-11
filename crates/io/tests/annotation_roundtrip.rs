//! Inverse round-trip tests: serialize -> parse == identity, for all 7
//! payload kinds. This is the core contract of Task 7 (GB/T 33190 §15.2
//! annotation serialization fidelity).

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
    PageId, PathCommand, PathData, Point, Rect, ShapeKind,
};

fn ann(
    id: u64,
    kind: AnnotationKind,
    payload: AnnotationPayload,
    reply_to: Option<u64>,
) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(id),
        kind,
        page: PageId::new("1"),
        creator: "flw".into(),
        created: 1_783_656_237_000,
        modified: 1_783_656_237_000,
        reply_to: reply_to.map(AnnotationId::from_int),
        payload,
    }
}

fn roundtrip(a: &Annotation) -> Annotation {
    let xml =
        rofd_io::serialize::annotation::serialize_page_annot(&a.page, std::slice::from_ref(a));
    let parsed = rofd_io::parse::annotation::parse_page_annot(&xml, &a.page).unwrap();
    parsed.into_iter().next().expect("one annot")
}

#[test]
fn markup_highlight_roundtrips() {
    let a = ann(
        1,
        AnnotationKind::Highlight,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 20.0 }],
            color: Color::Rgb(255, 255, 0),
        },
        None,
    );
    let b = roundtrip(&a);
    assert_eq!(a, b);
}

#[test]
fn underline_roundtrips() {
    let a = ann(
        2,
        AnnotationKind::Underline,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 38.0, y: 4.4 }],
            color: Color::Rgb(0, 239, 89),
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn freehand_roundtrips() {
    let a = ann(
        3,
        AnnotationKind::Freehand,
        AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)],
            },
            color: Color::Rgb(0, 0, 255),
            width: 1.5,
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn shape_rect_roundtrips() {
    let a = ann(
        4,
        AnnotationKind::Shape(ShapeKind::Rect),
        AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: Some(Color::Rgb(255, 255, 255)),
            width: 2.0,
            points: vec![],
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn note_roundtrips_with_reply_to() {
    let parent = ann(
        5,
        AnnotationKind::Note,
        AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            color: Color::Rgb(255, 200, 0),
            content: "parent".into(),
            icon: NoteIcon::Note,
        },
        None,
    );
    let reply = ann(
        6,
        AnnotationKind::Note,
        AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 40.0,
                w: 40.0,
                h: 20.0,
            },
            color: Color::Rgb(255, 200, 0),
            content: "reply".into(),
            icon: NoteIcon::Note,
        },
        Some(5),
    );
    assert_eq!(parent, roundtrip(&parent));
    let r = roundtrip(&reply);
    assert_eq!(reply, r);
    assert_eq!(r.reply_to, Some(AnnotationId::from_int(5)));
}

#[test]
fn textbox_roundtrips() {
    let tb = ann(
        7,
        AnnotationKind::TextBox,
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 30.0,
            },
            content: "hello".into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
        },
        None,
    );
    assert_eq!(tb, roundtrip(&tb));
}

#[test]
fn stamp_roundtrips() {
    let st = ann(
        8,
        AnnotationKind::Stamp,
        AnnotationPayload::Stamp {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            },
            image: ImageId::new("9905"),
        },
        None,
    );
    assert_eq!(st, roundtrip(&st));
}

#[test]
fn watermark_roundtrips() {
    let wm = ann(
        9,
        AnnotationKind::Watermark,
        AnnotationPayload::Watermark {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 100.0,
            },
            content: "DRAFT".into(),
            opacity: 0.3,
            angle: 45.0,
            font: FontId::new("F2"),
            size: 48.0,
            color: Color::Rgb(200, 200, 200),
        },
        None,
    );
    assert_eq!(wm, roundtrip(&wm));
}

// --- C1.5 Task 5: inverse round-trip for the remaining payload kinds ---
// (FreeText/TextBox, Squiggly, Polygon, PolyLine). These complete the
// coverage of the §15.2 serialization fidelity contract started above.

#[test]
fn freetext_textbox_roundtrips() {
    let a = ann(
        20,
        AnnotationKind::TextBox,
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 10.0,
            },
            content: "文字批注".into(),
            font: FontId::new("F1"),
            size: 5.0,
            color: Color::Rgb(13, 13, 13),
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn squiggly_roundtrips() {
    let a = ann(
        21,
        AnnotationKind::Squiggly,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 14.0 }],
            color: Color::Rgb(0, 164, 247),
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn polygon_roundtrips() {
    let a = ann(
        22,
        AnnotationKind::Shape(ShapeKind::Polygon),
        AnnotationPayload::Shape {
            kind: ShapeKind::Polygon,
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
                Point { x: 0.0, y: 0.0 },
                Point { x: 5.0, y: 10.0 },
                Point { x: 10.0, y: 0.0 },
            ],
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn polyline_roundtrips() {
    let a = ann(
        23,
        AnnotationKind::Shape(ShapeKind::PolyLine),
        AnnotationPayload::Shape {
            kind: ShapeKind::PolyLine,
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
                Point { x: 0.0, y: 0.0 },
                Point { x: 5.0, y: 10.0 },
                Point { x: 10.0, y: 0.0 },
            ],
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn line_roundtrips_with_direction() {
    // A Line's endpoints (including anti-diagonal direction TR -> BL) must
    // survive serialize -> parse so the rendered line keeps the drawn
    // diagonal after save/reload. The bbox `rect` alone would lose this.
    let a = ann(
        30,
        AnnotationKind::Shape(ShapeKind::Line),
        AnnotationPayload::Shape {
            kind: ShapeKind::Line,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![Point { x: 100.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }],
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

#[test]
fn arrow_roundtrips_with_direction() {
    // An Arrow's direction (BR -> TL, arrowhead at TL) must survive
    // serialize -> parse. The `Vertices` Parameter carries the endpoints.
    let a = ann(
        31,
        AnnotationKind::Shape(ShapeKind::Arrow),
        AnnotationPayload::Shape {
            kind: ShapeKind::Arrow,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 2.0,
            points: vec![Point { x: 100.0, y: 50.0 }, Point { x: 0.0, y: 0.0 }],
        },
        None,
    );
    assert_eq!(a, roundtrip(&a));
}

/// Multiple annotations on one page serialize/parse together.
#[test]
fn multiple_annots_roundtrip_together() {
    let a1 = ann(
        10,
        AnnotationKind::Highlight,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
            color: Color::Rgb(255, 255, 0),
        },
        None,
    );
    let a2 = ann(
        11,
        AnnotationKind::Freehand,
        AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0)],
            },
            color: Color::Rgb(0, 0, 0),
            width: 1.0,
        },
        None,
    );
    let xml =
        rofd_io::serialize::annotation::serialize_page_annot(&a1.page, &[a1.clone(), a2.clone()]);
    let parsed = rofd_io::parse::annotation::parse_page_annot(&xml, &a1.page).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0], a1);
    assert_eq!(parsed[1], a2);
}
