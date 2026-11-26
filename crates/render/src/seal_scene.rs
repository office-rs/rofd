//! Signature seals on the page (spec 2026-11-23 §8.3).
//!
//! A seal is page content fixed at signing time. It is drawn after the body
//! and before the annotations, on screen and in print alike.

use imaging::kurbo::Affine;
use imaging::record::Scene;
use imaging::Painter;
use peniko::ImageData;
use rofd_dom::{OfdDocument, PageId, Rect};

use crate::image::decode_image;

/// Draw every seal placed on `page_id` into `scene`, in page-local
/// coordinates at `zoom` pixels per millimetre.
///
/// Only stamps that carry a picture are drawn. A `PageRef` naming no page
/// places nothing, and so does a picture that cannot be decoded.
/// `StampAnnot@Clip` is not applied: the whole picture fills the stamp
/// boundary.
pub fn draw_seals(scene: &mut Scene, doc: &OfdDocument, page_id: &PageId, zoom: f64) {
    let mut painter = Painter::new(scene);
    draw_seals_at(&mut painter, doc, page_id, Affine::scale(zoom));
}

/// [`draw_seals`] for a caller that draws several pages into one scene:
/// `base` maps page-local millimetres into that scene.
pub fn draw_seals_at(
    painter: &mut Painter<Scene>,
    doc: &OfdDocument,
    page_id: &PageId,
    base: Affine,
) {
    for (stamp, picture) in doc.signatures.stamps_on(page_id) {
        let Some(image) = decode_image(&picture.png) else {
            continue;
        };
        painter.draw_image(&image, base * place(&stamp.boundary, &image));
    }
}

/// The transform that fills `rect` (millimetres) with `image` at its
/// natural pixel size.
fn place(rect: &Rect, image: &ImageData) -> Affine {
    let scale_x = if image.width > 0 {
        rect.w / f64::from(image.width)
    } else {
        1.0
    };
    let scale_y = if image.height > 0 {
        rect.h / f64::from(image.height)
    } else {
        1.0
    };
    Affine::translate((rect.x, rect.y)) * Affine::scale_non_uniform(scale_x, scale_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use imaging::kurbo::Point;
    use imaging::peniko::Brush;
    use imaging::record::{Command, Draw};
    use rofd_dom::{PageId, Rect, SealPicture, SignatureInfo, SignatureKind, StampAnnot};

    /// A 2x1 red PNG, generated in-test (deterministic, no fixture file).
    fn png() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// A document with one page and one signature stamp on `page`.
    fn doc_with(picture: Option<Vec<u8>>, page: &str) -> OfdDocument {
        let mut doc = OfdDocument::default();
        doc.pages.push(rofd_dom::Page {
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 210.0,
                h: 297.0,
            },
            ..Default::default()
        });
        doc.signatures.items.push(SignatureInfo {
            id: "1".to_owned(),
            kind: SignatureKind::Seal,
            base_loc: String::new(),
            stamps: vec![StampAnnot {
                page_id: PageId::new(page),
                boundary: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 40.0,
                },
                clip: None,
            }],
            seal_picture: picture.map(|png| SealPicture {
                png: Arc::new(png),
                width: 30.0,
                height: 40.0,
            }),
        });
        doc
    }

    /// The placement transforms of the image-brush fills in `scene`.
    fn image_placements(scene: &Scene) -> Vec<Affine> {
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => match scene.draw_op(*id) {
                    Draw::Fill {
                        brush: Brush::Image(_),
                        transform,
                        ..
                    } => Some(*transform),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn draw_seals_paints_one_image_per_stamp() {
        let doc = doc_with(Some(png()), "P0");
        let mut scene = Scene::new();
        draw_seals(&mut scene, &doc, &PageId::new("P0"), 2.0);
        let placements = image_placements(&scene);
        assert_eq!(placements.len(), 1, "one stamp, one image");
        let place = placements[0];
        // The image is 2x1 px and fills (10, 20, 30, 40) mm at 2 px/mm.
        let top_left = place * Point::new(0.0, 0.0);
        let bottom_right = place * Point::new(2.0, 1.0);
        assert_eq!(top_left, Point::new(20.0, 40.0));
        assert_eq!(bottom_right, Point::new(80.0, 120.0));
    }

    #[test]
    fn draw_seals_skips_stamps_without_picture_and_unknown_pages() {
        let page = PageId::new("P0");
        let mut scene = Scene::new();
        draw_seals(&mut scene, &doc_with(None, "P0"), &page, 2.0);
        assert!(
            image_placements(&scene).is_empty(),
            "no picture, nothing to draw"
        );
        scene = Scene::new();
        draw_seals(
            &mut scene,
            &doc_with(Some(png()), "no-such-page"),
            &page,
            2.0,
        );
        assert!(
            image_placements(&scene).is_empty(),
            "a PageRef naming no page places nothing"
        );
        scene = Scene::new();
        draw_seals(
            &mut scene,
            &doc_with(Some(b"not a png".to_vec()), "P0"),
            &page,
            2.0,
        );
        assert!(
            image_placements(&scene).is_empty(),
            "an undecodable picture"
        );
    }
}
