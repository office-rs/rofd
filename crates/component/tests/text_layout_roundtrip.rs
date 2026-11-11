//! End-to-end guard for the annotation text layout contract (spec
//! 2026-11-09): save a wrapped TextBox, reparse it, confirm the logical
//! content survived (soft wraps are not baked into `content`), then widen
//! the reloaded box and confirm it reflows.

use std::sync::Arc;

use rofd_component::{OfdComponent, OfdConfig};
use rofd_dom::{
    AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, Layer, OfdDocument, Page,
    PageId, Rect,
};

const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

fn page_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        raw_children: Vec::new(),
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    doc
}

fn textbox_component(content: &str, w: f64, h: f64) -> (OfdComponent, AnnotationId) {
    let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(FONT.to_vec())));
    c.load_document(page_doc());
    let id = c.create_annotation(
        AnnotationKind::TextBox,
        PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w,
                h,
            },
            content: content.into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
            border: None,
            spans: Vec::new(),
        },
    );
    (c, id)
}

#[test]
fn saved_text_reopens_with_logical_content_and_reflows_on_resize() {
    let (c, id) = textbox_component("abc def ghi jkl", 45.0, 14.0);
    let snap = c.save_snapshot();
    assert_eq!(snap.text_layout[&id].len(), 2, "two wrapped lines");
    let bytes = rofd_io::write_ofd_with_text_layout(&snap.document, &snap.text_layout).unwrap();

    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let (reloaded_id, rect_h) = {
        let ann = &report.document.annotations.for_page(&PageId::new("P0"))[0];
        let AnnotationPayload::TextBox { content, rect, .. } = &ann.payload else {
            panic!("expected TextBox");
        };
        assert_eq!(
            content, "abc def ghi jkl",
            "logical content survives save+parse"
        );
        assert!(!content.contains('\n'), "soft wraps stay out of content");
        assert!(rect.h >= 14.0, "height was grown to fit two lines");
        (ann.id.clone(), rect.h)
    };

    let mut c2 = OfdComponent::new_native(OfdConfig::new(Arc::new(FONT.to_vec())));
    c2.load_document(report.document);
    c2.resize_annotation(
        &reloaded_id,
        Rect {
            x: 10.0,
            y: 10.0,
            w: 120.0,
            h: rect_h,
        },
    );
    let snap2 = c2.save_snapshot();
    assert_eq!(
        snap2.text_layout[&reloaded_id].len(),
        1,
        "widening the box reflows back to one line"
    );
}

/// One line split into three color spans (only the middle carries a color).
fn span_textbox_component() -> (OfdComponent, AnnotationId) {
    let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(FONT.to_vec())));
    c.load_document(page_doc());
    let span = |text: &str, color: Option<Color>| rofd_dom::TextSpan {
        text: text.into(),
        font: FontId::new("F1"),
        size: 12.0,
        color,
        bold: false,
        italic: false,
    };
    let id = c.create_annotation(
        AnnotationKind::TextBox,
        PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 200.0,
                h: 40.0,
            },
            content: "ab cd ef".into(),
            font: FontId::new("F1"),
            size: 12.0,
            color: Color::Rgb(0, 0, 0),
            border: None,
            spans: vec![
                span("ab ", None),
                span("cd", Some(Color::Rgb(255, 0, 0))),
                span(" ef", None),
            ],
        },
    );
    (c, id)
}

#[test]
fn span_colors_survive_save_and_reload_through_the_real_layout() {
    let (c, _id) = span_textbox_component();
    let snap = c.save_snapshot();
    let bytes = rofd_io::write_ofd_with_text_layout(&snap.document, &snap.text_layout).unwrap();
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let ann = &report.document.annotations.for_page(&PageId::new("P0"))[0];
    let AnnotationPayload::TextBox { content, spans, .. } = &ann.payload else {
        panic!("expected TextBox");
    };
    assert_eq!(content, "ab cd ef");
    let colors: Vec<Option<Color>> = spans.iter().map(|s| s.color).collect();
    assert_eq!(
        colors,
        vec![None, Some(Color::Rgb(255, 0, 0)), None],
        "per-word color and the surrounding None colors survive the round trip"
    );
}
