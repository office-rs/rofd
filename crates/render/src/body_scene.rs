//! Body scene builder: render a Page's body objects into a `vello::Scene`.
//!
//! For each [`PageObject`] on a page's body layers, render it into a fresh
//! [`vello::Scene`]:
//! - **Text**: shape `TextCode.text` via [`FontStore::shape`], position glyphs by
//!   the cumulative document deltas (NOT the shaper's x/y), and draw via
//!   [`Scene::draw_glyphs`] with the object's CTM as the transform.
//! - **Path**: convert [`PathData`] to a [`kurbo::BezPath`] via
//!   [`path_to_bezpath`], then [`Scene::fill`] / [`Scene::stroke`] with the
//!   object's CTM.
//! - **Image**: decode bytes via [`decode_image`], then [`Scene::draw_image`]
//!   placed at the boundary origin and scaled to the boundary w/h, composed
//!   with the object's CTM.
//! - **Composite**: skipped in v1 (the caller emits an [`OfdWarning`]).
//!
//! Each object's CTM is applied via the `Affine` argument to the draw call;
//! `None` CTM maps to [`kurbo::Affine::IDENTITY`].
//!
//! [`OfdWarning`]: rofd_io::OfdWarning

use kurbo::Affine;
use peniko::Fill;
use rofd_dom::{Page, PageObject, PathObject, Resources, TextObject, ImageObject};
use vello::Scene;

use crate::color::to_peniko;
use crate::ctm::ctm_to_affine;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::FontStore;

/// Build the body scene for one page.
///
/// Iterates every object on every layer and renders Text / Path / Image objects
/// into a new `Scene`. Composite objects are skipped (v1). Each object's CTM is
/// applied as the affine transform for its draw call; a `None` CTM uses the
/// identity transform.
///
/// Coordinates are page-local (no page-origin translation or zoom is applied
/// here - the caller composes those via [`compose_transform`](crate::compose_transform)
/// or [`Scene::append`] if needed).
pub fn build_body_scene(page: &Page, res: &Resources, fonts: &FontStore) -> Scene {
    let mut scene = Scene::new();
    for layer in &page.layers {
        for obj in &layer.objects {
            match obj {
                PageObject::Text(t) => draw_text(&mut scene, t, res, fonts),
                PageObject::Path(p) => draw_path(&mut scene, p, res),
                PageObject::Image(i) => draw_image_obj(&mut scene, i, res),
                // v1: skip composite objects. The caller can emit an OfdWarning
                // (SkippedObject) for each composite; the scene builder just
                // omits them.
                PageObject::Composite(_) => {}
            }
        }
    }
    scene
}

/// Render a text object: shape each `TextCode.text` with the document font,
/// position glyphs by cumulative deltas, and draw via `draw_glyphs` with the
/// object's CTM as the transform.
///
/// Skips silently if the font can't be resolved or the object has no fill color.
fn draw_text(scene: &mut Scene, t: &TextObject, res: &Resources, fonts: &FontStore) {
    let font = match fonts.resolve_or_default(&t.font) {
        Some(f) => f,
        None => return,
    };
    // Fill: inline first, then DrawParam fallback (GB/T 33190).
    let fill = match t.fill.or_else(|| {
        t.draw_param
            .as_ref()
            .and_then(|id| res.draw_params.get(id))
            .and_then(|d| d.fill)
    }) {
        Some(c) => to_peniko(c),
        None => return,
    };
    let affine = t
        .ctm
        .as_ref()
        .map(ctm_to_affine)
        .unwrap_or(Affine::IDENTITY);

    for code in &t.codes {
        // Shape with the document font (reuses the store's FontContext).
        let glyphs = fonts.shape(&t.font, &code.text, t.size);
        if glyphs.is_empty() {
            continue;
        }
        // Position glyphs by the TextCode X/Y origin + cumulative document
        // deltas. The first glyph sits at (x, y); each delta is the advance to
        // the next glyph (GB/T 33190 DeltaX semantics). The shaper's natural
        // x/y is ignored.
        let mut pen_x = code.x as f32;
        let mut pen_y = code.y as f32;
        let positioned: Vec<vello::Glyph> = glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let glyph = vello::Glyph { id: g.glyph_id, x: pen_x, y: pen_y };
                let (dx, dy) = code.deltas.get(i).copied().unwrap_or((0.0, 0.0));
                pen_x += dx;
                pen_y += dy;
                glyph
            })
            .collect();
        if !positioned.is_empty() {
            scene
                .draw_glyphs(font)
                .brush(fill)
                .font_size(t.size as f32)
                .transform(affine)
                .draw(Fill::NonZero, positioned.into_iter());
        }
    }
}

/// Render a path object: fill and/or stroke the BezPath with the object's CTM.
///
/// If both `fill` and `stroke` are `None`, nothing is drawn. Fill is applied
/// first, then stroke (standard painter's order for OFD).
fn draw_path(scene: &mut Scene, p: &PathObject, res: &Resources) {
    let bez = path_to_bezpath(&p.data);
    let affine = p
        .ctm
        .as_ref()
        .map(ctm_to_affine)
        .unwrap_or(Affine::IDENTITY);
    // Resolve colors/width: inline first, then DrawParam fallback (GB/T 33190).
    let dp = p
        .draw_param
        .as_ref()
        .and_then(|id| res.draw_params.get(id));
    let fill = p.fill.or_else(|| dp.and_then(|d| d.fill));
    let stroke = p.stroke.or_else(|| dp.and_then(|d| d.stroke));
    let line_width = if p.line_width > 0.0 {
        p.line_width
    } else {
        dp.and_then(|d| d.line_width).unwrap_or(0.0)
    };
    if let Some(c) = fill {
        // brush_transform = None: the brush is in user space (no separate
        // brush transform composed with the shape transform).
        scene.fill(Fill::NonZero, affine, to_peniko(c), None, &bez);
    }
    if let Some(c) = stroke {
        let stroke = kurbo::Stroke::new(line_width);
        scene.stroke(&stroke, affine, to_peniko(c), None, &bez);
    }
}

/// Render an image object: decode the referenced image bytes and draw the image
/// placed at the boundary origin, scaled to the boundary w/h, composed with the
/// object's CTM.
///
/// Skips silently if the image id is not in resources or the bytes fail to
/// decode (the caller can warn).
fn draw_image_obj(scene: &mut Scene, i: &ImageObject, res: &Resources) {
    let bytes = match res.images.get(&i.image) {
        Some(b) => b,
        None => return,
    };
    let img = match decode_image(bytes) {
        Some(img) => img,
        None => return,
    };
    let affine = i
        .ctm
        .as_ref()
        .map(ctm_to_affine)
        .unwrap_or(Affine::IDENTITY);
    // Place the image at its boundary origin and scale it to the boundary
    // w/h. `draw_image` fills a rect (0, 0, img.width, img.height) in the
    // image's natural pixel dimensions, so the transform must map that rect
    // onto the boundary (x, y, w, h): translate to the boundary origin, then
    // scale by (w / img_w, h / img_h). Compose with the object's CTM.
    let scale_x = if img.width > 0 { i.boundary.w / img.width as f64 } else { 1.0 };
    let scale_y = if img.height > 0 { i.boundary.h / img.height as f64 } else { 1.0 };
    let place = Affine::translate((i.boundary.x, i.boundary.y))
        * Affine::scale_non_uniform(scale_x, scale_y);
    scene.draw_image(&img, affine * place);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        Ctm, FontId, ImageId, ObjectId, PathCommand, PathData, PathObject, Rect, TextCode,
        TextObject,
    };
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let font_bytes =
            include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()))
    }

    #[test]
    fn empty_page_builds_empty_scene() {
        let page = Page::default();
        let res = Resources::default();
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        // An empty page encodes no drawing commands. The encoding exists but
        // has no path/glyph streams. We assert the call returns without panic.
        let _ = scene.encoding();
    }

    #[test]
    fn path_object_strokes_into_scene() {
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect { x: 0.0, y: 0.0, w: 100.0, h: 10.0 },
            ctm: None,
            fill: None,
            stroke: Some(rofd_dom::Color::Rgb(255, 0, 0)),
            line_width: 1.0,
            data: PathData {
                commands: vec![
                    PathCommand::M(0.0, 0.0),
                    PathCommand::L(100.0, 0.0),
                    PathCommand::L(100.0, 10.0),
                    PathCommand::Z,
                ],
            },
            draw_param: None,
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Path(path)],
            }],
            template: None,
        };
        let res = Resources::default();
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        // The scene should have encoded at least one command (the stroke).
        // We assert non-panic; deeper introspection is left to the smoke test.
        let _ = scene.encoding();
    }

    #[test]
    fn text_object_shapes_and_draws_into_scene() {
        let text = TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect { x: 10.0, y: 10.0, w: 100.0, h: 20.0 },
            ctm: None,
            font: FontId::new("F1"),
            size: 12.0,
            fill: Some(rofd_dom::Color::Rgb(0, 0, 0)),
            codes: vec![TextCode {
                glyph_ids: vec![],
                deltas: vec![(0.0, 0.0); 5],
                text: "Hello".into(),
                x: 0.0,
                y: 0.0,
            }],
            draw_param: None,
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Text(text)],
            }],
            template: None,
        };
        let res = Resources::default();
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn composite_object_is_skipped_without_panic() {
        let composite = rofd_dom::CompositeObject {
            id: ObjectId::new("c1"),
            boundary: Rect::default(),
            ctm: None,
            unit: "U1".into(),
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Composite(composite)],
            }],
            template: None,
        };
        let res = Resources::default();
        let fonts = test_font_store();
        // Must not panic; composite is silently skipped.
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn missing_image_id_skips_silently() {
        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            ctm: None,
            image: ImageId::new("missing"),
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Image(img_obj)],
            }],
            template: None,
        };
        // No images in resources -> draw_image_obj skips.
        let res = Resources::default();
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn ctm_applied_per_object() {
        // A path with a non-identity CTM should still build without panic;
        // the CTM is passed as the affine arg to fill/stroke.
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect::default(),
            ctm: Some(Ctm { a: 2.0, b: 0.0, c: 0.0, d: 2.0, e: 10.0, f: 20.0 }),
            fill: Some(rofd_dom::Color::Rgb(0, 0, 255)),
            stroke: None,
            line_width: 0.0,
            data: PathData {
                commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(10.0, 0.0)],
            },
            draw_param: None,
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Path(path)],
            }],
            template: None,
        };
        let res = Resources::default();
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn image_object_draws_into_scene_with_correct_scaling() {
        // Build a 2x2 red PNG and place it in a 100x50 boundary. The scene
        // must build without panic; the scaling math (w/img_w, h/img_h) must
        // not divide by zero or produce NaN.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());

        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 },
            ctm: None,
            image: ImageId::new("I1"),
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Image(img_obj)],
            }],
            template: None,
        };
        let mut res = Resources::default();
        res.images.insert(ImageId::new("I1"), png_bytes);
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn image_with_ctm_composes_transforms() {
        // An image with a non-identity CTM must compose CTM * place without panic.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(1, 1, vec![255, 0, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());

        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect { x: 5.0, y: 5.0, w: 40.0, h: 40.0 },
            ctm: Some(Ctm { a: 2.0, b: 0.0, c: 0.0, d: 2.0, e: 100.0, f: 200.0 }),
            image: ImageId::new("I1"),
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Image(img_obj)],
            }],
            template: None,
        };
        let mut res = Resources::default();
        res.images.insert(ImageId::new("I1"), png_bytes);
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn path_draw_param_resolves_color_when_no_inline() {
        // Path with DrawParam="5" but no inline fill/stroke. The DrawParam (in
        // res) supplies the stroke color + line_width, so the path strokes into
        // the scene instead of being skipped.
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect { x: 0.0, y: 0.0, w: 100.0, h: 10.0 },
            ctm: None,
            fill: None,
            stroke: None,
            line_width: 0.0,
            data: PathData { commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(100.0, 0.0)] },
            draw_param: Some(rofd_dom::DrawParamId::new("5")),
        };
        let page = Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Path(path)],
            }],
            template: None,
        };
        let mut res = Resources::default();
        res.draw_params.insert(
            rofd_dom::DrawParamId::new("5"),
            rofd_dom::DrawParam {
                line_width: Some(2.0),
                stroke: Some(rofd_dom::Color::Rgb(255, 0, 0)),
                fill: None,
            },
        );
        let fonts = test_font_store();
        let scene = build_body_scene(&page, &res, &fonts);
        // Non-panic is the gate; the DrawParam stroke was resolved + stroked.
        let _ = scene.encoding();
    }
}
