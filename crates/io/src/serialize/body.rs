//! Body-object serialization for the full write path (spec 2026-11-11 §6.1).
//!
//! The element/attribute layout mirrors `crate::parse::page` one-for-one:
//! the parser is the source of truth for the wire format, so round-trip
//! fidelity is "parse can read back exactly what we emit".

use rofd_dom::{Ctm, Layer, LayerType, PageObject, Rect, TextObject};

use super::annotation::{color_str, xml_escape, xml_escape_preserving_edges};

/// Serialize one layer (4-space indent, as nested inside `<ofd:Content>`).
/// An object-free layer emits the self-closing form - the parser accepts
/// both and the compact form keeps files small.
pub(crate) fn layer_xml(layer: &Layer) -> String {
    let ty = match layer.layer_type {
        LayerType::Body => "Body",
        LayerType::Foreground => "Foreground",
        LayerType::Background => "Background",
    };
    let mut children = String::new();
    for obj in &layer.objects {
        if let PageObject::Text(t) = obj {
            children.push_str(&text_object_xml(t));
        }
        // Path/Image/Composite arms land with the following tasks
        // (spec 2026-11-11 ①期 Task 2/3).
    }
    if children.is_empty() {
        format!("    <ofd:Layer Type=\"{ty}\"/>\n")
    } else {
        format!("    <ofd:Layer Type=\"{ty}\">\n{children}    </ofd:Layer>\n")
    }
}

/// One `<ofd:TextObject>`: attributes ID/Boundary/CTM?/Font/Size/DrawParam?,
/// child `<ofd:FillColor>` when set, then per TextCode an optional
/// `<ofd:CGTransform><ofd:Glyphs>` (subset-font glyph mapping, §8.3.3)
/// followed by the `<ofd:TextCode>` itself.
pub(crate) fn text_object_xml(t: &TextObject) -> String {
    let mut s = format!(
        "      <ofd:TextObject ID=\"{}\" Boundary=\"{}\"{} Font=\"{}\" Size=\"{}\"",
        xml_escape(&t.id.0),
        rect_str(&t.boundary),
        ctm_attr(&t.ctm),
        xml_escape(&t.font.0),
        t.size
    );
    if let Some(dp) = &t.draw_param {
        s.push_str(&format!(" DrawParam=\"{}\"", xml_escape(&dp.0)));
    }
    s.push_str(">\n");
    if let Some(c) = t.fill {
        s.push_str(&format!(
            "        <ofd:FillColor Value=\"{}\"/>\n",
            color_str(c)
        ));
    }
    // CGTransform CodePosition is the char offset of this code's text inside
    // the object (the parser ignores the attribute; strict readers use it).
    let mut char_pos = 0usize;
    for code in &t.codes {
        if !code.glyph_ids.is_empty() {
            let ids: Vec<String> = code.glyph_ids.iter().map(|g| g.to_string()).collect();
            s.push_str(&format!(
                "        <ofd:CGTransform CodePosition=\"{char_pos}\" CodeCount=\"{}\" GlyphCount=\"{}\">\n          <ofd:Glyphs>{}</ofd:Glyphs>\n        </ofd:CGTransform>\n",
                code.text.chars().count(),
                code.glyph_ids.len(),
                ids.join(" ")
            ));
        }
        char_pos += code.text.chars().count();
        // DeltaX: the parser regenerates all-zero deltas when the attribute
        // is absent, so emit only when some advance is non-zero; emit every
        // value (zeros included) to keep per-glyph positions exact.
        let mut deltas_attr = String::new();
        if code.deltas.iter().any(|(dx, _)| *dx != 0.0) {
            let xs: Vec<String> = code.deltas.iter().map(|(dx, _)| dx.to_string()).collect();
            deltas_attr.push_str(&format!(" DeltaX=\"{}\"", xs.join(" ")));
        }
        // DeltaY is not parsed back today (v1 boundary): best-effort emit.
        if code.deltas.iter().any(|(_, dy)| *dy != 0.0) {
            let ys: Vec<String> = code.deltas.iter().map(|(_, dy)| dy.to_string()).collect();
            deltas_attr.push_str(&format!(" DeltaY=\"{}\"", ys.join(" ")));
        }
        s.push_str(&format!(
            "        <ofd:TextCode X=\"{}\" Y=\"{}\"{}>{}</ofd:TextCode>\n",
            code.x,
            code.y,
            deltas_attr,
            xml_escape_preserving_edges(&code.text)
        ));
    }
    s.push_str("      </ofd:TextObject>\n");
    s
}

fn rect_str(r: &Rect) -> String {
    format!("{} {} {} {}", r.x, r.y, r.w, r.h)
}

fn ctm_attr(c: &Option<Ctm>) -> String {
    match c {
        Some(c) => format!(" CTM=\"{} {} {} {} {} {}\"", c.a, c.b, c.c, c.d, c.e, c.f),
        None => String::new(),
    }
}
