//! Hit-test: convert a viewport pixel point to a HitTarget
//! (Annotation/AnnotationText/Page/Empty).
//!
//! Pure geometry - no vello/parley. The page-origin computation MUST match
//! `composite.rs` (centering + scroll on both axes + page_gap + zoom) so that
//! clicks land where pixels are rendered. Annotations render above the body, so
//! they are tested first (topmost first = reverse doc order).

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationModel, AnnotationPayload, Color, FontId,
    ImageId, NoteIcon, OfdDocument, Page, PageId, PathCommand, PathData, Point, Rect, ShapeKind,
};
use rofd_render::{hit_test, HitTarget, Viewport};

#[path = "../../io/tests/fixtures/fixtures.rs"]
mod fixtures;

/// Build a document with one page of the given physical box and a set of
/// annotations on that page.
fn doc_with_page_and_anns(phys: Rect, anns: Vec<Annotation>) -> OfdDocument {
    let page = Page {
        id: PageId::new("P0"),
        physical_box: phys,
        layers: vec![],
        template: None,
    };
    let mut model = AnnotationModel::default();
    model.by_page.insert(page.id.clone(), anns);
    OfdDocument {
        meta: Default::default(),
        pages: vec![page],
        resources: Default::default(),
        annotations: model,
        max_unit_id: 0,
    }
}

fn ann(_id: &str, page: &PageId, payload: AnnotationPayload, kind: AnnotationKind) -> Annotation {
    Annotation {
        id: AnnotationId::from_int(1),
        kind,
        page: page.clone(),
        creator: "tester".into(),
        created: 0,
        modified: 0,
        reply_to: None,
        payload,
    }
}

// ---------------------------------------------------------------------------
// Brief's test (fixture-based).
// ---------------------------------------------------------------------------

#[test]
fn hit_test_empty_viewport_returns_empty() {
    let bytes = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (800.0, 600.0),
        page_gap: 20.0,
    };
    // Click far from any page -> Empty.
    let target = hit_test(&report.document, &vp, (1.0, 1.0));
    assert!(matches!(target, HitTarget::Empty) || matches!(target, HitTarget::Page(_)));
}

// ---------------------------------------------------------------------------
// Page-origin computation matches composite.rs.
// ---------------------------------------------------------------------------

/// A click in the center of the only page hits that page.
#[test]
fn hit_test_center_of_page_returns_page() {
    // Page 100x100, viewport 200x200, zoom 1, no scroll, gap 20.
    // page_x = ((200-100)/2).max(0) = 50; page_y = 20.
    // Page rect: x[50,150], y[20,120]. Center = (100, 70).
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    let target = hit_test(&doc, &vp, (100.0, 70.0));
    assert_eq!(target, HitTarget::Page(PageId::new("P0")));
}

/// A click outside every page returns Empty.
#[test]
fn hit_test_outside_pages_returns_empty() {
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    // (10,10) is above the page (page starts at y=20) and left of it (x starts
    // at 50).
    assert!(matches!(
        hit_test(&doc, &vp, (10.0, 10.0)),
        HitTarget::Empty
    ));
}

/// scroll.0 shifts pages horizontally - a click that hits the page with scroll
/// must miss without it (and vice versa). This guards the composite alignment.
#[test]
fn hit_test_scroll_x_shifts_page_horizontally() {
    // Page 100x100, viewport 200x200, zoom 1, gap 20.
    // No scroll: page_x = 50, rect x[50,150].
    // scroll.0 = 100: page_origin.x = 50 + 100 = 150, rect x[150,250].
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    let vp_no_scroll = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    let vp_scroll_x = Viewport {
        scroll: (100.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    // Center of the page with scroll applied: x=200, y=70.
    assert_eq!(
        hit_test(&doc, &vp_scroll_x, (200.0, 70.0)),
        HitTarget::Page(PageId::new("P0"))
    );
    // Same point without scroll is off the page (page x is [50,150], 200 > 150).
    assert!(matches!(
        hit_test(&doc, &vp_no_scroll, (200.0, 70.0)),
        HitTarget::Empty
    ));
}

/// scroll.1 shifts pages vertically - matching composite's `y = page_gap -
/// scroll.1`.
#[test]
fn hit_test_scroll_y_shifts_page_vertically() {
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    // scroll.1 = 50 -> page_y = 20 - 50 = -30; rect y[-30, 70]. Center y = 20.
    let vp = Viewport {
        scroll: (0.0, 50.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    // page_x = 50; center = (100, 20).
    assert_eq!(
        hit_test(&doc, &vp, (100.0, 20.0)),
        HitTarget::Page(PageId::new("P0"))
    );
    // Without scroll the page center is at y=70; (100,20) would be above the
    // page (page y starts at 20) - actually on the top edge. Pick a point that
    // is clearly empty without scroll: y=20 is the top edge (inclusive), so use
    // y=10 which is above page[20,120].
    let vp_no_scroll = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    assert!(matches!(
        hit_test(&doc, &vp_no_scroll, (100.0, 10.0)),
        HitTarget::Empty
    ));
}

/// zoom scales the page and its placement.
#[test]
fn hit_test_zoom_scales_page() {
    // Page 100x100, viewport 400x400, zoom 2, gap 20.
    // page_w = 200, page_h = 200; page_x = ((400-200)/2).max(0) = 100.
    // page_y = 20. Rect x[100,300], y[20,220]. Center = (200, 120).
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 2.0,
        size: (400.0, 400.0),
        page_gap: 20.0,
    };
    assert_eq!(
        hit_test(&doc, &vp, (200.0, 120.0)),
        HitTarget::Page(PageId::new("P0"))
    );
}

/// Multiple pages stack vertically with page_gap; the second page is hit at its
/// own center.
#[test]
fn hit_test_second_page_stacked_vertically() {
    let mut doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![],
    );
    doc.pages.push(Page {
        id: PageId::new("P1"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        layers: vec![],
        template: None,
    });
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 600.0),
        page_gap: 20.0,
    };
    // Page 0: y[20,120]. Page 1: y = 120 + 20 = 140 -> y[140,240]. Center y=190.
    assert_eq!(
        hit_test(&doc, &vp, (100.0, 190.0)),
        HitTarget::Page(PageId::new("P1"))
    );
}

// ---------------------------------------------------------------------------
// Annotations (render above body -> tested first, topmost = reverse doc order).
// ---------------------------------------------------------------------------

/// A Markup annotation hit returns Annotation(id); a miss falls through to
/// Page.
#[test]
fn hit_test_markup_annotation_hit_and_miss() {
    let page_id = PageId::new("P0");
    // Markup quad from (10,10) to (20,20) in page-local coords.
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 20.0, y: 20.0 }],
            color: Color::Rgb(255, 255, 0),
        },
        AnnotationKind::Highlight,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![ann],
    );
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    // page_x=50, page_y=20. Local of viewport (60,30) = (10,10) -> inside quad.
    let hit = hit_test(&doc, &vp, (60.0, 30.0));
    assert!(
        matches!(hit, HitTarget::Annotation(_)),
        "expected Annotation hit, got {hit:?}"
    );
    // Just outside the quad (local (25,25)) falls through to Page.
    assert_eq!(
        hit_test(&doc, &vp, (75.0, 45.0)),
        HitTarget::Page(PageId::new("P0"))
    );
}

/// Topmost-first: when two annotations overlap, the later one in doc order
/// wins (reverse iteration).
#[test]
fn hit_test_topmost_annotation_wins() {
    let page_id = PageId::new("P0");
    // Two overlapping markup quads at the same location.
    let ann_first = ann(
        "a1",
        &page_id,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 30.0, y: 30.0 }],
            color: Color::Rgb(255, 0, 0),
        },
        AnnotationKind::Highlight,
    );
    let ann_second = ann(
        "a2",
        &page_id,
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 30.0, y: 30.0 }],
            color: Color::Rgb(0, 255, 0),
        },
        AnnotationKind::Highlight,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![ann_first, ann_second],
    );
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    // Local (20,20) -> viewport (70,40). Should hit ann_second (reverse order).
    let hit = hit_test(&doc, &vp, (70.0, 40.0));
    match hit {
        HitTarget::Annotation(id) => {
            // The second annotation's id should be returned.
            let second_id = doc
                .annotations
                .for_page(&page_id)
                .last()
                .unwrap()
                .id
                .clone();
            assert_eq!(
                id, second_id,
                "topmost (last in doc order) annotation should win"
            );
        }
        other => panic!("expected Annotation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// All annotation payload variants hit-test correctly.
// ---------------------------------------------------------------------------

/// Helper: build a doc with one annotation and click at a page-local point.
/// Computes the viewport offset from the doc's first page so the local->viewport
/// mapping matches composite.rs (page_x = centered, page_y = page_gap).
fn hit_at_local(doc: &OfdDocument, local: (f64, f64)) -> HitTarget {
    let page = &doc.pages[0];
    let page_w = page.physical_box.w; // zoom = 1 in this helper
    let page_x = ((200.0 - page_w) / 2.0).max(0.0);
    let vp = Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 20.0,
    };
    hit_test(doc, &vp, (local.0 + page_x, local.1 + 20.0))
}

#[test]
fn hit_test_shape_annotation_rect_bbox() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Shape {
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
        },
        AnnotationKind::Shape(ShapeKind::Rect),
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![ann],
    );
    // Inside the rect bbox: local (30, 20).
    assert!(matches!(
        hit_at_local(&doc, (30.0, 20.0)),
        HitTarget::Annotation(_)
    ));
    // Outside: local (5, 5).
    assert_eq!(
        hit_at_local(&doc, (5.0, 5.0)),
        HitTarget::Page(PageId::new("P0"))
    );
}

#[test]
fn hit_test_freehand_annotation_bbox() {
    let page_id = PageId::new("P0");
    // Path M(10,10) L(50,50) -> bbox x[10,50], y[10,50].
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![PathCommand::M(10.0, 10.0), PathCommand::L(50.0, 50.0)],
            },
            color: Color::Rgb(0, 0, 255),
            width: 1.5,
        },
        AnnotationKind::Freehand,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![ann],
    );
    // Inside bbox: local (30, 30).
    assert!(matches!(
        hit_at_local(&doc, (30.0, 30.0)),
        HitTarget::Annotation(_)
    ));
    // Outside bbox: local (5, 5).
    assert_eq!(
        hit_at_local(&doc, (5.0, 5.0)),
        HitTarget::Page(PageId::new("P0"))
    );
}

#[test]
fn hit_test_freehand_with_curve_commands() {
    let page_id = PageId::new("P0");
    // Mix of M, C, Q, A, Z to exercise all PathCommand variants in the bbox fold.
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Freehand {
            path: PathData {
                commands: vec![
                    PathCommand::M(10.0, 10.0),
                    PathCommand::C(20.0, 20.0, 30.0, 30.0, 40.0, 40.0),
                    PathCommand::Q(50.0, 50.0, 60.0, 60.0),
                    PathCommand::A(0.0, 0.0, 0.0, 0.0, 70.0, 70.0),
                    PathCommand::Z,
                ],
            },
            color: Color::Rgb(0, 0, 255),
            width: 1.0,
        },
        AnnotationKind::Freehand,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        vec![ann],
    );
    // bbox should be x[10,70], y[10,70]. local (40,40) inside.
    assert!(matches!(
        hit_at_local(&doc, (40.0, 40.0)),
        HitTarget::Annotation(_)
    ));
}

#[test]
fn hit_test_note_annotation_bbox() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Note {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            color: Color::Rgb(255, 200, 0),
            content: "note".into(),
            icon: NoteIcon::Note,
        },
        AnnotationKind::Note,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        vec![ann],
    );
    assert!(matches!(
        hit_at_local(&doc, (30.0, 20.0)),
        HitTarget::Annotation(_)
    ));
}

#[test]
fn hit_test_textbox_annotation_bbox() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 100.0,
                h: 30.0,
            },
            content: "hi".into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
        },
        AnnotationKind::TextBox,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        vec![ann],
    );
    assert!(matches!(
        hit_at_local(&doc, (50.0, 25.0)),
        HitTarget::Annotation(_)
    ));
}

#[test]
fn hit_test_stamp_annotation_bbox() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Stamp {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 50.0,
            },
            image: ImageId::new("I1"),
        },
        AnnotationKind::Stamp,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        vec![ann],
    );
    assert!(matches!(
        hit_at_local(&doc, (30.0, 30.0)),
        HitTarget::Annotation(_)
    ));
}

#[test]
fn hit_test_watermark_annotation_bbox() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Watermark {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 100.0,
            },
            content: "DRAFT".into(),
            opacity: 0.3,
            angle: 0.785,
            font: FontId::new("F2"),
            size: 48.0,
            color: Color::Rgb(200, 200, 200),
        },
        AnnotationKind::Watermark,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        },
        vec![ann],
    );
    assert!(matches!(
        hit_at_local(&doc, (100.0, 50.0)),
        HitTarget::Annotation(_)
    ));
}

/// Markup with multiple quad pairs: any pair hit wins.
#[test]
fn hit_test_markup_multiple_quad_pairs() {
    let page_id = PageId::new("P0");
    let ann = ann(
        "a1",
        &page_id,
        AnnotationPayload::Markup {
            quad_points: vec![
                Point { x: 10.0, y: 10.0 },
                Point { x: 20.0, y: 20.0 },
                Point { x: 50.0, y: 50.0 },
                Point { x: 60.0, y: 60.0 },
            ],
            color: Color::Rgb(255, 255, 0),
        },
        AnnotationKind::Highlight,
    );
    let doc = doc_with_page_and_anns(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        vec![ann],
    );
    // First pair: local (15, 15).
    assert!(matches!(
        hit_at_local(&doc, (15.0, 15.0)),
        HitTarget::Annotation(_)
    ));
    // Second pair: local (55, 55).
    assert!(matches!(
        hit_at_local(&doc, (55.0, 55.0)),
        HitTarget::Annotation(_)
    ));
    // Between pairs: local (35, 35) -> miss -> Page.
    assert_eq!(
        hit_at_local(&doc, (35.0, 35.0)),
        HitTarget::Page(PageId::new("P0"))
    );
}
