// Ported from rword@4e0b25b6 (crates/gpui-view/src/gpui_sink.rs) — kernel-agnostic halves, imaging IR identical.

//! GpuiSink: imaging `PaintSink` -> recorded `PaintCmd`s (plan Task 1.4).
//!
//! Split in two halves so the mapping is fully headless-testable:
//! - RECORDING (`GpuiSink`): translates borrowed imaging draws into owned
//!   `PaintCmd`s — clip stack resolved (intersected) per command, affine
//!   transforms baked into geometry/origins/sizes/stroke widths/stroke
//!   styles. No gpui Window, no TextSystem.
//! - REPLAY (`replay`): paints cmds into a `Window`, resolving glyph
//!   fonts through the shared [`FontCache`].
//!
//! Degradations (spec §5.4): path clips degrade to their bbox;
//! groups/masks/composites and blurred rects are skipped with a one-time
//! log; gradient brushes take the first stop's color. Faux bold needs NO
//! handling — the IR already double-draws synthetic-bold runs (spike 0.1).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{fill, px, Bounds, Corners, Hsla, PaintQuad, Path, PathBuilder, Pixels, Point, Window};
use imaging::kurbo::{Affine, BezPath, Point as KPoint, Rect as KRect, Shape};
use imaging::peniko::{Brush, Color, FontData};
use imaging::record::{Geometry, Glyph, GlyphRun};
use imaging::{BlurredRoundedRect, ClipRef, FillRef, GlyphRunRef, GroupRef, StrokeRef};
use imaging::{GeometryRef, PaintSink};

use crate::font_cache::FontCache;

/// One replayable draw command, clip mask pre-resolved (intersected stack).
#[derive(Debug, Clone)]
pub enum PaintCmd {
    Quad {
        bounds: Bounds<Pixels>,
        radii: Corners<Pixels>,
        color: Hsla,
        mask: Option<Bounds<Pixels>>,
    },
    Path {
        /// KURBO path (gpui Path can't be offset post-hoc — conversion +
        /// element-origin offset happen together at replay time).
        bez: BezPath,
        width: Option<Pixels>,
        color: Hsla,
        mask: Option<Bounds<Pixels>>,
    },
    Glyph {
        origin: Point<Pixels>,
        font: FontData,
        glyph_id: u32,
        size: Pixels,
        color: Hsla,
        mask: Option<Bounds<Pixels>>,
    },
    Image {
        bounds: Bounds<Pixels>,
        image: Arc<gpui::RenderImage>,
        mask: Option<Bounds<Pixels>>,
    },
}

impl PaintCmd {
    fn mask(&self) -> Option<Bounds<Pixels>> {
        match self {
            PaintCmd::Quad { mask, .. }
            | PaintCmd::Path { mask, .. }
            | PaintCmd::Glyph { mask, .. }
            | PaintCmd::Image { mask, .. } => *mask,
        }
    }
}

/// Recording half of the gpui replay path.
#[derive(Default)]
pub struct GpuiSink {
    cmds: Vec<PaintCmd>,
    clip_stack: Vec<Bounds<Pixels>>,
    images: HashMap<(usize, usize), Arc<gpui::RenderImage>>,
    degraded_logged: bool,
}

impl GpuiSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the recorded commands (the sink is single-shot).
    pub fn finish(self) -> Vec<PaintCmd> {
        self.cmds
    }

    fn mask(&self) -> Option<Bounds<Pixels>> {
        self.clip_stack.last().copied()
    }

    fn degrade_log(&mut self, what: &str) {
        if !self.degraded_logged {
            self.degraded_logged = true;
            eprintln!("[rofd-gpui-view] degraded rendering: {what} (first occurrence)");
        }
    }

    fn decode_image(&mut self, bytes: &[u8]) -> Option<Arc<gpui::RenderImage>> {
        let key = (bytes.as_ptr() as usize, bytes.len());
        if let Some(hit) = self.images.get(&key) {
            return Some(hit.clone());
        }
        let decoded = image::load_from_memory(bytes).ok()?;
        let rgba = decoded.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let mut raw = rgba.into_raw();
        for pixel in raw.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2); // RGBA -> BGRA, gpui's own idiom
        }
        let buffer = image::RgbaImage::from_raw(w, h, raw)?;
        let render = Arc::new(gpui::RenderImage::new(smallvec::smallvec![
            image::Frame::new(buffer)
        ]));
        self.images.insert(key, render.clone());
        Some(render)
    }
}

fn uniform_scale(a: &Affine) -> f64 {
    let c = a.as_coeffs();
    (c[0] * c[0] + c[1] * c[1]).sqrt()
}

fn affine_apply(a: &Affine, x: f64, y: f64) -> (f64, f64) {
    let c = a.as_coeffs();
    (c[0] * x + c[2] * y + c[4], c[1] * x + c[3] * y + c[5])
}

fn bounds_px(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds<Pixels> {
    Bounds {
        origin: Point::new(px(x0 as f32), px(y0 as f32)),
        size: gpui::size(px((x1 - x0) as f32), px((y1 - y0) as f32)),
    }
}

trait Rgba8Ext {
    fn rgba8(&self) -> imaging::peniko::color::Rgba8;
}

impl Rgba8Ext for Color {
    fn rgba8(&self) -> imaging::peniko::color::Rgba8 {
        self.to_rgba8()
    }
}

impl Rgba8Ext for imaging::peniko::color::DynamicColor {
    fn rgba8(&self) -> imaging::peniko::color::Rgba8 {
        (*self)
            .to_alpha_color::<imaging::peniko::color::Srgb>()
            .to_rgba8()
    }
}

fn color_hsla(color: &impl Rgba8Ext) -> Hsla {
    let c = color.rgba8();
    let conv = |v: u8| v as f32 / 255.0;
    gpui::Rgba {
        r: conv(c.r),
        g: conv(c.g),
        b: conv(c.b),
        a: conv(c.a),
    }
    .into()
}

fn brush_color(brush: &Brush) -> Hsla {
    match brush {
        Brush::Solid(c) => color_hsla(c),
        Brush::Gradient(g) => g
            .stops
            .first()
            .map(|s| color_hsla(&s.color))
            .unwrap_or_default(),
        Brush::Image(_) => Hsla::default(),
    }
}

fn quad_from_rect(rect: &KRect, transform: &Affine) -> Bounds<Pixels> {
    // Transform all four corners and take the min/max box (correct for
    // translation + uniform scale, our only zoom shape).
    let corners = [
        affine_apply(transform, rect.x0, rect.y0),
        affine_apply(transform, rect.x1, rect.y0),
        affine_apply(transform, rect.x0, rect.y1),
        affine_apply(transform, rect.x1, rect.y1),
    ];
    bounds_px(
        corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        corners
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max),
        corners
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max),
    )
}

fn geometry_to_bez(shape: &Geometry) -> BezPath {
    match shape {
        Geometry::Path(bez) => bez.clone(),
        Geometry::Rect(rect) => rect.to_path(0.25),
        Geometry::RoundedRect(rr) => rr.to_path(0.25),
    }
}

/// Apply an affine to every path element (self-contained bake).
fn bake_bez(transform: &Affine, bez: &BezPath) -> BezPath {
    let map = |q: &KPoint| {
        let (x, y) = affine_apply(transform, q.x, q.y);
        KPoint::new(x, y)
    };
    let mut out = BezPath::new();
    for el in bez.elements() {
        use imaging::kurbo::PathEl::*;
        match el {
            MoveTo(q) => out.move_to(map(q)),
            LineTo(q) => out.line_to(map(q)),
            QuadTo(c, q) => out.quad_to(map(c), map(q)),
            CurveTo(c1, c2, q) => out.curve_to(map(c1), map(c2), map(q)),
            ClosePath => out.close_path(),
        }
    }
    out
}

fn shape_bbox(shape: GeometryRef<'_>, transform: &Affine) -> Bounds<Pixels> {
    let bez: BezPath = match shape {
        GeometryRef::Rect(r) => r.to_path(0.25),
        GeometryRef::RoundedRect(rr) => rr.to_path(0.25),
        GeometryRef::Path(p) => p.clone(),
        GeometryRef::OwnedPath(p) => p.clone(),
    };
    let bbox = bake_bez(transform, &bez).bounding_box();
    bounds_px(bbox.x0, bbox.y0, bbox.x1, bbox.y1)
}

impl PaintSink for GpuiSink {
    fn push_clip(&mut self, clip: ClipRef<'_>) {
        // Content masks are rectangles only: degrade to the transformed
        // shape's bbox (spec §5.4).
        let bounds = match clip {
            ClipRef::Fill {
                transform, shape, ..
            } => shape_bbox(shape, &transform),
            ClipRef::Stroke {
                transform,
                shape,
                stroke,
            } => {
                let mut b = shape_bbox(shape, &transform);
                let grow = (stroke.width * uniform_scale(&transform) / 2.0) as f32;
                b.origin.x -= px(grow);
                b.origin.y -= px(grow);
                b.size.width += px(grow * 2.0);
                b.size.height += px(grow * 2.0);
                b
            }
        };
        let effective = match self.clip_stack.last() {
            Some(top) => top.intersect(&bounds),
            None => bounds,
        };
        self.clip_stack.push(effective);
    }

    fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    fn push_group(&mut self, _group: GroupRef<'_>) {
        self.degrade_log("isolated group (mask/blend/composite) skipped");
    }

    fn pop_group(&mut self) {}

    fn fill(&mut self, draw: FillRef<'_>) {
        let imaging::record::Draw::Fill {
            shape,
            transform,
            brush,
            ..
        } = draw.to_owned()
        else {
            return;
        };
        let mask = self.mask();
        match shape {
            Geometry::Rect(rect) => {
                if let Brush::Image(image_brush) = &brush {
                    let bytes: &[u8] = image_brush.image.data.as_ref();
                    match self.decode_image(bytes) {
                        Some(image) => {
                            self.cmds.push(PaintCmd::Image {
                                bounds: quad_from_rect(&rect, &transform),
                                image,
                                mask,
                            });
                            return;
                        }
                        // Undecodable image bytes: surface it once instead
                        // of silently dropping the picture (review minor).
                        None => self.degrade_log("image decode failed — picture skipped"),
                    }
                }
                self.cmds.push(PaintCmd::Quad {
                    bounds: quad_from_rect(&rect, &transform),
                    radii: Corners::default(),
                    color: brush_color(&brush),
                    mask,
                });
            }
            Geometry::RoundedRect(rr) => {
                let scale = uniform_scale(&transform) as f32;
                let r = rr.radii();
                self.cmds.push(PaintCmd::Quad {
                    bounds: quad_from_rect(&rr.rect(), &transform),
                    radii: Corners {
                        top_left: px(r.top_left as f32 * scale),
                        top_right: px(r.top_right as f32 * scale),
                        bottom_left: px(r.bottom_left as f32 * scale),
                        bottom_right: px(r.bottom_right as f32 * scale),
                    },
                    color: brush_color(&brush),
                    mask,
                });
            }
            Geometry::Path(bez) => {
                let baked = bake_bez(&transform, &bez);
                self.cmds.push(PaintCmd::Path {
                    bez: baked,
                    width: None,
                    color: brush_color(&brush),
                    mask,
                });
            }
        }
    }

    fn stroke(&mut self, draw: StrokeRef<'_>) {
        let imaging::record::Draw::Stroke {
            shape,
            transform,
            stroke: stroke_style,
            brush,
            ..
        } = draw.to_owned()
        else {
            return;
        };
        let mask = self.mask();
        let scale = uniform_scale(&transform) as f32;
        let width = stroke_style.width as f32 * scale;
        let baked = bake_bez(&transform, &geometry_to_bez(&shape));
        self.cmds.push(PaintCmd::Path {
            bez: baked,
            width: Some(px(width)),
            color: brush_color(&brush),
            mask,
        });
    }

    fn glyph_run(&mut self, draw: GlyphRunRef<'_>, glyphs: &mut dyn Iterator<Item = Glyph>) {
        let run: GlyphRun = draw.to_owned(glyphs);
        let mask = self.mask();
        let scale = uniform_scale(&run.transform) as f32;
        let size = px(run.font_size * scale);
        let color = brush_color(&run.brush);
        let c = run.transform.as_coeffs();
        for glyph in &run.glyphs {
            let x = c[0] * glyph.x as f64 + c[2] * glyph.y as f64 + c[4];
            let y = c[1] * glyph.x as f64 + c[3] * glyph.y as f64 + c[5];
            self.cmds.push(PaintCmd::Glyph {
                origin: Point::new(px(x as f32), px(y as f32)),
                font: run.font.clone(),
                glyph_id: glyph.id,
                size,
                color,
                mask,
            });
        }
    }

    fn blurred_rounded_rect(&mut self, _draw: BlurredRoundedRect) {
        self.degrade_log("blurred rounded rect (shadow) skipped");
    }
}

/// Replay half: paint the recorded commands into a window, offset by the
/// element's bounds origin (the kernel records in viewport coordinates
/// starting at 0,0; the element may sit anywhere in the window).
pub fn replay(
    window: &mut Window,
    cmds: &[PaintCmd],
    fonts: &mut FontCache,
    origin: Point<Pixels>,
) {
    let text_system = window.text_system().clone();
    for cmd in cmds {
        let mask = cmd.mask().map(|b| gpui::ContentMask {
            bounds: Bounds {
                origin: b.origin + origin,
                ..b
            },
        });
        window.with_content_mask(mask, |window| match cmd {
            PaintCmd::Quad {
                bounds,
                radii,
                color,
                ..
            } => {
                let quad: PaintQuad = fill(
                    Bounds {
                        origin: bounds.origin + origin,
                        ..*bounds
                    },
                    *color,
                );
                let mut quad = quad;
                quad.corner_radii = *radii;
                window.paint_quad(quad);
            }
            PaintCmd::Path {
                bez, width, color, ..
            } => {
                // Convert kurbo path → gpui Path with the offset baked in.
                let stroke_width = width.map(f32::from);
                if let Some(path) = bez_to_path_offset(bez, stroke_width, origin) {
                    window.paint_path(path, *color);
                }
            }
            PaintCmd::Glyph {
                origin: g_origin,
                font,
                glyph_id,
                size,
                color,
                ..
            } => {
                if let Some(font_id) = fonts.font_id(font, &text_system) {
                    let _ = window.paint_glyph(
                        *g_origin + origin,
                        font_id,
                        gpui::GlyphId(*glyph_id),
                        *size,
                        *color,
                    );
                }
            }
            PaintCmd::Image { bounds, image, .. } => {
                let b = Bounds {
                    origin: bounds.origin + origin,
                    ..*bounds
                };
                let _ = window.paint_image(b, b, Corners::default(), image.clone(), 0, false);
            }
        });
    }
}

/// Convert a kurbo BezPath to a gpui Path, offsetting every point by the
/// element origin (the recording half stored viewport-local coordinates).
fn bez_to_path_offset(
    bez: &BezPath,
    stroke_width: Option<f32>,
    origin: Point<Pixels>,
) -> Option<Path<Pixels>> {
    let mut builder = match stroke_width {
        Some(w) => PathBuilder::stroke(px(w)),
        None => PathBuilder::fill(),
    };
    let p =
        |q: imaging::kurbo::Point| Point::new(px(q.x as f32) + origin.x, px(q.y as f32) + origin.y);
    for el in bez.elements() {
        use imaging::kurbo::PathEl::*;
        match &el {
            MoveTo(q) => builder.move_to(p(*q)),
            LineTo(q) => builder.line_to(p(*q)),
            QuadTo(ctrl, to) => builder.curve_to(p(*to), p(*ctrl)),
            CurveTo(c1, c2, to) => builder.cubic_bezier_to(p(*to), p(*c1), p(*c2)),
            ClosePath => builder.close(),
        }
    }
    builder.build().ok()
}
