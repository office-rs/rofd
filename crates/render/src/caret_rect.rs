//! Caret rect (viewport space) for a text annotation at a char offset.
//!
//! Given a text annotation ([`AnnotationPayload::TextBox`] / [`Note`] /
//! [`Watermark`]) and a char `offset`, [`caret_rect`] computes the caret's
//! rectangle in viewport (device-pixel) space - the rectangle the editor
//! paints as the text cursor.
//!
//! The viewport transform uses [`crate::composite::page_origin`] (the shared
//! page-stacking helper: centering + scroll on both axes + `page_gap` +
//! `zoom`), so the caret aligns with the rendered glyph run. Glyph x positions
//! come from [`FontStore::shape`] (the shaper's natural advances); the caret
//! height is the font `size` scaled by `zoom`.
//!
//! Non-text annotations ([`Markup`] / [`Freehand`] / [`Shape`] / [`Stamp`])
//! return `None` - they have no caret.
//!
//! [`Note`]: AnnotationPayload::Note
//! [`Markup`]: AnnotationPayload::Markup
//! [`Freehand`]: AnnotationPayload::Freehand
//! [`Shape`]: AnnotationPayload::Shape
//! [`Stamp`]: AnnotationPayload::Stamp

use rofd_dom::{AnnotationId, AnnotationPayload, OfdDocument, Rect};

use crate::text::FontStore;
use crate::viewport::Viewport;

/// Caret rect (viewport space) for a text annotation at char `offset`.
///
/// Returns `None` when the annotation isn't found, isn't a text annotation
/// (`TextBox`/`Note`/`Watermark`), or its page isn't in the document.
///
/// The viewport transform uses [`crate::composite::page_origin`] (the shared
/// page-stacking helper: centering + scroll on both axes + `page_gap` +
/// `zoom`).
///
/// The caret x is the shaped glyph's x at `offset` (page-local), scaled by
/// `zoom` and offset by the annotation rect's x + the page origin. The caret
/// height is the font `size` scaled by `zoom`; width is `1.0 * zoom`.
pub fn caret_rect(
    doc: &OfdDocument,
    vp: &Viewport,
    fonts: &FontStore,
    ann_id: &AnnotationId,
    offset: usize,
) -> Option<Rect> {
    let ann = doc
        .annotations
        .by_page
        .values()
        .flatten()
        .find(|a| &a.id == ann_id)?;

    // Extract (content, font_id, size, rect) for text-bearing payloads.
    // Notes carry their own content; non-text payloads return None.
    let (content, font_id, size, rect) = match &ann.payload {
        AnnotationPayload::TextBox {
            rect,
            content,
            font,
            size,
            ..
        } => (content.as_str(), font, *size, *rect),
        AnnotationPayload::Note { rect, content, .. } => {
            // Notes: caret placed inside the popup rect using the note's text.
            (content.as_str(), &rofd_dom::FontId::default(), 12.0, *rect)
        }
        AnnotationPayload::Watermark {
            rect,
            content,
            font,
            size,
            ..
        } => (content.as_str(), font, *size, *rect),
        _ => return None,
    };

    // Shape with the document font (or the default fallback) for glyph advances.
    let (_font, glyphs) = fonts.shape(font_id, content, size);

    // Caret x (page-local) = the shaped glyph x at `offset`. For an offset at
    // or past the end of the run, place the caret at the last glyph's x (the
    // shaper's pen position for that glyph). For an empty run, x = 0.
    let caret_x_local = glyphs
        .get(offset)
        .map(|g| g.x)
        .unwrap_or_else(|| glyphs.last().map(|g| g.x).unwrap_or(0.0));

    // Find the annotation's page index and compute its viewport origin via the
    // shared page_origin helper (same geometry as composite.rs).
    let page_idx = doc.pages.iter().position(|p| p.id == ann.page)?;
    let (origin_x, origin_y) = crate::composite::page_origin(doc, vp, page_idx)?;

    let vx = origin_x + (rect.x + caret_x_local as f64) * vp.zoom;
    let vy = origin_y + rect.y * vp.zoom;
    Some(Rect {
        x: vx,
        y: vy,
        w: 1.0 * vp.zoom,
        h: size * vp.zoom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, NoteIcon,
        OfdDocument, Page, PageId, Rect, ShapeKind,
    };

    /// A minimal doc + FontStore for positive tests: one page + the TestFont
    /// registered as the default (so unknown FontIds still shape).
    fn doc_with_font() -> (OfdDocument, FontStore) {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 300.0,
            },
            layers: vec![],
            template: None,
        });
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        (doc, fonts)
    }

    /// Push an annotation onto page P0 and return its id.
    fn push_ann(
        doc: &mut OfdDocument,
        payload: AnnotationPayload,
        kind: AnnotationKind,
    ) -> AnnotationId {
        let id = AnnotationId::from_int(1);
        let ann = Annotation {
            id: id.clone(),
            kind,
            page: PageId::new("P0"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload,
        };
        doc.annotations
            .by_page
            .entry(PageId::new("P0"))
            .or_default()
            .push(ann);
        id
    }

    fn vp() -> Viewport {
        Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (800.0, 600.0),
            page_gap: 20.0,
        }
    }

    #[test]
    fn caret_rect_none_for_non_text_annotation() {
        let doc = OfdDocument::default(); // no annotations
        let vp = vp();
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        let res = caret_rect(&doc, &vp, &fonts, &AnnotationId::default(), 0);
        assert!(res.is_none(), "empty doc (no annotation) -> None");
    }

    #[test]
    fn caret_rect_none_for_markup_annotation() {
        // Markup has no caret - it's a highlight/strikeout, not editable text.
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::Markup {
                quad_points: vec![
                    rofd_dom::Point { x: 0.0, y: 0.0 },
                    rofd_dom::Point { x: 10.0, y: 10.0 },
                ],
                color: Color::Rgb(255, 255, 0),
            },
            AnnotationKind::Highlight,
        );
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(res.is_none(), "Markup annotation -> None");
    }

    #[test]
    fn caret_rect_none_for_shape_annotation() {
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
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
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(res.is_none(), "Shape annotation -> None");
    }

    #[test]
    fn caret_rect_some_for_textbox_at_offset_zero() {
        // TextBox "Hi" at rect origin (10, 20). At offset 0 the caret sits at
        // the annotation rect's x (the first glyph's x is ~0 from the shaper),
        // at the rect's y, with height = size.
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "Hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
            },
            AnnotationKind::TextBox,
        );

        // vp: page_gap=20, scroll=(0,0), zoom=1, size=(800,600).
        // page_w = 200, page_x = (800-200)/2 = 300. page_origin = (300, 20).
        // caret x_local at offset 0 = glyphs[0].x (first glyph's shaped x).
        let (_font, glyphs) = fonts.shape(&FontId::new("F1"), "Hi", 12.0);
        let first_x = glyphs[0].x as f64;
        let expected_x = 300.0 + (10.0 + first_x) * 1.0;
        let expected_y = 20.0 + 20.0 * 1.0;

        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        let r = res.expect("TextBox -> Some caret rect");
        assert_eq!(
            r.x, expected_x,
            "caret x = page_origin_x + (rect.x + glyph0.x) * zoom"
        );
        assert_eq!(r.y, expected_y, "caret y = page_origin_y + rect.y * zoom");
        assert_eq!(r.w, 1.0, "caret width = 1px * zoom");
        assert_eq!(r.h, 12.0, "caret height = size * zoom");
    }

    #[test]
    fn caret_rect_respects_scroll_and_zoom() {
        // Scroll both axes + zoom != 1: the caret must track the page origin
        // exactly as composite does (scroll.0 on x, scroll.1 on y).
        let (mut doc, fonts) = doc_with_font();
        let id = push_ann(
            &mut doc,
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "Hi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
            },
            AnnotationKind::TextBox,
        );

        let vp = Viewport {
            scroll: (50.0, 30.0),
            zoom: 2.0,
            size: (800.0, 600.0),
            page_gap: 20.0,
        };
        // page_w = 200*2 = 400, page_x = (800-400)/2 = 200.
        // page_origin = (200 + 50, 20 - 30) = (250, -10).
        let (_font, glyphs) = fonts.shape(&FontId::new("F1"), "Hi", 12.0);
        let first_x = glyphs[0].x as f64;
        let expected_x = 250.0 + (10.0 + first_x) * 2.0;
        let expected_y = -10.0 + 20.0 * 2.0;

        let res = caret_rect(&doc, &vp, &fonts, &id, 0);
        let r = res.expect("TextBox -> Some caret rect with scroll+zoom");
        assert_eq!(r.x, expected_x, "caret x tracks scroll.0 + zoom");
        assert_eq!(r.y, expected_y, "caret y tracks scroll.1 + zoom");
        assert_eq!(r.w, 2.0, "caret width = 1px * zoom");
        assert_eq!(r.h, 24.0, "caret height = size * zoom");
    }

    #[test]
    fn caret_rect_some_for_note_and_watermark() {
        // Note and Watermark are text-bearing -> Some.
        let (mut doc, fonts) = doc_with_font();

        let note_id = push_ann(
            &mut doc,
            AnnotationPayload::Note {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    w: 40.0,
                    h: 20.0,
                },
                color: Color::Rgb(255, 200, 0),
                content: "note text".into(),
                icon: NoteIcon::Note,
            },
            AnnotationKind::Note,
        );
        assert!(
            caret_rect(&doc, &vp(), &fonts, &note_id, 0).is_some(),
            "Note -> Some"
        );

        let wm_id = push_ann(
            &mut doc,
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
        assert!(
            caret_rect(&doc, &vp(), &fonts, &wm_id, 0).is_some(),
            "Watermark -> Some"
        );
    }

    #[test]
    fn caret_rect_none_when_page_missing() {
        // Annotation references a page that isn't in doc.pages -> None.
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        let mut doc = OfdDocument::default();
        // No pages pushed, but an annotation claims page P0.
        let id = AnnotationId::from_int(1);
        doc.annotations
            .by_page
            .entry(PageId::new("P0"))
            .or_default()
            .push(Annotation {
                id: id.clone(),
                kind: AnnotationKind::TextBox,
                page: PageId::new("P0"),
                creator: "tester".into(),
                created: 0,
                modified: 0,
                reply_to: None,
                payload: AnnotationPayload::TextBox {
                    rect: Rect {
                        x: 10.0,
                        y: 20.0,
                        w: 100.0,
                        h: 30.0,
                    },
                    content: "Hi".into(),
                    font: FontId::new("F1"),
                    size: 12.0,
                    color: Color::Rgb(0, 0, 0),
                },
            });
        let fonts = FontStore::from_resources(&doc.resources, Arc::new(font_bytes.to_vec()));
        let res = caret_rect(&doc, &vp(), &fonts, &id, 0);
        assert!(
            res.is_none(),
            "annotation on a page not in doc.pages -> None"
        );
    }
}
