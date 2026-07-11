use rofd_dom::{Annotation, AnnotationKind, AnnotationPayload, Color, PageId};

/// Serialize one page's annotations to <ofd:Annotations> XML.
pub fn serialize_page_annotations(page: &PageId, anns: &[Annotation]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    for a in anns {
        s.push_str(&serialize_one(a, page));
    }
    s.push_str("</ofd:Annotations>");
    s
}

fn serialize_one(a: &Annotation, _page: &PageId) -> String {
    let ty = match &a.kind {
        AnnotationKind::Highlight => "Highlight",
        AnnotationKind::Underline => "Underline",
        AnnotationKind::Strikeout => "Strikeout",
        AnnotationKind::Freehand => "Freehand",
        AnnotationKind::Shape(_) => "Shape",
        AnnotationKind::Note => "Note",
        AnnotationKind::TextBox => "Text",
        AnnotationKind::Stamp => "Stamp",
        AnnotationKind::Watermark => "Watermark",
    };
    let color = match &a.payload {
        AnnotationPayload::Markup { color, .. } => Some(*color),
        _ => None,
    };
    let mut s = format!("  <ofd:Annotation ID=\"{}\" Type=\"{}\">\n", a.id.0, ty);
    if let Some(c) = color {
        s.push_str(&format!("    <ofd:Color Value=\"{}\"/>\n", color_str(&c)));
    }
    s.push_str(&format!("    <ofd:Creator>{}</ofd:Creator>\n", xml_escape(&a.creator)));
    s.push_str("  </ofd:Annotation>\n");
    s
}

fn color_str(c: &Color) -> String {
    match c { Color::Rgb(r, g, b) => format!("{r} {g} {b}") }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
