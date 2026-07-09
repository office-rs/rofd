//! Annotation overlay scene builder: render an OFD page's annotations into a
//! `vello::Scene`.
//!
//! For each [`Annotation`] on a page, render its [`AnnotationPayload`] into a
//! fresh [`vello::Scene`]:
//! - **Markup** (highlight/underline/strikeout): semi-transparent rectangles
//!   drawn over each pair of quad points (~38% opacity, so the underlying text
//!   remains visible through the highlight).
//! - **Freehand**: the annotation's [`PathData`] stroked with `color`/`width`.
//! - **Shape**: a filled and/or stroked rectangle / ellipse / arrow / line
//!   bounded by `rect` (arrow/line use the rect's bounding box in v1).
//! - **Note**: a filled rectangle for the sticky-note icon (popup text is
//!   rendered by the host UI, not in the scene).
//! - **TextBox**: `content` shaped with the annotation's font and drawn inside
//!   `rect` (baseline at `rect.y + size`).
//! - **Stamp**: the referenced image decoded and drawn into `rect`.
//! - **Watermark**: `content` shaped, drawn translucent (`opacity`) and rotated
//!   by `angle` about the rect center.
//!
//! All draw calls use page-local coordinates (no page-origin translation or
//! zoom is applied here - the caller composes those if needed).
//!
//! [`PathData`]: rofd_dom::PathData

use kurbo::{Affine, BezPath, Ellipse, Rect as KurboRect, Shape, Stroke};
use peniko::Fill;
use rofd_dom::{Annotation, AnnotationPayload, Color, FontId, Rect, Resources, ShapeKind};
use vello::Scene;

use crate::color::to_peniko;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::FontStore;

/// Alpha (0.0-1.0) for markup highlights. ~38% opacity keeps the underlying
/// text legible through the highlight rectangle.
const MARKUP_ALPHA: f32 = 96.0 / 255.0;

/// Tolerance for converting kurbo shapes (Rect/Ellipse) to BezPath. kurbo
/// subdivides curves until they deviate from the true curve by less than this;
/// `1e-3` is the kurbo standard. A tolerance of 0.0 would never converge for
/// the ellipse's arcs (infinite subdivision), so a small positive value is
/// required.
const SHAPE_TOLERANCE: f64 = 1e-3;

/// Build the annotation overlay scene for one page's annotations.
///
/// Iterates every annotation and renders its payload into a new `Scene`. Each
/// payload variant is drawn in page-local coordinates; missing fonts / images
/// are skipped silently (the caller can warn).
pub fn build_annotation_scene(
    anns: &[Annotation],
    res: &Resources,
    fonts: &FontStore,
) -> Scene {
    let mut scene = Scene::new();
    for ann in anns {
        match &ann.payload {
            AnnotationPayload::Markup { quad_points, color } => {
                draw_markup(&mut scene, quad_points, color);
            }
            AnnotationPayload::Freehand { path, color, width } => {
                draw_freehand(&mut scene, path, *color, *width);
            }
            AnnotationPayload::Shape {
                kind,
                rect,
                stroke,
                fill,
                width,
            } => {
                draw_shape(&mut scene, *kind, rect, *stroke, *fill, *width);
            }
            AnnotationPayload::Note {
                rect,
                color,
                content: _,
                icon: _,
            } => {
                // v1: draw the note's icon as a filled rectangle (popup text is
                // rendered by the host UI, not in the scene).
                let bez = rofd_rect_to_kurbo(rect).to_path(SHAPE_TOLERANCE);
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    to_peniko(*color),
                    None,
                    &bez,
                );
            }
            AnnotationPayload::TextBox {
                rect,
                content,
                font,
                size,
                color,
            } => {
                let text = TextParams { content, font, size: *size, color: *color };
                draw_text_in_rect(&mut scene, &text, rect, fonts);
            }
            AnnotationPayload::Stamp { rect, image } => {
                draw_stamp(&mut scene, rect, image, res);
            }
            AnnotationPayload::Watermark {
                rect,
                content,
                opacity,
                angle,
                font,
                size,
                color,
            } => {
                let text = TextParams { content, font, size: *size, color: *color };
                draw_watermark_text(&mut scene, &text, *opacity, *angle, rect, fonts);
            }
        }
    }
    scene
}

/// Highlight/underline/strikeout: draw a semi-transparent rectangle over each
/// pair of quad points. OFD quad points come in pairs (start/end of a
/// highlighted line segment); the rectangle spans the two points.
fn draw_markup(scene: &mut Scene, quad_points: &[rofd_dom::Point], color: &Color) {
    let translucent = to_peniko(*color).with_alpha(MARKUP_ALPHA);
    for chunk in quad_points.chunks(2) {
        if chunk.len() == 2 {
            let (p0, p1) = (chunk[0], chunk[1]);
            // kurbo::Rect is corner-based: (x0, y0, x1, y1). Use min/max so the
            // rectangle is well-formed regardless of point ordering.
            let rect = KurboRect::new(
                p0.x.min(p1.x),
                p0.y.min(p1.y),
                p0.x.max(p1.x),
                p0.y.max(p1.y),
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                translucent,
                None,
                &rect,
            );
        }
    }
}

/// Freehand: convert the PathData to a BezPath and stroke it.
fn draw_freehand(scene: &mut Scene, path: &rofd_dom::PathData, color: Color, width: f64) {
    let bez = path_to_bezpath(path);
    scene.stroke(
        &Stroke::new(width),
        Affine::IDENTITY,
        to_peniko(color),
        None,
        &bez,
    );
}

/// Shape: fill and/or stroke a rectangle / ellipse / arrow / line bounded by
/// `rect`. Arrow/line use the rect's bounding box in v1 (true arrowhead geometry
/// is deferred). Fill is applied first, then stroke (painter's order).
fn draw_shape(
    scene: &mut Scene,
    kind: ShapeKind,
    rect: &Rect,
    stroke: Color,
    fill: Option<Color>,
    width: f64,
) {
    let bez: BezPath = match kind {
        ShapeKind::Rect | ShapeKind::Arrow | ShapeKind::Line => {
            rofd_rect_to_kurbo(rect).to_path(SHAPE_TOLERANCE)
        }
        ShapeKind::Ellipse => {
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let radii = (rect.w / 2.0, rect.h / 2.0);
            Ellipse::new(center, radii, 0.0).to_path(SHAPE_TOLERANCE)
        }
    };
    if let Some(fc) = fill {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            to_peniko(fc),
            None,
            &bez,
        );
    }
    scene.stroke(
        &Stroke::new(width),
        Affine::IDENTITY,
        to_peniko(stroke),
        None,
        &bez,
    );
}

/// Stamp: decode the referenced image and draw it into `rect` (translate to the
/// rect origin, scale to the rect's w/h).
fn draw_stamp(scene: &mut Scene, rect: &Rect, image: &rofd_dom::ImageId, res: &Resources) {
    let bytes = match res.images.get(image) {
        Some(b) => b,
        None => return,
    };
    let img = match decode_image(bytes) {
        Some(img) => img,
        None => return,
    };
    // `draw_image` fills a rect (0, 0, img.width, img.height) in the image's
    // natural pixel dimensions, so the transform maps that onto (x, y, w, h):
    // translate to the rect origin, then scale by (w / img_w, h / img_h).
    let scale_x = if img.width > 0 {
        rect.w / img.width as f64
    } else {
        1.0
    };
    let scale_y = if img.height > 0 {
        rect.h / img.height as f64
    } else {
        1.0
    };
    let place = Affine::translate((rect.x, rect.y)) * Affine::scale_non_uniform(scale_x, scale_y);
    scene.draw_image(&img, place);
}

/// Bundle the text fields common to TextBox and Watermark payloads.
struct TextParams<'a> {
    content: &'a str,
    font: &'a FontId,
    size: f64,
    color: Color,
}

/// Shape `content` with the annotation's font and draw the glyphs into `rect`
/// (non-rotated, opaque). The glyph baseline is placed at `rect.y + size` (so
/// text sits just inside the top of the rect), and the pen x starts at `rect.x`.
///
/// Skips silently if the font can't be resolved or shaping yields no glyphs.
fn draw_text_in_rect(
    scene: &mut Scene,
    text: &TextParams,
    rect: &Rect,
    fonts: &FontStore,
) {
    let font = match fonts.resolve_or_default(text.font) {
        Some(f) => f,
        None => return,
    };
    let glyphs = shape_positioned(text.content, text.font, text.size, fonts);
    let affine = Affine::translate((rect.x, rect.y));
    draw_glyph_run(scene, font, &glyphs, affine, to_peniko(text.color), text.size);
}

/// Watermark text: shaped, drawn translucent (`opacity`) and rotated by `angle`
/// about the rect center.
///
/// Skips silently if the font can't be resolved or shaping yields no glyphs.
fn draw_watermark_text(
    scene: &mut Scene,
    text: &TextParams,
    opacity: f64,
    angle: f64,
    rect: &Rect,
    fonts: &FontStore,
) {
    let font = match fonts.resolve_or_default(text.font) {
        Some(f) => f,
        None => return,
    };
    let glyphs = shape_positioned(text.content, text.font, text.size, fonts);
    let translucent = to_peniko(text.color).with_alpha(opacity as f32);
    // Rotate about the rect center. The shaped glyphs are positioned relative
    // to the rect origin; the rotation composes as: translate(center) * rotate.
    let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let affine = Affine::translate(center) * Affine::rotate(angle);
    draw_glyph_run(scene, font, &glyphs, affine, translucent, text.size);
}

/// Shape `content` and offset each glyph's y by `size` (baseline drop), so text
/// sits just inside the top of its bounding rect. Annotation text uses the
/// shaper's natural x/y directly (unlike body text which uses document deltas).
fn shape_positioned(
    content: &str,
    font_id: &FontId,
    size: f64,
    fonts: &FontStore,
) -> Vec<vello::Glyph> {
    let glyphs = fonts.shape(font_id, content, size);
    let baseline_offset = size as f32;
    glyphs
        .iter()
        .map(|g| vello::Glyph {
            id: g.glyph_id,
            x: g.x,
            y: g.y + baseline_offset,
        })
        .collect()
}

/// Draw a run of positioned glyphs with the given font, transform, brush, and
/// size. No-op if `glyphs` is empty.
fn draw_glyph_run(
    scene: &mut Scene,
    font: &peniko::FontData,
    glyphs: &[vello::Glyph],
    affine: Affine,
    brush: peniko::Color,
    size: f64,
) {
    if glyphs.is_empty() {
        return;
    }
    scene
        .draw_glyphs(font)
        .brush(brush)
        .font_size(size as f32)
        .transform(affine)
        .draw(Fill::NonZero, glyphs.iter().copied());
}

/// Convert a rofd `Rect { x, y, w, h }` (origin + dimensions) to a kurbo
/// `Rect` (corner-based: `(x0, y0, x1, y1)`).
fn rofd_rect_to_kurbo(r: &Rect) -> KurboRect {
    KurboRect::new(r.x, r.y, r.x + r.w, r.y + r.h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId,
        NoteIcon, PathCommand, PathData, Point, Rect, ShapeKind,
    };
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()))
    }

    fn ann(payload: AnnotationPayload, kind: AnnotationKind) -> Annotation {
        Annotation {
            id: AnnotationId::new(),
            kind,
            page: rofd_dom::PageId::new("P0"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload,
        }
    }

    fn build(anns: &[Annotation]) -> Scene {
        let res = Resources::default();
        let fonts = test_font_store();
        build_annotation_scene(anns, &res, &fonts)
    }

    #[test]
    fn empty_annotations_build_empty_scene() {
        let scene = build(&[]);
        // No panic; encoding exists (empty).
        let _ = scene.encoding();
    }

    #[test]
    fn markup_variant_draws_translucent_rects() {
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![
                    Point { x: 0.0, y: 0.0 },
                    Point { x: 10.0, y: 10.0 },
                    Point { x: 20.0, y: 0.0 },
                    Point { x: 30.0, y: 10.0 },
                ],
                color: Color::Rgb(255, 255, 0),
            },
            AnnotationKind::Highlight,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn markup_with_single_quad_point_does_not_panic() {
        // Odd number of quad points: the lone point forms no pair and is skipped.
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 0.0 }],
                color: Color::Rgb(255, 0, 0),
            },
            AnnotationKind::Strikeout,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn freehand_variant_strokes_path() {
        let ann = ann(
            AnnotationPayload::Freehand {
                path: PathData {
                    commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(50.0, 50.0)],
                },
                color: Color::Rgb(0, 0, 255),
                width: 1.5,
            },
            AnnotationKind::Freehand,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn shape_variant_rect_draws_fill_and_stroke() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
            },
            AnnotationKind::Shape(ShapeKind::Rect),
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn shape_variant_ellipse_draws_without_panic() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Ellipse,
                rect: Rect { x: 10.0, y: 10.0, w: 60.0, h: 30.0 },
                stroke: Color::Rgb(255, 0, 0),
                fill: None,
                width: 1.0,
            },
            AnnotationKind::Shape(ShapeKind::Ellipse),
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn shape_variant_arrow_uses_rect_bbox() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Arrow,
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 10.0 },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 1.0,
            },
            AnnotationKind::Shape(ShapeKind::Arrow),
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn note_variant_draws_filled_rect() {
        let ann = ann(
            AnnotationPayload::Note {
                rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
                color: Color::Rgb(255, 200, 0),
                content: "a note".into(),
                icon: NoteIcon::Help,
            },
            AnnotationKind::Note,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn textbox_variant_shapes_and_draws_text() {
        let ann = ann(
            AnnotationPayload::TextBox {
                rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 30.0 },
                content: "hello".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn stamp_variant_missing_image_skips_silently() {
        // No images in resources -> draw_stamp skips without panic.
        let ann = ann(
            AnnotationPayload::Stamp {
                rect: Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 },
                image: ImageId::new("missing"),
            },
            AnnotationKind::Stamp,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn stamp_variant_with_image_draws_into_rect() {
        // Build a 2x2 red PNG and place it in a 100x50 rect via resources.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(
            2,
            2,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
        )
        .unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());

        let mut res = Resources::default();
        res.images.insert(ImageId::new("I1"), png_bytes);
        let fonts = test_font_store();

        let ann = ann(
            AnnotationPayload::Stamp {
                rect: Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 },
                image: ImageId::new("I1"),
            },
            AnnotationKind::Stamp,
        );
        let scene = build_annotation_scene(&[ann], &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn watermark_variant_draws_rotated_translucent_text() {
        let ann = ann(
            AnnotationPayload::Watermark {
                rect: Rect { x: 0.0, y: 0.0, w: 200.0, h: 100.0 },
                content: "DRAFT".into(),
                opacity: 0.3,
                angle: std::f64::consts::FRAC_PI_4, // 45 degrees
                font: FontId::new("F2"),
                size: 48.0,
                color: Color::Rgb(200, 200, 200),
            },
            AnnotationKind::Watermark,
        );
        let scene = build(&[ann]);
        let _ = scene.encoding();
    }

    #[test]
    fn all_seven_variants_in_one_scene() {
        // Exercise every AnnotationPayload variant together to ensure the match
        // is exhaustive and the scene builds without panic.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbImage::from_raw(1, 1, vec![0, 255, 0]).unwrap();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let png_bytes = Arc::new(buf.into_inner());
        let mut res = Resources::default();
        res.images.insert(ImageId::new("I1"), png_bytes);
        let fonts = test_font_store();

        let anns = vec![
            ann(
                AnnotationPayload::Markup {
                    quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                    color: Color::Rgb(255, 255, 0),
                },
                AnnotationKind::Highlight,
            ),
            ann(
                AnnotationPayload::Freehand {
                    path: PathData {
                        commands: vec![PathCommand::M(0.0, 0.0), PathCommand::L(5.0, 5.0)],
                    },
                    color: Color::Rgb(0, 0, 255),
                    width: 1.0,
                },
                AnnotationKind::Freehand,
            ),
            ann(
                AnnotationPayload::Shape {
                    kind: ShapeKind::Rect,
                    rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                    stroke: Color::Rgb(0, 0, 0),
                    fill: Some(Color::Rgb(255, 255, 255)),
                    width: 2.0,
                },
                AnnotationKind::Shape(ShapeKind::Rect),
            ),
            ann(
                AnnotationPayload::Note {
                    rect: Rect { x: 10.0, y: 10.0, w: 40.0, h: 20.0 },
                    color: Color::Rgb(255, 200, 0),
                    content: "note".into(),
                    icon: NoteIcon::Note,
                },
                AnnotationKind::Note,
            ),
            ann(
                AnnotationPayload::TextBox {
                    rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 30.0 },
                    content: "hi".into(),
                    font: FontId::new("F1"),
                    size: 12.0,
                    color: Color::Rgb(0, 0, 0),
                },
                AnnotationKind::TextBox,
            ),
            ann(
                AnnotationPayload::Stamp {
                    rect: Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 },
                    image: ImageId::new("I1"),
                },
                AnnotationKind::Stamp,
            ),
            ann(
                AnnotationPayload::Watermark {
                    rect: Rect { x: 0.0, y: 0.0, w: 200.0, h: 100.0 },
                    content: "DRAFT".into(),
                    opacity: 0.3,
                    angle: std::f64::consts::FRAC_PI_4,
                    font: FontId::new("F2"),
                    size: 48.0,
                    color: Color::Rgb(200, 200, 200),
                },
                AnnotationKind::Watermark,
            ),
        ];
        let scene = build_annotation_scene(&anns, &res, &fonts);
        let _ = scene.encoding();
    }

    #[test]
    fn rofd_rect_to_kurbo_uses_corners() {
        // rofd Rect { x, y, w, h } -> kurbo Rect { x0, y0, x1, y1 } (corners).
        let r = Rect { x: 10.0, y: 20.0, w: 100.0, h: 30.0 };
        let k = rofd_rect_to_kurbo(&r);
        assert_eq!(k.x0, 10.0);
        assert_eq!(k.y0, 20.0);
        assert_eq!(k.x1, 110.0); // x + w
        assert_eq!(k.y1, 50.0); // y + h
    }
}
