//! Regression: drag-select on sample.ofd must work even where text-markup
//! annotations (highlight/underline/strikeout/squiggly) cover the text.
//! WPS 行为（spec §5.2）：markup 贴附正文文字，不是点击目标--指针穿透
//! 到文字选区流程。

use rofd_component::{EditorComponent, EditorConfig, Tool, ViewEvent};
use std::sync::Arc;

#[test]
fn drag_select_over_markup_annotation_produces_selection() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/sample.ofd"
    ))
    .expect("fixture test/sample.ofd");
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let mut c = EditorComponent::new(EditorConfig::new(Arc::new(vec![])));
    c.load_document(report.document);
    c.set_tool(Tool::Text);
    c.handle_event(&ViewEvent::Resize {
        width: 1280.0,
        height: 900.0,
    });

    let doc = c.document().clone();
    let vp = rofd_render::Viewport {
        scroll: (0.0, 0.0),
        zoom: 96.0 / 25.4,
        size: (1280.0, 900.0),
        page_gap: 20.0,
    };

    // Whether the point is covered by a Markup annotation (the regression
    // condition: markup must not swallow the pointer).
    let covered_by_markup = |p: (f64, f64)| -> bool {
        let target = rofd_render::hit_test(&doc, &vp, c.selection(), p);
        match &target {
            rofd_render::HitTarget::Annotation(id)
            | rofd_render::HitTarget::AnnotationText(id, _) => {
                doc.annotations.find(id).is_some_and(|ann| {
                    matches!(ann.payload, rofd_dom::AnnotationPayload::Markup { .. })
                })
            }
            _ => false,
        }
    };

    // Find a drag pair whose anchor sits under a Markup annotation and
    // whose end hits a different character offset.
    let mut anchor = None;
    let mut end = None;
    'outer: for (idx, page) in doc.pages.iter().enumerate() {
        let Some((ox, oy)) = rofd_render::composite::page_origin(&doc, &vp, idx) else {
            continue;
        };
        for layer in &page.layers {
            for obj in &layer.objects {
                let rofd_dom::PageObject::Text(t) = obj else {
                    continue;
                };
                let m = rofd_render::ctm::compose_object_transform(
                    (ox, oy),
                    vp.zoom,
                    t.boundary,
                    t.ctm.as_ref(),
                );
                for code in &t.codes {
                    if code.text.is_empty() {
                        continue;
                    }
                    // Probe along the baseline inside the band.
                    for k in 0..8 {
                        let p = m * kurbo::Point::new(
                            code.x + t.size * (0.5 + k as f64),
                            code.y - t.size * 0.3,
                        );
                        if !covered_by_markup((p.x, p.y)) {
                            continue;
                        }
                        if let Some(h) = rofd_render::hit_test_body_text(&doc, &vp, (p.x, p.y)) {
                            let q = m * kurbo::Point::new(
                                code.x + t.size * (3.5 + k as f64),
                                code.y - t.size * 0.3,
                            );
                            if let Some(h2) = rofd_render::hit_test_body_text(&doc, &vp, (q.x, q.y))
                            {
                                if h2.char_offset != h.char_offset
                                    || h2.object != h.object
                                    || h2.code_index != h.code_index
                                {
                                    anchor = Some((p.x, p.y));
                                    end = Some((q.x, q.y));
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let (ax, ay) = anchor.expect("found a text point covered by a markup annotation");
    let (ex, ey) = end.expect("found an end point with a different char offset");

    // The drag: markup must not intercept the press (pass-through), so the
    // text selection flow arms and produces a selection.
    c.handle_event(&ViewEvent::PointerDown {
        button: rofd_component::MouseButton::Left,
        x: ax,
        y: ay,
        modifiers: Default::default(),
        click_count: 1,
    });
    c.handle_event(&ViewEvent::PointerMove { x: ex, y: ey });
    c.handle_event(&ViewEvent::PointerUp {
        button: rofd_component::MouseButton::Left,
        x: ex,
        y: ey,
    });
    let sel = c.text_selection().expect("drag must leave a selection");
    assert!(!sel.ranges.is_empty());
    assert!(
        c.selected_text().is_some_and(|t| !t.is_empty()),
        "selected text must be non-empty"
    );
    assert!(
        !rofd_render::text_selection_rects(&doc, &vp, sel).is_empty(),
        "selection rects must be non-empty"
    );

    // The scene must contain the selection overlay: clearing the selection
    // must shrink the command list.
    let with_sel = c.build_scene().commands().len();
    c.set_tool(Tool::Text); // clears text_selection
    let without_sel = c.build_scene().commands().len();
    assert!(
        with_sel > without_sel,
        "selection overlay must add commands to the scene ({with_sel} vs {without_sel})"
    );
}
