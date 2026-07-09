use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ids::{AnnotationId, FontId, ImageId, PageId};
use crate::object::{NoteIcon, ShapeKind};
use crate::primitives::{Color, PathData, Point, Rect};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationKind {
    Highlight,
    Underline,
    Strikeout,
    Freehand,
    Shape(ShapeKind),
    Note,
    TextBox,
    Stamp,
    Watermark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationPayload {
    Markup { quad_points: Vec<Point>, color: Color },
    Freehand { path: PathData, color: Color, width: f64 },
    Shape { kind: ShapeKind, rect: Rect, stroke: Color, fill: Option<Color>, width: f64 },
    Note { rect: Rect, color: Color, content: String, icon: NoteIcon },
    TextBox { rect: Rect, content: String, font: FontId, size: f64, color: Color },
    Stamp { rect: Rect, image: ImageId },
    Watermark {
        rect: Rect,
        content: String,
        opacity: f64,
        angle: f64,
        font: FontId,
        size: f64,
        color: Color,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
    pub page: PageId,
    pub creator: String,
    pub created: i64,
    pub modified: i64,
    pub reply_to: Option<AnnotationId>,
    pub payload: AnnotationPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationModel {
    pub by_page: HashMap<PageId, Vec<Annotation>>,
}

impl AnnotationModel {
    pub fn for_page(&self, page: &PageId) -> &[Annotation] {
        self.by_page.get(page).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{FontId, ImageId};
    use crate::object::{NoteIcon, ShapeKind};
    use crate::primitives::{Color, PathData, PathCommand, Point, Rect};

    fn base_ann(payload: AnnotationPayload, kind: AnnotationKind) -> Annotation {
        Annotation {
            id: AnnotationId::new(),
            kind,
            page: PageId::new("P0"),
            creator: "张三".into(),
            created: 1_700_000_000_000,
            modified: 1_700_000_000_000,
            reply_to: None,
            payload,
        }
    }

    fn assert_round_trips(ann: &Annotation) {
        let s = serde_json::to_string(ann).unwrap();
        let back: Annotation = serde_json::from_str(&s).unwrap();
        assert_eq!(ann, &back);
    }

    #[test]
    fn annotation_round_trips_serde_json() {
        let ann = base_ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                color: Color::Rgb(255, 255, 0),
            },
            AnnotationKind::Highlight,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_markup_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 1.0, y: 2.0 }, Point { x: 3.0, y: 4.0 }],
                color: Color::Rgb(255, 0, 0),
            },
            AnnotationKind::Strikeout,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_freehand_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Freehand {
                path: PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)] },
                color: Color::Rgb(0, 0, 255),
                width: 1.5,
            },
            AnnotationKind::Freehand,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_shape_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
            },
            AnnotationKind::Shape(ShapeKind::Rect),
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_note_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Note {
                rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
                color: Color::Rgb(255, 200, 0),
                content: "a note".into(),
                icon: NoteIcon::Help,
            },
            AnnotationKind::Note,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_textbox_round_trips() {
        let ann = base_ann(
            AnnotationPayload::TextBox {
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 30.0 },
                content: "hello".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
            },
            AnnotationKind::TextBox,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_stamp_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Stamp {
                rect: Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 },
                image: ImageId::new("Img_0"),
            },
            AnnotationKind::Stamp,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_watermark_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Watermark {
                rect: Rect { x: 0.0, y: 0.0, w: 200.0, h: 100.0 },
                content: "DRAFT".into(),
                opacity: 0.3,
                angle: 45.0,
                font: FontId::new("F2"),
                size: 48.0,
                color: Color::Rgb(200, 200, 200),
            },
            AnnotationKind::Watermark,
        );
        assert_round_trips(&ann);
    }
}
