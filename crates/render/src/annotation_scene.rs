//! Annotation overlay drawer: render an OFD page's annotations into the shared
//! imaging scene via a [`Painter`].
//!
//! For each [`Annotation`] on a page, draw its [`AnnotationPayload`] with the
//! page-origin + zoom transform (`base = compose_transform(page_origin, zoom, None)`)
//! applied per draw call:
//! - **Markup** (highlight/underline/strikeout): semi-transparent rectangles
//!   drawn over each pair of quad points (~38% opacity, so the underlying text
//!   remains visible through the highlight). **Squiggly** is the exception:
//!   it strokes a wavy Q-curve path (alternating up/down) instead of filling.
//! - **Freehand**: the annotation's [`PathData`] stroked with `color`/`width`.
//! - **Shape**: a filled and/or stroked rectangle / ellipse bounded by `rect`,
//!   a polygon/polyline built from `points`, or a **line/arrow** drawn from the
//!   two endpoints in `points` (direction = `points[0] -> points[1]`, the
//!   arrowhead sits at `points[1]`). When `points` is empty (legacy/external
//!   OFD carrying only a boundary), line/arrow fall back to the rect's
//!   TL->BR diagonal.
//! - **Note**: a filled rectangle for the sticky-note icon (popup text is
//!   rendered by the host UI, not in the scene).
//! - **TextBox**: `content` shaped with the annotation's font and drawn inside
//!   `rect` (baseline at `rect.y + size`).
//! - **Stamp**: the referenced image decoded and drawn into `rect`.
//! - **Watermark**: `content` shaped, drawn translucent (`opacity`) and rotated
//!   by `angle` about the rect center.
//!
//! All draw calls use page-local coordinates; `base` (page-origin + zoom) is
//! applied per draw (no cached sub-scene - see [`composite`] docs).
//!
//! [`PathData`]: rofd_dom::PathData

use imaging::kurbo::{Affine, BezPath, Ellipse, Rect as KurboRect, Shape, Stroke};
use imaging::record::{Glyph, Scene};
use imaging::Painter;
use peniko::{Fill, FontData, Style};
use rofd_dom::{
    Annotation, AnnotationKind, AnnotationPayload, Color, FontId, Rect, Resources, ShapeKind,
};

use crate::color::to_peniko;
use crate::ctm::compose_transform;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::FontStore;

/// Alpha (0.0-1.0) for markup highlights. ~38% opacity keeps the underlying
/// text legible through the highlight rectangle.
const MARKUP_ALPHA: f32 = 96.0 / 255.0;

/// Amplitude (page-local units) of the Squiggly wavy underline. The control
/// point of each Q-curve segment alternates +/- this value around the baseline.
/// 1.0 matches the visual weight of a typical squiggly underline.
const SQUIGGLY_AMPLITUDE: f64 = 1.0;

/// Stroke width (page-local mm) for Underline/Strikeout/Squiggly markup lines.
/// sample.ofd uses 0.2-0.25mm; the old 1.0mm drew ~4px bars at 100% zoom that
/// looked like blocking rectangles. 0.3mm ~ 1.1px at 96 DPI.
const MARKUP_LINE_WIDTH: f64 = 0.3;

/// Tolerance for converting kurbo shapes (Rect/Ellipse) to BezPath. kurbo
/// subdivides curves until they deviate from the true curve by less than this;
/// `1e-3` is the kurbo standard. A tolerance of 0.0 would never converge for
/// the ellipse's arcs (infinite subdivision), so a small positive value is
/// required.
const SHAPE_TOLERANCE: f64 = 1e-3;

/// Draw the annotation overlay for one page's annotations into `painter`.
///
/// Iterates every annotation and draws its payload. Each payload variant is
/// drawn in page-local coordinates with `base` (page-origin + zoom) applied per
/// draw call; missing fonts / images are skipped silently (the caller can warn).
pub fn draw_annotations(
    painter: &mut Painter<Scene>,
    anns: &[Annotation],
    res: &Resources,
    fonts: &FontStore,
    page_origin: (f64, f64),
    zoom: f64,
) {
    let base = compose_transform(page_origin, zoom, None);
    for ann in anns {
        match &ann.payload {
            AnnotationPayload::Markup { quad_points, color } => {
                draw_markup(painter, &ann.kind, quad_points, color, base);
            }
            AnnotationPayload::Freehand { path, color, width } => {
                draw_freehand(painter, path, *color, *width, base);
            }
            AnnotationPayload::Shape {
                kind,
                rect,
                stroke,
                fill,
                width,
                points,
            } => {
                draw_shape(painter, *kind, rect, *stroke, *fill, *width, points, base);
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
                painter.fill(&bez, to_peniko(*color)).transform(base).draw();
            }
            AnnotationPayload::TextBox {
                rect,
                content,
                font,
                size,
                color,
            } => {
                let text = TextParams {
                    content,
                    font,
                    size: *size,
                    color: *color,
                };
                draw_text_in_rect(painter, &text, rect, fonts, base);
            }
            AnnotationPayload::Stamp { rect, image } => {
                draw_stamp(painter, rect, image, res, base);
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
                let text = TextParams {
                    content,
                    font,
                    size: *size,
                    color: *color,
                };
                draw_watermark_text(painter, &text, *opacity, *angle, rect, fonts, base);
            }
        }
    }
}

/// Highlight/underline/strikeout: draw a semi-transparent rectangle over each
/// pair of quad points. OFD quad points come in pairs (start/end of a
/// highlighted line segment); the rectangle spans the two points.
///
/// Squiggly is the exception: instead of a filled rectangle it strokes a wavy
/// Q-curve path (alternating up/down around the baseline `p0.y`) so the
/// signature squiggle is visible.
fn draw_markup(
    painter: &mut Painter<Scene>,
    kind: &AnnotationKind,
    quad_points: &[rofd_dom::Point],
    color: &Color,
    base: Affine,
) {
    // Highlight keeps ~38% alpha so text shows through; Squiggly/Underline/
    // Strikeout render at full opacity (they're stroked lines, not fills).
    let alpha = if matches!(kind, AnnotationKind::Highlight) {
        MARKUP_ALPHA
    } else {
        1.0
    };
    let peniko_color = to_peniko(*color).with_alpha(alpha);
    for chunk in quad_points.chunks(2) {
        if chunk.len() == 2 {
            let (p0, p1) = (chunk[0], chunk[1]);
            match kind {
                AnnotationKind::Squiggly => {
                    // 波浪线 baseline 在 quad 底部 (文字下方), NOT p0.y (which
                    // is the Appearance.Boundary top -> drew on the text top).
                    let baseline_y = p0.y.max(p1.y);
                    let x0 = p0.x.min(p1.x);
                    let x1 = p0.x.max(p1.x);
                    let path = squiggly_path(
                        rofd_dom::Point {
                            x: x0,
                            y: baseline_y,
                        },
                        rofd_dom::Point {
                            x: x1,
                            y: baseline_y,
                        },
                    );
                    painter
                        .stroke(&path, &Stroke::new(MARKUP_LINE_WIDTH), peniko_color)
                        .transform(base)
                        .draw();
                }
                AnnotationKind::Underline => {
                    // 底线: a straight line at the quad pair's bottom edge
                    // (NOT a filled rect, which would cover the text).
                    let y = p0.y.max(p1.y);
                    let mut path = BezPath::new();
                    path.move_to((p0.x.min(p1.x), y));
                    path.line_to((p0.x.max(p1.x), y));
                    painter
                        .stroke(&path, &Stroke::new(MARKUP_LINE_WIDTH), peniko_color)
                        .transform(base)
                        .draw();
                }
                AnnotationKind::Strikeout => {
                    // 中线: a straight line through the quad pair's vertical
                    // midpoint (NOT a filled rect, which would cover the text).
                    let y = (p0.y + p1.y) / 2.0;
                    let mut path = BezPath::new();
                    path.move_to((p0.x.min(p1.x), y));
                    path.line_to((p0.x.max(p1.x), y));
                    painter
                        .stroke(&path, &Stroke::new(MARKUP_LINE_WIDTH), peniko_color)
                        .transform(base)
                        .draw();
                }
                _ => {
                    // Highlight: 半透明填充矩形 (文字透过高亮可见).
                    // kurbo::Rect is corner-based: (x0, y0, x1, y1). Use min/max
                    // so the rectangle is well-formed regardless of point order.
                    let rect = KurboRect::new(
                        p0.x.min(p1.x),
                        p0.y.min(p1.y),
                        p0.x.max(p1.x),
                        p0.y.max(p1.y),
                    );
                    painter.fill(rect, peniko_color).transform(base).draw();
                }
            }
        }
    }
}

/// Build a wavy Q-curve BezPath from `p0` to `p1`: `steps` segments, each a
/// quadratic Bezier whose control point alternates above/below the baseline
/// (`p0.y`) by `SQUIGGLY_AMPLITUDE`. Produces the classic squiggly-underline
/// shape. Panics never (steps clamped to >= 1).
fn squiggly_path(p0: rofd_dom::Point, p1: rofd_dom::Point) -> BezPath {
    let steps = 20;
    let dx = (p1.x - p0.x) / steps as f64;
    let mut path = BezPath::new();
    path.move_to((p0.x, p0.y));
    for i in 0..steps {
        let xm = p0.x + dx * (i as f64 + 0.5);
        let x1 = p0.x + dx * (i as f64 + 1.0);
        // Alternate the control point above/below the baseline.
        let ym = if i % 2 == 0 {
            p0.y - SQUIGGLY_AMPLITUDE
        } else {
            p0.y + SQUIGGLY_AMPLITUDE
        };
        path.quad_to((xm, ym), (x1, p0.y));
    }
    path
}

/// Freehand: convert the PathData to a BezPath and stroke it.
fn draw_freehand(
    painter: &mut Painter<Scene>,
    path: &rofd_dom::PathData,
    color: Color,
    width: f64,
    base: Affine,
) {
    let bez = path_to_bezpath(path);
    painter
        .stroke(&bez, &Stroke::new(width), to_peniko(color))
        .transform(base)
        .draw();
}

/// Shape: fill and/or stroke a rectangle / ellipse / polygon / polyline, or
/// draw a line/arrow from its two endpoints.
///
/// - **Rect**: rect bounding box; fill (if Some) + stroke.
/// - **Ellipse**: kurbo `Ellipse` from the rect; fill (if Some) + stroke.
/// - **Polygon**: BezPath through `points` (closed); fill (if Some) + stroke.
/// - **PolyLine**: BezPath through `points` (open); stroke only (fill is
///   typically None, but if provided it is still applied to the open path).
/// - **Line**: a single stroked segment `points[0] -> points[1]` (stroke only,
///   no fill - `fill` is ignored). Falls back to the rect's TL->BR diagonal
///   when `points` has fewer than 2 entries.
/// - **Arrow**: the same segment stroked as the shaft, plus a filled triangle
///   head at `points[1]` oriented along the shaft direction (filled with the
///   stroke color; the `fill` field is ignored). Same fallback as Line.
///
/// For Polygon/PolyLine an empty `points` vector draws nothing. Fill is
/// applied first, then stroke (painter's order).
#[allow(clippy::too_many_arguments)] // all params describe one shape draw call
fn draw_shape(
    painter: &mut Painter<Scene>,
    kind: ShapeKind,
    rect: &Rect,
    stroke: Color,
    fill: Option<Color>,
    width: f64,
    points: &[rofd_dom::Point],
    base: Affine,
) {
    match kind {
        ShapeKind::Line | ShapeKind::Arrow => {
            let (p0, p1) = line_endpoints(rect, points);
            // Shaft: a single stroked segment p0 -> p1.
            let mut shaft = BezPath::new();
            shaft.move_to((p0.x, p0.y));
            shaft.line_to((p1.x, p1.y));
            painter
                .stroke(&shaft, &Stroke::new(width), to_peniko(stroke))
                .transform(base)
                .draw();
            // Arrow: fill a triangle head at p1 along the shaft direction.
            // Line stops here (stroke only).
            if matches!(kind, ShapeKind::Arrow) {
                let head = arrow_head_path(p0, p1, width);
                painter
                    .fill(&head, to_peniko(stroke))
                    .transform(base)
                    .draw();
            }
        }
        ShapeKind::Rect => {
            let bez = rofd_rect_to_kurbo(rect).to_path(SHAPE_TOLERANCE);
            fill_then_stroke(painter, &bez, fill, stroke, width, base);
        }
        ShapeKind::Ellipse => {
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let radii = (rect.w / 2.0, rect.h / 2.0);
            let bez = Ellipse::new(center, radii, 0.0).to_path(SHAPE_TOLERANCE);
            fill_then_stroke(painter, &bez, fill, stroke, width, base);
        }
        ShapeKind::Polygon | ShapeKind::PolyLine => {
            if points.is_empty() {
                return;
            }
            let mut path = BezPath::new();
            path.move_to((points[0].x, points[0].y));
            for p in &points[1..] {
                path.line_to((p.x, p.y));
            }
            if matches!(kind, ShapeKind::Polygon) {
                path.close_path();
            }
            fill_then_stroke(painter, &path, fill, stroke, width, base);
        }
    }
}

/// Resolve the two endpoints of a Line/Arrow. Prefers `points` (direction =
/// `points[0] -> points[1]`, the arrowhead tip is `points[1]`); falls back to
/// the rect's TL->BR diagonal when `points` has fewer than 2 entries (legacy
/// or external OFD that carries only a boundary and no vertices).
fn line_endpoints(rect: &Rect, points: &[rofd_dom::Point]) -> (rofd_dom::Point, rofd_dom::Point) {
    if points.len() >= 2 {
        (points[0], points[1])
    } else {
        (
            rofd_dom::Point {
                x: rect.x,
                y: rect.y,
            },
            rofd_dom::Point {
                x: rect.x + rect.w,
                y: rect.y + rect.h,
            },
        )
    }
}

/// Arrowhead tip-to-corner side length as a multiple of the stroke width
/// (matches the reference arrow in `test/sample.ofd`:
/// side 1.7639mm at LineWidth 0.3528mm => exactly 1.5x).
const ARROW_HEAD_SIDE_PER_WIDTH: f64 = 5.0 * 1.5;

/// Arrowhead half-angle between the shaft axis and each tip->corner edge
/// (25 degrees, measured from the reference arrow in `test/sample.ofd`).
const ARROW_HEAD_HALF_ANGLE: f64 = 25.0 * std::f64::consts::PI / 180.0;

/// Build the filled triangle arrowhead at `tip` (`p1`), oriented along the
/// `p0 -> p1` shaft direction. The head size scales with the stroke `width`
/// (base corners at `5 x width` from the tip, +/-25 degrees off the shaft
/// axis), matching `io::annotation_geom::arrow_path_points` so the rendered
/// head matches the serialized AbbreviatedData head. A degenerate `width`
/// (0.0, e.g. a parsed PathObject without LineWidth) falls back to the
/// default 1pt stroke. Returns a closed BezPath (M-L-L-Z) ready to fill.
pub(crate) fn arrow_head_path(p0: rofd_dom::Point, p1: rofd_dom::Point, width: f64) -> BezPath {
    // Shared with `composite::draw_drag_preview` for the Arrow drag preview.
    let side = width.max(0.3528) * ARROW_HEAD_SIDE_PER_WIDTH;
    let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
    let (c, s) = (angle.cos(), angle.sin());
    // Base corners: `back` along the reversed shaft, `perp` perpendicular.
    let back = side * ARROW_HEAD_HALF_ANGLE.cos();
    let perp = side * ARROW_HEAD_HALF_ANGLE.sin();
    let mut path = BezPath::new();
    path.move_to((p1.x - c * back - s * perp, p1.y - s * back + c * perp));
    path.line_to((p1.x, p1.y));
    path.line_to((p1.x - c * back + s * perp, p1.y - s * back - c * perp));
    path.close_path();
    path
}

/// Fill (if `Some`) then stroke a BezPath with `base` applied per draw call
/// (painter's order: fill first, then stroke on top).
fn fill_then_stroke(
    painter: &mut Painter<Scene>,
    bez: &BezPath,
    fill: Option<Color>,
    stroke: Color,
    width: f64,
    base: Affine,
) {
    if let Some(fc) = fill {
        painter.fill(bez, to_peniko(fc)).transform(base).draw();
    }
    painter
        .stroke(bez, &Stroke::new(width), to_peniko(stroke))
        .transform(base)
        .draw();
}

/// Stamp: decode the referenced image and draw it into `rect` (translate to the
/// rect origin, scale to the rect's w/h).
fn draw_stamp(
    painter: &mut Painter<Scene>,
    rect: &Rect,
    image: &rofd_dom::ImageId,
    res: &Resources,
    base: Affine,
) {
    let bytes = match res.images.get(image) {
        Some(b) => b,
        None => return,
    };
    let img = match decode_image(bytes) {
        Some(img) => img,
        None => return,
    };
    // `draw_image` fills a rect (0, 0, img.width, img.height) in the image's
    // natural pixel dimensions, so the place transform maps that onto
    // (x, y, w, h): translate to the rect origin, then scale by
    // (w / img_w, h / img_h).
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
    painter.draw_image(&img, base * place);
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
    painter: &mut Painter<Scene>,
    text: &TextParams,
    rect: &Rect,
    fonts: &FontStore,
    base: Affine,
) {
    let (font, glyphs) = shape_positioned(text.content, text.font, text.size, fonts);
    let font = match font {
        Some(f) => f,
        None => return,
    };
    let affine = base * Affine::translate((rect.x, rect.y));
    draw_glyph_run(
        painter,
        &font,
        &glyphs,
        affine,
        to_peniko(text.color),
        text.size,
    );
}

/// Watermark text: shaped, drawn translucent (`opacity`) and rotated by `angle`
/// about the rect center.
///
/// Skips silently if the font can't be resolved or shaping yields no glyphs.
fn draw_watermark_text(
    painter: &mut Painter<Scene>,
    text: &TextParams,
    opacity: f64,
    angle: f64,
    rect: &Rect,
    fonts: &FontStore,
    base: Affine,
) {
    let (font, glyphs) = shape_positioned(text.content, text.font, text.size, fonts);
    let font = match font {
        Some(f) => f,
        None => return,
    };
    let translucent = to_peniko(text.color).with_alpha(opacity as f32);
    // Rotate about the rect center. The shaped glyphs are positioned relative
    // to the rect origin; the rotation composes as: translate(center) * rotate.
    let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let affine = base * Affine::translate(center) * Affine::rotate(angle);
    draw_glyph_run(painter, &font, &glyphs, affine, translucent, text.size);
}

/// Shape `content` and offset each glyph's y by `size` (baseline drop), so text
/// sits just inside the top of its bounding rect. Annotation text uses the
/// shaper's natural x/y directly (unlike body text which uses document deltas).
/// Returns the font that shaped the glyphs (caller draws with it).
fn shape_positioned(
    content: &str,
    font_id: &FontId,
    size: f64,
    fonts: &FontStore,
) -> (Option<FontData>, Vec<Glyph>) {
    let (font, glyphs) = fonts.shape(font_id, content, size);
    let baseline_offset = size as f32;
    let positioned: Vec<Glyph> = glyphs
        .iter()
        .map(|g| Glyph {
            id: g.glyph_id,
            x: g.x,
            y: g.y + baseline_offset,
        })
        .collect();
    (font, positioned)
}

/// Draw a run of positioned glyphs with the given font, transform, brush, and
/// size. No-op if `glyphs` is empty.
fn draw_glyph_run(
    painter: &mut Painter<Scene>,
    font: &FontData,
    glyphs: &[Glyph],
    affine: Affine,
    brush: peniko::Color,
    size: f64,
) {
    if glyphs.is_empty() {
        return;
    }
    painter
        .glyphs(font, brush)
        .font_size(size as f32)
        .transform(affine)
        .draw(&Style::Fill(Fill::NonZero), glyphs);
}

/// Convert a rofd `Rect { x, y, w, h }` (origin + dimensions) to a kurbo
/// `Rect` (corner-based: `(x0, y0, x1, y1)`).
fn rofd_rect_to_kurbo(r: &Rect) -> KurboRect {
    KurboRect::new(r.x, r.y, r.x + r.w, r.y + r.h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use imaging::kurbo::Rect as KurboRect;
    use rofd_dom::{
        AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
        PathCommand, PathData, Point, Rect, ShapeKind,
    };
    use std::sync::Arc;

    fn test_font_store() -> FontStore {
        let font_bytes = include_bytes!("../tests/fixtures/fonts/TestFont.ttf") as &[u8];
        FontStore::from_resources(&Resources::default(), Arc::new(font_bytes.to_vec()))
    }

    fn ann(payload: AnnotationPayload, kind: AnnotationKind) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(1),
            kind,
            page: rofd_dom::PageId::new("P0"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload,
        }
    }

    /// Draw into a fresh scene and return it (callers assert non-panic).
    fn build(anns: &[Annotation]) -> Scene {
        let res = Resources::default();
        let fonts = test_font_store();
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);
        painter.fill_rect(KurboRect::new(0.0, 0.0, 800.0, 600.0), peniko::Color::BLACK);
        draw_annotations(&mut painter, anns, &res, &fonts, (0.0, 0.0), 1.0);
        scene
    }

    #[test]
    fn empty_annotations_draw_without_panic() {
        let _ = build(&[]);
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
        let _ = build(&[ann]);
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
        let _ = build(&[ann]);
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
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_rect_draws_fill_and_stroke() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Rect,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 40.0,
                    h: 20.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 255, 255)),
                width: 2.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Rect),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_ellipse_draws_without_panic() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Ellipse,
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 60.0,
                    h: 30.0,
                },
                stroke: Color::Rgb(255, 0, 0),
                fill: None,
                width: 1.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Ellipse),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_arrow_uses_rect_bbox() {
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Arrow,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 10.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 1.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Arrow),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_line_strokes_diagonal_not_fills_rect() {
        // Line: a stroked line, NOT a filled/stroked rectangle outline (the
        // "line became a rectangle" bug). build() paints one desk-bg fill, so
        // the line must add a stroke but NO fill.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Line,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 50.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 2.0,
                points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 50.0 }],
            },
            AnnotationKind::Shape(ShapeKind::Line),
        );
        let scene = build(&[ann]);
        let (fills, strokes) = count_fills_strokes(&scene);
        assert_eq!(
            fills, 1,
            "Line must not add a fill (desk bg only), got {fills}"
        );
        assert!(strokes >= 1, "Line must stroke a segment, got {strokes}");
    }

    #[test]
    fn shape_arrow_strokes_shaft_and_fills_head() {
        // Arrow: shaft stroked + head filled. build() paints one desk-bg fill,
        // so the arrow adds one stroke (shaft) and one fill (head triangle).
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Arrow,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 50.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 2.0,
                points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 50.0 }],
            },
            AnnotationKind::Shape(ShapeKind::Arrow),
        );
        let scene = build(&[ann]);
        let (fills, strokes) = count_fills_strokes(&scene);
        assert!(strokes >= 1, "Arrow must stroke the shaft, got {strokes}");
        assert!(
            fills >= 2,
            "Arrow must fill the head triangle (+desk bg), got {fills}"
        );
    }

    #[test]
    fn shape_line_uses_points_direction() {
        // points = [TR, BL] -> the stroked line must run TR -> BL, NOT the
        // bbox's TL -> BR diagonal (the "rect loses which diagonal" bug).
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Line,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 50.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 2.0,
                points: vec![Point { x: 100.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }],
            },
            AnnotationKind::Shape(ShapeKind::Line),
        );
        let scene = build(&[ann]);
        let path = first_stroke_path(&scene).expect("a stroked line");
        use imaging::kurbo::PathSeg;
        let seg = path.segments().next();
        match seg {
            Some(PathSeg::Line(l)) => {
                assert!(
                    (l.p0.x - 100.0).abs() < 1e-9 && (l.p0.y - 0.0).abs() < 1e-9,
                    "segment start = points[0] = (100, 0), got {:?}",
                    l.p0
                );
                assert!(
                    (l.p1.x - 0.0).abs() < 1e-9 && (l.p1.y - 50.0).abs() < 1e-9,
                    "segment end = points[1] = (0, 50), got {:?}",
                    l.p1
                );
            }
            other => panic!("expected a Line segment, got {other:?}"),
        }
    }

    #[test]
    fn shape_line_falls_back_to_rect_diagonal_when_no_points() {
        // Legacy/external OFD with no `points`: fall back to the rect's
        // TL -> BR diagonal so a bare-boundary Line still renders as a line.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Line,
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 80.0,
                    h: 30.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 2.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Line),
        );
        let scene = build(&[ann]);
        let path = first_stroke_path(&scene).expect("a stroked line");
        use imaging::kurbo::PathSeg;
        let seg = path.segments().next();
        match seg {
            Some(PathSeg::Line(l)) => {
                assert!(
                    (l.p0.x - 10.0).abs() < 1e-9,
                    "fallback start = rect TL.x (10), got {:?}",
                    l.p0
                );
                assert!(
                    (l.p1.x - 90.0).abs() < 1e-9,
                    "fallback end = rect BR.x (90), got {:?}",
                    l.p1
                );
                assert!(
                    (l.p1.y - 50.0).abs() < 1e-9,
                    "fallback end = rect BR.y (50), got {:?} - must be the diagonal, not the top edge",
                    l.p1
                );
            }
            other => panic!("expected a Line segment, got {other:?}"),
        }
    }

    #[test]
    fn shape_variant_polygon_draws_from_points() {
        // Polygon: closed path through points, fill + stroke.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Polygon,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: Some(Color::Rgb(255, 0, 0)),
                width: 1.5,
                points: vec![
                    Point { x: 0.0, y: 0.0 },
                    Point { x: 50.0, y: 0.0 },
                    Point { x: 50.0, y: 50.0 },
                    Point { x: 0.0, y: 50.0 },
                ],
            },
            AnnotationKind::Shape(ShapeKind::Polygon),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_polyline_draws_from_points() {
        // PolyLine: open path through points, stroke only.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::PolyLine,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                stroke: Color::Rgb(0, 255, 0),
                fill: None,
                width: 2.0,
                points: vec![
                    Point { x: 10.0, y: 10.0 },
                    Point { x: 40.0, y: 30.0 },
                    Point { x: 70.0, y: 10.0 },
                    Point { x: 90.0, y: 50.0 },
                ],
            },
            AnnotationKind::Shape(ShapeKind::PolyLine),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_polygon_empty_points_does_not_panic() {
        // Empty points: draw_shape returns early without drawing.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Polygon,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 1.0,
                points: vec![],
            },
            AnnotationKind::Shape(ShapeKind::Polygon),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn shape_variant_polygon_single_point_does_not_panic() {
        // Single point: move_to only, no line_to, no close -> no panic.
        let ann = ann(
            AnnotationPayload::Shape {
                kind: ShapeKind::Polygon,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                stroke: Color::Rgb(0, 0, 0),
                fill: None,
                width: 1.0,
                points: vec![Point { x: 5.0, y: 5.0 }],
            },
            AnnotationKind::Shape(ShapeKind::Polygon),
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn markup_squiggly_draws_wavy_path() {
        // Squiggly: wavy Q-curve stroke (not a filled rect).
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 20.0 }, Point { x: 100.0, y: 20.0 }],
                color: Color::Rgb(255, 0, 0),
            },
            AnnotationKind::Squiggly,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn markup_squiggly_single_quad_point_does_not_panic() {
        // Odd quad points: the lone point forms no pair and is skipped.
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 0.0, y: 20.0 }],
                color: Color::Rgb(255, 0, 0),
            },
            AnnotationKind::Squiggly,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn squiggly_path_produces_segments() {
        // The wavy path should have 20 quad_to segments (the move_to is not
        // counted as a segment by kurbo's segments() iterator).
        let p0 = Point { x: 0.0, y: 10.0 };
        let p1 = Point { x: 100.0, y: 10.0 };
        let path = squiggly_path(p0, p1);
        assert_eq!(path.segments().count(), 20);
    }

    #[test]
    fn note_variant_draws_filled_rect() {
        let ann = ann(
            AnnotationPayload::Note {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 40.0,
                    h: 20.0,
                },
                color: Color::Rgb(255, 200, 0),
                content: "a note".into(),
                icon: NoteIcon::Help,
            },
            AnnotationKind::Note,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn textbox_variant_shapes_and_draws_text() {
        let ann = ann(
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 30.0,
                },
                content: "hello".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
            },
            AnnotationKind::TextBox,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn stamp_variant_missing_image_skips_silently() {
        // No images in resources -> draw_stamp skips without panic.
        let ann = ann(
            AnnotationPayload::Stamp {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                },
                image: ImageId::new("missing"),
            },
            AnnotationKind::Stamp,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn stamp_variant_with_image_draws_into_rect() {
        // Build a 2x2 red PNG and place it in a 100x50 rect via resources.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img =
            image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
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
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 50.0,
                },
                image: ImageId::new("I1"),
            },
            AnnotationKind::Stamp,
        );
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);
        draw_annotations(&mut painter, &[ann], &res, &fonts, (0.0, 0.0), 1.0);
        let _ = scene;
    }

    #[test]
    fn watermark_variant_draws_rotated_translucent_text() {
        let ann = ann(
            AnnotationPayload::Watermark {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 100.0,
                },
                content: "DRAFT".into(),
                opacity: 0.3,
                angle: std::f64::consts::FRAC_PI_4, // 45 degrees
                font: FontId::new("F2"),
                size: 48.0,
                color: Color::Rgb(200, 200, 200),
            },
            AnnotationKind::Watermark,
        );
        let _ = build(&[ann]);
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
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 40.0,
                        h: 20.0,
                    },
                    stroke: Color::Rgb(0, 0, 0),
                    fill: Some(Color::Rgb(255, 255, 255)),
                    width: 2.0,
                    points: vec![],
                },
                AnnotationKind::Shape(ShapeKind::Rect),
            ),
            ann(
                AnnotationPayload::Note {
                    rect: Rect {
                        x: 10.0,
                        y: 10.0,
                        w: 40.0,
                        h: 20.0,
                    },
                    color: Color::Rgb(255, 200, 0),
                    content: "note".into(),
                    icon: NoteIcon::Note,
                },
                AnnotationKind::Note,
            ),
            ann(
                AnnotationPayload::TextBox {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 100.0,
                        h: 30.0,
                    },
                    content: "hi".into(),
                    font: FontId::new("F1"),
                    size: 12.0,
                    color: Color::Rgb(0, 0, 0),
                },
                AnnotationKind::TextBox,
            ),
            ann(
                AnnotationPayload::Stamp {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 50.0,
                        h: 50.0,
                    },
                    image: ImageId::new("I1"),
                },
                AnnotationKind::Stamp,
            ),
            ann(
                AnnotationPayload::Watermark {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 200.0,
                        h: 100.0,
                    },
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
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);
        draw_annotations(&mut painter, &anns, &res, &fonts, (0.0, 0.0), 1.0);
        let _ = scene;
    }

    #[test]
    fn rofd_rect_to_kurbo_uses_corners() {
        // rofd Rect { x, y, w, h } -> kurbo Rect { x0, y0, x1, y1 } (corners).
        let r = Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 30.0,
        };
        let k = rofd_rect_to_kurbo(&r);
        assert_eq!(k.x0, 10.0);
        assert_eq!(k.y0, 20.0);
        assert_eq!(k.x1, 110.0); // x + w
        assert_eq!(k.y1, 50.0); // y + h
    }

    fn count_fills_strokes(scene: &Scene) -> (usize, usize) {
        use imaging::record::{Command, Draw};
        let mut fills = 0;
        let mut strokes = 0;
        for cmd in scene.commands() {
            if let Command::Draw(id) = cmd {
                match scene.draw_op(*id) {
                    Draw::Fill { .. } => fills += 1,
                    Draw::Stroke { .. } => strokes += 1,
                    _ => {}
                }
            }
        }
        (fills, strokes)
    }

    /// Extract the first stroked path from the scene (the Line/Arrow shaft),
    /// converted to a BezPath so tests can assert on its endpoints.
    fn first_stroke_path(scene: &Scene) -> Option<BezPath> {
        use imaging::record::{Command, Draw};
        for cmd in scene.commands() {
            if let Command::Draw(id) = cmd {
                if let Draw::Stroke { shape, .. } = scene.draw_op(*id) {
                    return Some(shape.to_path(SHAPE_TOLERANCE));
                }
            }
        }
        None
    }

    #[test]
    fn underline_strokes_line_not_fills_rect() {
        // Underline must stroke a bottom line, NOT fill the quad rect (which
        // covers the text - the "long rectangle blocking text" bug). build()
        // paints one desk-bg fill, so the only fill allowed is that desk bg.
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 14.0 }],
                color: Color::Rgb(0, 239, 89),
            },
            AnnotationKind::Underline,
        );
        let scene = build(&[ann]);
        let (fills, strokes) = count_fills_strokes(&scene);
        assert_eq!(
            fills, 1,
            "Underline must not add a fill (only desk bg expected), got {fills}"
        );
        assert!(
            strokes >= 1,
            "Underline must stroke a bottom line, got {strokes}"
        );
    }

    #[test]
    fn strikeout_strokes_midline_not_fills_rect() {
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 14.0 }],
                color: Color::Rgb(255, 0, 0),
            },
            AnnotationKind::Strikeout,
        );
        let scene = build(&[ann]);
        let (fills, strokes) = count_fills_strokes(&scene);
        assert_eq!(
            fills, 1,
            "Strikeout must not add a fill (only desk bg expected), got {fills}"
        );
        assert!(
            strokes >= 1,
            "Strikeout must stroke a midline, got {strokes}"
        );
    }

    #[test]
    fn squiggly_strokes_wavy_line_not_fills_rect() {
        let ann = ann(
            AnnotationPayload::Markup {
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 14.0 }],
                color: Color::Rgb(0, 164, 247),
            },
            AnnotationKind::Squiggly,
        );
        let scene = build(&[ann]);
        let (fills, strokes) = count_fills_strokes(&scene);
        assert_eq!(
            fills, 1,
            "Squiggly must not add a fill (only desk bg expected), got {fills}"
        );
        assert!(
            strokes >= 1,
            "Squiggly must stroke a wavy line, got {strokes}"
        );
    }

    #[test]
    fn squiggly_path_baseline_is_p0_y() {
        // draw_markup passes the quad bottom (max y) as p0.y; squiggly_path
        // must use p0.y as the wave baseline so the wave sits at the text
        // bottom, not the top (the "wave drew on the text top" bug).
        use imaging::kurbo::PathSeg;
        let p0 = Point { x: 0.0, y: 14.0 };
        let p1 = Point { x: 40.0, y: 10.0 };
        let path = squiggly_path(p0, p1);
        // First segment is a Quad whose start = move_to point = (p0.x, p0.y).
        let first = path.segments().next();
        match first {
            Some(PathSeg::Quad(q)) => {
                assert!(
                    (q.p0.y - 14.0).abs() < 1e-9,
                    "baseline = p0.y = 14 (quad bottom), got {}",
                    q.p0.y
                );
            }
            _ => panic!("expected Quad as first segment"),
        }
    }
}
