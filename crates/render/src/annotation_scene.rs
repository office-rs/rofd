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
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, Rect, Resources,
    ShapeKind,
};

use crate::color::to_peniko;
use crate::ctm::compose_transform;
use crate::image::decode_image;
use crate::path::path_to_bezpath;
use crate::text::FontStore;

/// Alpha (0.0-1.0) for markup highlights. ~38% opacity keeps the underlying
/// text legible through the highlight rectangle.
const MARKUP_ALPHA: f32 = 96.0 / 255.0;

/// Amplitude (page-local mm) of the Squiggly wavy underline. The control
/// point of each Q-curve segment alternates +/- this value around the
/// baseline. 0.5625mm measured from reference-authoring-tool squiggles; must
/// equal io's `annotation_geom::SQUIGGLY_AMPLITUDE` so the rendered wave
/// matches the serialized one.
const SQUIGGLY_AMPLITUDE: f64 = 0.5625;

/// X-span (page-local mm) of one Squiggly Q-curve half-wave. Fixed so wave
/// density is independent of quad width (text length): the number of arcs
/// grows with the selection while the wavelength stays constant. 0.5925mm
/// measured from reference squiggles; must equal io's
/// `annotation_geom::SQUIGGLY_HALF_WAVE`.
const SQUIGGLY_HALF_WAVE: f64 = 0.5925;

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

/// An in-progress IME composition to preview inline in the TextBox that owns
/// it. Flat boxes (`spans` empty) render the composition inside the text flow
/// (`content[..offset] + text + content[offset..]`), so the text after the
/// composition point moves aside instead of being overpainted - the same
/// layout the committed text will take.
pub struct PreeditPreview<'a> {
    /// Annotation the composition targets.
    pub annotation: &'a AnnotationId,
    /// Char offset (not bytes) where the composition will commit.
    pub offset: usize,
    /// Composition text (not yet in the dom).
    pub text: &'a str,
}

/// Draw the annotation overlay for one page's annotations into `painter`.
///
/// Iterates every annotation and draws its payload. Each payload variant is
/// drawn in page-local coordinates with `base` (page-origin + zoom) applied per
/// draw call; missing fonts / images are skipped silently (the caller can warn).
///
/// `preedit` previews an active IME composition inline in its flat TextBox;
/// pass `None` for print/export (a composition is never part of the output).
pub fn draw_annotations(
    painter: &mut Painter<Scene>,
    anns: &[Annotation],
    res: &Resources,
    fonts: &FontStore,
    page_origin: (f64, f64),
    zoom: f64,
    preedit: Option<&PreeditPreview<'_>>,
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
                border,
                spans,
            } => {
                // Bordered text box (文本框): stroke the frame around the text.
                // Inset by LineWidth/2 to mirror the serialized border path
                // (reference convention: frame starts 0.1764 inside a
                // Boundary at LineWidth 0.3528).
                if let Some(border_color) = border {
                    let bez = rofd_rect_to_kurbo(rect)
                        .inflate(-0.1764, -0.1764)
                        .to_path(SHAPE_TOLERANCE);
                    painter
                        .stroke(&bez, &Stroke::new(0.3528), to_peniko(*border_color))
                        .transform(base)
                        .draw();
                }
                let runs = crate::text::layout_runs(content, spans, font, *size, *color);
                if spans.is_empty() {
                    // Inline composition preview: shape the text WITH the
                    // composition inserted, so the following text wraps/shifts
                    // instead of being overpainted by a detached overlay.
                    let composed = preedit
                        .filter(|p| p.annotation == &ann.id)
                        .map(|p| compose_preedit_text(content, p.offset, p.text));
                    let layout = match composed.as_deref() {
                        Some(text) => crate::text::layout_runs(text, &[], font, *size, *color),
                        None => runs,
                    };
                    let text = composed.as_deref().unwrap_or(content);
                    draw_text_in_rect(painter, rect, text, &layout, *color, fonts, base);
                } else {
                    draw_text_in_rect(painter, rect, content, &runs, *color, fonts, base);
                }
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
                    // 波浪线 baseline 在 quad 底部上方一个振幅处 (文字下方,
                    // NOT p0.y which is the quad top -> drew on the text top).
                    // 与序列化端一致: 严格阅读器把 PathObject 裁剪到其
                    // Boundary, baseline 贴底边 (y=h) 会把波形的下半裁掉。
                    let baseline_y = p0.y.max(p1.y) - SQUIGGLY_AMPLITUDE;
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

/// Build a wavy Q-curve BezPath from `p0` to `p1`. Half-waves have a FIXED
/// `SQUIGGLY_HALF_WAVE` (0.5925mm, reference-measured) x-span: the number of
/// arcs grows with the quad width while their density stays constant, so a
/// 4-char and a whole-line squiggly share the same wavelength. Each arc is a
/// quadratic Bezier whose control point alternates above/below the baseline
/// (`p0.y`) by `SQUIGGLY_AMPLITUDE`, producing the classic squiggly-underline
/// shape. A span that is not a multiple of the half-wave ends with one partial
/// arc landing exactly on `p1.x`. Degenerate spans (< 1e-6) yield an M-only
/// path.
fn squiggly_path(p0: rofd_dom::Point, p1: rofd_dom::Point) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((p0.x, p0.y));
    let span = p1.x - p0.x;
    if span.abs() < 1e-6 {
        return path;
    }
    let dir = span.signum();
    let full = (span.abs() / SQUIGGLY_HALF_WAVE).floor();
    let mut x = p0.x;
    let mut up = true;
    for _ in 0..full as usize {
        let x_next = x + dir * SQUIGGLY_HALF_WAVE;
        // Alternate the control point above/below the baseline.
        let ym = if up {
            p0.y - SQUIGGLY_AMPLITUDE
        } else {
            p0.y + SQUIGGLY_AMPLITUDE
        };
        path.quad_to(((x + x_next) / 2.0, ym), (x_next, p0.y));
        x = x_next;
        up = !up;
    }
    // Remainder (< one half-wave): a final partial arc ending exactly at
    // p1.x so the wave stays flush with the text end.
    if (p1.x - x).abs() > 1e-6 {
        let ym = if up {
            p0.y - SQUIGGLY_AMPLITUDE
        } else {
            p0.y + SQUIGGLY_AMPLITUDE
        };
        path.quad_to(((x + p1.x) / 2.0, ym), (p1.x, p0.y));
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
            // Inset by half the stroke width, mirroring the serialized form:
            // strict readers clip a PathObject to its Boundary, so io insets
            // the stroked rect by LineWidth/2 (reference convention). Keeping
            // render identical means what rofd shows is what other readers
            // show after a save.
            let inset = width / 2.0;
            let bez = rofd_rect_to_kurbo(rect)
                .inflate(-inset, -inset)
                .to_path(SHAPE_TOLERANCE);
            fill_then_stroke(painter, &bez, fill, stroke, width, base);
        }
        ShapeKind::Ellipse => {
            // Same inset as the serialized form: center unchanged, both radii
            // shrink by LineWidth/2 so the stroke's outer half stays inside
            // the appearance Boundary.
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let inset = width / 2.0;
            let radii = (
                (rect.w / 2.0 - inset).max(0.0),
                (rect.h / 2.0 - inset).max(0.0),
            );
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

/// Shape `content` with per-run styles, wrap it at the rect width, and draw
/// each style span's glyphs into `rect` with its own font/size/color, synthetic
/// bold (second draw) and italic (skew). Glyph y positions are layout-relative,
/// so the first line's ink top sits at `rect.y` and wrapped lines stack below.
///
/// `flat_color` is the box's color, used by spans whose `color` is `None`.
/// Skips a span silently if its font can't be resolved or shaping yields no
/// glyphs (same degradation contract as the flat path).
fn draw_text_in_rect(
    painter: &mut Painter<Scene>,
    rect: &Rect,
    content: &str,
    runs: &[crate::text::LayoutRun],
    flat_color: Color,
    fonts: &FontStore,
    base: Affine,
) {
    let max_size = runs.iter().map(|r| r.size).fold(0.0_f64, f64::max).max(0.0);
    let wrapped = fonts.shape_wrapped_runs(content, runs, rect.w.max(max_size));
    let translate = base * Affine::translate((rect.x, rect.y));
    for span in wrapped.spans.iter() {
        let glyphs: Vec<Glyph> = wrapped.glyphs[span.glyph_start..span.glyph_end]
            .iter()
            .map(|g| Glyph {
                id: g.glyph_id,
                x: g.x,
                y: g.y,
            })
            .collect();
        if glyphs.is_empty() {
            continue;
        }
        let mut affine = translate;
        if span.italic {
            affine *= Affine::new([1.0, 0.0, -ITALIC_SKEW, 1.0, 0.0, 0.0]);
        }
        let color = runs[span.style_index].color.unwrap_or(flat_color);
        let brush = to_peniko(color);
        draw_glyph_run(painter, &span.font, &glyphs, affine, brush, span.size);
        if span.bold {
            // Second pass shifted along x: the overlapping fills fatten stems
            // (no downstream sink applies `font_embolden`).
            let bold = affine * Affine::translate((span.size * BOLD_EMBOLDEN_RATIO, 0.0));
            draw_glyph_run(painter, &span.font, &glyphs, bold, brush, span.size);
        }
    }
}

/// Italic slant for rich spans: x' = x - 0.25*y (glyph space, baseline at
/// y = 0, glyphs above have y < 0 -> tops lean right, ~14 degrees).
const ITALIC_SKEW: f64 = 0.25;

/// Synthetic bold amount as a fraction of the run's font size, applied via
/// `GlyphRun::font_embolden` (outlines fattened in X).
const BOLD_EMBOLDEN_RATIO: f64 = 0.022;

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

/// Insert `preedit` into `content` at char offset `offset` for the inline
/// composition preview. An out-of-range offset clamps to the content end.
/// Returns a fresh string (the preview is never written back to the dom).
fn compose_preedit_text(content: &str, offset: usize, preedit: &str) -> String {
    let byte = content
        .char_indices()
        .nth(offset)
        .map(|(i, _)| i)
        .unwrap_or(content.len());
    let (head, tail) = content.split_at(byte);
    let mut out = String::with_capacity(content.len() + preedit.len());
    out.push_str(head);
    out.push_str(preedit);
    out.push_str(tail);
    out
}

/// Shape `content` and return the glyphs with the shaper's natural positions.
/// parley's `positioned_glyphs()` y is layout-relative (the first line's
/// baseline already sits at the font's ascent from the layout top, and later
/// lines add the line height), so the glyphs are used as-is: the first line's
/// ink top lands at the rect origin and subsequent lines stack below it.
/// Annotation text uses the shaper's natural x/y directly (unlike body text
/// which uses document deltas). Returns the font that shaped the glyphs
/// (caller draws with it).
pub(crate) fn shape_positioned(
    content: &str,
    font_id: &FontId,
    size: f64,
    fonts: &FontStore,
) -> (Option<FontData>, Vec<Glyph>) {
    let (font, glyphs) = fonts.shape(font_id, content, size);
    let positioned: Vec<Glyph> = glyphs
        .iter()
        .map(|g| Glyph {
            id: g.glyph_id,
            x: g.x,
            y: g.y,
        })
        .collect();
    (font, positioned)
}

/// Draw a run of positioned glyphs with the given font, transform, brush, and
/// size. No-op if `glyphs` is empty.
pub(crate) fn draw_glyph_run(
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
    use imaging::record::{Command, Draw};
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
        draw_annotations(&mut painter, anns, &res, &fonts, (0.0, 0.0), 1.0, None);
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
        // Fixed 0.5925mm half-wave span: a 100-wide quad yields 168 full
        // half-waves + 1 partial = 169 quad_to segments (the move_to is not
        // counted as a segment by kurbo's segments() iterator). Segment count
        // scales with width so wave density stays constant.
        let p0 = Point { x: 0.0, y: 10.0 };
        let p1 = Point { x: 100.0, y: 10.0 };
        let path = squiggly_path(p0, p1);
        assert_eq!(path.segments().count(), 169);
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
                border: None,
                spans: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let _ = build(&[ann]);
    }

    #[test]
    fn span_textbox_flows_at_the_box_width_and_keeps_each_color() {
        let spans = vec![
            rofd_dom::TextSpan {
                text: "abc ".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Some(Color::Rgb(255, 0, 0)),
                bold: false,
                italic: false,
            },
            rofd_dom::TextSpan {
                text: "def ghi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Some(Color::Rgb(0, 0, 255)),
                bold: false,
                italic: false,
            },
        ];
        let ann = ann(
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 45.0,
                    h: 40.0,
                },
                content: "abc def ghi".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans,
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let mut ys: Vec<i64> = glyph_baselines(&scene)
            .into_iter()
            .map(|y| (y * 1000.0).round() as i64)
            .collect();
        ys.sort_unstable();
        ys.dedup();
        assert!(ys.len() >= 2, "span text flows onto multiple lines: {ys:?}");
        let brushes = glyph_brushes(&scene);
        assert!(
            brushes
                .iter()
                .any(|c| c.to_rgba8().to_u8_array() == [255, 0, 0, 255]),
            "first span keeps its red brush"
        );
        assert!(
            brushes
                .iter()
                .any(|c| c.to_rgba8().to_u8_array() == [0, 0, 255, 255]),
            "second span keeps its blue brush"
        );
    }

    #[test]
    fn same_line_color_only_spans_draw_each_color() {
        // Color does not affect shaping; parley would coalesce these runs into
        // one glyph run unless the style layout keeps them apart.
        let spans = vec![
            rofd_dom::TextSpan {
                text: "ab ".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: None,
                bold: false,
                italic: false,
            },
            rofd_dom::TextSpan {
                text: "cd".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Some(Color::Rgb(255, 0, 0)),
                bold: false,
                italic: false,
            },
            rofd_dom::TextSpan {
                text: " ef".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: None,
                bold: false,
                italic: false,
            },
        ];
        let ann = ann(
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 40.0,
                },
                content: "ab cd ef".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans,
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let brushes = glyph_brushes(&scene);
        assert!(
            brushes
                .iter()
                .any(|c| c.to_rgba8().to_u8_array() == [255, 0, 0, 255]),
            "the middle word must draw red, got {brushes:?}"
        );
        assert!(
            brushes
                .iter()
                .any(|c| c.to_rgba8().to_u8_array() == [0, 0, 0, 255]),
            "the surrounding words stay black"
        );
    }

    /// Collect the solid brush color of every glyph run in the scene.
    fn glyph_brushes(scene: &Scene) -> Vec<peniko::Color> {
        scene_glyph_runs(scene)
            .into_iter()
            .filter_map(|gr| match gr.brush {
                peniko::Brush::Solid(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// Collect the scene's glyph spans (transform + embolden + brush + glyph
    /// count per run) for the rich-spans assertions.
    fn scene_glyph_runs(scene: &Scene) -> Vec<imaging::record::GlyphRun> {
        let mut out = Vec::new();
        for cmd in scene.commands() {
            if let imaging::record::Command::Draw(id) = cmd {
                if let imaging::record::Draw::GlyphRun(gr) = scene.draw_op(*id) {
                    out.push(gr.clone());
                }
            }
        }
        out
    }

    #[test]
    fn textbox_spans_apply_italic_skew_bold_and_color_in_flow_layout() {
        // Three fragments: plain, italic, bold+colored. Flow layout puts them
        // on one line starting at rect.x; italic carries the skew coefficient;
        // bold is drawn twice; color lands on the brush.
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            w: 46.0,
            h: 10.0,
        };
        let span =
            |text: &str, italic: bool, bold: bool, color: Option<Color>| rofd_dom::TextSpan {
                text: text.into(),
                font: FontId::new("F1"),
                size: 12.0,
                color,
                bold,
                italic,
            };
        let ann = ann(
            AnnotationPayload::TextBox {
                rect,
                content: "abc".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: vec![
                    span("a", false, false, None),
                    span("b", true, false, None),
                    span("c", false, true, Some(Color::Rgb(75, 172, 198))),
                ],
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let runs = scene_glyph_runs(&scene);
        assert!(runs.len() >= 4, "3 base draws + bold second pass");
        // Italic: skew coefficient c == -0.25 exactly on ONE draw.
        let skewed = runs
            .iter()
            .filter(|g| (g.transform.as_coeffs()[2] + 0.25).abs() < 1e-9)
            .count();
        assert_eq!(skewed, 1, "exactly one skewed (italic) draw");
        // Bold = DOUBLE-DRAW: the second pass is offset by 0.022 em along x.
        let second_pass = runs
            .iter()
            .filter(|g| {
                let tx = g.transform.as_coeffs()[4];
                (tx - (10.0 + 0.022 * 12.0)).abs() < 1e-6
            })
            .count();
        assert_eq!(second_pass, 1, "bold span's second pass at +0.022em");
        assert!(
            runs.iter().all(|g| g.font_embolden.x.abs() < 1e-12),
            "font_embolden must stay unused (no sink applies it)"
        );
        // Colored span: the teal brush is present.
        let teal = runs.iter().any(|g| {
            matches!(
                &g.brush,
                peniko::Brush::Solid(c) if c.to_rgba8().to_u8_array() == [75, 172, 198, 255]
            )
        });
        assert!(teal, "teal brush for the colored span");
        // Same baseline: every draw transform shares one ty.
        let tys: Vec<f64> = runs.iter().map(|g| g.transform.as_coeffs()[5]).collect();
        assert!(
            tys.iter().all(|t| (t - tys[0]).abs() < 1e-6),
            "same baseline -> same ty: {tys:?}"
        );
    }

    /// Collect every glyph baseline (viewport y = run transform ty + glyph y)
    /// from the scene's glyph spans.
    fn glyph_baselines(scene: &Scene) -> Vec<f64> {
        let mut out = Vec::new();
        for cmd in scene.commands() {
            if let Command::Draw(id) = cmd {
                if let Draw::GlyphRun(gr) = scene.draw_op(*id) {
                    for g in &gr.glyphs {
                        out.push(gr.transform.as_coeffs()[5] + g.y as f64);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn textbox_glyph_baseline_stays_inside_rect() {
        // Regression: the selection frame is the payload rect, so the drawn
        // text must sit INSIDE it. `shape_positioned` used to add `+size` on
        // top of parley's layout-relative baseline y (first-line baseline is
        // already at the font's ascent), pushing the baseline ~2.2 em below
        // the rect top - below the frame's bottom edge for a typical ~1.4 em
        // tall box (text drawn under the frame). With a 1.5 em tall rect the
        // baseline (ascent, <= ~1.2 em for ordinary fonts) must fit inside.
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 18.0, // 1.5 x size(12)
        };
        let ann = ann(
            AnnotationPayload::TextBox {
                rect,
                content: "hello".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let baselines = glyph_baselines(&scene);
        assert!(!baselines.is_empty(), "text was drawn");
        for b in &baselines {
            assert!(
                *b >= rect.y && *b <= rect.y + rect.h,
                "baseline {b} outside rect y span [{}, {}]",
                rect.y,
                rect.y + rect.h
            );
        }
    }

    #[test]
    fn textbox_wraps_glyphs_to_multiple_baselines() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 45.0,
            h: 40.0,
        };
        let ann = ann(
            AnnotationPayload::TextBox {
                rect,
                content: "abc def ghi jkl".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let mut baselines = glyph_baselines(&scene);
        baselines.sort_by(|a, b| a.partial_cmp(b).unwrap());
        baselines.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(
            baselines.len() >= 2,
            "wrapped text draws on at least two baselines, got {baselines:?}"
        );
    }

    #[test]
    fn textbox_multiline_baselines_stay_inside_rect() {
        // Two lines: line 1 baseline = ascent, line 2 = ascent + line height
        // (~2.6 em combined for CJK-metric fonts) - must stay inside a 3 em
        // tall rect. The old `+size` double drop put line 2 at ~3.6 em,
        // below the frame.
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 36.0, // 3 x size(12)
        };
        let ann = ann(
            AnnotationPayload::TextBox {
                rect,
                content: "two\nlines".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: Vec::new(),
            },
            AnnotationKind::TextBox,
        );
        let scene = build(&[ann]);
        let baselines = glyph_baselines(&scene);
        assert!(baselines.len() >= 2, "two lines were drawn");
        for b in &baselines {
            assert!(
                *b >= rect.y && *b <= rect.y + rect.h,
                "baseline {b} outside rect y span [{}, {}]",
                rect.y,
                rect.y + rect.h
            );
        }
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
        draw_annotations(&mut painter, &[ann], &res, &fonts, (0.0, 0.0), 1.0, None);
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
                    border: None,
                    spans: Vec::new(),
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
        draw_annotations(&mut painter, &anns, &res, &fonts, (0.0, 0.0), 1.0, None);
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

    #[test]
    fn squiggly_path_fixed_wavelength_not_length_scaled() {
        // 每个 Q 段 (半波) 的 x 跨度恒为 0.5925 页面局部 mm (参考工具实测
        // 波距): 波浪密度不随 quad 宽度 (文字长度) 变化. 短选区与整行选区
        // 波长相同.
        use imaging::kurbo::PathSeg;
        const HALF_WAVE: f64 = 0.5925;
        for width in [40.0f64, 400.0] {
            let path = squiggly_path(Point { x: 0.0, y: 14.0 }, Point { x: width, y: 14.0 });
            let quads: Vec<_> = path
                .segments()
                .filter_map(|s| match s {
                    PathSeg::Quad(q) => Some(q),
                    _ => None,
                })
                .collect();
            let full = (width / HALF_WAVE).floor() as usize;
            let tail = width - full as f64 * HALF_WAVE > 1e-6;
            assert_eq!(quads.len(), full + usize::from(tail), "width={width}");
            for (i, q) in quads.iter().enumerate() {
                if i < full {
                    // Full half-wave: ends on the (i+1)-th grid point, control
                    // at its midpoint.
                    let expected_end = (i as f64 + 1.0) * HALF_WAVE;
                    assert!(
                        (q.p2.x - expected_end).abs() < 1e-9,
                        "width={width} seg {i} must end {expected_end}, got {}",
                        q.p2.x
                    );
                    let mid = (i as f64 + 0.5) * HALF_WAVE;
                    assert!(
                        (q.p1.x - mid).abs() < 1e-9,
                        "width={width} seg {i} control at {mid}, got {}",
                        q.p1.x
                    );
                } else {
                    // Partial tail: control at the REMAINING span's midpoint,
                    // end exactly at p1.x.
                    let mid = (i as f64 * HALF_WAVE + width) / 2.0;
                    assert!(
                        (q.p1.x - mid).abs() < 1e-9,
                        "width={width} tail control at {mid}, got {}",
                        q.p1.x
                    );
                    assert!(
                        (q.p2.x - width).abs() < 1e-9,
                        "width={width} tail must end at {width}, got {}",
                        q.p2.x
                    );
                }
            }
        }
    }

    #[test]
    fn squiggly_path_partial_tail_ends_exactly_at_x1() {
        // 宽度非半波整数倍时: 41mm = 69 个完整半波 (69 × 0.5925 =
        // 40.8825) + 1 段 0.1175mm 部分波, 终点精确落在 p1.x, 与文字末尾对齐.
        use imaging::kurbo::PathSeg;
        let path = squiggly_path(Point { x: 0.0, y: 14.0 }, Point { x: 41.0, y: 14.0 });
        let quads: Vec<_> = path
            .segments()
            .filter_map(|s| match s {
                PathSeg::Quad(q) => Some(q),
                _ => None,
            })
            .collect();
        assert_eq!(quads.len(), 70); // 69 full + 1 partial
        let last = quads[69];
        assert!((last.p0.x - 40.8825).abs() < 1e-9);
        assert!((last.p1.x - 40.94125).abs() < 1e-9);
        assert!((last.p2.x - 41.0).abs() < 1e-9);
    }

    /// One flat TextBox (id 1) on a 200x200 page, ready for
    /// `draw_annotations` preedit tests.
    fn textbox_doc(
        rect: Rect,
        content: &str,
        spans: Vec<rofd_dom::TextSpan>,
    ) -> rofd_dom::OfdDocument {
        let a = ann(
            AnnotationPayload::TextBox {
                rect,
                content: content.into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans,
            },
            AnnotationKind::TextBox,
        );
        let mut annotations = rofd_dom::AnnotationModel::default();
        annotations
            .by_page
            .insert(rofd_dom::PageId::new("P0"), vec![a]);
        rofd_dom::OfdDocument {
            pages: vec![rofd_dom::Page {
                id: rofd_dom::PageId::new("P0"),
                physical_box: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 200.0,
                },
                layers: vec![],
                template: None,
            }],
            annotations,
            ..Default::default()
        }
    }

    /// Draw the page's annotations (no page origin offset, zoom 1). `preedit`
    /// previews an IME composition inline.
    fn annotation_scene_with(
        doc: &rofd_dom::OfdDocument,
        preedit: Option<&PreeditPreview<'_>>,
    ) -> Scene {
        let fonts = test_font_store();
        let res = Resources::default();
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);
        draw_annotations(
            &mut painter,
            doc.annotations.for_page(&rofd_dom::PageId::new("P0")),
            &res,
            &fonts,
            (0.0, 0.0),
            1.0,
            preedit,
        );
        scene
    }

    /// `(transform, glyphs)` of every glyph run in the scene (draw order).
    fn glyph_runs(scene: &Scene) -> Vec<(Affine, Vec<Glyph>)> {
        let mut out = Vec::new();
        for cmd in scene.commands() {
            if let Command::Draw(id) = cmd {
                if let Draw::GlyphRun(gr) = scene.draw_op(*id) {
                    out.push((gr.transform, gr.glyphs.clone()));
                }
            }
        }
        out
    }

    #[test]
    fn preedit_preview_flows_inline_without_overlapping_text() {
        // Root cause of the reported overlap: the composition used to be
        // painted detached ON TOP of the content at the composition offset,
        // so it sat on the characters that follow it. Inline preview shapes
        // the content WITH the composition inserted: the following chars
        // shift past it, exactly as they will after commit.
        let doc = textbox_doc(
            Rect {
                x: 30.0,
                y: 40.0,
                w: 100.0,
                h: 60.0,
            },
            "hello",
            Vec::new(),
        );
        let fonts = test_font_store();
        let id = AnnotationId::from_int(1);
        let preview = PreeditPreview {
            annotation: &id,
            offset: 3,
            text: "XY",
        };
        let scene = annotation_scene_with(&doc, Some(&preview));
        let spans = glyph_runs(&scene);
        assert_eq!(spans.len(), 1, "flat box draws one text run");
        let glyphs = &spans[0].1;
        assert_eq!(glyphs.len(), 7, "'helXYlo' -> 7 glyphs");
        for w in glyphs.windows(2) {
            assert!(
                w[1].x > w[0].x,
                "glyph x must advance (overlap would repeat x): {glyphs:?}"
            );
        }
        // Composition starts where 'hel' ends ...
        let prefix = fonts.line_width(&FontId::new("F1"), "hel", 12.0);
        assert!(
            (glyphs[3].x as f64 - prefix).abs() < 1e-3,
            "composition starts at the offset: {} vs {prefix}",
            glyphs[3].x
        );
        // ... and the original 'l' after it moved past the composition.
        let after = fonts.line_width(&FontId::new("F1"), "helXY", 12.0);
        assert!(
            (glyphs[5].x as f64 - after).abs() < 1e-3,
            "following text shifts past the composition: {} vs {after}",
            glyphs[5].x
        );
        // Control: without a preview the content keeps its 5 glyphs.
        let plain = annotation_scene_with(&doc, None);
        assert_eq!(glyph_runs(&plain)[0].1.len(), 5);
    }

    #[test]
    fn preedit_preview_wraps_with_the_text_flow() {
        // Composition at a soft-wrap boundary joins the second line and
        // pushes the rest - it must sit on that line (same y as the text
        // following it), not float above line 1.
        let content = "abc def ghi jkl";
        let doc = textbox_doc(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 45.0,
                h: 40.0,
            },
            content,
            Vec::new(),
        );
        let fonts = test_font_store();
        let id = AnnotationId::from_int(1);
        let preview = PreeditPreview {
            annotation: &id,
            offset: 8,
            text: "XY",
        };
        let scene = annotation_scene_with(&doc, Some(&preview));
        let glyphs = &glyph_runs(&scene)[0].1;
        assert_eq!(glyphs.len(), content.chars().count() + 2);
        let composed = String::from("abc def XYghi jkl");
        let w = fonts.shape_wrapped(&FontId::new("F1"), &composed, 12.0, 45.0);
        let (line, _) = w.line_at_offset(8);
        assert_eq!(line, 1, "composition sits on the second line");
        let baseline = w.lines[line].baseline as f32;
        for i in [8, 9, 10] {
            assert!(
                (glyphs[i].y - baseline).abs() < 1e-3,
                "glyph {i} on the composition line (got y={}, want {baseline})",
                glyphs[i].y
            );
        }
        assert!(glyphs[9].x > glyphs[8].x, "composition advances");
        assert!(glyphs[10].x > glyphs[9].x, "text after it follows");
    }

    #[test]
    fn preedit_preview_is_offset_by_textbox_origin() {
        // Regression: the inline composition goes through the same
        // draw_text_in_rect transform as the box text (rect origin), not the
        // page corner.
        let doc = textbox_doc(
            Rect {
                x: 30.0,
                y: 40.0,
                w: 100.0,
                h: 60.0,
            },
            "hello",
            Vec::new(),
        );
        let id = AnnotationId::from_int(1);
        let preview = PreeditPreview {
            annotation: &id,
            offset: 0,
            text: "hi",
        };
        let scene = annotation_scene_with(&doc, Some(&preview));
        let spans = glyph_runs(&scene);
        assert_eq!(spans.len(), 1);
        let coeffs = spans[0].0.as_coeffs();
        assert!((coeffs[4] - 30.0).abs() < 1e-6, "x: {}", coeffs[4]);
        assert!((coeffs[5] - 40.0).abs() < 1e-6, "y: {}", coeffs[5]);
    }

    #[test]
    fn preedit_preview_ignores_other_annotations() {
        let doc = textbox_doc(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 40.0,
            },
            "hello",
            Vec::new(),
        );
        let other = AnnotationId::from_int(99);
        let preview = PreeditPreview {
            annotation: &other,
            offset: 3,
            text: "XY",
        };
        let scene = annotation_scene_with(&doc, Some(&preview));
        assert_eq!(
            glyph_runs(&scene)[0].1.len(),
            5,
            "a preview for another annotation leaves the text untouched"
        );
    }

    #[test]
    fn preedit_overlay_is_skipped_for_flat_textboxes() {
        // Flat boxes preview inline; the detached overlay must draw nothing
        // (painting both would double-draw the composition).
        let doc = textbox_doc(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 40.0,
            },
            "hello",
            Vec::new(),
        );
        let vp = crate::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let fonts = test_font_store();
        let mut scene = Scene::new();
        crate::preedit_overlay::paint_preedit_overlay(
            &mut scene,
            &doc,
            &vp,
            &fonts,
            "XY",
            &AnnotationId::from_int(1),
            3,
        );
        assert!(
            glyph_runs(&scene).is_empty(),
            "no detached composition for flat boxes"
        );
    }

    #[test]
    fn preedit_span_textbox_anchors_at_the_caret() {
        // Span boxes keep the detached composition, but it must sit at the
        // caret's line (not the first line / flat prefix width).
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 45.0,
            h: 40.0,
        };
        let a = ann(
            AnnotationPayload::TextBox {
                rect,
                content: "abc def ghi jkl".into(),
                font: FontId::new("F1"),
                size: 12.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: vec![rofd_dom::TextSpan {
                    text: "abc def ghi jkl".into(),
                    font: FontId::new("F1"),
                    size: 12.0,
                    color: None,
                    bold: false,
                    italic: false,
                }],
            },
            AnnotationKind::TextBox,
        );
        let mut annotations = rofd_dom::AnnotationModel::default();
        annotations
            .by_page
            .insert(rofd_dom::PageId::new("P0"), vec![a]);
        let doc = rofd_dom::OfdDocument {
            pages: vec![rofd_dom::Page {
                id: rofd_dom::PageId::new("P0"),
                physical_box: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 200.0,
                },
                layers: vec![],
                template: None,
            }],
            annotations,
            ..Default::default()
        };
        let vp = crate::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 20.0,
        };
        let fonts = test_font_store();
        let offset = 8; // second visual line
        let caret =
            crate::caret_rect_on_line(&doc, &vp, &fonts, &AnnotationId::from_int(1), offset, None)
                .expect("caret rect");
        let mut scene = Scene::new();
        crate::preedit_overlay::paint_preedit_overlay(
            &mut scene,
            &doc,
            &vp,
            &fonts,
            "XY",
            &AnnotationId::from_int(1),
            offset,
        );
        let mut transforms = Vec::new();
        for cmd in scene.commands() {
            if let imaging::record::Command::Draw(id) = cmd {
                if let imaging::record::Draw::GlyphRun(gr) = scene.draw_op(*id) {
                    transforms.push(gr.transform.as_coeffs());
                }
            }
        }
        assert!(!transforms.is_empty(), "composition was drawn");
        for t in &transforms {
            assert!(
                (t[4] - caret.x).abs() < 1e-3 && (t[5] - caret.y).abs() < 1e-3,
                "composition anchors at the caret: got ({}, {}), caret ({}, {})",
                t[4],
                t[5],
                caret.x,
                caret.y
            );
        }
    }
}
