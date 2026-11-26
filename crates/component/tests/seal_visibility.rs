//! Signature seals are visible in the editor (spec 2026-11-23 §8.3): a
//! signed sample opened in the component paints its seal on the page, as
//! page content between the body and the annotations.

use std::sync::Arc;

use imaging::record::{Command, Draw};
use rofd_component::{OfdComponent, OfdConfig, RenderTarget, ViewEvent};

/// A render target that keeps the last scene it was handed.
struct Capture(rofd_render::Scene);

impl RenderTarget for Capture {
    fn draw_scene(&mut self, scene: &rofd_render::Scene) -> bool {
        self.0 = scene.clone();
        true
    }
    fn size(&self) -> (f64, f64) {
        (800.0, 600.0)
    }
}

/// The placement transforms of the image-brush fills in `scene`.
fn image_placements(scene: &rofd_render::Scene) -> Vec<imaging::kurbo::Affine> {
    scene
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Draw(id) => match scene.draw_op(*id) {
                Draw::Fill {
                    brush: imaging::peniko::Brush::Image(_),
                    transform,
                    ..
                } => Some(*transform),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn sample() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../io/tests/fixtures/sign/SESV4SignDoc.ofd"
    ))
    .expect("signed sample fixture")
}

const VIEWPORT: (f64, f64) = (800.0, 600.0);

fn rendered_component(bytes: &[u8]) -> (OfdComponent, rofd_dom::OfdDocument, Capture) {
    let report = rofd_io::parse_ofd(bytes).expect("parse_ofd");
    let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    c.load_document(report.document.clone());
    c.handle_event(&ViewEvent::Resize {
        width: VIEWPORT.0,
        height: VIEWPORT.1,
    });
    let mut target = Capture(rofd_render::Scene::default());
    c.render(&mut target);
    (c, report.document, target)
}

#[test]
fn sesv4_sample_paints_its_seal() {
    let (component, document, target) = rendered_component(&sample());
    assert_eq!(document.signatures.items.len(), 1, "the sample is signed");
    let zoom = component.zoom();
    // The sample's stamp boundary is (50, 50, 40, 40) mm on page 1; the
    // scene is in viewport coordinates, so the page origin is added.
    let viewport = rofd_render::Viewport {
        scroll: (0.0, 0.0),
        zoom,
        size: VIEWPORT,
        page_gap: 20.0,
    };
    let (origin_x, origin_y) =
        rofd_render::composite::page_origin(&document, &viewport, 0).expect("page 0 origin");
    let top_left = imaging::kurbo::Point::new(origin_x + 50.0 * zoom, origin_y + 50.0 * zoom);
    assert!(
        image_placements(&target.0)
            .iter()
            .any(|place| *place * imaging::kurbo::Point::new(0.0, 0.0) == top_left),
        "a seal is drawn at the stamp boundary (zoom {zoom}, origin {origin_x}, {origin_y})"
    );
}

#[test]
fn an_unsigned_document_paints_no_seal() {
    let bytes = rofd_io::write_ofd(&rofd_dom::OfdDocument::default()).expect("blank package");
    let (_, document, target) = rendered_component(&bytes);
    assert!(document.signatures.items.is_empty());
    assert!(image_placements(&target.0).is_empty());
}
