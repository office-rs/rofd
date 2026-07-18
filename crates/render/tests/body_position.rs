//! Body object positioning: text/path glyphs must land at their Boundary
//! origin (page-local), not collapse to (0,0) when CTM has no translation.
//!
//! Regression for the "everything piles up at top-left" bug: a TextObject with
//! CTM=scale (no translation) and TextCode X=0 must still place glyphs at
//! `boundary.origin + ctm × (X,Y)`, not at `ctm × (X,Y)` (which lands on the
//! page's top-left corner).

use imaging::record::{Command, Draw, Scene};
use imaging::Painter;
use rofd_dom::{
    Ctm, FontId, Layer, LayerType, ObjectId, Page, PageId, PageObject, Rect, TextCode, TextObject,
};
use rofd_render::body_scene::draw_body;
use rofd_render::text::FontStore;
use std::sync::Arc;

fn font_store() -> FontStore {
    let bytes = include_bytes!("fixtures/fonts/TestFont.ttf") as &[u8];
    FontStore::from_resources(&rofd_dom::Resources::default(), Arc::new(bytes.to_vec()))
}

/// Find the transform of the first glyph run in `scene`, if any.
fn first_glyph_transform(scene: &Scene) -> Option<imaging::kurbo::Affine> {
    for cmd in scene.commands() {
        if let Command::Draw(id) = cmd {
            if let Draw::GlyphRun(gr) = scene.draw_op(*id) {
                return Some(gr.transform);
            }
        }
    }
    None
}

#[test]
fn text_glyph_transform_includes_boundary_origin() {
    // Mirrors sample.ofd's TextObject: CTM=scale(0.0176) (no translation),
    // TextCode X=0 Y=179.5313, Boundary=(31.75, 26.3149, ...).
    // Correct glyph page position = (31.75, 26.3149) + 0.0176×(0, 179.5313)
    //                             = (31.75, 29.4745).
    // Old bug (no Boundary): (0, 3.16) -> piles at top-left.
    let text = TextObject {
        id: ObjectId::new("t1"),
        boundary: Rect {
            x: 31.75,
            y: 26.3149,
            w: 17.583,
            h: 3.6829,
        },
        ctm: Some(Ctm {
            a: 0.0176,
            b: 0.0,
            c: 0.0,
            d: 0.0176,
            e: 0.0,
            f: 0.0,
        }),
        font: FontId::new("F1"),
        size: 209.0,
        fill: Some(rofd_dom::Color::Rgb(0, 0, 0)),
        codes: vec![TextCode {
            glyph_ids: vec![],
            deltas: vec![(0.0, 0.0)],
            text: "A".into(),
            x: 0.0,
            y: 179.5313,
        }],
        draw_param: None,
    };
    let page = Page {
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: 210.0,
            h: 297.0,
        },
        layers: vec![Layer {
            layer_type: LayerType::Body,
            objects: vec![PageObject::Text(text)],
        }],
        template: None,
    };
    let fonts = font_store();
    let mut scene = Scene::new();
    let mut painter = Painter::new(&mut scene);
    draw_body(
        &mut painter,
        &page,
        &rofd_dom::Resources::default(),
        &fonts,
        (0.0, 0.0),
        1.0,
    );

    let transform = first_glyph_transform(&scene)
        .expect("expected at least one glyph draw (shape 'A' failed?)");
    let mapped = transform * imaging::kurbo::Point::new(0.0, 179.5313);
    assert!(
        (mapped.x - 31.75).abs() < 1e-3,
        "glyph x must include boundary.x=31.75, got {} (old bug: 0)",
        mapped.x
    );
    assert!(
        (mapped.y - 29.4745).abs() < 1e-3,
        "glyph y must be 26.3149 + 0.0176*179.5313 = 29.47, got {} (old bug: 3.16)",
        mapped.y
    );
}
