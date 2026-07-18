use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Ctm, DrawParamId, FontId, ImageId, ImageObject, Layer, LayerType, ObjectId, Page, PageId,
    PageObject, PathData, PathObject, Rect, TextCode, TextObject,
};

use crate::abbreviated::parse_abbreviated;
use crate::error::{OfdError, OfdWarning};
use crate::parse::attr;
use crate::parse::document::DocHeader;
use crate::parse::{parse_color_value, parse_rect_ws};

pub fn parse_page(
    page_id: PageId,
    page_xml: &str,
    header: &DocHeader,
    warnings: &mut Vec<OfdWarning>,
) -> Result<Page, OfdError> {
    let mut reader = Reader::from_str(page_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut page = Page {
        id: page_id.clone(),
        physical_box: header.page_area.unwrap_or_default(),
        layers: vec![],
        template: None,
    };
    let mut current_layer: Option<Layer> = None;
    let mut current_text: Option<TextObject> = None;
    let mut pending_text_delta: Option<String> = None;
    let mut pending_text_body: Option<String> = None;
    let mut in_text_code = false;
    // TextCode X/Y origin (page-local) captured on its Start, applied at End.
    let mut text_origin = (0.0_f64, 0.0_f64);
    // Page-level PhysicalBox (inside <Area>) overrides the doc default; its
    // geometry is element text content ("x y w h"), not attributes.
    let mut in_physical_box = false;
    // <Glyphs> inside <CGTransform>: text is the glyph-ID list for the next
    // TextCode. OFD subset fonts have no cmap, so these IDs are the only way to
    // address glyphs (shape would return .notdef and the text would vanish).
    let mut in_glyphs = false;
    let mut pending_glyph_ids: Option<Vec<u32>> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = e.name().local_name();
                if local.as_ref() == b"PhysicalBox" {
                    in_physical_box = true;
                } else if local.as_ref() == b"Template" {
                    // GB/T 33190 §7.5: <ofd:Template> references a template
                    // page. v1 does not expand templates; capture a marker so
                    // parse_ofd can emit a MissingFeature warning.
                    page.template = Some(String::new());
                } else {
                    handle_element_start(
                        &e,
                        &mut current_layer,
                        &mut current_text,
                        &mut pending_text_delta,
                        &mut pending_text_body,
                        &mut in_text_code,
                        &mut text_origin,
                        &mut in_glyphs,
                        &mut pending_glyph_ids,
                        &page_id,
                        warnings,
                    );
                }
            }
            Ok(Event::Empty(e)) => {
                let local = e.name().local_name();
                if local.as_ref() == b"Layer" {
                    // Empty <ofd:Layer/> has no End event; push immediately so the
                    // layer survives (write_ofd emits self-closing Layer tags).
                    let lt = match attr(&e, "Type").as_deref() {
                        Some("Foreground") => LayerType::Foreground,
                        Some("Background") => LayerType::Background,
                        _ => LayerType::Body,
                    };
                    page.layers.push(Layer {
                        layer_type: lt,
                        objects: vec![],
                    });
                } else if local.as_ref() == b"Template" {
                    page.template = Some(String::new());
                } else {
                    handle_element_start(
                        &e,
                        &mut current_layer,
                        &mut current_text,
                        &mut pending_text_delta,
                        &mut pending_text_body,
                        &mut in_text_code,
                        &mut text_origin,
                        &mut in_glyphs,
                        &mut pending_glyph_ids,
                        &page_id,
                        warnings,
                    );
                }
            }
            Ok(Event::Text(t)) => {
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if in_physical_box {
                    page.physical_box = parse_rect_ws(&s);
                    in_physical_box = false;
                } else if in_glyphs {
                    // <Glyphs> text = whitespace-separated glyph IDs for the
                    // next TextCode (OFD subset-font glyph mapping, §8.3.3).
                    pending_glyph_ids = Some(parse_glyph_ids(&s));
                    in_glyphs = false;
                } else if in_text_code {
                    pending_text_body = Some(s);
                } else if !s.is_empty() {
                    if let Some(l) = current_layer.as_mut() {
                        // AbbreviatedData text for the last PathObject
                        if let Some(PageObject::Path(p)) = l.objects.last_mut() {
                            p.data = parse_abbreviated(&s);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"TextCode" => {
                    if let Some(t) = current_text.as_mut() {
                        let body = pending_text_body.take().unwrap_or_default();
                        let glyph_ids = pending_glyph_ids.take().unwrap_or_default();
                        // DeltaX lists per-glyph advances; glyph count is the
                        // glyph_ids len (subset font, CGTransform) or the char
                        // count (shaping a cmap font with no CGTransform).
                        let glyph_count = if !glyph_ids.is_empty() {
                            glyph_ids.len()
                        } else {
                            body.chars().count()
                        };
                        let deltas = parse_delta_x(pending_text_delta.as_deref(), glyph_count);
                        t.codes.push(TextCode {
                            glyph_ids,
                            deltas,
                            text: body,
                            x: text_origin.0,
                            y: text_origin.1,
                        });
                    }
                    pending_text_delta = None;
                    in_text_code = false;
                }
                b"Glyphs" => in_glyphs = false,
                // CGTransform is a container for Glyphs; nothing to do on End.
                b"CGTransform" => {}
                b"TextObject" => {
                    if let (Some(t), Some(l)) = (current_text.take(), current_layer.as_mut()) {
                        l.objects.push(PageObject::Text(t));
                    }
                }
                b"Layer" => {
                    if let Some(l) = current_layer.take() {
                        page.layers.push(l);
                    }
                }
                b"PhysicalBox" => in_physical_box = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Page.xml".into(),
                    loc: String::new(),
                    source: e,
                })
            }
            _ => {}
        }
    }
    Ok(page)
}

#[allow(clippy::too_many_arguments)]
fn handle_element_start(
    e: &BytesStart,
    current_layer: &mut Option<Layer>,
    current_text: &mut Option<TextObject>,
    pending_text_delta: &mut Option<String>,
    pending_text_body: &mut Option<String>,
    in_text_code: &mut bool,
    text_origin: &mut (f64, f64),
    in_glyphs: &mut bool,
    pending_glyph_ids: &mut Option<Vec<u32>>,
    page_id: &PageId,
    warnings: &mut Vec<OfdWarning>,
) {
    match e.name().local_name().as_ref() {
        b"Layer" => {
            let lt = match attr(e, "Type").as_deref() {
                Some("Foreground") => LayerType::Foreground,
                Some("Background") => LayerType::Background,
                _ => LayerType::Body,
            };
            *current_layer = Some(Layer {
                layer_type: lt,
                objects: vec![],
            });
        }
        b"TextObject" => {
            *current_text = Some(TextObject {
                id: ObjectId::new(attr(e, "ID").unwrap_or_default()),
                boundary: parse_rect_attr(e, "Boundary"),
                ctm: attr(e, "CTM").and_then(parse_ctm),
                font: FontId::new(attr(e, "Font").unwrap_or_default()),
                size: attr(e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                fill: None,
                codes: vec![],
                draw_param: attr(e, "DrawParam").map(DrawParamId::new),
            });
        }
        b"FillColor" | b"StrokeColor" => {
            if let Some(c) = attr(e, "Value").and_then(|v| parse_color_value(&v)) {
                let local = e.name().local_name();
                if local.as_ref() == b"FillColor" {
                    if let Some(t) = current_text.as_mut() {
                        t.fill = Some(c);
                    }
                }
                // Also apply to the last PathObject in the current layer:
                // FillColor -> p.fill, StrokeColor -> p.stroke.
                if let Some(PageObject::Path(p)) =
                    current_layer.as_mut().and_then(|l| l.objects.last_mut())
                {
                    if local.as_ref() == b"FillColor" {
                        p.fill = Some(c);
                    } else {
                        p.stroke = Some(c);
                    }
                }
            }
        }
        b"TextCode" => {
            *in_text_code = true;
            *pending_text_delta = attr(e, "DeltaX");
            *pending_text_body = None;
            // X/Y is the absolute pen origin (page-local) for the first glyph.
            *text_origin = (
                attr(e, "X").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                attr(e, "Y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            );
        }
        b"ImageObject" => {
            if let Some(l) = current_layer.as_mut() {
                l.objects.push(PageObject::Image(ImageObject {
                    id: ObjectId::new(attr(e, "ID").unwrap_or_default()),
                    boundary: parse_rect_attr(e, "Boundary"),
                    ctm: attr(e, "CTM").and_then(parse_ctm),
                    image: ImageId::new(attr(e, "ResourceID").unwrap_or_default()),
                }));
            }
        }
        b"PathObject" => {
            if let Some(l) = current_layer.as_mut() {
                l.objects.push(PageObject::Path(PathObject {
                    id: ObjectId::new(attr(e, "ID").unwrap_or_default()),
                    boundary: parse_rect_attr(e, "Boundary"),
                    ctm: attr(e, "CTM").and_then(parse_ctm),
                    fill: None,
                    stroke: None,
                    line_width: attr(e, "LineWidth")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0),
                    data: PathData::default(),
                    draw_param: attr(e, "DrawParam").map(DrawParamId::new),
                }));
            }
        }
        b"AbbreviatedData" => { /* text captured in Text event */ }
        // OFD §8.3.3 <CGTransform><Glyphs>: glyph-ID mapping for subset fonts
        // (which carry no cmap). CGTransform precedes its TextCode; Glyphs
        // text is the glyph-ID list, captured into pending_glyph_ids in the
        // Text event and consumed at TextCode End. Recognized elements (not
        // "unknown element" warnings).
        b"CGTransform" => {
            *pending_glyph_ids = None;
        }
        b"Glyphs" => {
            *in_glyphs = true;
        }
        // Structural/container elements that are expected but have no direct
        // object representation - silently pass through (not unknown objects).
        b"Area" | b"Content" | b"Page" => {}
        _ => {
            // Unknown element -> skip + warning (not fatal, AGENTS.md §4.6).
            let name = String::from_utf8_lossy(e.name().local_name().as_ref()).into_owned();
            warnings.push(OfdWarning::SkippedObject {
                page: page_id.clone(),
                reason: format!("unknown element <{name}>"),
            });
        }
    }
}

fn parse_delta_x(s: Option<&str>, glyph_count: usize) -> Vec<(f32, f32)> {
    let s = match s {
        Some(s) => s,
        None => return vec![(0.0, 0.0); glyph_count.max(1)],
    };
    let nums: Vec<f32> = s
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.is_empty() {
        return vec![(0.0, 0.0); glyph_count.max(1)];
    }
    (0..glyph_count.max(1))
        .map(|i| (nums.get(i).copied().unwrap_or(0.0), 0.0))
        .collect()
}

/// Parse `<Glyphs>` text (whitespace-separated glyph IDs) into a `Vec<u32>`.
/// Empty/whitespace text -> empty vec. Non-numeric tokens are skipped (a
/// malformed glyph id should not abort the whole TextCode).
fn parse_glyph_ids(s: &str) -> Vec<u32> {
    s.split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn parse_rect_attr(e: &BytesStart, name: &str) -> Rect {
    // OFD Boundary="x y w h"
    let s = match attr(e, name) {
        Some(s) => s,
        None => return Rect::default(),
    };
    let n: Vec<f64> = s
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    Rect {
        x: n.first().copied().unwrap_or(0.0),
        y: n.get(1).copied().unwrap_or(0.0),
        w: n.get(2).copied().unwrap_or(0.0),
        h: n.get(3).copied().unwrap_or(0.0),
    }
}

fn parse_ctm(s: String) -> Option<Ctm> {
    let n: Vec<f64> = s
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if n.len() != 6 {
        return None;
    }
    Some(Ctm {
        a: n[0],
        b: n[1],
        c: n[2],
        d: n[3],
        e: n[4],
        f: n[5],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{DocMeta, LayerType, PageObject, Rect};

    #[test]
    fn textcode_without_deltax_captures_body() {
        // TextCode WITHOUT DeltaX - body text must still be captured, not
        // misrouted to the (nonexistent) last PathObject.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016" ID="P0">
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="0 0 100 20" Font="f1" Size="14">
        <ofd:TextCode X="0" Y="14">Hi</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;
        let page = parse_page(
            PageId::new("P0"),
            xml,
            &DocHeader {
                page_area: Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 210.0,
                    h: 297.0,
                }),
                pages: vec![],
                meta: DocMeta::default(),
                max_unit_id: 0,
                annotations_loc: None,
            },
            &mut Vec::new(),
        )
        .expect("page parses");
        let body = page
            .layers
            .iter()
            .find(|l| l.layer_type == LayerType::Body)
            .expect("body layer exists");
        let text = body
            .objects
            .iter()
            .find_map(|o| match o {
                PageObject::Text(t) => Some(t),
                _ => None,
            })
            .expect("text object exists");
        // "Hi" has 2 glyphs; deltas vec length tracks glyph count. Under the
        // old DeltaX-gated routing the body was dropped, leaving codes empty.
        assert_eq!(text.codes.len(), 1, "one TextCode captured");
        assert_eq!(text.codes[0].deltas.len(), 2, "2 glyphs for 'Hi'");
    }

    #[test]
    fn textobject_with_cgtransform_captures_glyph_ids() {
        // OFD subset fonts carry no `cmap` table; <CGTransform><Glyphs> lists
        // the glyph IDs for the TextCode's characters. parse MUST capture them
        // into TextCode.glyph_ids so render can draw by glyph ID (no cmap/shape
        // - shape returns .notdef on a cmap-less font and the text vanishes).
        // Mirrors sample.ofd's "高亮测试" TextObject.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016" ID="P0">
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="31.75 26.31 17.58 3.68" Font="4" Size="209" CTM="0.0176 0 0 0.0176 0 0">
        <ofd:CGTransform CodePosition="0" CodeCount="4" GlyphCount="4">
          <ofd:Glyphs>20744 1246 9083 16901</ofd:Glyphs>
        </ofd:CGTransform>
        <ofd:TextCode X="0" Y="179.5313" DeltaX="208.7971 211.1879 208.7969">高亮测试</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;
        let mut warnings = Vec::new();
        let page = parse_page(
            PageId::new("P0"),
            xml,
            &DocHeader {
                page_area: Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 210.0,
                    h: 297.0,
                }),
                pages: vec![],
                meta: DocMeta::default(),
                max_unit_id: 0,
                annotations_loc: None,
            },
            &mut warnings,
        )
        .expect("page parses");
        let body = page
            .layers
            .iter()
            .find(|l| l.layer_type == LayerType::Body)
            .expect("body layer");
        let text = body
            .objects
            .iter()
            .find_map(|o| match o {
                PageObject::Text(t) => Some(t),
                _ => None,
            })
            .expect("text object");
        assert_eq!(text.codes.len(), 1);
        assert_eq!(
            text.codes[0].glyph_ids,
            vec![20744u32, 1246, 9083, 16901],
            "CGTransform Glyphs -> glyph_ids"
        );
        assert_eq!(text.codes[0].text, "高亮测试");
        // CGTransform/Glyphs are recognized -> no SkippedObject warnings.
        assert!(
            warnings.is_empty(),
            "CGTransform/Glyphs should not warn, got: {:?}",
            warnings
        );
    }
}
