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
    Squiggly,
    Freehand,
    Shape(ShapeKind),
    Note,
    TextBox,
    Stamp,
    Watermark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationPayload {
    Markup {
        quad_points: Vec<Point>,
        color: Color,
    },
    Freehand {
        path: PathData,
        color: Color,
        width: f64,
    },
    Shape {
        kind: ShapeKind,
        rect: Rect,
        stroke: Color,
        fill: Option<Color>,
        width: f64,
        points: Vec<Point>,
    },
    Note {
        rect: Rect,
        color: Color,
        content: String,
        icon: NoteIcon,
    },
    TextBox {
        rect: Rect,
        content: String,
        font: FontId,
        size: f64,
        color: Color,
        /// Bordered text box (文本框): stroke color of the frame around the
        /// text. `None` for borderless typewriter-style text. Round-trips the
        /// border PathObject that reference authoring tools emit ahead of the
        /// TextObjects.
        #[serde(default)]
        border: Option<Color>,
        /// Rich appearance runs parsed from the FreeText Appearance: one entry
        /// per styled TextObject/TextCode fragment with its own font/size/
        /// color/bold/italic and pen position (appearance-local mm, relative
        /// to the annotation's Appearance/@Boundary origin). Empty for
        /// rofd-created boxes (flat single-style layout). Rendering prefers
        /// runs; the first text edit collapses them to the flat style.
        #[serde(default)]
        runs: Vec<TextRun>,
    },
    Stamp {
        rect: Rect,
        image: ImageId,
    },
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

/// One styled fragment of a TextBox's FreeText appearance (a run). Producers
/// that support per-fragment styling (bold/italic/color/size within one box)
/// emit one TextObject per fragment on a shared line; the parse flattens the
/// plain text into `content` and keeps the styled fragments here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub font: FontId,
    pub size: f64,
    pub color: Option<Color>,
    /// `Weight >= 600` on the source TextObject.
    pub bold: bool,
    /// `Italic="true"` on the source TextObject.
    pub italic: bool,
    /// Pen x and baseline y, appearance-local mm (relative to the
    /// annotation's Appearance/@Boundary origin).
    pub x: f64,
    pub baseline: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationModel {
    pub by_page: HashMap<PageId, Vec<Annotation>>,
}

impl AnnotationModel {
    pub fn for_page(&self, page: &PageId) -> &[Annotation] {
        self.by_page.get(page).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Find an annotation by id (searches all pages).
    pub fn find(&self, id: &AnnotationId) -> Option<&Annotation> {
        self.by_page.values().flatten().find(|a| &a.id == id)
    }

    /// Find an annotation mutably by id.
    pub fn find_mut(&mut self, id: &AnnotationId) -> Option<&mut Annotation> {
        self.by_page.values_mut().flatten().find(|a| &a.id == id)
    }

    /// Insert an annotation onto its page (ann.page determines the page).
    pub fn insert(&mut self, ann: Annotation) {
        self.by_page.entry(ann.page.clone()).or_default().push(ann);
    }

    /// Remove an annotation by id. Returns the removed annotation, or None if not found.
    pub fn remove(&mut self, id: &AnnotationId) -> Option<Annotation> {
        for anns in self.by_page.values_mut() {
            if let Some(pos) = anns.iter().position(|a| &a.id == id) {
                return Some(anns.remove(pos));
            }
        }
        None
    }
}

/// Line pitch coefficient used by the OFD appearance serializer's legacy
/// (no-layout-hints) path: one `TextObject` per explicit line, stacked at
/// `size * this`. The layout-hint path uses real parley line geometry instead.
pub const TEXTBOX_LINE_PITCH_RATIO: f64 = 1.2;

/// One visual line's layout geometry, handed to the OFD appearance serializer
/// (spec 2026-11-09 §1.5). Coordinates are millimetres relative to the text
/// box origin.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLineHint {
    /// Line text (trailing spaces kept; no line break characters).
    pub text: String,
    /// Line box top (first line is 0).
    pub top: f64,
    /// Baseline offset from the text box origin.
    pub baseline: f64,
    /// Line box height (next line's `top` advances by this).
    pub height: f64,
    /// Line advance (pen position after the last glyph), used as the
    /// `TextObject` Boundary width.
    pub width: f64,
    /// Per-character advances; the serializer writes the first `n - 1` as
    /// `TextCode DeltaX`.
    pub advances: Vec<f64>,
}

/// Per-annotation visual line layout, keyed by annotation id.
pub type TextLayoutHints = HashMap<AnnotationId, Vec<TextLineHint>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{FontId, ImageId};
    use crate::object::{NoteIcon, ShapeKind};
    use crate::primitives::{Color, PathCommand, PathData, Point, Rect};

    fn base_ann(payload: AnnotationPayload, kind: AnnotationKind) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(1),
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
                path: PathData {
                    commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)],
                },
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
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 20.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Rect),
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn squiggly_kind_round_trips_serde() {
        let ann = base_ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 4.0 }],
                color: Color::Rgb(0, 164, 247),
            },
            AnnotationKind::Squiggly,
        );
        let s = serde_json::to_string(&ann).unwrap();
        let back: Annotation = serde_json::from_str(&s).unwrap();
        assert_eq!(ann, back);
    }

    #[test]
    fn shape_polygon_with_points_round_trips() {
        let ann = base_ann(
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
            AnnotationKind::Shape(ShapeKind::Polygon),
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_note_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Note {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 40.0,
                    h: 20.0,
                },
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
                border: None,
                runs: vec![],
            },
            AnnotationKind::TextBox,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn annotation_payload_textbox_runs_round_trips() {
        // Rich appearance runs (per-run style + position, appearance-local mm).
        let ann = base_ann(
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    w: 46.0,
                    h: 10.0,
                },
                content: "发的古典风格".into(),
                font: FontId::new("118"),
                size: 4.2333,
                color: Color::Rgb(13, 13, 13),
                border: Some(Color::Rgb(255, 0, 0)),
                runs: vec![
                    TextRun {
                        text: "发的古".into(),
                        font: FontId::new("118"),
                        size: 4.2333,
                        color: None,
                        bold: false,
                        italic: false,
                        x: 1.1764,
                        baseline: 6.631,
                    },
                    TextRun {
                        text: "格".into(),
                        font: FontId::new("157"),
                        size: 4.2333,
                        color: Some(Color::Rgb(75, 172, 198)),
                        bold: true,
                        italic: false,
                        x: 24.4596,
                        baseline: 6.631,
                    },
                ],
            },
            AnnotationKind::TextBox,
        );
        assert_round_trips(&ann);
    }

    #[test]
    fn textbox_runs_default_empty_for_legacy_json() {
        // Payloads serialized before `runs` existed carry no runs field; the
        // serde default keeps them loadable with runs == [].
        let ann = base_ann(
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                runs: vec![],
            },
            AnnotationKind::TextBox,
        );
        let json = serde_json::to_string(&ann).unwrap();
        let legacy = json.replace(",\"runs\":[]", "");
        assert!(!legacy.contains("runs"), "legacy JSON has no runs field");
        let back: Annotation = serde_json::from_str(&legacy).unwrap();
        match back.payload {
            AnnotationPayload::TextBox { content, runs, .. } => {
                assert_eq!(content, "hi");
                assert!(runs.is_empty(), "missing runs field -> default []");
            }
            other => panic!("expected TextBox, got {other:?}"),
        }
    }

    #[test]
    fn annotation_payload_stamp_round_trips() {
        let ann = base_ann(
            AnnotationPayload::Stamp {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                },
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
            AnnotationKind::Watermark,
        );
        assert_round_trips(&ann);
    }

    fn sample_ann(id: &str, page: &str) -> Annotation {
        Annotation {
            id: AnnotationId::new(id),
            kind: AnnotationKind::Note,
            page: PageId::new(page),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "hi".into(),
                icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn find_returns_annotation_by_id() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("1", "P0");
        m.insert(ann.clone());
        assert_eq!(
            m.find(&ann.id).map(|a| a.id.0.clone()),
            Some(ann.id.0.clone())
        );
    }

    #[test]
    fn find_mut_allows_in_place_edit() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("2", "P0");
        m.insert(ann.clone());
        if let Some(a) = m.find_mut(&ann.id) {
            a.creator = "changed".into();
        }
        assert_eq!(m.find(&ann.id).unwrap().creator, "changed");
    }

    #[test]
    fn insert_places_on_correct_page() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("3", "P5");
        m.insert(ann);
        assert_eq!(m.by_page.get(&PageId::new("P5")).unwrap().len(), 1);
    }

    #[test]
    fn remove_returns_and_deletes() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("4", "P0");
        m.insert(ann.clone());
        let removed = m.remove(&ann.id);
        assert_eq!(removed.map(|a| a.id.0.clone()), Some(ann.id.0.clone()));
        assert!(m.find(&ann.id).is_none());
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut m = AnnotationModel::default();
        let id = AnnotationId::from_int(2);
        assert!(m.remove(&id).is_none());
    }
}
