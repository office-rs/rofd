//! Hit-test: convert a viewport pixel point to a [`HitTarget`].
//!
//! Pure geometry - no vello/parley. The page-origin computation uses
//! [`crate::composite::page_origin`] (the shared page-stacking helper), so a
//! click lands on whatever is rendered at that pixel. Annotations render above
//! the body, so they are tested first; within a page, annotations are tested
//! topmost-first (reverse document order), matching painter's order (later
//! annotations paint over earlier ones).
//!
//! When an annotation is selected, its 8 resize handles (4 corners + 4 edge
//! midpoints) are tested first - before the annotation body - so that a click
//! on a handle returns [`HitTarget::Handle`] rather than [`HitTarget::Annotation`].
//!
//! [`HitTarget::AnnotationText`] is reserved for future caret placement on text
//! annotations; v1 returns [`HitTarget::Annotation`] for all hits.

use rofd_dom::{
    Annotation, AnnotationId, AnnotationPayload, AnnotationSelection, OfdDocument, PageId,
    PathCommand, Rect,
};

use crate::viewport::Viewport;

/// Edge / corner of a selected annotation's bounding rect. Used by both
/// hit-testing (which handle was clicked) and the component (which resize
/// operation to perform).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlePos {
    /// North-west corner (top-left).
    Nw,
    /// North-east corner (top-right).
    Ne,
    /// South-west corner (bottom-left).
    Sw,
    /// South-east corner (bottom-right).
    Se,
    /// North edge midpoint (top-center).
    N,
    /// South edge midpoint (bottom-center).
    S,
    /// East edge midpoint (right-center).
    E,
    /// West edge midpoint (left-center).
    W,
}

/// The topmost thing under a viewport point.
#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    /// An annotation was hit (rendered above the body).
    Annotation(AnnotationId),
    /// A text annotation was hit at this character offset (future: caret
    /// placement). v1 does not compute offsets and returns [`HitTarget::Annotation`]
    /// instead.
    AnnotationText(AnnotationId, usize),
    /// A selection handle on a selected annotation was hit (resize grip).
    Handle(AnnotationId, HandlePos),
    /// The page body (no annotation under the point).
    Page(PageId),
    /// The desk background (no page under the point).
    Empty,
}

/// Handle visual size in screen pixels (not zoom-scaled). Handles stay a
/// constant on-screen size regardless of zoom level.
pub(crate) const HANDLE_SIZE: f64 = 8.0;

/// Extra hit padding around each handle (screen pixels), so handles are
/// grabbable even if the user's click is slightly off.
const HIT_PAD: f64 = 4.0;

/// Hit-test a viewport point (device pixels). Returns the topmost annotation it
/// hits (annotations render above the body), else the page, else [`HitTarget::Empty`].
///
/// If `selection` is [`AnnotationSelection::Single`], the selected annotation's
/// 8 resize handles are tested first - a hit returns
/// [`HitTarget::Handle(id, pos)`]. Otherwise the annotation body / page / empty
/// logic runs as usual.
///
/// The page-origin is computed via [`crate::composite::page_origin`] (the
/// shared page-stacking helper): centering + scroll on both axes + `page_gap`
/// + `zoom`. Page-local coordinates are `(point - page_origin) / zoom`.
pub fn hit_test(
    doc: &OfdDocument,
    vp: &Viewport,
    selection: &AnnotationSelection,
    point: (f64, f64),
) -> HitTarget {
    let (px, py) = point;

    // 1. If an annotation is selected, test its handles first (before any
    //    page/annotation body hit-test). Handles are screen-space (not
    //    page-local), so we compute the selected annotation's viewport-space
    //    bounding rect and check if the point falls on any of the 8 handle
    //    positions.
    if let AnnotationSelection::Single(id) = selection {
        if let Some(ann) = doc.annotations.find(id) {
            if let Some(viewport_rect) = annotation_viewport_rect(doc, ann, vp) {
                if let Some(h) = hit_handle(viewport_rect, point) {
                    return HitTarget::Handle(id.clone(), h);
                }
            }
        }
    }

    let origins = crate::composite::page_origins(doc, vp);
    for (i, page) in doc.pages.iter().enumerate() {
        let Some(&(origin_x, origin_y)) = origins.get(i) else {
            continue;
        };
        let page_w = page.physical_box.w * vp.zoom;
        let page_h = page.physical_box.h * vp.zoom;

        if px < origin_x || px > origin_x + page_w || py < origin_y || py > origin_y + page_h {
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

/// Compute the viewport-space (screen pixel) bounding rect of an annotation.
///
/// Finds the annotation's page in `doc`, then applies the page-origin + zoom
/// transform to the annotation's page-local bounding rect. Returns `None` if
/// the annotation's page is not found or the payload has no geometry (e.g.
/// empty Freehand path).
///
/// The page origin is computed via [`crate::composite::page_origin`] (the
/// shared page-stacking helper), so multi-page docs use the correct stacked Y
/// origin for an annotation on page 1+.
pub(crate) fn annotation_viewport_rect(
    doc: &OfdDocument,
    ann: &Annotation,
    vp: &Viewport,
) -> Option<Rect> {
    // Find the annotation's page index, then use the shared page_origin helper.
    let page_idx = doc.pages.iter().position(|p| p.id == ann.page)?;
    let (origin_x, origin_y) = crate::composite::page_origin(doc, vp, page_idx)?;

    let local = annotation_local_rect(ann)?;
    Some(Rect {
        x: origin_x + local.x * vp.zoom,
        y: origin_y + local.y * vp.zoom,
        w: local.w * vp.zoom,
        h: local.h * vp.zoom,
    })
}

/// Compute the page-local bounding rect of an annotation's payload.
///
/// - Rect-bearing payloads (`Note`/`TextBox`/`Stamp`/`Watermark`/`Shape`):
///   the payload's `rect` field directly.
/// - `Markup`: the bounding box of all quad-point pairs.
/// - `Freehand`: the bounding box of all path control/end points. Returns
///   `None` if the path is empty.
pub fn annotation_local_rect(ann: &Annotation) -> Option<Rect> {
    match &ann.payload {
        AnnotationPayload::Markup { quad_points, .. } => {
            let (minx, miny, maxx, maxy) =
                quad_points
                    .iter()
                    .fold(None::<(f64, f64, f64, f64)>, |acc, p| match acc {
                        None => Some((p.x, p.y, p.x, p.y)),
                        Some((minx, miny, maxx, maxy)) => {
                            Some((minx.min(p.x), miny.min(p.y), maxx.max(p.x), maxy.max(p.y)))
                        }
                    })?;
            Some(Rect {
                x: minx,
                y: miny,
                w: maxx - minx,
                h: maxy - miny,
            })
        }
        AnnotationPayload::Freehand { path, .. } => {
            let (minx, miny, maxx, maxy) =
                path.commands
                    .iter()
                    .fold(None::<(f64, f64, f64, f64)>, |acc, cmd| {
                        let pts = path_points(cmd);
                        pts.into_iter().fold(acc, |a, (px, py)| match a {
                            None => Some((px, py, px, py)),
                            Some((minx, miny, maxx, maxy)) => {
                                Some((minx.min(px), miny.min(py), maxx.max(px), maxy.max(py)))
                            }
                        })
                    })?;
            Some(Rect {
                x: minx,
                y: miny,
                w: maxx - minx,
                h: maxy - miny,
            })
        }
        AnnotationPayload::Shape { rect, .. }
        | AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. }
        | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => Some(*rect),
    }
}

/// Test whether a viewport-space point falls within the hit radius of any of
/// the 8 handles on `rect`. Returns the first hit handle, or `None`.
///
/// Handles are positioned at the 4 corners + 4 edge midpoints of `rect`. The
/// hit radius is `HANDLE_SIZE / 2 + HIT_PAD` (screen pixels, not zoom-scaled).
///
/// Corner handles are tested before edge handles (corners take priority when
/// they overlap with edges at very small rect sizes).
fn hit_handle(rect: Rect, point: (f64, f64)) -> Option<HandlePos> {
    let r = HANDLE_SIZE / 2.0 + HIT_PAD;
    let (px, py) = point;
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w;
    let y1 = rect.y + rect.h;
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;

    // 4 corners first (priority over edges).
    let corners = [
        (HandlePos::Nw, x0, y0),
        (HandlePos::Ne, x1, y0),
        (HandlePos::Sw, x0, y1),
        (HandlePos::Se, x1, y1),
    ];
    for (pos, hx, hy) in &corners {
        if (px - hx).abs() <= r && (py - hy).abs() <= r {
            return Some(*pos);
        }
    }

    // 4 edge midpoints.
    let edges = [
        (HandlePos::N, cx, y0),
        (HandlePos::S, cx, y1),
        (HandlePos::E, x1, cy),
        (HandlePos::W, x0, cy),
    ];
    for (pos, hx, hy) in &edges {
        if (px - hx).abs() <= r && (py - hy).abs() <= r {
            return Some(*pos);
        }
    }

    None
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
        AnnotationPayload::Markup { quad_points, .. } => quad_points.chunks(2).any(|chunk| {
            if chunk.len() < 2 {
                return false;
            }
            let (p0, p1) = (chunk[0], chunk[1]);
            x >= p0.x.min(p1.x) && x <= p0.x.max(p1.x) && y >= p0.y.min(p1.y) && y <= p0.y.max(p1.y)
        }),
        AnnotationPayload::Freehand { path, .. } => {
            // v1: bounding-box test on the path's control/end points (coarse).
            let bbox = path
                .commands
                .iter()
                .fold(None::<(f64, f64, f64, f64)>, |acc, cmd| {
                    let pts = path_points(cmd);
                    pts.into_iter().fold(acc, |a, (px, py)| match a {
                        None => Some((px, py, px, py)),
                        Some((minx, miny, maxx, maxy)) => {
                            Some((minx.min(px), miny.min(py), maxx.max(px), maxy.max(py)))
                        }
                    })
                });
            match bbox {
                Some((minx, miny, maxx, maxy)) => x >= minx && x <= maxx && y >= miny && y <= maxy,
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
    use rofd_dom::{AnnotationKind, Color, NoteIcon, PathData, Point, Rect, ShapeKind};

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
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            stroke: Color::Rgb(0, 0, 0),
            fill: Some(Color::Rgb(255, 255, 255)),
            width: 2.0,
            points: vec![],
        });
        assert!(hit_annotation(&a, (30.0, 20.0)));
        assert!(!hit_annotation(&a, (9.0, 20.0)));
    }

    #[test]
    fn note_hit_inside_rect() {
        let a = ann(AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            color: Color::Rgb(255, 200, 0),
            content: "n".into(),
            icon: NoteIcon::Note,
        });
        assert!(hit_annotation(&a, (50.0, 30.0)));
    }

    #[test]
    fn freehand_empty_path_never_hits() {
        let a = ann(AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::Z],
            },
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

    #[test]
    fn hit_test_handle_when_point_on_selected_corner() {
        // Select a rect annotation, point on its NW corner handle -> Handle(id, Nw).
        //
        // Annotation rect: page-local (10,10) w=80 h=60 -> corners:
        //   NW=(10,10), NE=(90,10), SW=(10,70), SE=(90,70).
        // Viewport: 200x200, zoom 1, page 100x100, gap 20.
        //   page_x = ((200-100)/2).max(0) = 50; page_y = 20.
        //   NW corner in viewport = (50+10, 20+10) = (60, 30).
        // Handle center is at the corner; hit radius = HANDLE_SIZE/2 + HIT_PAD
        //   = 4 + 4 = 8. Point (60, 30) is exactly on the corner -> hit.
        use rofd_dom::{AnnotationModel, AnnotationSelection, OfdDocument, Page};

        let page_id = PageId::new("P0");
        let annotation = Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Shape(ShapeKind::Rect),
            page: page_id.clone(),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 80.0,
                    h: 60.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
        };
        let page = Page {
            id: page_id.clone(),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            layers: vec![],
            template: None,
        };
        let mut model = AnnotationModel::default();
        model.insert(annotation.clone());
        let doc = OfdDocument {
            meta: Default::default(),
            pages: vec![page],
            resources: Default::default(),
            annotations: model,
            max_unit_id: 0,
        };
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let selection = AnnotationSelection::Single(annotation.id.clone());

        // Point on the NW corner handle.
        let target = hit_test(&doc, &vp, &selection, (60.0, 30.0));
        assert_eq!(
            target,
            HitTarget::Handle(annotation.id.clone(), HandlePos::Nw),
            "NW corner handle should be hit"
        );

        // Point on the SE corner: viewport (50+90, 20+70) = (140, 90).
        let target = hit_test(&doc, &vp, &selection, (140.0, 90.0));
        assert_eq!(
            target,
            HitTarget::Handle(annotation.id.clone(), HandlePos::Se),
            "SE corner handle should be hit"
        );
    }

    #[test]
    fn hit_test_no_handle_when_not_selected() {
        // Same annotation, but selection = None -> clicking on the corner falls
        // through to the annotation body (not a handle).
        use rofd_dom::{AnnotationModel, AnnotationSelection, OfdDocument, Page};

        let page_id = PageId::new("P0");
        let annotation = Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Shape(ShapeKind::Rect),
            page: page_id.clone(),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 80.0,
                    h: 60.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
        };
        let page = Page {
            id: page_id.clone(),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            layers: vec![],
            template: None,
        };
        let mut model = AnnotationModel::default();
        model.insert(annotation);
        let doc = OfdDocument {
            meta: Default::default(),
            pages: vec![page],
            resources: Default::default(),
            annotations: model,
            max_unit_id: 0,
        };
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };

        // Point on the NW corner, but no selection -> Annotation, not Handle.
        let target = hit_test(&doc, &vp, &AnnotationSelection::None, (60.0, 30.0));
        assert!(
            matches!(target, HitTarget::Annotation(_)),
            "without selection, corner falls through to annotation body, got {target:?}"
        );
    }

    #[test]
    fn hit_test_handle_on_page_1_uses_correct_y_origin() {
        // Regression: annotation_viewport_rect and the handle hit-test must
        // use the annotation's page's stacked Y origin, NOT page 0's.
        //
        // Two pages, each 100x100, gap 20, zoom 1, viewport 200x400.
        //   page_x = ((200-100)/2).max(0) = 50.
        //   Page 0 Y origin = 20 (page_gap - scroll.1).
        //   Page 1 Y origin = 20 + 100 + 20 = 140.
        // Annotation on page 1: rect (10,10) w=80 h=60.
        //   NW corner viewport = (50+10, 140+10) = (60, 150).
        //   SE corner viewport = (50+90, 140+70) = (140, 210).
        //
        // Before the fix, annotation_viewport_rect hardcoded page 0's Y (20),
        // so the handle was tested at (60, 30) instead of (60, 150) - a click
        // at (60, 150) would miss and fall through to the page body.
        use rofd_dom::{AnnotationModel, AnnotationSelection, OfdDocument, Page};

        let page0_id = PageId::new("P0");
        let page1_id = PageId::new("P1");
        let annotation = Annotation {
            id: AnnotationId::from_int(1),
            kind: AnnotationKind::Shape(ShapeKind::Rect),
            page: page1_id.clone(),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 80.0,
                    h: 60.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
        };
        let mk_page = |id: PageId| Page {
            id,
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            layers: vec![],
            template: None,
        };
        let mut model = AnnotationModel::default();
        model.insert(annotation.clone());
        let doc = OfdDocument {
            meta: Default::default(),
            pages: vec![mk_page(page0_id), mk_page(page1_id)],
            resources: Default::default(),
            annotations: model,
            max_unit_id: 0,
        };
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 400.0),
            page_gap: 20.0,
        };
        let selection = AnnotationSelection::Single(annotation.id.clone());

        // Point on the NW corner handle of page 1's annotation.
        // Correct Y origin (140) -> (60, 150). Buggy Y origin (20) -> (60, 30).
        let target = hit_test(&doc, &vp, &selection, (60.0, 150.0));
        assert_eq!(
            target,
            HitTarget::Handle(annotation.id.clone(), HandlePos::Nw),
            "page 1 NW corner handle should be hit at its stacked Y origin (60, 150), got {target:?}"
        );

        // Point on the SE corner handle of page 1's annotation.
        // Correct Y origin (140) -> (140, 210). Buggy Y origin (20) -> (140, 90).
        let target = hit_test(&doc, &vp, &selection, (140.0, 210.0));
        assert_eq!(
            target,
            HitTarget::Handle(annotation.id.clone(), HandlePos::Se),
            "page 1 SE corner handle should be hit at its stacked Y origin (140, 210), got {target:?}"
        );

        // Sanity: clicking at the buggy position (60, 30) should NOT hit the
        // page 1 annotation's handle (it's on page 0's area, which has no
        // annotation). Before the fix this would erroneously return Handle.
        let target = hit_test(&doc, &vp, &selection, (60.0, 30.0));
        assert_ne!(
            target,
            HitTarget::Handle(annotation.id.clone(), HandlePos::Nw),
            "page 1 handle should NOT be hit at page 0's Y origin (60, 30)"
        );

        // Direct unit-level check: annotation_viewport_rect returns the rect
        // at page 1's Y origin, not page 0's.
        let vr = annotation_viewport_rect(&doc, &annotation, &vp)
            .expect("annotation_viewport_rect should find the annotation on page 1");
        assert_eq!(
            vr,
            Rect {
                x: 60.0,
                y: 150.0,
                w: 80.0,
                h: 60.0
            },
            "annotation_viewport_rect should use page 1's stacked Y origin (150), not page 0's (30)"
        );
    }
}
