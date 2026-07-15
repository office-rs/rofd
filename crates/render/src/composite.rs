//! Render engine: composite a paper-on-desk scene from a document + viewport.
//!
//! [`RenderEngine::composite`] builds the top-level [`imaging::record::Scene`] the
//! host paints: a gray "desk" background filling the viewport, white pages centered
//! horizontally and stacked vertically with a gap, and each page's body + annotation
//! content drawn directly with the page-origin + zoom transform baked into every
//! draw call.
//!
//! # Why imaging (not vello::Scene)
//!
//! Masonry (the widget toolkit xilem builds on) authors widget paint via the
//! `imaging::Painter` command-stream API, not `vello::Scene` directly. imaging is a
//! backend-agnostic IR layered above vello: the native xilem canvas consumes an
//! `imaging::record::Scene` via `Painter::replay`, and the web-view converts it to a
//! `vello::Scene` via `imaging_vello::VelloSceneSink`. Producing an imaging Scene
//! here lets both hosts share one render path and insulates rofd-render from vello
//! API churn (the imaging_vello backend absorbs it).
//!
//! # Transforms
//!
//! imaging has no "replay a pre-built sub-scene with a transform" primitive (unlike
//! vello's `Scene::append(child, Some(affine))`), so the body/annotation builders do
//! not produce cached sub-scenes. Instead they draw into the shared painter with
//! `compose_transform(page_origin, zoom, ctm)` applied per draw call - equivalent to
//! the old "page-local sub-scene + append with page transform" but with the transform
//! folded into each command.

use std::sync::Arc;

use imaging::kurbo::{Rect, Stroke};
use imaging::peniko::Color;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::{AnnotationKind, AnnotationSelection, OfdDocument, Rect as RofdRect};

use crate::annotation_scene::draw_annotations;
use crate::body_scene::draw_body;
use crate::hit_test::{annotation_viewport_rect, HANDLE_SIZE};
use crate::text::FontStore;
use crate::viewport::Viewport;

/// Compute the viewport-space origin (page_x, page_y) of the page at index
/// `page_idx`.
///
/// This is the single source of truth for the page-stacking geometry: pages are
/// centered horizontally within the viewport (`((size - page_w) / 2).max(0)`,
/// offset by `scroll.0`) and stacked vertically with `page_gap` between them
/// (first page's Y = `page_gap - scroll.1`, each subsequent page advances by
/// `page_h + page_gap` where `page_h = physical_box.h * zoom`).
///
/// Returns `None` if `page_idx` is out of bounds (or a preceding page is
/// missing - cannot happen for a valid `Vec` index, but the `?` keeps it
/// robust). All callers (composite, hit_test, caret_rect, annotation_viewport_rect,
/// draw_drag_preview, and the component's current_page_id / viewport_to_page_local
/// / visible_page_index) must use this helper rather than re-deriving the loop,
/// so the geometry stays consistent across the codebase.
pub fn page_origin(doc: &OfdDocument, vp: &Viewport, page_idx: usize) -> Option<(f64, f64)> {
    let page = doc.pages.get(page_idx)?;
    let page_w = page.physical_box.w * vp.zoom;
    let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0) + vp.scroll.0;
    let mut y = vp.page_gap - vp.scroll.1;
    for i in 0..page_idx {
        let h = doc.pages.get(i)?.physical_box.h * vp.zoom;
        y += h + vp.page_gap;
    }
    Some((page_x, y))
}

/// Compute all page origins in one pass (O(n)).
///
/// Equivalent to calling [`page_origin`] for every page index, but avoids the
/// O(n²) re-loop that results from calling `page_origin(i)` inside a full-page
/// loop (each call re-walks 0..i). Use this in full-page iteration (composite,
/// hit_test, visible_page_index); use [`page_origin`] for single-page lookups
/// (caret_rect, annotation_viewport_rect, viewport_to_page_local) where O(n)
/// per call is fine.
pub fn page_origins(doc: &OfdDocument, vp: &Viewport) -> Vec<(f64, f64)> {
    let mut origins = Vec::with_capacity(doc.pages.len());
    let mut y = vp.page_gap - vp.scroll.1;
    for page in &doc.pages {
        let page_w = page.physical_box.w * vp.zoom;
        let page_x = ((vp.size.0 - page_w) / 2.0).max(0.0) + vp.scroll.0;
        origins.push((page_x, y));
        y += page.physical_box.h * vp.zoom + vp.page_gap;
    }
    origins
}

/// In-progress drag visualization. Passed by the component (T2/T3) during a
/// pointer drag so the user sees a live preview of the annotation being
/// created, moved, or resized.
///
/// `composite` draws this as a semi-transparent overlay on top of the existing
/// scene (after body + annotations + selection handles).
#[derive(Debug, Clone)]
pub enum DragPreview {
    /// Creating a new rect-bounded annotation (Shape/Note/TextBox/etc.).
    /// `rect` is in page-local coordinates.
    Create {
        kind: AnnotationKind,
        rect: RofdRect,
    },
    /// Creating a freehand annotation; `path` is viewport-space points.
    CreateFreehand { path: Vec<(f64, f64)> },
    /// Moving an existing annotation; `rect` is the new page-local position.
    Move {
        id: rofd_dom::AnnotationId,
        rect: RofdRect,
    },
    /// Resizing an existing annotation; `rect` is the new page-local rect.
    Resize {
        id: rofd_dom::AnnotationId,
        rect: RofdRect,
    },
}

/// Color for selection handle fills (opaque blue).
const HANDLE_COLOR: Color = Color::from_rgba8(0x00, 0x72, 0xC6, 0xFF);

/// Color for the selection frame stroke (opaque blue).
const FRAME_COLOR: Color = Color::from_rgba8(0x00, 0x72, 0xC6, 0xFF);

/// Color for drag preview outlines (semi-transparent blue).
const PREVIEW_COLOR: Color = Color::from_rgba8(0x00, 0x72, 0xC6, 0x80);

/// Stroke width for the selection frame (screen pixels).
const FRAME_STROKE_WIDTH: f64 = 1.0;

/// Stroke width for drag preview outlines (screen pixels).
const PREVIEW_STROKE_WIDTH: f64 = 1.0;

/// Renders an [`OfdDocument`] into a paper-on-desk [`imaging::record::Scene`] for a [`Viewport`].
///
/// Stores only the default fallback font bytes (`Arc`-shared, no copy per
/// composite). A per-document [`FontStore`] (document fonts + the default) is
/// built once per `composite` call - cheap, because the font bytes are
/// `Arc`-shared.
pub struct RenderEngine {
    pub default_font_bytes: Arc<Vec<u8>>,
}

impl RenderEngine {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self { default_font_bytes }
    }

    /// Composite paper-on-desk: gray viewport background, white centered pages,
    /// body + annotation per page drawn with the page-origin + zoom transform.
    ///
    /// Pages are stacked vertically with `page_gap` between them and centered
    /// horizontally within the viewport. `scroll` offsets the stack (positive y
    /// scrolls pages downward) and `zoom` scales each page's physical box.
    ///
    /// `fonts` is a caller-cached [`FontStore`] (document fonts + default). It is
    /// passed in rather than built per call so a large default CJK font is not
    /// re-registered every frame.
    ///
    /// `selection` controls handle drawing: if [`AnnotationSelection::Single`],
    /// 8 resize handles + a selection frame are drawn on the selected
    /// annotation's viewport-space bounding rect. `drag` draws a live
    /// semi-transparent preview of an in-progress create/move/resize operation.
    pub fn composite(
        &self,
        doc: &OfdDocument,
        vp: &Viewport,
        fonts: &FontStore,
        selection: &AnnotationSelection,
        drag: Option<&DragPreview>,
    ) -> Scene {
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);

        // Gray "desk" background filling the viewport.
        let gray = Color::from_rgba8(0xE0, 0xE0, 0xE0, 0xFF);
        let bg = Rect::new(0.0, 0.0, vp.size.0, vp.size.1);
        painter.fill_rect(bg, gray);

        // Stack pages vertically, centered horizontally, offset by scroll.
        // page_origins computes all origins in one O(n) pass (avoids the O(n²)
        // re-loop from calling page_origin(i) per page). Cull off-screen pages
        // to avoid shaping/drawing pages the user can't see (critical for
        // multi-page docs - shaping dominates render time).
        let origins = page_origins(doc, vp);
        for (i, page) in doc.pages.iter().enumerate() {
            let Some(&origin) = origins.get(i) else {
                continue;
            };
            let page_w = page.physical_box.w * vp.zoom;
            let page_h = page.physical_box.h * vp.zoom;

            // Cull: skip pages fully above the viewport (scrolled past) and
            // stop once a page starts below it.
            if origin.1 + page_h < 0.0 {
                continue;
            }
            if origin.1 > vp.size.1 {
                break;
            }

            // White page background.
            let white = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);
            let page_rect = Rect::new(origin.0, origin.1, origin.0 + page_w, origin.1 + page_h);
            painter.fill_rect(page_rect, white);

            // Body + annotation, drawn directly with page_origin + zoom baked
            // into each draw call (no cached sub-scenes - see module docs).
            draw_body(&mut painter, page, &doc.resources, fonts, origin, vp.zoom);
            let anns = doc.annotations.for_page(&page.id);
            draw_annotations(&mut painter, anns, &doc.resources, fonts, origin, vp.zoom);
        }

        // Selection overlay: draw 8 handles + a frame on the selected
        // annotation (if Single). Drawn after all pages so handles appear on
        // top. Handles are screen-space (not page-local); they don't scale
        // with zoom.
        if let AnnotationSelection::Single(id) = selection {
            if let Some(ann) = doc.annotations.find(id) {
                if let Some(vr) = annotation_viewport_rect(doc, ann, vp) {
                    draw_selection_overlay(&mut painter, vr);
                }
            }
        }

        // Drag preview: semi-transparent outline of the in-progress operation.
        if let Some(preview) = drag {
            draw_drag_preview(&mut painter, preview, doc, vp);
        }

        scene
    }
}

/// Draw the selection frame (stroked bounding rect) + 8 handle fills on the
/// annotation's viewport-space rect.
///
/// Handles are `HANDLE_SIZE` x `HANDLE_SIZE` screen-pixel squares centered on
/// the 4 corners + 4 edge midpoints. The frame is a 1px stroked rect.
fn draw_selection_overlay(painter: &mut Painter<Scene>, vr: RofdRect) {
    let kurbo_rect = Rect::new(vr.x, vr.y, vr.x + vr.w, vr.y + vr.h);

    // Selection frame: stroked bounding rect.
    painter
        .stroke(kurbo_rect, &Stroke::new(FRAME_STROKE_WIDTH), FRAME_COLOR)
        .draw();

    // 8 handles: 4 corners + 4 edge midpoints.
    let half = HANDLE_SIZE / 2.0;
    let x0 = vr.x;
    let y0 = vr.y;
    let x1 = vr.x + vr.w;
    let y1 = vr.y + vr.h;
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;

    let handle_centers = [
        (x0, y0), // Nw
        (x1, y0), // Ne
        (x0, y1), // Sw
        (x1, y1), // Se
        (cx, y0), // N
        (cx, y1), // S
        (x1, cy), // E
        (x0, cy), // W
    ];
    for (hx, hy) in &handle_centers {
        let handle_rect = Rect::new(hx - half, hy - half, hx + half, hy + half);
        painter.fill_rect(handle_rect, HANDLE_COLOR);
    }
}

/// Draw a semi-transparent preview of an in-progress drag operation.
///
/// - `Create`: stroked page-local rect transformed to viewport coords (uses
///   page 0's origin - T2/T3 will pass the active page).
/// - `CreateFreehand`: stroked viewport-space path.
/// - `Move`/`Resize`: stroked page-local rect transformed to viewport coords,
///   using the annotation's page's stacked Y origin (NOT page 0's).
///
/// The page origin is computed via [`page_origin`] (the shared page-stacking
/// helper), so multi-page docs use the correct stacked Y origin for an
/// annotation on page 1+.
fn draw_drag_preview(
    painter: &mut Painter<Scene>,
    preview: &DragPreview,
    doc: &OfdDocument,
    vp: &Viewport,
) {
    match preview {
        DragPreview::Create { rect, .. }
        | DragPreview::Move { rect, .. }
        | DragPreview::Resize { rect, .. } => {
            // For Move/Resize, find the annotation's page index so we use the
            // correct stacked Y origin. For Create (no annotation id), fall
            // back to page 0 (its origin is correct for page 0).
            let target_page_idx = match preview {
                DragPreview::Move { id, .. } | DragPreview::Resize { id, .. } => doc
                    .annotations
                    .find(id)
                    .and_then(|a| doc.pages.iter().position(|p| p.id == a.page)),
                _ => Some(0), // Create: first page
            };

            let Some((origin_x, origin_y)) =
                target_page_idx.and_then(|idx| page_origin(doc, vp, idx))
            else {
                return;
            };

            let vr = Rect::new(
                origin_x + rect.x * vp.zoom,
                origin_y + rect.y * vp.zoom,
                origin_x + (rect.x + rect.w) * vp.zoom,
                origin_y + (rect.y + rect.h) * vp.zoom,
            );
            painter
                .stroke(vr, &Stroke::new(PREVIEW_STROKE_WIDTH), PREVIEW_COLOR)
                .draw();
        }
        DragPreview::CreateFreehand { path } => {
            if path.len() < 2 {
                return;
            }
            let mut bez = imaging::kurbo::BezPath::new();
            bez.move_to((path[0].0, path[0].1));
            for &(x, y) in &path[1..] {
                bez.line_to((x, y));
            }
            painter
                .stroke(&bez, &Stroke::new(PREVIEW_STROKE_WIDTH), PREVIEW_COLOR)
                .draw();
        }
    }
}

/// Count `Draw::Fill` commands in a scene (used by tests to assert handle count).
#[cfg(test)]
fn count_fills(scene: &Scene) -> usize {
    use imaging::record::{Command, Draw};
    scene
        .commands()
        .iter()
        .filter(|cmd| match cmd {
            Command::Draw(id) => matches!(scene.draw_op(*id), Draw::Fill { .. }),
            _ => false,
        })
        .count()
}

/// Count `Draw::Stroke` commands in a scene (used by tests to assert frame count).
#[cfg(test)]
fn count_strokes(scene: &Scene) -> usize {
    use imaging::record::{Command, Draw};
    scene
        .commands()
        .iter()
        .filter(|cmd| match cmd {
            Command::Draw(id) => matches!(scene.draw_op(*id), Draw::Stroke { .. }),
            _ => false,
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationModel, AnnotationPayload,
        AnnotationSelection, Color, OfdDocument, Page, PageId, Rect, ShapeKind,
    };
    use std::sync::Arc;

    /// Build a doc with one page (100x100) and one rect annotation on it.
    fn doc_with_rect_annotation() -> (OfdDocument, AnnotationId) {
        let page_id = PageId::new("P0");
        let ann_id = AnnotationId::from_int(1);
        let annotation = Annotation {
            id: ann_id.clone(),
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
            id: page_id,
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
        (doc, ann_id)
    }

    fn build_font_store() -> FontStore {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(
            &rofd_dom::Resources::default(),
            Arc::new(font_bytes.to_vec()),
        )
    }

    #[test]
    fn composite_with_selection_draws_handles() {
        // Select a rect annotation -> composite draws 8 handle fills + 1 frame stroke.
        let (doc, ann_id) = doc_with_rect_annotation();
        let fonts = build_font_store();
        let engine = RenderEngine::new(Arc::new(vec![]));
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let selection = AnnotationSelection::Single(ann_id);

        // With selection: the scene should have at least 8 more fills (handles)
        // and 1 more stroke (selection frame) than without selection.
        let scene_no_sel = engine.composite(&doc, &vp, &fonts, &AnnotationSelection::None, None);
        let scene_sel = engine.composite(&doc, &vp, &fonts, &selection, None);

        let fills_no_sel = count_fills(&scene_no_sel);
        let fills_sel = count_fills(&scene_sel);
        let strokes_no_sel = count_strokes(&scene_no_sel);
        let strokes_sel = count_strokes(&scene_sel);

        assert_eq!(
            fills_sel - fills_no_sel,
            8,
            "selection should add exactly 8 handle fills"
        );
        assert!(
            strokes_sel > strokes_no_sel,
            "selection should add at least 1 frame stroke ({} -> {})",
            strokes_no_sel,
            strokes_sel
        );
    }

    #[test]
    fn composite_with_no_selection_draws_no_handles() {
        // No selection -> no extra fills beyond the desk bg + page bg + annotation fill.
        let (doc, _ann_id) = doc_with_rect_annotation();
        let fonts = build_font_store();
        let engine = RenderEngine::new(Arc::new(vec![]));
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let scene = engine.composite(&doc, &vp, &fonts, &AnnotationSelection::None, None);
        let fills = count_fills(&scene);
        // Desk bg (1) + page bg (1) + annotation fill (1) = 3 fills. No handles.
        assert_eq!(
            fills, 3,
            "no selection -> desk bg + page bg + annotation fill, no handles"
        );
    }

    #[test]
    fn composite_with_drag_preview_draws_preview() {
        // A DragPreview::Create should add a stroke (the preview rect outline).
        let (doc, _ann_id) = doc_with_rect_annotation();
        let fonts = build_font_store();
        let engine = RenderEngine::new(Arc::new(vec![]));
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let preview = DragPreview::Create {
            kind: AnnotationKind::Shape(ShapeKind::Rect),
            rect: Rect {
                x: 5.0,
                y: 5.0,
                w: 30.0,
                h: 20.0,
            },
        };
        let scene_no_drag = engine.composite(&doc, &vp, &fonts, &AnnotationSelection::None, None);
        let scene_drag = engine.composite(
            &doc,
            &vp,
            &fonts,
            &AnnotationSelection::None,
            Some(&preview),
        );
        // The drag preview adds 1 stroke (the preview rect outline).
        assert!(
            count_strokes(&scene_drag) > count_strokes(&scene_no_drag),
            "drag preview should add strokes ({} -> {})",
            count_strokes(&scene_no_drag),
            count_strokes(&scene_drag)
        );
    }

    #[test]
    fn composite_does_not_panic_on_empty_doc() {
        let doc = OfdDocument::default();
        let fonts = build_font_store();
        let engine = RenderEngine::new(Arc::new(vec![]));
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (800.0, 600.0),
            page_gap: 20.0,
        };
        let _scene = engine.composite(&doc, &vp, &fonts, &AnnotationSelection::None, None);
    }

    // --- page_origin helper tests ---

    /// Build a doc with `n` pages, each 100x100, ids "P0".."P{n-1}".
    fn doc_with_n_pages(n: usize) -> OfdDocument {
        let mut doc = OfdDocument::default();
        for i in 0..n {
            doc.pages.push(Page {
                id: PageId::new(format!("P{i}")),
                physical_box: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                layers: vec![],
                template: None,
            });
        }
        doc
    }

    #[test]
    fn page_origin_none_for_empty_doc() {
        let doc = OfdDocument::default();
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        assert!(page_origin(&doc, &vp, 0).is_none(), "empty doc -> None");
    }

    #[test]
    fn page_origin_none_for_out_of_bounds() {
        let doc = doc_with_n_pages(2);
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        assert!(
            page_origin(&doc, &vp, 2).is_none(),
            "idx 2 of 2-page doc -> None"
        );
    }

    #[test]
    fn page_origin_page_zero_no_scroll() {
        // Page 100x100, viewport 200x200, gap 20, zoom 1, scroll (0,0).
        // page_x = ((200-100)/2).max(0) + 0 = 50. page_y = 20 - 0 = 20.
        let doc = doc_with_n_pages(1);
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let (x, y) = page_origin(&doc, &vp, 0).expect("page 0 exists");
        assert!((x - 50.0).abs() < 1e-9, "page_x centered = 50, got {x}");
        assert!(
            (y - 20.0).abs() < 1e-9,
            "page_y = gap - scroll = 20, got {y}"
        );
    }

    #[test]
    fn page_origin_page_one_stacked_y() {
        // Two pages 100x100, gap 20, zoom 1, scroll (0,0).
        // Page 0: y = 20. Page 1: y = 20 + 100 + 20 = 140.
        let doc = doc_with_n_pages(2);
        let vp = Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 400.0),
            page_gap: 20.0,
        };
        let (_, y0) = page_origin(&doc, &vp, 0).expect("page 0");
        let (_, y1) = page_origin(&doc, &vp, 1).expect("page 1");
        assert!((y0 - 20.0).abs() < 1e-9, "page 0 y = 20, got {y0}");
        assert!((y1 - 140.0).abs() < 1e-9, "page 1 y = 140, got {y1}");
    }

    #[test]
    fn page_origin_respects_scroll_and_zoom() {
        // Two pages 100x100, gap 20, zoom 2, scroll (10, 30).
        // page_w = 200, page_x = ((200-200)/2).max(0) + 10 = 10.
        // Page 0: y = 20 - 30 = -10. Page 1: y = -10 + (100*2) + 20 = 210.
        let doc = doc_with_n_pages(2);
        let vp = Viewport {
            scroll: (10.0, 30.0),
            zoom: 2.0,
            size: (200.0, 600.0),
            page_gap: 20.0,
        };
        let (x0, y0) = page_origin(&doc, &vp, 0).expect("page 0");
        let (x1, y1) = page_origin(&doc, &vp, 1).expect("page 1");
        assert!((x0 - 10.0).abs() < 1e-9, "page_x = 10, got {x0}");
        assert!((y0 - (-10.0)).abs() < 1e-9, "page 0 y = -10, got {y0}");
        assert!(
            (x1 - 10.0).abs() < 1e-9,
            "page 1 x = 10 (same centering), got {x1}"
        );
        assert!((y1 - 210.0).abs() < 1e-9, "page 1 y = 210, got {y1}");
    }

    #[test]
    fn page_origin_x_never_negative() {
        // Viewport narrower than page -> ((size - page_w)/2).max(0) = 0.
        // Page 100 wide, viewport 50, zoom 1 -> page_x = 0 + scroll.0.
        let doc = doc_with_n_pages(1);
        let vp = Viewport {
            scroll: (5.0, 0.0),
            zoom: 1.0,
            size: (50.0, 200.0),
            page_gap: 20.0,
        };
        let (x, _) = page_origin(&doc, &vp, 0).expect("page 0");
        assert!((x - 5.0).abs() < 1e-9, "page_x = 0 + scroll.0 = 5, got {x}");
    }
}
