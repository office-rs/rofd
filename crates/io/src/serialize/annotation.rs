use rofd_dom::{
    Annotation, AnnotationKind, AnnotationPayload, AnnotationPayload as P, Color, PageId, ShapeKind,
};

/// Serialize one page's annotations to GB/T 33190 §15.2 `<PageAnnot>` XML.
///
/// This is the inverse of `parse::annotation::parse_page_annot`. T7 will
/// harden this with full payload fidelity; this minimal version round-trips
/// the fields the parser consumes (Type/Subtype/ID/Creator/LastModDate +
/// Appearance/PathObject/TextObject/ImageObject).
pub fn serialize_page_annotations(_page: &PageId, anns: &[Annotation]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    for a in anns {
        s.push_str(&serialize_one(a));
    }
    s.push_str("</ofd:PageAnnot>");
    s
}

fn serialize_one(a: &Annotation) -> String {
    let (ty, sub) = type_subtype(&a.kind);
    let mut s = format!(
        "  <ofd:Annot Type=\"{ty}\" ID=\"{}\" Creator=\"{}\" LastModDate=\"{}\"",
        a.id.0,
        xml_escape(&a.creator),
        crate::dateutil::format_last_mod_date(a.modified)
    );
    if let Some(sub) = sub {
        s.push_str(&format!(" Subtype=\"{sub}\""));
    }
    s.push_str(">\n");
    // Parameters: CreationDate + InReplyTo (if present).
    if a.created != 0 || a.reply_to.is_some() {
        s.push_str("    <ofd:Parameters>\n");
        if a.created != 0 {
            s.push_str(&format!(
                "      <ofd:Parameter Name=\"CreationDate\">{}</ofd:Parameter>\n",
                crate::dateutil::format_last_mod_date(a.created)
            ));
        }
        if let Some(rt) = &a.reply_to {
            s.push_str(&format!(
                "      <ofd:Parameter Name=\"InReplyTo\">{}</ofd:Parameter>\n",
                rt.0
            ));
        }
        s.push_str("    </ofd:Parameters>\n");
    }
    // Remark (Note content).
    if let P::Note { content, .. } = &a.payload {
        if !content.is_empty() {
            s.push_str(&format!(
                "    <ofd:Remark>{}</ofd:Remark>\n",
                xml_escape(content)
            ));
        }
    }
    // Appearance.
    s.push_str(&serialize_appearance(&a.payload));
    s.push_str("  </ofd:Annot>\n");
    s
}

fn serialize_appearance(payload: &AnnotationPayload) -> String {
    let (boundary, body) = appearance_parts(payload);
    let mut s = String::new();
    s.push_str(&format!(
        "    <ofd:Appearance Boundary=\"{} {} {} {}\">\n",
        boundary.x, boundary.y, boundary.w, boundary.h
    ));
    s.push_str(&body);
    s.push_str("    </ofd:Appearance>\n");
    s
}

/// Return (appearance_boundary, appearance_body_xml) for the payload.
fn appearance_parts(payload: &AnnotationPayload) -> (rofd_dom::Rect, String) {
    match payload {
        P::Markup { quad_points, color } => {
            // Emit one PathObject per quad pair (2 points = 1 quad).
            let mut body = String::new();
            for chunk in quad_points.chunks(2) {
                if chunk.len() == 2 {
                    let p0 = chunk[0];
                    let p1 = chunk[1];
                    let r = rofd_dom::Rect {
                        x: p0.x,
                        y: p0.y,
                        w: p1.x - p0.x,
                        h: p1.y - p0.y,
                    };
                    body.push_str(&format!(
                        "      <ofd:PathObject Boundary=\"{} {} {} {}\" LineWidth=\"1\">\n",
                        r.x, r.y, r.w, r.h
                    ));
                    body.push_str(&format!(
                        "        <ofd:StrokeColor Value=\"{}\"/>\n",
                        color_str(color)
                    ));
                    body.push_str("      </ofd:PathObject>\n");
                }
            }
            // Boundary = first quad's rect (approximation for round-trip).
            let b = quad_points
                .first()
                .zip(quad_points.get(1))
                .map(|(p0, p1)| rofd_dom::Rect {
                    x: p0.x,
                    y: p0.y,
                    w: p1.x - p0.x,
                    h: p1.y - p0.y,
                })
                .unwrap_or_default();
            (b, body)
        }
        P::Freehand { path, color, width } => {
            let body = format!(
                "      <ofd:PathObject Boundary=\"0 0 0 0\" LineWidth=\"{width}\">\n        <ofd:StrokeColor Value=\"{}\"/>\n      </ofd:PathObject>\n",
                color_str(color)
            );
            let _ = path; // path data serialization is T7's full job
            (rofd_dom::Rect::default(), body)
        }
        P::Shape {
            kind: _,
            rect,
            stroke,
            fill,
            width,
        } => {
            let mut body = format!(
                "      <ofd:PathObject Boundary=\"{} {} {} {}\" LineWidth=\"{width}\">\n",
                rect.x, rect.y, rect.w, rect.h
            );
            body.push_str(&format!(
                "        <ofd:StrokeColor Value=\"{}\"/>\n",
                color_str(stroke)
            ));
            if let Some(f) = fill {
                body.push_str(&format!(
                    "        <ofd:FillColor Value=\"{}\"/>\n",
                    color_str(f)
                ));
            }
            body.push_str("      </ofd:PathObject>\n");
            (*rect, body)
        }
        P::Note { rect, color, .. } => {
            let body = format!(
                "      <ofd:PathObject Boundary=\"{} {} {} {}\" LineWidth=\"1\">\n        <ofd:StrokeColor Value=\"{}\"/>\n      </ofd:PathObject>\n",
                rect.x, rect.y, rect.w, rect.h, color_str(color)
            );
            (*rect, body)
        }
        P::TextBox {
            rect,
            content,
            font,
            size,
            color,
        } => {
            let body = format!(
                "      <ofd:TextObject Boundary=\"{} {} {} {}\" Font=\"{}\" Size=\"{size}\">\n        <ofd:FillColor Value=\"{}\"/>\n        <ofd:TextCode X=\"0\" Y=\"{size}\">{}</ofd:TextCode>\n      </ofd:TextObject>\n",
                rect.x, rect.y, rect.w, rect.h, font.0, color_str(color), xml_escape(content)
            );
            (*rect, body)
        }
        P::Stamp { rect, image } => {
            let body = format!(
                "      <ofd:ImageObject Boundary=\"{} {} {} {}\" ResourceID=\"{}\"/>\n",
                rect.x, rect.y, rect.w, rect.h, image.0
            );
            (*rect, body)
        }
        P::Watermark {
            rect,
            content,
            font,
            size,
            color,
            ..
        } => {
            let body = format!(
                "      <ofd:TextObject Boundary=\"{} {} {} {}\" Font=\"{}\" Size=\"{size}\">\n        <ofd:FillColor Value=\"{}\"/>\n        <ofd:TextCode X=\"0\" Y=\"{size}\">{}</ofd:TextCode>\n      </ofd:TextObject>\n",
                rect.x, rect.y, rect.w, rect.h, font.0, color_str(color), xml_escape(content)
            );
            (*rect, body)
        }
    }
}

/// Map AnnotationKind back to (Type, Subtype) attribute strings.
fn type_subtype(k: &AnnotationKind) -> (&'static str, Option<&'static str>) {
    match k {
        AnnotationKind::Highlight => ("Highlight", Some("Highlight")),
        AnnotationKind::Underline => ("Highlight", Some("Underline")),
        AnnotationKind::Strikeout => ("Highlight", Some("Strikeout")),
        AnnotationKind::Freehand => ("Path", Some("Freehand")),
        AnnotationKind::Shape(sk) => {
            let sub = match sk {
                ShapeKind::Rect => "Rectangle",
                ShapeKind::Ellipse => "Ellipse",
                ShapeKind::Arrow => "Arrow",
                ShapeKind::Line => "Line",
            };
            ("Path", Some(sub))
        }
        AnnotationKind::Note => ("Note", Some("Note")),
        AnnotationKind::TextBox => ("Text", Some("TextBox")),
        AnnotationKind::Stamp => ("Stamp", None),
        AnnotationKind::Watermark => ("Watermark", None),
    }
}

fn color_str(c: &Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("{r} {g} {b}"),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
