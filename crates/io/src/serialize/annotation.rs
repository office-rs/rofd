//! GB/T 33190 §15.2 `<PageAnnot><Annot>` serialization. Type (5 enums) +
//! Subtype express the rofd kind; Appearance = CT_PageBlock containing
//! PathObject/TextObject/ImageObject per payload kind; Remark stores Note
//! content; Parameters stores CreationDate / InReplyTo.
//!
//! This is the exact inverse of `parse::annotation::parse_page_annot`:
//! `parse(serialize(a)) == a` for all payload kinds (see
//! `tests/annotation_roundtrip.rs`).

use rofd_dom::{
    Annotation, AnnotationKind, AnnotationPayload, Color, PageId, PathData, Rect, ShapeKind,
};

use crate::annotation_geom::{
    arrow_path, ellipse_path, line_path, markup_line_path, polygon_path, polyline_path, rect_path,
    squiggly_path,
};
use crate::dateutil::format_last_mod_date;

/// Serialize one page's annotations to GB/T 33190 §15.2 `<PageAnnot>` XML.
///
/// This is the inverse of `parse::annotation::parse_page_annot`.
pub fn serialize_page_annot(_page: &PageId, anns: &[Annotation]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\">");
    for a in anns {
        s.push_str(&serialize_one(a));
    }
    s.push_str("</ofd:PageAnnot>");
    s
}

/// Legacy alias (save.rs / write_ofd call this); equivalent to `serialize_page_annot`.
pub fn serialize_page_annotations(page: &PageId, anns: &[Annotation]) -> String {
    serialize_page_annot(page, anns)
}

/// Serialize a single `<Annot>` element.
fn serialize_one(a: &Annotation) -> String {
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
    // Polygon/PolyLine: Vertices Parameter (GB/T 33190 §15.2.3.5) carries the
    // control points as "x y x y ..." so parse can reconstruct `points`.
    if let AnnotationPayload::Shape {
        points,
        kind: ShapeKind::Polygon | ShapeKind::PolyLine,
        ..
    } = &a.payload
    {
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
    s.push_str(&appearance_xml(&a.kind, &a.payload));
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
fn appearance_xml(kind: &AnnotationKind, payload: &AnnotationPayload) -> String {
    match (kind, payload) {
        (AnnotationKind::Highlight, AnnotationPayload::Markup { quad_points, color }) => {
            markup_highlight_appearance(quad_points, color)
        }
        (AnnotationKind::Underline, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, true)
        }
        (AnnotationKind::Strikeout, AnnotationPayload::Markup { quad_points, color }) => {
            markup_line_appearance(quad_points, color, false)
        }
        (AnnotationKind::Squiggly, AnnotationPayload::Markup { quad_points, color }) => {
            // Same boundary structure as Underline/Strikeout (so parse reconstructs
            // the same quad_points), but the path is a wavy squiggly_path.
            markup_squiggly_appearance(quad_points, color)
        }
        (AnnotationKind::Freehand, AnnotationPayload::Freehand { path, color, width }) => {
            let r = path_bounds(path);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                r.x,
                r.y,
                r.w,
                r.h,
                path_object_xml(&r, *color, None, *width, path)
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
            let path = polygon_path(points);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(rect, *stroke, *fill, *width, &path)
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
            let path = polyline_path(points);
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(rect, *stroke, *fill, *width, &path)
            )
        }
        (
            AnnotationKind::Shape(sk),
            AnnotationPayload::Shape {
                rect,
                stroke,
                fill,
                width,
                ..
            },
        ) => {
            let path = match sk {
                ShapeKind::Rect => rect_path(rect),
                ShapeKind::Ellipse => ellipse_path(rect),
                ShapeKind::Arrow => arrow_path(rect),
                ShapeKind::Line => line_path(rect),
                // Polygon/PolyLine handled by their own arms above (with Vertices).
                ShapeKind::Polygon | ShapeKind::PolyLine => rect_path(rect),
            };
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(rect, *stroke, *fill, *width, &path)
            )
        }
        (AnnotationKind::Note, AnnotationPayload::Note { rect, color, .. }) => {
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                path_object_xml(rect, *color, None, 1.0, &rect_path(rect))
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
            },
        ) => {
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\">{}</ofd:Appearance>",
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                text_object_xml(rect, &font.0, *size, *color, content)
            )
        }
        (AnnotationKind::Stamp, AnnotationPayload::Stamp { rect, image }) => {
            format!(
                "<ofd:Appearance Boundary=\"{} {} {} {}\"><ofd:ImageObject ID=\"s1\" Boundary=\"0 0 {} {}\" ResourceID=\"{}\"/></ofd:Appearance>",
                rect.x,
                rect.y,
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
                text_object_xml_with_alpha(rect, &font.0, *size, *color, content, alpha, &ctm)
            )
        }
        _ => "<ofd:Appearance Boundary=\"0 0 0 0\"/>".into(),
    }
}

// ---------------------------------------------------------------------------
// Appearance object helpers
// ---------------------------------------------------------------------------

/// Build a `<PathObject>` element. The boundary is the absolute (page-local)
/// rect so that the parser can extract quad_points from it for Markup kinds.
fn path_object_xml(
    r: &Rect,
    stroke: Color,
    fill: Option<Color>,
    width: f64,
    path: &PathData,
) -> String {
    let mut s = format!(
        "<ofd:PathObject ID=\"a0\" Boundary=\"{} {} {} {}\" LineWidth=\"{}\">",
        r.x, r.y, r.w, r.h, width
    );
    if let Some(f) = fill {
        s.push_str(&format!("<ofd:FillColor Value=\"{}\"/>", color_str(f)));
    }
    s.push_str(&format!(
        "<ofd:StrokeColor Value=\"{}\"/>",
        color_str(stroke)
    ));
    s.push_str(&format!(
        "<ofd:AbbreviatedData>{}</ofd:AbbreviatedData>",
        path_to_abbrev(path)
    ));
    s.push_str("</ofd:PathObject>");
    s
}

/// Build a `<TextObject>` element (TextBox).
fn text_object_xml(r: &Rect, font: &str, size: f64, color: Color, content: &str) -> String {
    format!(
        "<ofd:TextObject ID=\"t0\" Boundary=\"0 0 {} {}\" Font=\"{}\" Size=\"{}\"><ofd:FillColor Value=\"{}\"/><ofd:TextCode X=\"0\" Y=\"{}\">{}</ofd:TextCode></ofd:TextObject>",
        r.w,
        r.h,
        xml_escape(font),
        size,
        color_str(color),
        size,
        xml_escape(content)
    )
}

/// Build a `<TextObject>` element with Alpha + CTM (Watermark).
fn text_object_xml_with_alpha(
    r: &Rect,
    font: &str,
    size: f64,
    color: Color,
    content: &str,
    alpha: u8,
    ctm: &str,
) -> String {
    format!(
        "<ofd:TextObject ID=\"w0\" Boundary=\"0 0 {} {}\" Font=\"{}\" Size=\"{}\" CTM=\"{}\" Alpha=\"{}\"><ofd:FillColor Value=\"{}\"/><ofd:TextCode X=\"0\" Y=\"{}\">{}</ofd:TextCode></ofd:TextObject>",
        r.w,
        r.h,
        xml_escape(font),
        size,
        ctm,
        alpha,
        color_str(color),
        size,
        xml_escape(content)
    )
}

/// Highlight appearance: one filled rectangle per quad pair, with Darken blend.
/// The PathObject boundary uses absolute coords (p0.x p0.y dx dy) so the parser
/// reconstructs the exact quad_points from boundary corners.
fn markup_highlight_appearance(quad_points: &[rofd_dom::Point], color: &Color) -> String {
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
        s.push_str(&path_object_xml(&r, *color, None, 0.5, &rect_path(&r)));
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
) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_point_pairs(quad_points) {
        let r = Rect {
            x: p0.x.min(p1.x),
            y: p0.y.min(p1.y),
            w: (p1.x - p0.x).abs(),
            h: (p1.y - p0.y).abs(),
        };
        let path = markup_line_path(p0, p1, at_bottom);
        s.push_str(&format!(
            "<ofd:Appearance Boundary=\"{} {} {} {}\">",
            r.x, r.y, r.w, r.h
        ));
        s.push_str(&path_object_xml(&r, *color, None, 0.5, &path));
        s.push_str("</ofd:Appearance>");
    }
    s
}

/// Squiggly appearance: one wavy path per quad pair. The PathObject boundary
/// matches `markup_line_appearance` (so parse reconstructs the same
/// quad_points), but the AbbreviatedData uses `squiggly_path` (Q curves) for
/// the wavy rendering (GB/T 33190 §15.2.3.4).
fn markup_squiggly_appearance(quad_points: &[rofd_dom::Point], color: &Color) -> String {
    let mut s = String::new();
    for (p0, p1) in quad_point_pairs(quad_points) {
        let r = Rect {
            x: p0.x.min(p1.x),
            y: p0.y.min(p1.y),
            w: (p1.x - p0.x).abs(),
            h: (p1.y - p0.y).abs(),
        };
        let path = squiggly_path(p0, p1);
        s.push_str(&format!(
            "<ofd:Appearance Boundary=\"{} {} {} {}\">",
            r.x, r.y, r.w, r.h
        ));
        s.push_str(&path_object_xml(&r, *color, None, 0.5, &path));
        s.push_str("</ofd:Appearance>");
    }
    s
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
    for c in &p.commands {
        match c {
            rofd_dom::PathCommand::M(x, y) => {
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
                s.push_str("Z ");
            }
            rofd_dom::PathCommand::A(a, b, c, d, e, f) => {
                s.push_str(&format!("A {} {} {} {} {} {} ", a, b, c, d, e, f));
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
        let parsed = crate::abbreviated::parse_abbreviated(&abbrev);
        assert_eq!(pd, parsed);
    }

    #[test]
    fn path_to_abbrev_round_trips_arc_commands() {
        // T3: A (arc) operators must round-trip (ellipse_path emits them).
        // PathCommand::A has 6 params (rx, ry, rot, sweep, x, y).
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
        assert_eq!(pd, parsed);
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
        let xml = serialize_page_annot(&PageId::new("1"), &[]);
        assert!(xml.contains("<ofd:PageAnnot"));
        assert!(xml.contains("</ofd:PageAnnot>"));
        // No <Annot> elements.
        assert!(!xml.contains("<ofd:Annot"));
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
