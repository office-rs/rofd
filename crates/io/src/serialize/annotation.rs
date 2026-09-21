//! GB/T 33190 §15.2 `<PageAnnot><Annot>` serialization. Type (5 enums) +
//! Subtype express the rofd kind; Appearance = CT_PageBlock containing
//! PathObject/TextObject/ImageObject per payload kind; Remark stores Note
//! content; Parameters stores CreationDate / InReplyTo.
//!
//! This is the exact inverse of `parse::annotation::parse_page_annot`:
//! `parse(serialize(a)) == a` for all payload kinds (see
//! `tests/annotation_roundtrip.rs`).

use rofd_dom::{
    Annotation, AnnotationKind, AnnotationPayload, Color, OfdDocument, PageId, PathData, Rect,
    ShapeKind,
};

use crate::annotation_geom::{
    arrow_path, arrow_path_points, ellipse_path_inset, inset_rect_path, line_path,
    line_path_points, markup_line_path, polygon_path, polyline_path, rect_path, squiggly_path,
    translate_path, SQUIGGLY_AMPLITUDE,
};
use crate::dateutil::format_last_mod_date;

/// Serialize one page's annotations to GB/T 33190 §15.2 `<PageAnnot>` XML.
///
/// This is the inverse of `parse::annotation::parse_page_annot`.
///
/// `next_id` is the document-wide ST_ID allocator: it carries the highest
/// object ID reserved so far, so callers seed it from `OfdDocument.max_unit_id`
/// (see `object_id_seed`). Every appearance object reserves a fresh integer
/// from it - GB/T 33190 表 2 types object IDs as unsigned integers, and strict
/// readers drop an annotation file outright on a non-integer or duplicated
/// object ID. The caller patches `<MaxUnitID>` to the final counter value.
pub fn serialize_page_annot(_page: &PageId, anns: &[Annotation], next_id: &mut u64) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\">");
    for a in anns {
        s.push_str(&serialize_one(a, next_id));
    }
    s.push_str("</ofd:PageAnnot>");
    s
}

/// Seed for the document-wide object ID allocator: the highest ID that any
/// object in the document may already use. Well-formed files guarantee
/// `MaxUnitID` covers every body object, but a foreign writer may leave it
/// stale, so also cover the annotation IDs rofd knows about - appearance
/// object IDs must be unique document-wide no matter what the input looked
/// like.
pub fn object_id_seed(doc: &OfdDocument) -> u64 {
    let mut seed = doc.max_unit_id;
    for ann in doc.annotations.by_page.values().flatten() {
        if let Ok(n) = ann.id.0.parse::<u64>() {
            seed = seed.max(n);
        }
    }
    seed
}

/// Reserve the next document-wide object ID from the allocator.
fn mint_object_id(next_id: &mut u64) -> u64 {
    *next_id += 1;
    *next_id
}

/// Serialize a single `<Annot>` element.
fn serialize_one(a: &Annotation, next_id: &mut u64) -> String {
    let (ty, sub) = kind_to_type_subtype(&a.kind);
    let mut s = format!(
        "<ofd:Annot ID=\"{}\" Type=\"{}\" Creator=\"{}\" LastModDate=\"{}\" ReadOnly=\"false\"",
        xml_escape(&a.id.0),
        ty,
        xml_escape(&a.creator),
        format_last_mod_date(a.modified),
    );
    if let Some(sub) = sub {
        s.push_str(&format!(" Subtype=\"{}\"", sub));
    }
    s.push('>');
    // Parameters: CreationDate always; InReplyTo only when Some.
    s.push_str("<ofd:Parameters>");
    s.push_str(&format!(
        "<ofd:Parameter Name=\"CreationDate\">{}</ofd:Parameter>",
        format_last_mod_date(a.created)
    ));
    if let Some(r) = &a.reply_to {
        s.push_str(&format!(
            "<ofd:Parameter Name=\"InReplyTo\">{}</ofd:Parameter>",
            xml_escape(&r.0)
        ));
    }
    // Watermark: Opacity + Angle as Parameters (lossless f64 round-trip).
    // Alpha + CTM are still emitted on the TextObject for rendering compliance.
    if let AnnotationPayload::Watermark { opacity, angle, .. } = &a.payload {
        s.push_str(&format!(
            "<ofd:Parameter Name=\"Opacity\">{}</ofd:Parameter>",
            opacity
        ));
        s.push_str(&format!(
            "<ofd:Parameter Name=\"Angle\">{}</ofd:Parameter>",
            angle
        ));
    }
    // Vertices Parameter (GB/T 33190 §15.2.3.5): carries control points as
    // "x y x y ..." so parse can reconstruct `points`. Line/Arrow store their
    // two endpoints here so the drawn direction (and arrowhead position)
    // survives save/reload - the bbox `rect` alone loses which diagonal was
    // drawn. Foreign-authored Rect/Ellipse annots may also carry Vertices;
    // emit whenever the model has points so they survive the round-trip.
    if let AnnotationPayload::Shape { points, .. } = &a.payload {
        if !points.is_empty() {
            let mut verts = String::new();
            for p in points {
                verts.push_str(&format!("{} {} ", p.x, p.y));
            }
            s.push_str(&format!(
                "<ofd:Parameter Name=\"Vertices\">{}</ofd:Parameter>",
                verts.trim_end()
            ));
        }
    }
    s.push_str("</ofd:Parameters>");
    // Remark (Note content only).
    if matches!(a.kind, AnnotationKind::Note) {
        if let AnnotationPayload::Note { content, .. } = &a.payload {
            s.push_str(&format!("<ofd:Remark>{}</ofd:Remark>", xml_escape(content)));
        }
    }
    // Appearance per payload kind.
    s.push_str(&appearance_xml(&a.kind, &a.payload, next_id));
    s.push_str("</ofd:Annot>");
    s
}

/// Map `AnnotationKind` to GB/T 33190 Annot (Type, Subtype) attribute strings.
/// This is the inverse of `parse::annotation::map_type_subtype`.
fn kind_to_type_subtype(k: &AnnotationKind) -> (&'static str, Option<&'static str>) {
    match k {
        AnnotationKind::Highlight => ("Highlight", Some("Highlight")),
        AnnotationKind::Underline => ("Highlight", Some("Underline")),
        AnnotationKind::Strikeout => ("Highlight", Some("Strikeout")),
        AnnotationKind::Squiggly => ("Highlight", Some("Squiggly")),
        AnnotationKind::Freehand => ("Path", Some("Freehand")),
        AnnotationKind::Shape(ShapeKind::Rect) => ("Path", Some("Rectangle")),
        AnnotationKind::Shape(ShapeKind::Ellipse) => ("Path", Some("Ellipse")),
        AnnotationKind::Shape(ShapeKind::Arrow) => ("Path", Some("Arrow")),
        AnnotationKind::Shape(ShapeKind::Line) => ("Path", Some("Line")),
        AnnotationKind::Shape(ShapeKind::Polygon) => ("Path", Some("Polygon")),
        AnnotationKind::Shape(ShapeKind::PolyLine) => ("Path", Some("PolyLine")),
        AnnotationKind::Note => ("Path", Some("Note")),
        AnnotationKind::TextBox => ("FreeText", Some("FreeText")),
        AnnotationKind::Stamp => ("Stamp", None),
        AnnotationKind::Watermark => ("Watermark", None),
    }
}

/// Build the `<Appearance>` XML for the given kind + payload.
fn appearance_xml(kind: &AnnotationKind, payload: &AnnotationPayload, next_id: &mut u64) -> String {
    match (kind, payload) {
        (AnnotationKind::Highlight, AnnotationPayload::Markup { quad_points, color }) => {
            markup_highlight_appearance(quad_points, color, next_id)
        }
        (AnnotationKind::Underline, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, true, next_id)
        }
        (AnnotationKind::Strikeout, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, false, next_id)
        }
        (AnnotationKind::Squiggly, AnnotationPayload::Markup { quad_points, color }) => {
            // Same boundary structure as Underline/Strikeout (so parse reconstructs
            // the same quad_points), but the path is a wavy squiggly_path.
            markup_squiggly_appearance(quad_points, color, next_id)
        }
        (AnnotationKind::Freehand, AnnotationPayload::Freehand { path, color, width }) => {
            let r = path_bounds(path);
            // PathObject is appearance-relative: data shifts from page-local
            // into object-local coordinates.
            let local = translate_path(path, -r.x, -r.y);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                r.x,
                r.y,
                r.w,
                r.h,
                path_object_xml(&local_rect(&r), Some(*color), None, *width, &local, next_id)
            )
        }
        (
            AnnotationKind::Shape(ShapeKind::Polygon),
            AnnotationPayload::Shape {
                points,
                rect,
                stroke,
                fill,
                width,
                ..
            },
        ) => {
            let local: Vec<rofd_dom::Point> =
                points.iter().map(|p| to_object_local(p, rect)).collect();
            let path = polygon_path(&local);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(
                    &local_rect(rect),
                    Some(*stroke),
                    *fill,
                    *width,
                    &path,
                    next_id
                )
            )
        }
        (
            AnnotationKind::Shape(ShapeKind::PolyLine),
            AnnotationPayload::Shape {
                points,
                rect,
                stroke,
                fill,
                width,
                ..
            },
        ) => {
            let local: Vec<rofd_dom::Point> =
                points.iter().map(|p| to_object_local(p, rect)).collect();
            let path = polyline_path(&local);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(
                    &local_rect(rect),
                    Some(*stroke),
                    *fill,
                    *width,
                    &path,
                    next_id
                )
            )
        }
        (
            AnnotationKind::Shape(sk),
            AnnotationPayload::Shape {
                rect,
                stroke,
                fill,
                width,
                points,
                ..
            },
        ) => {
            // rect_path/ellipse_path only read w/h (already object-local);
            // line/arrow convert their endpoints relative to the absolute rect
            // origin, which equals the appearance origin.
            //
            // Rect/Ellipse strokes are inset by LineWidth/2: strict readers
            // clip a PathObject to its Boundary, so a path touching the box
            // edges loses the stroke's outer half on all four sides.
            let inset = width / 2.0;
            let path = match sk {
                ShapeKind::Rect => inset_rect_path(rect, inset),
                ShapeKind::Ellipse => ellipse_path_inset(rect, inset),
                // Direction-aware: when endpoints are stored, emit the actual
                // p0 -> p1 geometry (object-local) so other OFD readers also
                // see the drawn direction; fall back to the bbox diagonal.
                ShapeKind::Arrow => arrow_path_from(rect, points, *width),
                ShapeKind::Line => line_path_from(rect, points),
                // Polygon/PolyLine handled by their own arms above (with Vertices).
                ShapeKind::Polygon | ShapeKind::PolyLine => rect_path(rect),
            };
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(
                    &local_rect(rect),
                    Some(*stroke),
                    *fill,
                    *width,
                    &path,
                    next_id
                )
            )
        }
        (AnnotationKind::Note, AnnotationPayload::Note { rect, color, .. }) => {
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(
                    &local_rect(rect),
                    Some(*color),
                    None,
                    1.0,
                    // Inset by LineWidth/2 like every stroked frame: the
                    // stroke's outer half outside the Boundary is clipped.
                    &inset_rect_path(rect, 0.5),
                    next_id
                )
            )
        }
        (
            AnnotationKind::TextBox,
            AnnotationPayload::TextBox {
                rect,
                content,
                font,
                size,
                color,
                border,
            },
        ) => {
            // A bordered text box re-emits its frame ahead of the text (the
            // reference layout: border PathObject, then TextObjects). The
            // frame is inset by LineWidth/2 (reference convention: path starts
            // at 0.1764 inside a Boundary at LineWidth 0.3528) so the stroke
            // is not clipped to the Boundary edge.
            let mut inner = String::new();
            if let Some(border_color) = border {
                inner.push_str(&path_object_xml(
                    &local_rect(rect),
                    Some(*border_color),
                    None,
                    0.3528,
                    &inset_rect_path(rect, 0.3528 / 2.0),
                    next_id,
                ));
            }
            inner.push_str(&text_object_xml(
                rect, &font.0, *size, *color, content, next_id,
            ));
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x, rect.y, rect.w, rect.h, inner
            )
        }
        (AnnotationKind::Stamp, AnnotationPayload::Stamp { rect, image }) => {
            let id = mint_object_id(next_id);
            // CTM scales the image to the boundary: without it the image
            // renders as a ~1mm speck. Darken matches the reference stamps
            // (white-ish stamp backgrounds blend into the page).
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\"><ofd:ImageObject BlendMode=\"Darken\" ID=\"{}\" CTM=\"{} 0 0 {} 0 0\" Boundary=\"0 0 {} {}\" ResourceID=\"{}\"/></ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                id,
                rect.w,
                rect.h,
                rect.w,
                rect.h,
                xml_escape(&image.0)
            )
        }
        (
            AnnotationKind::Watermark,
            AnnotationPayload::Watermark {
                rect,
                content,
                opacity,
                angle,
                font,
                size,
                color,
            },
        ) => {
            let alpha = (*opacity * 255.0).round() as u8;
            let ctm = rotation_ctm(*angle, rect);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                text_object_xml_with_alpha(
                    rect, &font.0, *size, *color, content, alpha, &ctm, next_id
                )
            )
        }
        _ => "<ofd:Appearance Boundary=\"0 0 0 0\"/>".into(),
    }
}

// ---------------------------------------------------------------------------
// Appearance object helpers
// ---------------------------------------------------------------------------

/// Build a `<PathObject>` element.
///
/// Coordinate convention (matches reference authoring tools): the enclosing
/// `<Appearance Boundary>` stays page-absolute, but the PathObject's own
/// `Boundary` is APPEARANCE-RELATIVE (`0 0 w h`) and the AbbreviatedData is
/// relative to that object boundary (GB/T 33190 §8.2) - strict readers place
/// objects at `appearance.origin + object.boundary.origin + data`, so an
/// absolute object boundary (or page-local path data) lands the geometry at a
/// multiple of its true position. `r` is therefore the object-local rect and
/// `path` must already be in object-local coordinates.
///
/// Object IDs are minted from `next_id` (seeded at `doc.max_unit_id`) - GB/T
/// 33190 表 2 types object IDs as unsigned integers, and strict readers reject
/// the whole file on a non-integer or duplicated ID. A fill-only object
/// (highlight) carries `Stroke="false" Fill="true"` plus `<FillColor>`; a
/// stroke-only or stroke+fill object relies on the default `Stroke` and
/// writes the color elements it actually has.
fn path_object_xml(
    r: &Rect,
    stroke: Option<Color>,
    fill: Option<Color>,
    width: f64,
    path: &PathData,
    next_id: &mut u64,
) -> String {
    let id = mint_object_id(next_id);
    let mut s = format!(
        "<ofd:PathObject ID=\"{}\" Boundary=\"{} {} {} {}\" LineWidth=\"{}\"",
        id, r.x, r.y, r.w, r.h, width
    );
    if fill.is_some() && stroke.is_none() {
        s.push_str(" Stroke=\"false\" Fill=\"true\"");
    } else if fill.is_some() {
        // Fill defaults to false (GB/T 33190 表35); a fill+stroke shape (arrow
        // head, filled polygon) must opt in explicitly.
        s.push_str(" Fill=\"true\"");
    }
    s.push('>');
    if let Some(f) = fill {
        s.push_str(&format!("<ofd:FillColor Value=\"{}\"/>", color_str(f)));
    }
    if let Some(c) = stroke {
        s.push_str(&format!("<ofd:StrokeColor Value=\"{}\"/>", color_str(c)));
    }
    s.push_str(&format!(
        "<ofd:AbbreviatedData>{}</ofd:AbbreviatedData>",
        path_to_abbrev(path)
    ));
    s.push_str("</ofd:PathObject>");
    s
}

/// Build a `<TextObject>` element (TextBox). Multi-line content becomes one
/// TextObject per line, stacked top to bottom (the reference layout for
/// wrapped FreeText boxes).
fn text_object_xml(
    r: &Rect,
    font: &str,
    size: f64,
    color: Color,
    content: &str,
    next_id: &mut u64,
) -> String {
    let line_h = size * 1.2;
    let mut s = String::new();
    for (i, line) in content.split('\n').enumerate() {
        let id = mint_object_id(next_id);
        s.push_str(&format!(
            "<ofd:TextObject ID=\"{}\" Boundary=\"0 {} {} {}\" Font=\"{}\" Size=\"{}\"><ofd:FillColor Value=\"{}\"/>{}</ofd:TextObject>",
            id,
            i as f64 * line_h,
            r.w,
            line_h,
            xml_escape(font),
            size,
            color_str(color),
            text_code_xml(line, size)
        ));
    }
    s
}

/// Build a `<TextCode>` element carrying per-char `DeltaX` advances.
/// Without them strict readers place every glyph at X=0 - the whole line
/// collapses onto one spot. CJK/fullwidth chars advance one em (`size`),
/// ASCII halfwidth half an em: the metrics-free approximation of the
/// reference files' advances.
fn text_code_xml(line: &str, size: f64) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut deltas = String::new();
    for &c in chars.iter().take(chars.len().saturating_sub(1)) {
        let advance = if (0x20..=0x7E).contains(&(c as u32)) {
            size * 0.5
        } else {
            size
        };
        deltas.push_str(&format!("{} ", advance));
    }
    let delta_attr = if chars.len() >= 2 {
        format!(" DeltaX=\"{}\"", deltas.trim_end())
    } else {
        String::new()
    };
    format!(
        "<ofd:TextCode X=\"0\" Y=\"{}\"{}>{}</ofd:TextCode>",
        size,
        delta_attr,
        xml_escape(line)
    )
}

/// Build a `<TextObject>` element with Alpha + CTM (Watermark).
// Watermark styling is inherently wide (font/size/color/alpha/ctm + geometry
// + the ID allocator); folding it into a struct buys nothing at one call site.
#[allow(clippy::too_many_arguments)]
fn text_object_xml_with_alpha(
    r: &Rect,
    font: &str,
    size: f64,
    color: Color,
    content: &str,
    alpha: u8,
    ctm: &str,
    next_id: &mut u64,
) -> String {
    let id = mint_object_id(next_id);
    format!(
        "<ofd:TextObject ID=\"{}\" Boundary=\"0 0 {} {}\" Font=\"{}\" Size=\"{}\" CTM=\"{}\" Alpha=\"{}\"><ofd:FillColor Value=\"{}\"/>{}</ofd:TextObject>",
        id,
        r.w,
        r.h,
        xml_escape(font),
        size,
        ctm,
        alpha,
        color_str(color),
        text_code_xml(content, size)
    )
}

/// Highlight appearance: one filled rectangle per quad pair. The enclosing
/// `<Appearance Boundary>` uses absolute coords (p0.x p0.y dx dy) so the
/// parser reconstructs the exact quad_points from boundary corners; the inner
/// PathObject is appearance-relative with object-local path data. Fill-only
/// (no stroke) - a stroked highlight renders as a hollow outline in strict
/// readers.
fn markup_highlight_appearance(
    quad_points: &[rofd_dom::Point],
    color: &Color,
    next_id: &mut u64,
) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_point_pairs(quad_points) {
        let r = Rect {
            x: p0.x,
            y: p0.y,
            w: p1.x - p0.x,
            h: p1.y - p0.y,
        };
        s.push_str(&format!(
            "<ofd:Appearance Boundary=\"{} {} {} {}\">",
            r.x, r.y, r.w, r.h
        ));
        s.push_str(&path_object_xml(
            &local_rect(&r),
            None,
            Some(*color),
            0.5,
            &rect_path(&r),
            next_id,
        ));
        s.push_str("</ofd:Appearance>");
    }
    s
}

/// Underline/Strikeout appearance: one line per quad pair.
/// `at_bottom = true` for underline (bottom edge), `false` for strikeout (midline).
fn markup_line_appearance(
    quad_points: &[rofd_dom::Point],
    color: &Color,
    at_bottom: bool,
    next_id: &mut u64,
) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_point_pairs(quad_points) {
        let r = quad_rect(&p0, &p1);
        // Object-local: quad_points are page-local, the PathObject data must
        // be relative to the (appearance-relative) object boundary.
        let path = markup_line_path(
            to_object_local(&p0, &r),
            to_object_local(&p1, &r),
            at_bottom,
        );
        s.push_str(&format!(
            "<ofd:Appearance Boundary=\"{} {} {} {}\">",
            r.x, r.y, r.w, r.h
        ));
        s.push_str(&path_object_xml(
            &local_rect(&r),
            Some(*color),
            None,
            0.5,
            &path,
            next_id,
        ));
        s.push_str("</ofd:Appearance>");
    }
    s
}

/// Squiggly appearance: one wavy path per quad pair. The PathObject boundary
/// matches `markup_line_appearance` (so parse reconstructs the same
/// quad_points), but the AbbreviatedData uses `squiggly_path` (Q curves) for
/// the wavy rendering (GB/T 33190 §15.2.3.4).
fn markup_squiggly_appearance(
    quad_points: &[rofd_dom::Point],
    color: &Color,
    next_id: &mut u64,
) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_point_pairs(quad_points) {
        let r = quad_rect(&p0, &p1);
        let lp0 = to_object_local(&p0, &r);
        let lp1 = to_object_local(&p1, &r);
        // Baseline one amplitude ABOVE the quad bottom: strict readers clip a
        // PathObject to its Boundary, so a baseline ON the bottom edge (y=h)
        // loses the wave's lower half to clipping. Reference authoring tools
        // keep the whole wave inside the boundary this way. A baseline at
        // p0.y (quad top) instead renders the wave over the top of the glyphs.
        let baseline = (lp0.y.max(lp1.y) - SQUIGGLY_AMPLITUDE).max(0.0);
        let path = squiggly_path(
            rofd_dom::Point {
                x: lp0.x,
                y: baseline,
            },
            rofd_dom::Point {
                x: lp1.x,
                y: baseline,
            },
        );
        s.push_str(&format!(
            "<ofd:Appearance Boundary=\"{} {} {} {}\">",
            r.x, r.y, r.w, r.h
        ));
        s.push_str(&path_object_xml(
            &local_rect(&r),
            Some(*color),
            None,
            0.5,
            &path,
            next_id,
        ));
        s.push_str("</ofd:Appearance>");
    }
    s
}

/// Bounding rect (page-local) of one quad pair.
fn quad_rect(p0: &rofd_dom::Point, p1: &rofd_dom::Point) -> Rect {
    Rect {
        x: p0.x.min(p1.x),
        y: p0.y.min(p1.y),
        w: (p1.x - p0.x).abs(),
        h: (p1.y - p0.y).abs(),
    }
}

/// The appearance-relative form of a page-local rect: same size, origin at
/// (0, 0) - the object Boundary convention inside an annotation Appearance.
fn local_rect(r: &Rect) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        w: r.w,
        h: r.h,
    }
}

/// Iterate quad_points as pairs (p0, p1). Each pair defines one quad rectangle.
fn quad_point_pairs(
    quad_points: &[rofd_dom::Point],
) -> impl Iterator<Item = (rofd_dom::Point, rofd_dom::Point)> + use<'_> {
    quad_points.chunks(2).filter_map(|c| {
        if c.len() == 2 {
            Some((c[0], c[1]))
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Geometry / formatting helpers
// ---------------------------------------------------------------------------

/// Build a Line's AbbreviatedData path from stored endpoints (object-local)
/// when available, falling back to the rect's TL->BR diagonal when only the
/// bbox is known (legacy/external OFD).
fn line_path_from(rect: &Rect, points: &[rofd_dom::Point]) -> PathData {
    if let (Some(p0), Some(p1)) = (points.first(), points.get(1)) {
        line_path_points(to_object_local(p0, rect), to_object_local(p1, rect))
    } else {
        line_path(rect)
    }
}

/// Build an Arrow's AbbreviatedData path (shaft + filled head) from stored
/// endpoints (object-local) when available, falling back to the rect's
/// TL->BR diagonal + head when only the bbox is known. Head size follows the
/// stroke `width` (5 x width, +/-25 degrees - reference sample geometry).
fn arrow_path_from(rect: &Rect, points: &[rofd_dom::Point], width: f64) -> PathData {
    if let (Some(p0), Some(p1)) = (points.first(), points.get(1)) {
        arrow_path_points(to_object_local(p0, rect), to_object_local(p1, rect), width)
    } else {
        arrow_path(rect, width)
    }
}

/// Convert a page-local point to object-local (relative to the PathObject
/// Boundary origin). OFD AbbreviatedData is relative to the object boundary,
/// so endpoints stored in page-local coords must be shifted before emission.
fn to_object_local(p: &rofd_dom::Point, rect: &Rect) -> rofd_dom::Point {
    rofd_dom::Point {
        x: p.x - rect.x,
        y: p.y - rect.y,
    }
}

/// Compute the bounding rect of a PathData (min/max of M/L points).
fn path_bounds(p: &PathData) -> Rect {
    let (mut minx, mut miny, mut maxx, mut maxy) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for c in &p.commands {
        let (x, y) = match c {
            rofd_dom::PathCommand::M(x, y) => (*x, *y),
            rofd_dom::PathCommand::L(x, y) => (*x, *y),
            _ => continue,
        };
        minx = minx.min(x);
        miny = miny.min(y);
        maxx = maxx.max(x);
        maxy = maxy.max(y);
    }
    if !minx.is_finite() {
        return Rect::default();
    }
    Rect {
        x: minx,
        y: miny,
        w: maxx - minx,
        h: maxy - miny,
    }
}

/// Build a CTM string for a rotation around the rect center.
/// CTM = [cos sin -sin cos e f] where (e,f) translates so the center stays fixed.
fn rotation_ctm(angle_deg: f64, r: &Rect) -> String {
    let rad = angle_deg.to_radians();
    let (cos, sin) = (rad.cos(), rad.sin());
    let (cx, cy) = (r.w / 2.0, r.h / 2.0);
    let e = cx - cx * cos + cy * sin;
    let f = cy - cx * sin - cy * cos;
    format!("{} {} {} {} {} {}", cos, sin, -sin, cos, e, f)
}

/// Serialize PathData to OFD AbbreviatedData string (inverse of `parse_abbreviated`).
fn path_to_abbrev(p: &PathData) -> String {
    let mut s = String::new();
    // Subpath start for explicit closure: reference authoring tools do not
    // honor a "Z" close operator (a Z-terminated polygon renders open, a
    // Z-terminated rectangle loses its closing edge), so closure is written
    // as a plain L segment back to the subpath start.
    let mut start: Option<(f64, f64)> = None;
    for c in &p.commands {
        match c {
            rofd_dom::PathCommand::M(x, y) => {
                start = Some((*x, *y));
                s.push_str(&format!("M {} {} ", x, y));
            }
            rofd_dom::PathCommand::L(x, y) => {
                s.push_str(&format!("L {} {} ", x, y));
            }
            rofd_dom::PathCommand::C(a, b, c, d, e, g) => {
                s.push_str(&format!("C {} {} {} {} {} {} ", a, b, c, d, e, g));
            }
            rofd_dom::PathCommand::Q(a, b, c, d) => {
                s.push_str(&format!("Q {} {} {} {} ", a, b, c, d));
            }
            rofd_dom::PathCommand::Z => {
                if let Some((x, y)) = start {
                    s.push_str(&format!("L {} {} ", x, y));
                }
            }
            rofd_dom::PathCommand::A(a, b, c, d, e, f) => {
                // GB/T 33190 A arc has 7 params (rx ry rot large-arc-flag
                // sweep-flag x y). PathCommand::A carries 6 (rx ry rot sweep x
                // y), dropping large-arc-flag (quarter arcs are always
                // small-arc -> emit 0). So emit "A rx ry rot 0 sweep x y".
                s.push_str(&format!("A {} {} {} 0 {} {} {} ", a, b, c, d, e, f));
            }
        }
    }
    s
}

fn color_str(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("{} {} {}", r, g, b),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{PathCommand, Point};

    #[test]
    fn color_str_formats_rgb() {
        assert_eq!(color_str(Color::Rgb(255, 0, 128)), "255 0 128");
    }

    #[test]
    fn xml_escape_escapes_special() {
        assert_eq!(xml_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn path_to_abbrev_round_trips_through_parse() {
        // Closure (Z) is serialized as an explicit L back to the subpath
        // start (reference readers do not honor a Z operator), so it parses
        // back as that closing L segment.
        let pd = PathData {
            commands: vec![
                PathCommand::M(0.0, 0.0),
                PathCommand::L(10.0, 20.0),
                PathCommand::C(1.0, 2.0, 3.0, 4.0, 5.0, 6.0),
                PathCommand::Q(1.0, 2.0, 3.0, 4.0),
                PathCommand::Z,
            ],
        };
        let abbrev = path_to_abbrev(&pd);
        assert!(!abbrev.contains('Z'), "no Z on the wire: {abbrev}");
        let parsed = crate::abbreviated::parse_abbreviated(&abbrev);
        assert_eq!(
            parsed.commands.last(),
            Some(&PathCommand::L(0.0, 0.0)),
            "closes back at the subpath start"
        );
        assert_eq!(parsed.commands.len(), pd.commands.len());
    }

    #[test]
    fn path_to_abbrev_round_trips_arc_commands() {
        // T3: A (arc) operators must round-trip (ellipse_path emits them).
        // PathCommand::A has 6 params (rx, ry, rot, sweep, x, y); the wire
        // format has 7 (large-arc-flag inserted as 0). See GB/T 33190 §8.2.
        let pd = PathData {
            commands: vec![
                PathCommand::M(5.0, 0.0),
                PathCommand::A(5.0, 5.0, 0.0, 1.0, 0.0, 5.0),
                PathCommand::A(5.0, 5.0, 0.0, 1.0, -5.0, 0.0),
                PathCommand::A(5.0, 5.0, 0.0, 1.0, 0.0, -5.0),
                PathCommand::A(5.0, 5.0, 0.0, 1.0, 5.0, 0.0),
                PathCommand::Z,
            ],
        };
        let abbrev = path_to_abbrev(&pd);
        let parsed = crate::abbreviated::parse_abbreviated(&abbrev);
        // Z becomes the explicit closing L back to the M start (5, 0); the
        // four arcs round-trip unchanged.
        assert_eq!(
            parsed.commands.last(),
            Some(&PathCommand::L(5.0, 0.0)),
            "closes back at the subpath start"
        );
        assert_eq!(parsed.commands.len(), pd.commands.len());
    }

    #[test]
    fn path_to_abbrev_emits_7_values_per_arc() {
        // GB/T 33190 interop: the A operator must emit 7 values
        // (rx ry rot large-arc-flag sweep x y). ellipse_path produces 4 A
        // commands, so the AbbreviatedData must have 4*7 = 28 numeric tokens
        // following each "A" letter.
        let pd = PathData {
            commands: vec![PathCommand::A(5.0, 5.0, 0.0, 1.0, 0.0, 5.0)],
        };
        let abbrev = path_to_abbrev(&pd);
        // "A 5 5 0 0 1 0 5 " -> after "A", 7 numeric tokens.
        let toks: Vec<&str> = abbrev.split_whitespace().collect();
        assert_eq!(toks[0], "A");
        assert_eq!(toks.len(), 8); // "A" + 7 values
                                   // large-arc-flag (4th value) must be 0.
        let large_arc: f64 = toks[4].parse().unwrap();
        assert!(
            (large_arc - 0.0).abs() < 1e-10,
            "large-arc-flag should be 0, got {large_arc}"
        );
        // The 6-field PathCommand (rx ry rot sweep x y) maps to values
        // [1,2,3,5,6,7] (skipping index 4 = large-arc-flag).
        assert!((toks[1].parse::<f64>().unwrap() - 5.0).abs() < 1e-10); // rx
        assert!((toks[2].parse::<f64>().unwrap() - 5.0).abs() < 1e-10); // ry
        assert!((toks[3].parse::<f64>().unwrap() - 0.0).abs() < 1e-10); // rot
        assert!((toks[5].parse::<f64>().unwrap() - 1.0).abs() < 1e-10); // sweep
        assert!((toks[6].parse::<f64>().unwrap() - 0.0).abs() < 1e-10); // x
        assert!((toks[7].parse::<f64>().unwrap() - 5.0).abs() < 1e-10); // y
    }

    #[test]
    fn ellipse_serialization_has_7_value_arcs() {
        // An ellipse_path produces 4 A commands; each must emit 7 values
        // (28 numeric tokens total across the 4 A operators).
        use rofd_dom::Rect;
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let path = crate::annotation_geom::ellipse_path(&r);
        let abbrev = path_to_abbrev(&path);
        let toks: Vec<&str> = abbrev.split_whitespace().collect();
        // M + 2 values, then 4 * (A + 7 values), then the explicit closing
        // L back to the subpath start (L + 2 values; closure is a plain
        // segment, not a Z operator).
        // = 1 + 2 + 4 * 8 + 3 = 38
        assert_eq!(toks.len(), 38);
        // Each "A" is followed by exactly 7 numeric tokens.
        let mut i = 0;
        let mut arc_count = 0;
        while i < toks.len() {
            if toks[i] == "A" {
                arc_count += 1;
                // The next 7 tokens must all be numeric (parse as f64).
                for j in 1..=7 {
                    toks[i + j].parse::<f64>().unwrap_or_else(|_| {
                        panic!("A operand {} ({:?}) not numeric", j, toks[i + j])
                    });
                }
                i += 8;
            } else {
                i += 1;
            }
        }
        assert_eq!(arc_count, 4, "ellipse should emit 4 A arcs");
    }

    #[test]
    fn path_bounds_of_empty_is_default() {
        let pd = PathData::default();
        assert_eq!(path_bounds(&pd), Rect::default());
    }

    #[test]
    fn path_bounds_of_two_points() {
        let pd = PathData {
            commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(5.0, 8.0)],
        };
        let r = path_bounds(&pd);
        assert_eq!(r.x, 1.0);
        assert_eq!(r.y, 2.0);
        assert_eq!(r.w, 4.0);
        assert_eq!(r.h, 6.0);
    }

    #[test]
    fn rotation_ctm_at_zero_is_identity() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let ctm = rotation_ctm(0.0, &r);
        // cos(0)=1, sin(0)=0 -> "1 0 -0 1 0 0" but -0.0 formats as "-0"
        let parts: Vec<&str> = ctm.split_whitespace().collect();
        assert_eq!(parts.len(), 6);
        let a: f64 = parts[0].parse().unwrap();
        let d: f64 = parts[3].parse().unwrap();
        assert!((a - 1.0).abs() < 1e-10);
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn serialize_page_annot_empty_yields_empty_pagennot() {
        let mut next_id = 100u64;
        let xml = serialize_page_annot(&PageId::new("1"), &[], &mut next_id);
        assert!(xml.contains("<ofd:PageAnnot"));
        assert!(xml.contains("</ofd:PageAnnot>"));
        // No <Annot> elements.
        assert!(!xml.contains("<ofd:Annot"));
        // No object IDs minted for an empty page.
        assert_eq!(next_id, 100);
    }

    #[test]
    fn kind_to_type_subtype_covers_all_kinds() {
        // Ensure every variant produces a valid (Type, Subtype).
        let cases = [
            AnnotationKind::Highlight,
            AnnotationKind::Underline,
            AnnotationKind::Strikeout,
            AnnotationKind::Squiggly,
            AnnotationKind::Freehand,
            AnnotationKind::Shape(ShapeKind::Rect),
            AnnotationKind::Shape(ShapeKind::Ellipse),
            AnnotationKind::Shape(ShapeKind::Arrow),
            AnnotationKind::Shape(ShapeKind::Line),
            AnnotationKind::Shape(ShapeKind::Polygon),
            AnnotationKind::Shape(ShapeKind::PolyLine),
            AnnotationKind::Note,
            AnnotationKind::TextBox,
            AnnotationKind::Stamp,
            AnnotationKind::Watermark,
        ];
        for k in &cases {
            let (ty, sub) = kind_to_type_subtype(k);
            assert!(!ty.is_empty(), "Type empty for {:?}", k);
            // Stamp and Watermark have no Subtype; all others do.
            if !matches!(k, AnnotationKind::Stamp | AnnotationKind::Watermark) {
                assert!(sub.is_some(), "Subtype missing for {:?}", k);
            }
        }
    }

    #[test]
    fn textbox_maps_to_freetext_type() {
        // T3: TextBox serializes as Type=FreeText (was Path), matching real OFD
        // and parse's ("FreeText", _) => TextBox arm.
        let (ty, sub) = kind_to_type_subtype(&AnnotationKind::TextBox);
        assert_eq!(ty, "FreeText");
        assert_eq!(sub, Some("FreeText"));
    }

    #[test]
    fn squiggly_polygon_polyline_type_subtype() {
        // T3: verify the new kind arms produce the spec-correct (Type, Subtype).
        assert_eq!(
            kind_to_type_subtype(&AnnotationKind::Squiggly),
            ("Highlight", Some("Squiggly"))
        );
        assert_eq!(
            kind_to_type_subtype(&AnnotationKind::Shape(ShapeKind::Polygon)),
            ("Path", Some("Polygon"))
        );
        assert_eq!(
            kind_to_type_subtype(&AnnotationKind::Shape(ShapeKind::PolyLine)),
            ("Path", Some("PolyLine"))
        );
    }

    #[test]
    fn quad_point_pairs_iterates_pairs() {
        let pts = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 10.0, y: 10.0 },
            Point { x: 20.0, y: 20.0 },
            Point { x: 30.0, y: 30.0 },
        ];
        let pairs: Vec<_> = quad_point_pairs(&pts).collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0.x, 0.0);
        assert_eq!(pairs[0].1.x, 10.0);
        assert_eq!(pairs[1].0.x, 20.0);
        assert_eq!(pairs[1].1.x, 30.0);
    }

    #[test]
    fn quad_point_pairs_drops_odd() {
        let pts = vec![Point { x: 0.0, y: 0.0 }];
        let pairs: Vec<_> = quad_point_pairs(&pts).collect();
        assert_eq!(pairs.len(), 0);
    }
}
