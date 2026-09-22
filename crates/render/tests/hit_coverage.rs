//! Regression (real fixture): every text object in `test/ru-yuan-ji-lu.ofd`
//! must be hover-hittable. Catches degenerate hit geometry like the
//! zero-delta advance bug, where single-char codes with a redundant
//! `DeltaX="0"` collapsed to a zero-width hit extent (272/669 objects were
//! hover/drag-blind).

use rofd_render::{composite, hit_test_body_text};

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn every_fixture_text_object_is_hittable() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/ru-yuan-ji-lu.ofd"
    ))
    .expect("fixture test/ru-yuan-ji-lu.ofd (see AGENTS.md)");
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let doc = &report.document;
    // Browser-like viewport: default zoom, page centered.
    let vp = rofd_render::Viewport {
        scroll: (0.0, 0.0),
        zoom: 96.0 / 25.4,
        size: (1280.0, 900.0),
        page_gap: 20.0,
    };

    let mut total = 0;
    let mut misses = Vec::new();
    for (idx, page) in doc.pages.iter().enumerate() {
        let Some((ox, oy)) = composite::page_origin(doc, &vp, idx) else {
            continue;
        };
        for layer in &page.layers {
            for obj in &layer.objects {
                let rofd_dom::PageObject::Text(t) = obj else {
                    continue;
                };
                let Some(code) = t.codes.first() else {
                    continue;
                };
                if code.text.is_empty() {
                    continue;
                }
                total += 1;
                // Probe the vertical middle of the first char's band:
                // pen (boundary + code X/Y), baseline minus half an em.
                let px = ox + (t.boundary.x + code.x + t.size * 0.5) * vp.zoom;
                let py = oy + (t.boundary.y + code.y - t.size * 0.5) * vp.zoom;
                if hit_test_body_text(doc, &vp, (px, py)).is_none() {
                    misses.push(t.id.0.clone());
                }
            }
        }
    }
    assert!(
        misses.is_empty(),
        "{}/{} text objects are hit-blind: {:?}",
        misses.len(),
        total,
        &misses[..misses.len().min(10)]
    );
}
