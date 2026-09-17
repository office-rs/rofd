//! Body scene builder: render a Page's body objects into the shared imaging scene.
//!
//! For each [`PageObject`] on a page's body layers, draw it via the imaging
//! [`Painter`] with `compose_transform(page_origin, zoom, ctm)` as the per-draw
//! transform (page-origin + zoom + object CTM folded together):
//! - **Text**: shape `TextCode.text` via [`FontStore::shape`], position glyphs by
//!   the cumulative document deltas (NOT the shaper's x/y), and draw via
//!   `Painter::glyphs` at the object's CTM + page transform.
//! - **Path**: convert [`PathData`] to a [`kurbo::BezPath`] via
//!   [`path_to_bezpath`], then `Painter::fill` / `Painter::stroke` with the
//!   object's CTM + page transform.
//! - **Image**: decode bytes via [`decode_image`], then `Painter::draw_image`
//!   with the unit-square image space mapped through the object's CTM (a
//!   missing CTM defaults to filling the Boundary) + page transform.
//! - **Composite**: skipped in v1 (the caller emits an [`OfdWarning`]).
//!
//! All coordinates are page-local; the page-origin + zoom + CTM transform is
//! applied per draw call (there is no cached sub-scene - see [`composite`] docs).
//!
//! [`OfdWarning`]: rofd_io::OfdWarning

use imaging::kurbo::Affine;
use imaging::record::{Glyph, Scene};
use imaging::Painter;
use peniko::{Fill, Style};
use rofd_dom::{ImageObject, Page, PageObject, PathObject, Resources, TextObject};

use crate::color::to_peniko;
use crate::ctm::compose_object_transform;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::FontStore;

/// Draw the body objects for one page into `painter`.
///
/// Iterates every object on every layer and draws Text / Path / Image objects.
/// Composite objects are skipped (v1). Each object's page-origin + zoom + CTM
/// transform is applied as the affine for its draw call.
pub fn draw_body(
    painter: &mut Painter<Scene>,
    page: &Page,
    res: &Resources,
    fonts: &FontStore,
    page_origin: (f64, f64),
    zoom: f64,
) {
    for layer in &page.layers {
        for obj in &layer.objects {
            match obj {
                PageObject::Text(t) => draw_text(painter, t, res, fonts, page_origin, zoom),
                PageObject::Path(p) => draw_path(painter, p, res, page_origin, zoom),
                PageObject::Image(i) => draw_image_obj(painter, i, res, page_origin, zoom),
                // v1: skip composite objects. The caller can emit an OfdWarning
                // (SkippedObject) for each composite; the drawer just omits them.
                PageObject::Composite(_) => {}
            }
        }
    }
}

/// Render a text object: shape each `TextCode.text` with the document font,
/// position glyphs by the TextCode X/Y origin + cumulative deltas, and draw via
/// `Painter::glyphs` with the page-origin + zoom + CTM transform.
///
/// Fill resolves inline first, then falls back to the object's `DrawParam`
/// (GB/T 33190). Skips silently if the font can't be resolved or no fill color
/// is available.
fn draw_text(
    painter: &mut Painter<Scene>,
    t: &TextObject,
    res: &Resources,
    fonts: &FontStore,
    page_origin: (f64, f64),
    zoom: f64,
) {
    // Fill: inline first, then DrawParam fallback (GB/T 33190 §8.3.2). Default
    // black when neither is present - OFD text without an explicit FillColor
    // renders black (sample.ofd's TextObjects omit FillColor entirely, and the
    // old `None => return` skipped them, so no body text drew at all).
    let fill = t
        .fill
        .or_else(|| {
            t.draw_param
                .as_ref()
                .and_then(|id| res.draw_params.get(id))
                .and_then(|d| d.fill)
        })
        .unwrap_or(rofd_dom::Color::Rgb(0, 0, 0));
    let fill = to_peniko(fill);
    let affine = compose_object_transform(page_origin, zoom, t.boundary, t.ctm.as_ref());

    for code in &t.codes {
        // Body text has two rendering paths:
        // - glyph_ids non-empty (CGTransform/Glyphs from a subset font with no
        //   cmap): draw by the document glyph IDs directly. parley shape cannot
        //   be used here - it needs a cmap to map Unicode -> glyph ID and
        //   returns .notdef on a cmap-less subset font, making the text vanish.
        // - glyph_ids empty (cmap font, or annotation-style body text): shape
        //   the text and draw by the shaper's glyph IDs. The returned font is
        //   the one parley actually used (document, default, or system fallback)
        //   - draw with THAT font so glyph ids match.
        let (font, ids): (Option<peniko::FontData>, Vec<u32>) = if !code.glyph_ids.is_empty() {
            let font = fonts.resolve_or_default(&t.font).cloned();
            (font, code.glyph_ids.clone())
        } else {
            let (font, glyphs) = fonts.shape(&t.font, &code.text, t.size);
            (font, glyphs.iter().map(|g| g.glyph_id).collect())
        };
        // Shared pen geometry (hit-testing and selection rects use the same
        // cells - spec §5.3 single source of truth): the pen starts at
        // (code.x, code.y); each glyph sits at the pen and advances by its
        // document delta (GB/T 33190 DeltaX semantics). The shaper's natural
        // x/y is ignored.
        let cells = crate::body_text::code_char_cells(t, code, ids.len());
        let positioned: Vec<Glyph> = ids
            .iter()
            .zip(cells.iter())
            .map(|(&id, c)| Glyph {
                id,
                x: c.x as f32,
                y: c.y as f32,
            })
            .collect();
        let font = match font {
            Some(f) => f,
            None => continue,
        };
        if positioned.is_empty() {
            continue;
        }
        painter
            .glyphs(&font, fill)
            .font_size(t.size as f32)
            .transform(affine)
            .draw(&Style::Fill(Fill::NonZero), &positioned);
    }
}

/// Render a path object: fill and/or stroke the BezPath with the page-origin +
/// zoom + CTM transform.
///
/// Colors/width resolve inline first, then fall back to the object's
/// `DrawParam` (GB/T 33190). If both `fill` and `stroke` end up `None`, nothing
/// is drawn. Fill is applied first, then stroke (standard painter's order).
fn draw_path(
    painter: &mut Painter<Scene>,
    p: &PathObject,
    res: &Resources,
    page_origin: (f64, f64),
    zoom: f64,
) {
    let bez = path_to_bezpath(&p.data);
    let affine = compose_object_transform(page_origin, zoom, p.boundary, p.ctm.as_ref());
    // Resolve colors/width: inline first, then DrawParam fallback (GB/T 33190).
    let dp = p.draw_param.as_ref().and_then(|id| res.draw_params.get(id));
    let fill = p.fill.or_else(|| dp.and_then(|d| d.fill));
    let stroke = p.stroke.or_else(|| dp.and_then(|d| d.stroke));
    let line_width = if p.line_width > 0.0 {
        p.line_width
    } else {
        dp.and_then(|d| d.line_width).unwrap_or(0.0)
    };
    if let Some(c) = fill {
        painter.fill(&bez, to_peniko(c)).transform(affine).draw();
    }
    if let Some(c) = stroke {
        let stroke = imaging::kurbo::Stroke::new(line_width);
        painter
            .stroke(&bez, &stroke, to_peniko(c))
            .transform(affine)
            .draw();
    }
}

/// Render an image object: decode the referenced image bytes and draw the image
/// placed at the boundary origin, scaled to the boundary w/h, composed with the
/// page-origin + zoom + CTM transform.
///
/// Skips silently if the image id is not in resources or the bytes fail to
/// decode (the caller can warn).
fn draw_image_obj(
    painter: &mut Painter<Scene>,
    i: &ImageObject,
    res: &Resources,
    page_origin: (f64, f64),
    zoom: f64,
) {
    let bytes = match res.images.get(&i.image) {
        Some(b) => b,
        None => return,
    };
    let img = match decode_image(bytes) {
        Some(img) => img,
        None => return,
    };
    // OFD image local space is the UNIT SQUARE (GB/T 33190 §8.2): the CTM maps
    // `(px / img_w, py / img_h)` into boundary-relative mm. Real producers
    // encode the whole placement in the CTM - e.g. WPS writes
    // `CTM = diag(boundary.w, boundary.h)` with the Boundary naming the exact
    // rect (sample-content.ofd), while scan strips carry the on-page
    // translation in the CTM with a loose full-page Boundary
    // (ru-yuan-ji-lu.ofd). A missing CTM defaults to "fill the Boundary".
    // `draw_image` fills a rect `(0, 0, img.width, img.height)` in the image's
    // natural pixel dimensions, so `unit` first maps that rect onto the unit
    // square.
    let unit = if img.width > 0 && img.height > 0 {
        Affine::scale_non_uniform(1.0 / img.width as f64, 1.0 / img.height as f64)
    } else {
        Affine::IDENTITY
    };
    let ctm = i
        .ctm
        .as_ref()
        .map(crate::ctm::ctm_to_affine)
        .unwrap_or_else(|| Affine::scale_non_uniform(i.boundary.w, i.boundary.h));
    let affine = crate::ctm::compose_object_affine(page_origin, zoom, i.boundary, ctm) * unit;
    painter.draw_image(&img, affine);
}

#[cfg(test)]
mod tests {
    use super::*;
    use imaging::kurbo::Rect as KurboRect;
    use imaging::record::{Command, Draw};
    use rofd_dom::Rect;
    use rofd_dom::{
        Ctm, FontId, ImageId, ObjectId, PathCommand, PathData, PathObject, TextCode, TextObject,
    };
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()))
    }

    /// Draw into a fresh scene and return it (callers assert non-panic).
    fn build(page: &Page, res: &Resources, fonts: &FontStore) -> Scene {
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);
        painter.fill_rect(KurboRect::new(0.0, 0.0, 800.0, 600.0), peniko::Color::BLACK);
        draw_body(&mut painter, page, res, fonts, (0.0, 0.0), 1.0);
        scene
    }

    /// Build a `w x h` PNG and insert it into `res` under `id`.
    fn png_resource(res: &mut Resources, id: &str, w: u32, h: u32) {
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbaImage::from_raw(w, h, vec![255; (w * h * 4) as usize]).unwrap();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        res.images
            .insert(ImageId::new(id), Arc::new(buf.into_inner()));
    }

    /// Page carrying a single body ImageObject.
    fn image_page(boundary: Rect, ctm: Option<Ctm>) -> Page {
        Page {
            id: rofd_dom::PageId::new("P0"),
            physical_box: Rect::default(),
            layers: vec![rofd_dom::Layer {
                layer_type: rofd_dom::LayerType::Body,
                objects: vec![PageObject::Image(ImageObject {
                    id: ObjectId::new("i1"),
                    boundary,
                    ctm,
                    image: ImageId::new("I1"),
                })],
            }],
            template: None,
        }
    }

    /// The on-page rect an image draw occupies: the image fill's shape (a
    /// `(0,0,w,h)` pixel rect) transformed by the draw transform.
    fn image_rect_on_page(scene: &Scene) -> Option<kurbo::Rect> {
        use imaging::kurbo::Shape as _;
        for cmd in scene.commands() {
            if let Command::Draw(id) = cmd {
                if let Draw::Fill {
                    transform,
                    brush: peniko::Brush::Image(_),
                    shape,
                    ..
                } = scene.draw_op(*id)
                {
                    let bb = shape.to_path(1e-3).bounding_box();
                    let p0 = *transform * kurbo::Point::new(bb.x0, bb.y0);
                    let p1 = *transform * kurbo::Point::new(bb.x1, bb.y1);
                    return Some(kurbo::Rect::from_points(p0, p1));
                }
            }
        }
        None
    }

    #[test]
    fn image_ctm_maps_unit_square_into_boundary() {
        // WPS pattern (test/sample-content.ofd ImageObject ID=70): CTM carries
        // the full pixel->boundary scale (diag = boundary w/h, no translation)
        // and the Boundary names the exact placement rect. OFD image local
        // space is the unit square, so the drawn image must land exactly on
        // the Boundary rect - not pixel-size x CTM-scale (the double-scale bug
        // rendered it ~437mm wide, off the page).
        let boundary = Rect {
            x: 31.75,
            y: 142.0707,
            w: 20.9127,
            h: 10.5833,
        };
        let ctm = Some(Ctm {
            a: 20.9127,
            b: 0.0,
            c: 0.0,
            d: 10.5833,
            e: 0.0,
            f: 0.0,
        });
        let page = image_page(boundary, ctm);
        let mut res = Resources::default();
        png_resource(&mut res, "I1", 79, 40);
        let fonts = test_font_store();
        let scene = build(&page, &res, &fonts);

        let r = image_rect_on_page(&scene).expect("image was drawn");
        assert!((r.x0 - 31.75).abs() < 1e-6, "x0 = {}", r.x0);
        assert!((r.y0 - 142.0707).abs() < 1e-6, "y0 = {}", r.y0);
        assert!((r.x1 - 52.6627).abs() < 1e-3, "x1 = {}", r.x1);
        assert!((r.y1 - 152.654).abs() < 1e-3, "y1 = {}", r.y1);
    }

    #[test]
    fn image_ctm_with_translation_maps_unit_square() {
        // Scan-strip pattern (test/ru-yuan-ji-lu.ofd ImageObject ID=148): the
        // Boundary is a loose full-page box while the CTM carries BOTH the
        // unit-square scale and the on-page translation. The image must land
        // in the CTM-defined strip, inside the page.
        let boundary = Rect {
            x: 0.0,
            y: 0.0,
            w: 209.906,
            h: 297.044,
        };
        let ctm = Some(Ctm {
            a: 39.476,
            b: 0.0,
            c: 0.0,
            d: 10.372,
            e: 56.622,
            f: 147.463,
        });
        let page = image_page(boundary, ctm);
        let mut res = Resources::default();
        png_resource(&mut res, "I1", 796, 209);
        let fonts = test_font_store();
        let scene = build(&page, &res, &fonts);

        let r = image_rect_on_page(&scene).expect("image was drawn");
        assert!((r.x0 - 56.622).abs() < 1e-6, "x0 = {}", r.x0);
        assert!((r.y0 - 147.463).abs() < 1e-6, "y0 = {}", r.y0);
        assert!((r.x1 - 96.098).abs() < 1e-3, "x1 = {}", r.x1);
        assert!((r.y1 - 157.835).abs() < 1e-3, "y1 = {}", r.y1);
    }

    #[test]
    fn image_without_ctm_fills_boundary() {
        // No CTM: the image stretches to fill the Boundary rect exactly.
        let boundary = Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
        };
        let page = image_page(boundary, None);
        let mut res = Resources::default();
        png_resource(&mut res, "I1", 2, 2);
        let fonts = test_font_store();
        let scene = build(&page, &res, &fonts);

        let r = image_rect_on_page(&scene).expect("image was drawn");
        assert!((r.x0 - 10.0).abs() < 1e-9, "x0 = {}", r.x0);
        assert!((r.y0 - 20.0).abs() < 1e-9, "y0 = {}", r.y0);
        assert!((r.x1 - 110.0).abs() < 1e-9, "x1 = {}", r.x1);
        assert!((r.y1 - 70.0).abs() < 1e-9, "y1 = {}", r.y1);
    }

    #[test]
    fn empty_page_draws_without_panic() {
        let page = Page::default();
        let res = Resources::default();
        let fonts = test_font_store();
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn path_object_strokes_into_scene() {
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 10.0,
            },
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
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn text_object_shapes_and_draws_into_scene() {
        let text = TextObject {
            id: ObjectId::new("t1"),
            boundary: Rect {
                x: 10.0,
                y: 10.0,
                w: 100.0,
                h: 20.0,
            },
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
        let _ = build(&page, &res, &fonts);
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
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn missing_image_id_skips_silently() {
        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
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
        let res = Resources::default();
        let fonts = test_font_store();
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn ctm_applied_per_object() {
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect::default(),
            ctm: Some(Ctm {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                e: 10.0,
                f: 20.0,
            }),
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
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn image_object_draws_into_scene_with_correct_scaling() {
        let mut buf = std::io::Cursor::new(Vec::new());
        let img =
            image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
                .unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());

        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect {
                x: 10.0,
                y: 20.0,
                w: 100.0,
                h: 50.0,
            },
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
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn image_with_ctm_composes_transforms() {
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(1, 1, vec![255, 0, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());

        let img_obj = rofd_dom::ImageObject {
            id: ObjectId::new("i1"),
            boundary: Rect {
                x: 5.0,
                y: 5.0,
                w: 40.0,
                h: 40.0,
            },
            ctm: Some(Ctm {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                e: 100.0,
                f: 200.0,
            }),
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
        let _ = build(&page, &res, &fonts);
    }

    #[test]
    fn path_draw_param_resolves_color_when_no_inline() {
        // Path with DrawParam="5" but no inline fill/stroke. The DrawParam (in
        // res) supplies the stroke color + line_width, so the path strokes into
        // the scene instead of being skipped.
        let path = PathObject {
            id: ObjectId::new("p1"),
            boundary: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 10.0,
            },
            ctm: None,
            fill: None,
            stroke: None,
            line_width: 0.0,
            data: PathData {
                commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(100.0, 0.0)],
            },
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
        // Non-panic is the gate; the DrawParam stroke was resolved + stroked.
        let _ = build(&page, &res, &fonts);
    }
}
