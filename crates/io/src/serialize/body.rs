//! Body-object serialization for the full write path (spec 2026-11-11 §6.1).
//!
//! The element/attribute layout mirrors `crate::parse::page` one-for-one:
//! the parser is the source of truth for the wire format, so round-trip
//! fidelity is "parse can read back exactly what we emit".

use rofd_dom::{
    CompositeObject, Ctm, ImageObject, Layer, LayerType, PageObject, PathCommand, PathData,
    PathObject, Rect, TextObject,
};

use super::annotation::{color_str, xml_escape, xml_escape_preserving_edges};
use super::interleave;

/// Serialize one layer (4-space indent, as nested inside `<ofd:Content>`).
/// Modelled objects and captured raw fragments interleave at their original
/// positions (spec 2026-11-11 D3); a child-free layer emits the
/// self-closing form - the parser accepts both and it keeps files small.
pub(crate) fn layer_xml(layer: &Layer) -> String {
    let ty = match layer.layer_type {
        LayerType::Body => "Body",
        LayerType::Foreground => "Foreground",
        LayerType::Background => "Background",
    };
    let mut children = String::new();
    for child in interleave(layer.objects.len(), &layer.raw_children) {
        match child {
            super::Child::Known(i) => match &layer.objects[i] {
                PageObject::Text(t) => children.push_str(&text_object_xml(t)),
                PageObject::Path(p) => children.push_str(&path_object_xml(p)),
                PageObject::Image(i) => children.push_str(&image_object_xml(i)),
                PageObject::Composite(c) => children.push_str(&composite_object_xml(c)),
            },
            super::Child::Raw(f) => children.push_str(&format!("    {}\n", f.xml)),
        }
    }
    let mut attrs = String::new();
    for (k, v) in &layer.raw_attrs {
        attrs.push_str(&format!(" {k}=\"{}\"", xml_escape(v)));
    }
    if children.is_empty() {
        format!("    <ofd:Layer Type=\"{ty}\"{attrs}/>\n")
    } else {
        format!("    <ofd:Layer Type=\"{ty}\"{attrs}>\n{children}    </ofd:Layer>\n")
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
    s.push_str(&raw_attrs_str(&t.raw_attrs));
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

/// Unmodelled attributes, re-emitted verbatim after the known set.
fn raw_attrs_str(raws: &[(String, String)]) -> String {
    let mut s = String::new();
    for (k, v) in raws {
        s.push_str(&format!(" {k}=\"{}\"", xml_escape(v)));
    }
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

/// One `<ofd:PathObject>`: attributes ID/Boundary/CTM?/LineWidth plus the
/// explicit Stroke/Fill booleans only when the model carries `Some` (absent
/// attributes keep the spec defaults open), then FillColor/StrokeColor
/// children and the AbbreviatedData payload.
pub(crate) fn path_object_xml(p: &PathObject) -> String {
    let mut s = format!(
        "      <ofd:PathObject ID=\"{}\" Boundary=\"{}\"{} LineWidth=\"{}\"",
        xml_escape(&p.id.0),
        rect_str(&p.boundary),
        ctm_attr(&p.ctm),
        p.line_width
    );
    if let Some(v) = p.stroke_enabled {
        s.push_str(&format!(" Stroke=\"{v}\""));
    }
    if let Some(v) = p.fill_enabled {
        s.push_str(&format!(" Fill=\"{v}\""));
    }
    if let Some(dp) = &p.draw_param {
        s.push_str(&format!(" DrawParam=\"{}\"", xml_escape(&dp.0)));
    }
    s.push_str(&raw_attrs_str(&p.raw_attrs));
    s.push_str(">\n");
    if let Some(c) = p.fill {
        s.push_str(&format!(
            "        <ofd:FillColor Value=\"{}\"/>\n",
            color_str(c)
        ));
    }
    if let Some(c) = p.stroke {
        s.push_str(&format!(
            "        <ofd:StrokeColor Value=\"{}\"/>\n",
            color_str(c)
        ));
    }
    s.push_str(&format!(
        "        <ofd:AbbreviatedData>{}</ofd:AbbreviatedData>\n",
        abbrev_fidelity(&p.data)
    ));
    s.push_str("      </ofd:PathObject>\n");
    s
}

/// PathData -> AbbreviatedData, fidelity variant. Unlike the annotation-side
/// `path_to_abbrev` (which expands closure into an L back to the subpath
/// start because reference authoring tools ignore close operators), body
/// round-trip keeps `Z` as a close operator: the parser reads it back.
/// The A arc writes the GB/T 7-parameter form with large-arc pinned to 0 -
/// the same quarter-arc assumption the parser makes when it drops that flag.
pub(crate) fn abbrev_fidelity(p: &PathData) -> String {
    let mut s = String::new();
    for c in &p.commands {
        match c {
            PathCommand::M(x, y) => s.push_str(&format!("M {x} {y} ")),
            PathCommand::L(x, y) => s.push_str(&format!("L {x} {y} ")),
            PathCommand::C(a, b, c, d, e, g) => s.push_str(&format!("C {a} {b} {c} {d} {e} {g} ")),
            PathCommand::Q(a, b, c, d) => s.push_str(&format!("Q {a} {b} {c} {d} ")),
            PathCommand::A(a, b, c, d, e, f) => {
                s.push_str(&format!("A {a} {b} {c} 0 {d} {e} {f} "))
            }
            PathCommand::Z => s.push_str("Z "),
        }
    }
    s
}

/// One `<ofd:CompositeObject>`: the unit reference is stored raw in the
/// model and round-trips via ResourceID (mirroring the parser arm).
pub(crate) fn composite_object_xml(c: &CompositeObject) -> String {
    format!(
        "      <ofd:CompositeObject ID=\"{}\" Boundary=\"{}\"{} ResourceID=\"{}\"{}/>\n",
        xml_escape(&c.id.0),
        rect_str(&c.boundary),
        ctm_attr(&c.ctm),
        xml_escape(&c.unit),
        raw_attrs_str(&c.raw_attrs)
    )
}

/// One `<ofd:ImageObject>`: ResourceID references a MultiMedia resource.
pub(crate) fn image_object_xml(i: &ImageObject) -> String {
    format!(
        "      <ofd:ImageObject ID=\"{}\" Boundary=\"{}\"{} ResourceID=\"{}\"{}/>\n",
        xml_escape(&i.id.0),
        rect_str(&i.boundary),
        ctm_attr(&i.ctm),
        xml_escape(&i.image.0),
        raw_attrs_str(&i.raw_attrs)
    )
}
