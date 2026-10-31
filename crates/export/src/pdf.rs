//! Scene-IR → krilla PDF encoder (print spec 2026-10-31 §3). One krilla page
//! per page-local scene; a per-page transform maps scene px → PDF pt (×0.75).
//! krilla's Surface uses a top-left y-down coordinate system like the Scene —
//! no manual flip.
//!
//! Ported from rword-export's `pdf.rs` (as-built reference project); the
//! sink machinery below is kept verbatim.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use imaging::kurbo::{self, Affine, BezPath, Cap, Join, PathEl, Shape, Stroke as KurboStroke};
use imaging::peniko;
use krilla::color::rgb;
use krilla::geom::{Path, Point, Size, Transform};
use krilla::image::Image as KrillaImage;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{
    Fill as KrillaFill, FillRule, LineCap, LineJoin, Paint, Stroke as KrillaStroke, StrokeDash,
};
use krilla::surface::Surface;
use krilla::text::{Font as KrillaFont, GlyphId, KrillaGlyph};
use krilla::SerializeSettings;

/// px (96 dpi) → pt (72 pt/inch).
const PX_TO_PT: f32 = 0.75;
/// Flattening tolerance for geometry → path conversion (scene px).
const TOLERANCE: f64 = 0.1;
/// Reject images with a side beyond this many pixels — the embedded
/// uncompressed sample stream would be enormous (same guard as wordcraft's
/// exporter).
const MAX_IMAGE_SIDE: u32 = 30_000;

/// One page-local print scene (print spec §3): the pixels are the page's
/// own — origin at the page's top-left corner, no cross-page content.
pub struct PdfPage {
    /// Page width in scene pixels (96 dpi).
    pub width_px: f32,
    /// Page height in scene pixels (96 dpi).
    pub height_px: f32,
    /// The page-local recorded scene.
    pub scene: imaging::record::Scene,
}

/// Failure modes of the PDF encode (print spec §6).
#[derive(Debug)]
pub enum ExportError {
    /// The page list was empty — a PDF must have at least one page.
    EmptyPages,
    /// krilla failed to serialize the document.
    Encode(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::EmptyPages => write!(f, "no pages to encode"),
            ExportError::Encode(e) => write!(f, "PDF encode failed: {e}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Page placement for one krilla page (the ported shape of rword-export's
/// `pipeline::PageMeta`; page-local scenes always place at origin (0,0)).
struct PageMeta {
    index: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Encode page-local scenes into a PDF (print spec §3): one page per
/// [`PdfPage`], sized from the scene's own pixel dims. No offset culling —
/// each scene is already page-local. The font/image caches live on one sink
/// across pages, so a repeated face costs one subset.
pub fn pdf_from_page_scenes(pages: &[PdfPage]) -> Result<Vec<u8>, ExportError> {
    if pages.is_empty() {
        return Err(ExportError::EmptyPages);
    }
    let mut sink = PdfSink::new();
    for (i, p) in pages.iter().enumerate() {
        // Page-local placement: origin (0,0), the page's own pixel size.
        let meta = PageMeta {
            index: i,
            x: 0.0,
            y: 0.0,
            w: p.width_px,
            h: p.height_px,
        };
        sink.page_from_scene(&p.scene, &meta)?;
    }
    sink.finish()
}

/// A krilla document fed one page at a time: `new` creates the document,
/// `page_from_scene` replays one page, `finish` serializes. (Extracted from
/// rword-export's pdf.rs, where the same type also served a full-document
/// scene through a `ConvertSink` trait.)
struct PdfSink {
    doc: krilla::Document,
    ctx: SinkContext,
    warnings: Vec<String>,
}

impl PdfSink {
    fn new() -> PdfSink {
        PdfSink {
            // No dates are set (default Metadata carries none), keeping the
            // output byte-deterministic for identical input.
            doc: krilla::Document::new_with(SerializeSettings::default()),
            ctx: SinkContext::default(), // font/image caches (shared across pages)
            warnings: Vec::new(),
        }
    }

    /// Encode one page: `scene` is page-local (origin 0,0 — nothing to cull
    /// away beyond the page's own rect).
    fn page_from_scene(
        &mut self,
        scene: &imaging::record::Scene,
        page: &PageMeta,
    ) -> Result<(), ExportError> {
        let size = Size::from_wh(page.w * PX_TO_PT, page.h * PX_TO_PT).ok_or_else(|| {
            ExportError::Encode(format!("page {} has non-positive size", page.index))
        })?;
        let mut kp = self.doc.start_page_with(PageSettings::new(size));
        let mut surface = kp.surface();
        // Scene px (top-left, y-down) → page pt: translate the page's scene
        // rect to the origin, then scale px → pt. Draw coordinates stay in
        // scene px; this transform is pushed exactly once per page.
        let t = Transform::from_row(
            PX_TO_PT,
            0.0,
            0.0,
            PX_TO_PT,
            -page.x * PX_TO_PT,
            -page.y * PX_TO_PT,
        );
        surface.push_transform(&t);
        let mut sink = PdfPageSink {
            surface: &mut surface,
            ctx: &mut self.ctx,
            page,
            warnings: &mut self.warnings,
            clips: Vec::new(),
        };
        imaging::record::replay(scene, &mut sink);
        surface.pop(); // page transform
        surface.finish();
        kp.finish();
        Ok(())
    }

    /// Serialize the document. Non-fatal degradations collected along the
    /// way (skipped filters/masks/unsupported brushes) stay internal: the
    /// v1 public surface returns bytes only — fatal failures are the
    /// `ExportError` cases (print spec §6).
    fn finish(self) -> Result<Vec<u8>, ExportError> {
        match self.doc.finish() {
            Ok(bytes) => Ok(bytes),
            Err(e) => Err(ExportError::Encode(format!("krilla finish: {e:?}"))),
        }
    }
}

/// Cross-page caches shared by all page sinks: krilla fonts, reverse cmaps
/// and images, all keyed by stable blob pointers (the recorded scene owns
/// every blob for the whole encode, so pointers do not move).
#[derive(Default)]
struct SinkContext {
    /// One krilla font per distinct face. Fonts are cheap to clone (Arc
    /// internals); subsetting happens at `Document::finish`.
    fonts: HashMap<(usize, u32), KrillaFont>,
    /// Glyph id → char per face, built once from the font's own cmap — the
    /// source for ToUnicode text. Arc'd so per-run lookups avoid deep
    /// copies (CJK cmaps hold tens of thousands of entries).
    reverse_cmaps: HashMap<(usize, u32), Arc<HashMap<u32, char>>>,
    /// Krilla images by source-blob pointer. `None` memoizes a failed
    /// decode so the failure warns once per image, not per draw.
    images: HashMap<usize, Option<KrillaImage>>,
}

/// Cache key for a face: the identity of its byte buffer plus the
/// collection index.
fn font_key(data: &imaging::peniko::FontData) -> (usize, u32) {
    (data.data.as_ref().as_ptr() as usize, data.index)
}

/// Cache key for an image: the identity of its pixel buffer.
fn image_key(img: &peniko::ImageData) -> usize {
    img.data.as_ref().as_ptr() as usize
}

impl SinkContext {
    /// Construct (or fetch the cached) krilla font for a face. `None` when
    /// krilla cannot read the font program.
    fn font(&mut self, data: &imaging::peniko::FontData) -> Option<KrillaFont> {
        let key = font_key(data);
        match self.fonts.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => Some(e.get().clone()),
            std::collections::hash_map::Entry::Vacant(e) => {
                // Zero-copy: share the blob's Arc-backed buffer with krilla
                // instead of duplicating a potentially 20 MB CJK font.
                let (arc, _) = data.data.clone().into_raw_parts();
                let font = KrillaFont::new(arc.into(), data.index)?;
                Some(e.insert(font).clone())
            }
        }
    }

    /// Glyph id → char for a face, built once by enumerating the font's
    /// cmap (the lowest codepoint wins for glyphs shared between chars).
    /// Empty when the bytes are not a readable font.
    fn reverse_cmap(&mut self, data: &imaging::peniko::FontData) -> Arc<HashMap<u32, char>> {
        let key = font_key(data);
        if let Some(m) = self.reverse_cmaps.get(&key) {
            return m.clone();
        }
        let mut m = HashMap::new();
        if let Some(font_ref) = swash::FontRef::from_index(data.data.as_ref(), data.index as usize)
        {
            swash::Charmap::from_font(&font_ref).enumerate(|cp, gid| {
                // Skip .notdef mappings; they explain no real glyph.
                if gid != 0 {
                    if let Some(ch) = char::from_u32(cp) {
                        m.entry(u32::from(gid)).or_insert(ch);
                    }
                }
            });
        }
        let arc = Arc::new(m);
        self.reverse_cmaps.insert(key, arc.clone());
        arc
    }

    /// Construct (or fetch the cached) krilla image for a scene image.
    /// `None` when the pixel data cannot be embedded (cached, so callers
    /// can warn once rather than per draw).
    fn image(&mut self, img: &peniko::ImageData) -> Option<KrillaImage> {
        let key = image_key(img);
        match self.images.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let decoded = decode_image(img);
                e.insert(decoded.clone());
                decoded
            }
        }
    }
}

/// Build a krilla image from a scene image brush's pixel data.
///
/// The renderer's ImageCache records already-decoded raw pixels (`format`
/// Rgba8, exactly 4·w·h bytes) — that path is a plain sample copy. Data
/// that does not match its labeled raw layout falls back to treating the
/// bytes as an encoded image (wordcraft's exporter branches: JPEG with
/// gray/RGB samples passes through byte-identical as DCTDecode, everything
/// else decodes and re-encodes as RGBA8).
fn decode_image(img: &peniko::ImageData) -> Option<KrillaImage> {
    let (w, h) = (img.width, img.height);
    let bytes = img.data.as_ref();
    if w == 0 || h == 0 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
        return None;
    }
    // Labeled raw pixels: an exact length match is required — anything
    // else is not the labeled format and drops through to the encoded
    // sniff below.
    if img.format.size_in_bytes(w, h) == Some(bytes.len()) {
        let rgba = match img.format {
            peniko::ImageFormat::Rgba8 => bytes.to_vec(),
            // krilla takes RGBA only; swizzle the channel pairs.
            peniko::ImageFormat::Bgra8 => {
                let mut rgba = bytes.to_vec();
                for px in rgba.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                rgba
            }
            // Future raw formats are not interpretable here.
            _ => return None,
        };
        return Some(KrillaImage::from_rgba8(rgba, w, h));
    }
    let fmt = image::guess_format(bytes).ok()?;
    let dec = image::load_from_memory_with_format(bytes, fmt).ok()?;
    let (dw, dh) = (dec.width(), dec.height());
    if dw == 0 || dh == 0 || dw > MAX_IMAGE_SIDE || dh > MAX_IMAGE_SIDE {
        return None;
    }
    if fmt == image::ImageFormat::Jpeg
        && matches!(dec.color(), image::ColorType::L8 | image::ColorType::Rgb8)
    {
        // Zero-copy: share the blob's Arc-backed buffer with krilla.
        let (arc, _) = img.data.clone().into_raw_parts();
        return KrillaImage::from_jpeg(arc.into(), true).ok();
    }
    Some(KrillaImage::from_rgba8(dec.to_rgba8().into_raw(), dw, dh))
}

/// Replays one page's worth of the recorded scene into a krilla surface.
/// `'pg` is the surface's own (page) borrow; `'a` is the sink's borrow of it.
struct PdfPageSink<'a, 'pg> {
    surface: &'a mut Surface<'pg>,
    ctx: &'a mut SinkContext,
    page: &'a PageMeta,
    warnings: &'a mut Vec<String>,
    /// Whether each pushed clip actually reached the surface; `pop_clip` may
    /// only pop the ones that did (krilla's `pop` panics when unbalanced).
    clips: Vec<bool>,
}

impl PdfPageSink<'_, '_> {
    /// True when a scene-px bounding box lies entirely outside this page's
    /// rect (page-local scenes can still contain out-of-rect draws; they
    /// are dropped here). Compares scene px, before the px→pt transform.
    fn culls_bbox(&self, bbox: kurbo::Rect) -> bool {
        let (x0, y0) = (self.page.x as f64, self.page.y as f64);
        let (x1, y1) = (
            (self.page.x + self.page.w) as f64,
            (self.page.y + self.page.h) as f64,
        );
        bbox.x1 < x0 || bbox.x0 > x1 || bbox.y1 < y0 || bbox.y0 > y1
    }

    /// True when the path lies entirely outside this page's scene rect.
    fn culls(&self, path: &BezPath) -> bool {
        self.culls_bbox(path.bounding_box())
    }

    /// A draw's geometry as a kurbo path in scene px: flattened, with the
    /// draw transform applied, and culled against this page. `None` when
    /// empty or off-page.
    fn scene_path(&self, shape: imaging::GeometryRef<'_>, transform: Affine) -> Option<BezPath> {
        let mut path = shape.to_path(TOLERANCE);
        path.apply_affine(transform);
        if path.elements().is_empty() || self.culls(&path) {
            return None;
        }
        Some(path)
    }

    /// Draw an image-brush fill. `Painter::draw_image` records the
    /// natural-size rect (0,0,w,h) as geometry and carries the whole
    /// placement (translate to the destination rect + scale from natural
    /// size) on the draw transform; peniko 0.6 image brushes have no
    /// transform of their own, and `brush_transform` is `None` on that
    /// path — composed in anyway so a producer that splits the placement
    /// still lands correctly. Mapping the natural-size rect through that
    /// transform yields the destination bbox for culling.
    fn fill_image(&mut self, draw: imaging::FillRef<'_>, brush: peniko::ImageBrushRef<'_>) {
        let t = draw.transform * draw.brush_transform.unwrap_or(Affine::IDENTITY);
        let (w, h) = (brush.image.width, brush.image.height);
        if w == 0 || h == 0 {
            return;
        }
        let corners = [
            t * kurbo::Point::new(0.0, 0.0),
            t * kurbo::Point::new(f64::from(w), 0.0),
            t * kurbo::Point::new(0.0, f64::from(h)),
            t * kurbo::Point::new(f64::from(w), f64::from(h)),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in corners {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if self.culls_bbox(kurbo::Rect::new(x0, y0, x1, y1)) {
            return;
        }
        // Warn once per image: the cache memoizes the failed decode, so a
        // repeat `None` here means the failure was already reported. The
        // renderer has painted its gray fallback box as solid fills in the
        // scene, so a sink-level failure just skips the image.
        let known_failure = self.ctx.images.contains_key(&image_key(brush.image));
        if let Some(img) = self.ctx.image(brush.image) {
            if let Some(size) = Size::from_wh(w as f32, h as f32) {
                self.surface.push_transform(&affine_to_krilla(t));
                // draw_image places the image at (0,0) with the given size;
                // under the pushed placement that covers exactly the
                // destination rect.
                self.surface.draw_image(img, size);
                self.surface.pop();
            }
        } else if !known_failure {
            self.warnings
                .push("image decode failed in PDF export; draw skipped".into());
        }
    }

    /// A clip's geometry as a krilla fill path in scene px. Fill clips map
    /// their shape directly; stroke clips expand to their stroked outline
    /// (krilla clips are fill-based). `None` when the path is empty.
    fn clip_path(&self, clip: imaging::ClipRef<'_>) -> Option<(Path, FillRule)> {
        match clip {
            imaging::ClipRef::Fill {
                transform,
                shape,
                fill_rule,
            } => {
                let mut path = shape.to_path(TOLERANCE);
                path.apply_affine(transform);
                Some((self.to_krilla_path(&path)?, krilla_fill_rule(fill_rule)))
            }
            imaging::ClipRef::Stroke {
                transform,
                shape,
                stroke,
            } => {
                let mut path = shape.to_path(TOLERANCE);
                path.apply_affine(transform);
                let outline = kurbo::stroke(&path, stroke, &Default::default(), TOLERANCE);
                Some((self.to_krilla_path(&outline)?, FillRule::NonZero))
            }
        }
    }

    /// Convert a kurbo path (scene px, transforms already applied) to a
    /// krilla path; `None` when it has no verbs (nothing to draw).
    fn to_krilla_path(&self, path: &BezPath) -> Option<Path> {
        let mut pb = krilla::geom::PathBuilder::new();
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                PathEl::QuadTo(c, p) => pb.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32),
                PathEl::CurveTo(c1, c2, p) => pb.cubic_to(
                    c1.x as f32,
                    c1.y as f32,
                    c2.x as f32,
                    c2.y as f32,
                    p.x as f32,
                    p.y as f32,
                ),
                PathEl::ClosePath => pb.close(),
            }
        }
        pb.finish()
    }
}

impl imaging::PaintSink for PdfPageSink<'_, '_> {
    fn push_clip(&mut self, clip: imaging::ClipRef<'_>) {
        match self.clip_path(clip) {
            Some((path, rule)) => {
                self.surface.push_clip_path(&path, &rule);
                self.clips.push(true);
            }
            // Remember the skipped push so the paired pop stays balanced.
            None => self.clips.push(false),
        }
    }

    fn pop_clip(&mut self) {
        if self.clips.pop() == Some(true) {
            self.surface.pop();
        }
    }

    fn push_group(&mut self, group: imaging::GroupRef<'_>) {
        // v1: groups render as an isolated layer, honoring the group alpha
        // (watermarks use 0.15). Filters, masks and non-default blend modes
        // would need rasterization or explicit mapping — warn and drop.
        if !group.filters.is_empty() {
            self.warnings.push("group filters skipped in PDF v1".into());
        }
        if group.mask.is_some() {
            self.warnings.push("group mask skipped in PDF v1".into());
        }
        if group.composite.blend != peniko::BlendMode::default() {
            self.warnings
                .push("group blend mode skipped in PDF v1".into());
        }
        if group.composite.alpha < 1.0 {
            self.surface.push_opacity(normalized(group.composite.alpha));
        } else {
            self.surface.push_isolated();
        }
    }

    fn pop_group(&mut self) {
        self.surface.pop();
    }

    fn fill(&mut self, draw: imaging::FillRef<'_>) {
        // Image draws arrive as fills with an image brush (painter.draw_image).
        if let peniko::BrushRef::Image(image_brush) = draw.brush {
            self.fill_image(draw, image_brush);
            return;
        }
        let peniko::BrushRef::Solid(color) = draw.brush else {
            self.warnings
                .push("unsupported brush in fill (gradient handled separately)".into());
            return;
        };
        let Some(path) = self.scene_path(draw.shape, draw.transform) else {
            return;
        };
        let Some(kp) = self.to_krilla_path(&path) else {
            return;
        };
        let (paint, opacity) = split_color(color);
        let fill = KrillaFill {
            paint,
            opacity,
            rule: krilla_fill_rule(draw.fill_rule),
        };
        self.surface.set_fill(Some(fill));
        self.surface.set_stroke(None);
        self.surface.draw_path(&kp);
    }

    fn stroke(&mut self, draw: imaging::StrokeRef<'_>) {
        let peniko::BrushRef::Solid(color) = draw.brush else {
            self.warnings
                .push("unsupported brush in stroke (gradient handled separately)".into());
            return;
        };
        let Some(path) = self.scene_path(draw.shape, draw.transform) else {
            return;
        };
        let Some(kp) = self.to_krilla_path(&path) else {
            return;
        };
        let (paint, opacity) = split_color(color);
        self.surface.set_fill(None);
        self.surface
            .set_stroke(Some(krilla_stroke(draw.stroke, paint, opacity)));
        self.surface.draw_path(&kp);
    }

    fn glyph_run(
        &mut self,
        draw: imaging::GlyphRunRef<'_>,
        glyphs: &mut dyn Iterator<Item = imaging::record::Glyph>,
    ) {
        let size = draw.font_size;
        if !size.is_finite() || size <= 0.0 {
            self.warnings
                .push("glyph run with invalid font size dropped".into());
            return;
        }
        // scene_builder always fills glyphs (Style::Fill + NonZero); map the
        // solid brush through the shared color helper. Image/gradient
        // brushes have no text equivalent — warn and drop.
        let peniko::BrushRef::Solid(color) = draw.brush else {
            self.warnings
                .push("unsupported brush in glyph run (image/gradient handled separately)".into());
            return;
        };
        // Page culling in scene px. Glyph positions are run-local; map them
        // through the run transform (identity from scene_builder, which
        // records absolute scene px — record::Glyph docs).
        let (x0, y0, x1, y1) = (
            f64::from(self.page.x),
            f64::from(self.page.y),
            f64::from(self.page.x + self.page.w),
            f64::from(self.page.y + self.page.h),
        );
        let kept: Vec<imaging::record::Glyph> = glyphs
            .filter(|g| {
                let p = draw.transform * kurbo::Point::new(f64::from(g.x), f64::from(g.y));
                p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
            })
            .collect();
        if kept.is_empty() {
            return;
        }
        let Some(font) = self.ctx.font(draw.font) else {
            self.warnings
                .push("font construct failed; glyph run dropped".into());
            return;
        };
        let rc = self.ctx.reverse_cmap(draw.font);
        // ToUnicode source text: one char per kept glyph (reverse cmap;
        // U+FFFD for glyphs the cmap cannot explain, e.g. shaped ligatures).
        // Cluster ranges are byte ranges into this string.
        let mut text = String::with_capacity(kept.len() * 3);
        let ranges: Vec<Range<usize>> = kept
            .iter()
            .map(|g| {
                let start = text.len();
                text.push(rc.get(&g.id).copied().unwrap_or('\u{FFFD}'));
                start..text.len()
            })
            .collect();
        let (paint, opacity) = split_color(color);
        self.surface.set_fill(Some(KrillaFill {
            paint,
            opacity,
            rule: FillRule::NonZero,
        }));
        self.surface.set_stroke(None);
        // Non-identity run transform: push it so krilla composes it with
        // the page transform; the glyph math below stays in run-local units.
        let mut pushed = 0usize;
        if draw.transform != Affine::IDENTITY {
            self.surface
                .push_transform(&affine_to_krilla(draw.transform));
            pushed += 1;
        }
        match draw.glyph_transform {
            // General per-glyph transform (vertical-text rotation): vello
            // applies it about each glyph's own origin, so anchor at every
            // glyph position and draw one glyph per call — a single
            // run-wide push would orbit later glyphs around the first.
            Some(t) if !is_pure_x_shear(&t) => {
                for (g, range) in kept.iter().zip(&ranges) {
                    let anchored = glyph_transform_for_surface(t, (f64::from(g.x), f64::from(g.y)));
                    self.surface.push_transform(&affine_to_krilla(anchored));
                    let single = [KrillaGlyph::new(
                        GlyphId::new(g.id),
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        range.clone(),
                        None,
                    )];
                    self.surface.draw_glyphs(
                        Point::from_xy(g.x, g.y),
                        &single,
                        font.clone(),
                        &text,
                        size,
                        false,
                    );
                    self.surface.pop();
                }
            }
            // No per-glyph transform, or a pure italic shear (which only
            // displaces points by their distance from the anchor's
            // baseline, so one anchored push covers the run exactly): one
            // draw call at the first glyph, positions rebuilt from advance
            // deltas. Advances are position deltas in em units; y offsets
            // are y-up — a glyph below the run's first baseline is
            // negative (krilla subtracts y_offset from the baseline).
            shear => {
                if let Some(t) = shear {
                    let base = (f64::from(kept[0].x), f64::from(kept[0].y));
                    let anchored = glyph_transform_for_surface(t, base);
                    self.surface.push_transform(&affine_to_krilla(anchored));
                    pushed += 1;
                }
                let base = (kept[0].x, kept[0].y);
                let kglyphs: Vec<KrillaGlyph> = kept
                    .iter()
                    .zip(&ranges)
                    .enumerate()
                    .map(|(i, (g, range))| {
                        let next_x = kept.get(i + 1).map_or(f64::from(g.x), |n| f64::from(n.x));
                        let adv_em = ((next_x - f64::from(g.x)) / f64::from(size)) as f32;
                        let y_off_em =
                            ((f64::from(base.1) - f64::from(g.y)) / f64::from(size)) as f32;
                        KrillaGlyph::new(
                            GlyphId::new(g.id),
                            adv_em,
                            0.0,
                            y_off_em,
                            0.0,
                            range.clone(),
                            None,
                        )
                    })
                    .collect();
                self.surface.draw_glyphs(
                    Point::from_xy(base.0, base.1),
                    &kglyphs,
                    font,
                    &text,
                    size,
                    false,
                );
            }
        }
        for _ in 0..pushed {
            self.surface.pop();
        }
    }

    fn blurred_rounded_rect(&mut self, _draw: imaging::BlurredRoundedRect) {
        self.warnings
            .push("blurred rounded rect skipped in PDF v1".into());
    }
}

/// Split a peniko RGBA color into krilla's model: an opaque RGB paint color
/// plus a separate opacity factor (krilla colors carry no alpha channel).
fn split_color(c: peniko::Color) -> (Paint, NormalizedF32) {
    let rgba = c.to_rgba8();
    (
        rgb::Color::new(rgba.r, rgba.g, rgba.b).into(),
        normalized(f32::from(rgba.a) / 255.0),
    )
}

/// Convert a kurbo affine into a krilla transform. Both use the same
/// coefficient layout ([a,b,c,d,e,f] with x' = a*x + c*y + e), so the
/// conversion is positional.
fn affine_to_krilla(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(
        c[0] as f32,
        c[1] as f32,
        c[2] as f32,
        c[3] as f32,
        c[4] as f32,
        c[5] as f32,
    )
}

/// Re-express a vello per-glyph transform for the y-down surface space,
/// anchored at `at`. vello applies the transform in the font's y-up outline
/// space about each glyph's own origin; conjugating by the y-flip converts
/// the linear map to surface orientation and the anchor localizes it to a
/// glyph position.
fn glyph_transform_for_surface(t: Affine, at: (f64, f64)) -> Affine {
    let flip = Affine::scale_non_uniform(1.0, -1.0);
    let anchor = Affine::translate(at);
    anchor * flip * t * flip * anchor.inverse()
}

/// True for a pure horizontal shear (`Affine::skew(k, 0)` — the italic
/// case). Such a transform displaces points only by their distance from
/// the anchor's baseline, so one run-wide anchored push is exact.
fn is_pure_x_shear(t: &Affine) -> bool {
    let c = t.as_coeffs();
    c[0] == 1.0 && c[1] == 0.0 && c[3] == 1.0 && c[4] == 0.0 && c[5] == 0.0
}

/// Clamp into `0..=1`, treating non-finite values as opaque.
fn normalized(v: f32) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        1.0
    })
    .unwrap_or(NormalizedF32::ONE)
}

/// Map a peniko fill rule onto krilla's.
fn krilla_fill_rule(rule: peniko::Fill) -> FillRule {
    match rule {
        peniko::Fill::NonZero => FillRule::NonZero,
        peniko::Fill::EvenOdd => FillRule::EvenOdd,
    }
}

/// Map a kurbo stroke style onto krilla's. krilla has a single line cap, so
/// a differing kurbo end cap collapses to the start cap.
fn krilla_stroke(s: &KurboStroke, paint: Paint, opacity: NormalizedF32) -> KrillaStroke {
    let dash = if s.dash_pattern.is_empty() {
        None
    } else {
        Some(StrokeDash {
            array: s.dash_pattern.iter().map(|&d| d as f32).collect(),
            offset: s.dash_offset as f32,
        })
    };
    KrillaStroke {
        paint,
        width: s.width as f32,
        miter_limit: s.miter_limit as f32,
        line_cap: krilla_line_cap(s.start_cap),
        line_join: krilla_line_join(s.join),
        opacity,
        dash,
    }
}

fn krilla_line_cap(cap: Cap) -> LineCap {
    match cap {
        Cap::Butt => LineCap::Butt,
        Cap::Square => LineCap::Square,
        Cap::Round => LineCap::Round,
    }
}

fn krilla_line_join(join: Join) -> LineJoin {
    match join {
        Join::Bevel => LineJoin::Bevel,
        Join::Miter => LineJoin::Miter,
        Join::Round => LineJoin::Round,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Font fixture shared with the render crate's tests (src → up two
    // levels reaches crates/).
    const FONT: &[u8] = include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf");

    fn glyph_page() -> PdfPage {
        let mut scene = imaging::record::Scene::new();
        let font_ref = swash::FontRef::from_index(FONT, 0).expect("TestFont must be readable");
        let gid = swash::Charmap::from_font(&font_ref).map('D');
        assert_ne!(gid, 0, "TestFont must map 'D' to a real glyph");
        let font = imaging::peniko::FontData {
            data: imaging::peniko::Blob::new(Arc::new(FONT.to_vec())),
            index: 0,
        };
        let mut p = imaging::Painter::new(&mut scene);
        p.glyphs(
            &font,
            imaging::peniko::Color::from_rgba8(0x00, 0x00, 0x00, 0xFF),
        )
        .font_size(32.0)
        .draw(
            &imaging::peniko::Style::Fill(imaging::peniko::Fill::NonZero),
            [imaging::record::Glyph {
                x: 0.0,
                y: 32.0,
                id: u32::from(gid),
            }],
        );
        PdfPage {
            width_px: 200.0,
            height_px: 100.0,
            scene,
        }
    }

    /// The public face returns bytes only; drive the sink directly to
    /// check the non-fatal degradation channel stayed silent for a healthy
    /// glyph run (every "glyph run dropped" path warns first).
    #[test]
    fn glyph_run_emits_no_warnings() {
        let page = glyph_page();
        let mut sink = PdfSink::new();
        sink.page_from_scene(
            &page.scene,
            &PageMeta {
                index: 0,
                x: 0.0,
                y: 0.0,
                w: page.width_px,
                h: page.height_px,
            },
        )
        .unwrap();
        let warnings = std::mem::take(&mut sink.warnings);
        sink.finish().unwrap();
        assert!(warnings.is_empty(), "glyph run must not warn: {warnings:?}");
    }
}
