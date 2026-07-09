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

    #[test]
    fn annotation_round_trips_serde_json() {
        let ann = Annotation {
            id: AnnotationId::new(),
            kind: AnnotationKind::Highlight,
            page: PageId::new("P0"),
            creator: "张三".into(),
            created: 1_700_000_000_000,
            modified: 1_700_000_000_000,
            reply_to: None,
            payload: AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                color: Color::Rgb(255, 255, 0),
            },
        };
        let s = serde_json::to_string(&ann).unwrap();
        let back: Annotation = serde_json::from_str(&s).unwrap();
        assert_eq!(ann, back);
    }
}
