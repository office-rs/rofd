//! Hit-test: convert a viewport pixel point to a [`HitTarget`].
//!
//! Pure geometry - no vello/parley. The page-origin computation mirrors
//! [`crate::composite::RenderEngine::composite`] exactly (centering + scroll on
//! both axes + `page_gap` + `zoom`), so that a click lands on whatever is
//! rendered at that pixel. Annotations render above the body, so they are
//! tested first; within a page, annotations are tested topmost-first (reverse
//! document order), matching painter's order (later annotations paint over
//! earlier ones).
//!
//! [`HitTarget::AnnotationText`] is reserved for future caret placement on text
//! annotations; v1 returns [`HitTarget::Annotation`] for all hits.

use rofd_dom::{Annotation, AnnotationId, AnnotationPayload, OfdDocument, PageId, PathCommand};

use crate::viewport::Viewport;

/// The topmost thing under a viewport point.
#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    /// An annotation was hit (rendered above the body).
    Annotation(AnnotationId),
    /// A text annotation was hit at this character offset (future: caret
    /// placement). v1 does not compute offsets and returns [`Annotation`]
    /// instead.
    AnnotationText(AnnotationId, usize),
    /// The page body (no annotation under the point).
    Page(PageId),
    /// The desk background (no page under the point).
    Empty,
}

/// Hit-test a viewport point (device pixels). Returns the topmost annotation it
/// hits (annotations render above the body), else the page, else [`HitTarget::Empty`].
///
/// The page-origin computation matches `composite.rs`:
/// - `page_x = ((vp.size.0 - page_w) / 2.0).max(0.0)` (centered, never negative)
/// - `page_origin = (page_x + vp.scroll.0, y)` where `y` starts at
///   `vp.page_gap - vp.scroll.1` and advances by `page_h + vp.page_gap`
/// - `page_w = physical_box.w * zoom`, `page_h = physical_box.h * zoom`
///
/// Page-local coordinates are `(point - page_origin) / zoom`.
pub fn hit_test(doc: &OfdDocument, vp: &Viewport, point: (f64, f64)) -> HitTarget {
    let (px, py) = point;
    let mut y = vp.page_gap - vp.scroll.1;
    for page in &doc.pages {
        let page_w = page.physical_box.w * vp.zoom;
        let page_h = page.physical_box.h * vp.zoom;
        let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0);
        let origin_x = page_x + vp.scroll.0;
        let origin_y = y;

        if px < origin_x || px > origin_x + page_w || py < origin_y || py > origin_y + page_h {
            y += page_h + vp.page_gap;
            continue;
        }

        // Convert to page-local coords (undo origin + zoom).
        let local = ((px - origin_x) / vp.zoom, (py - origin_y) / vp.zoom);

        // Annotations render above the body and are painted in doc order, so
        // later annotations are on top -> iterate reverse for topmost-first.
        let anns = doc.annotations.for_page(&page.id);
        for ann in anns.iter().rev() {
            if hit_annotation(ann, local) {
                return HitTarget::Annotation(ann.id.clone());
            }
        }
        return HitTarget::Page(page.id.clone());
    }
    HitTarget::Empty
}

/// Test whether a page-local point falls inside an annotation's hit region.
///
/// - Rect-bearing payloads (`Note`/`TextBox`/`Stamp`/`Watermark`/`Shape`):
///   inside the rect's origin+dimensions box.
/// - `Markup`: inside any quad-point pair's bounding box.
/// - `Freehand`: inside the bounding box of the path's control/end points
///   (coarse v1 test; a true curve-distance test is deferred).
fn hit_annotation(ann: &Annotation, local: (f64, f64)) -> bool {
    let (x, y) = local;
    match &ann.payload {
        AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. }
        | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. }
        | AnnotationPayload::Shape { rect, .. } => {
            x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h
        }
        AnnotationPayload::Markup { quad_points, .. } => quad_points
            .chunks(2)
            .any(|chunk| {
                if chunk.len() < 2 {
                    return false;
                }
                let (p0, p1) = (chunk[0], chunk[1]);
                x >= p0.x.min(p1.x)
                    && x <= p0.x.max(p1.x)
                    && y >= p0.y.min(p1.y)
                    && y <= p0.y.max(p1.y)
            }),
        AnnotationPayload::Freehand { path, .. } => {
            // v1: bounding-box test on the path's control/end points (coarse).
            let bbox = path.commands.iter().fold(
                None::<(f64, f64, f64, f64)>,
                |acc, cmd| {
                    let pts = path_points(cmd);
                    pts.into_iter().fold(acc, |a, (px, py)| match a {
                        None => Some((px, py, px, py)),
                        Some((minx, miny, maxx, maxy)) => {
                            Some((minx.min(px), miny.min(py), maxx.max(px), maxy.max(py)))
                        }
                    })
                },
            );
            match bbox {
                Some((minx, miny, maxx, maxy)) => {
                    x >= minx && x <= maxx && y >= miny && y <= maxy
                }
                None => false,
            }
        }
    }
}

/// The (x, y) points contributing to a path command's bounding box: the
/// endpoint for M/L/A, plus control points for C/Q. `Z` contributes nothing.
fn path_points(cmd: &PathCommand) -> Vec<(f64, f64)> {
    match cmd {
        PathCommand::M(x, y) | PathCommand::L(x, y) => vec![(*x, *y)],
        // C: (x1,y1), (x2,y2), (x,y) - all influence the curve's extent.
        PathCommand::C(x1, y1, x2, y2, x, y) => vec![(*x1, *y1), (*x2, *y2), (*x, *y)],
        // Q: (x1,y1), (x,y).
        PathCommand::Q(x1, y1, x, y) => vec![(*x1, *y1), (*x, *y)],
        // A: arc command. The dom models `A(f64; 6)`; the final pair is treated
        // as the endpoint (x, y) for bbox purposes.
        PathCommand::A(_a, _b, _c, _d, x, y) => vec![(*x, *y)],
        PathCommand::Z => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        AnnotationKind, Color, NoteIcon, PathData, Point, Rect, ShapeKind,
    };

    fn ann(payload: AnnotationPayload) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Highlight,
            page: PageId::new("P0"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload,
        }
    }

    #[test]
    fn markup_hit_inside_quad_pair() {
        let a = ann(AnnotationPayload::Markup {
            quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
            color: Color::Rgb(255, 255, 0),
        });
        assert!(hit_annotation(&a, (5.0, 5.0)));
        assert!(!hit_annotation(&a, (11.0, 11.0)));
    }

    #[test]
    fn markup_single_quad_point_never_hits() {
        let a = ann(AnnotationPayload::Markup {
            quad_points: vec![Point { x: 0.0, y: 0.0 }],
            color: Color::Rgb(255, 0, 0),
        });
        assert!(!hit_annotation(&a, (0.0, 0.0)));
    }

    #[test]
    fn shape_hit_inside_rect() {
        let a = ann(AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
            stroke: Color::Rgb(0, 0, 0),
            fill: Some(Color::Rgb(255, 255, 255)),
            width: 2.0,
        });
        assert!(hit_annotation(&a, (30.0, 20.0)));
        assert!(!hit_annotation(&a, (9.0, 20.0)));
    }

    #[test]
    fn note_hit_inside_rect() {
        let a = ann(AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
            color: Color::Rgb(255, 200, 0),
            content: "n".into(),
            icon: NoteIcon::Note,
        });
        assert!(hit_annotation(&a, (50.0, 30.0)));
    }

    #[test]
    fn freehand_empty_path_never_hits() {
        let a = ann(AnnotationPayload::Freehand {
            path: PathData { commands: vec![PathCommand::Z] },
            color: Color::Rgb(0, 0, 255),
            width: 1.0,
        });
        assert!(!hit_annotation(&a, (0.0, 0.0)));
    }

    #[test]
    fn freehand_bbox_covers_endpoints() {
        let a = ann(AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(10.0, 10.0), PathCommand::L(50.0, 50.0)],
            },
            color: Color::Rgb(0, 0, 255),
            width: 1.0,
        });
        assert!(hit_annotation(&a, (30.0, 30.0)));
        assert!(!hit_annotation(&a, (5.0, 5.0)));
    }

    #[test]
    fn hit_target_eq_and_debug() {
        // PartialEq + Debug are part of the public surface (tests assert on
        // them); exercise them to guard derive regressions.
        let a = HitTarget::Page(PageId::new("P0"));
        let b = HitTarget::Page(PageId::new("P0"));
        assert_eq!(a, b);
        assert_ne!(a, HitTarget::Empty);
        let s = format!("{a:?}");
        assert!(s.contains("Page"));
    }
}
