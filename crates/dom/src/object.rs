use serde::{Deserialize, Serialize};

use crate::ids::{FontId, ImageId, ObjectId};
use crate::primitives::{Color, Ctm, PathData, Rect};

/// Shape kind for Shape annotations and composite primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Arrow,
    Line,
}

/// Sticky-note icon variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NoteIcon {
    #[default]
    Note,
    Comment,
    Help,
    Key,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextCode {
    pub glyph_ids: Vec<u32>,
    /// Per-glyph (dx, dy) deltas. Length == glyph_ids.len() (or text char count when glyph_ids empty).
    pub deltas: Vec<(f32, f32)>,
    /// The TextCode element's text content (e.g. "Hello"). v1: glyph_ids may be empty;
    /// renderers shape this string to obtain glyph IDs.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub font: FontId,
    pub size: f64,
    pub fill: Option<Color>,
    pub codes: Vec<TextCode>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ImageObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub image: ImageId,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PathObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub line_width: f64,
    pub data: PathData,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CompositeObject {
    pub id: ObjectId,
    pub boundary: Rect,
    pub ctm: Option<Ctm>,
    /// Reference to a reusable composite unit (v1: unresolved, stored as-is).
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PageObject {
    Text(TextObject),
    Image(ImageObject),
    Path(PathObject),
    Composite(CompositeObject),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_object_default_and_clone() {
        let t = TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect::default(),
            ctm: None,
            font: FontId::new("F1"),
            size: 12.0,
            fill: None,
            codes: vec![],
        };
        let _clone = t.clone();
        assert_eq!(t.size, 12.0);
    }

    #[test]
    fn page_object_enum_variants() {
        let p = PageObject::Path(PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect::default(),
            ctm: None,
            fill: None,
            stroke: Some(Color::Rgb(0, 0, 0)),
            line_width: 1.0,
            data: PathData::default(),
        });
        assert!(matches!(p, PageObject::Path(_)));
    }

    #[test]
    fn textcode_carries_text() {
        let tc = TextCode { glyph_ids: vec![], deltas: vec![], text: "Hello".into() };
        assert_eq!(tc.text, "Hello");
    }
}
