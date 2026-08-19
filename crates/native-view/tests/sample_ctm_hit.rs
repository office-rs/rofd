//! Regression (real fixture): sample.ofd text carries scale CTMs
//! (0.0176 unit conversion, Size=209). Every TextObject must be
//! hover-hittable where it renders - guards the full-affine inverse hit
//! test against regressing to translation-only CTM handling (which made
//! ALL of sample.ofd text un-selectable).

use kurbo::Point;
use rofd_render::{composite, hit_test_body_text};

#[test]
fn every_sample_ofd_text_object_is_hittable() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/sample.ofd"
    ))
    .expect("fixture test/sample.ofd");
    let report = rofd_io::parse_ofd(&bytes).unwrap();
    let doc = &report.document;
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
                // Probe the visual middle of the first char cell: local pen
                // (code.x + size/2, code.y - size/2) mapped through the
                // object's full affine (Boundary + CTM + zoom).
                let m = rofd_render::ctm::compose_object_transform(
                    (ox, oy),
                    vp.zoom,
                    t.boundary,
                    t.ctm.as_ref(),
                );
                let p = m * Point::new(code.x + t.size * 0.5, code.y - t.size * 0.5);
                if hit_test_body_text(doc, &vp, (p.x, p.y)).is_none() {
                    misses.push(t.id.0.clone());
                }
            }
        }
    }
    println!("sample.ofd text objects: {total}, misses: {}", misses.len());
    assert!(
        misses.is_empty(),
        "{}/{} text objects are hit-blind: {:?}",
        misses.len(),
        total,
        &misses[..misses.len().min(10)]
    );
}
