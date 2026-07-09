use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Ctm, Color, FontId, ImageId, ImageObject, Layer, LayerType, ObjectId, Page, PageId,
    PageObject, PathData, PathObject, Rect, TextCode, TextObject,
};

use crate::abbreviated::parse_abbreviated;
use crate::error::OfdError;
use crate::parse::document::DocHeader;

pub fn parse_page(page_id: PageId, page_xml: &str, header: &DocHeader) -> Result<Page, OfdError> {
    let mut reader = Reader::from_str(page_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut page = Page {
        id: page_id,
        physical_box: header.page_area.unwrap_or_default(),
        layers: vec![],
        template: None,
    };
    let mut current_layer: Option<Layer> = None;
    let mut current_text: Option<TextObject> = None;
    let mut pending_text_delta: Option<String> = None;
    let mut pending_text_body: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"PhysicalBox" => page.physical_box = parse_rect(&e),
                b"Layer" => {
                    let lt = match attr(&e, "Type").as_deref() {
                        Some("Foreground") => LayerType::Foreground,
                        Some("Background") => LayerType::Background,
                        _ => LayerType::Body,
                    };
                    current_layer = Some(Layer { layer_type: lt, objects: vec![] });
                }
                b"TextObject" => {
                    current_text = Some(TextObject {
                        id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                        boundary: parse_rect_attr(&e, "Boundary"),
                        ctm: attr(&e, "CTM").and_then(parse_ctm),
                        font: FontId::new(attr(&e, "Font").unwrap_or_default()),
                        size: attr(&e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        fill: None,
                        codes: vec![],
                    });
                }
                b"FillColor" | b"StrokeColor" => {
                    if let Some(c) = attr(&e, "Color").and_then(parse_color) {
                        if e.name().local_name().as_ref() == b"FillColor" {
                            if let Some(t) = current_text.as_mut() { t.fill = Some(c); }
                        }
                    }
                }
                b"TextCode" => {
                    pending_text_delta = attr(&e, "DeltaX");
                    pending_text_body = None;
                }
                b"ImageObject" => {
                    if let Some(l) = current_layer.as_mut() {
                        l.objects.push(PageObject::Image(ImageObject {
                            id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                            boundary: parse_rect_attr(&e, "Boundary"),
                            ctm: attr(&e, "CTM").and_then(parse_ctm),
                            image: ImageId::new(attr(&e, "ResourceID").unwrap_or_default()),
                        }));
                    }
                }
                b"PathObject" => {
                    if let Some(l) = current_layer.as_mut() {
                        l.objects.push(PageObject::Path(PathObject {
                            id: ObjectId::new(attr(&e, "ID").unwrap_or_default()),
                            boundary: parse_rect_attr(&e, "Boundary"),
                            ctm: attr(&e, "CTM").and_then(parse_ctm),
                            fill: None,
                            stroke: None,
                            line_width: attr(&e, "LineWidth").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                            data: PathData::default(),
                        }));
                    }
                }
                b"AbbreviatedData" => { /* text captured in Text event */ }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if current_text.is_some() && pending_text_delta.is_some() {
                    pending_text_body = Some(t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                } else if let Some(l) = current_layer.as_mut() {
                    // AbbreviatedData text for the last PathObject
                    if let Some(PageObject::Path(p)) = l.objects.last_mut() {
                        p.data = parse_abbreviated(&t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"TextCode" => {
                    if let Some(t) = current_text.as_mut() {
                        let body = pending_text_body.take().unwrap_or_default();
                        // v1: glyph_ids left empty (no Glyph attr in common subset); deltas derived from DeltaX string
                        let deltas = parse_delta_x(pending_text_delta.as_deref(), body.chars().count());
                        t.codes.push(TextCode { glyph_ids: vec![], deltas });
                    }
                    pending_text_delta = None;
                }
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
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Page.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(page)
}

fn parse_delta_x(s: Option<&str>, glyph_count: usize) -> Vec<(f32, f32)> {
    let s = match s { Some(s) => s, None => return vec![(0.0, 0.0); glyph_count.max(1)] };
    let nums: Vec<f32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if nums.is_empty() { return vec![(0.0, 0.0); glyph_count.max(1)]; }
    (0..glyph_count.max(1)).map(|i| (nums.get(i).copied().unwrap_or(0.0), 0.0)).collect()
}

fn parse_rect(e: &BytesStart) -> Rect {
    Rect {
        x: attr(e, "x").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        y: attr(e, "y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        w: attr(e, "w").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        h: attr(e, "h").and_then(|s| s.parse().ok()).unwrap_or(0.0),
    }
}

fn parse_rect_attr(e: &BytesStart, name: &str) -> Rect {
    // OFD Boundary="x y w h"
    let s = match attr(e, name) { Some(s) => s, None => return Rect::default() };
    let n: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    Rect { x: n.first().copied().unwrap_or(0.0), y: n.get(1).copied().unwrap_or(0.0), w: n.get(2).copied().unwrap_or(0.0), h: n.get(3).copied().unwrap_or(0.0) }
}

fn parse_ctm(s: String) -> Option<Ctm> {
    let n: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if n.len() != 6 { return None; }
    Some(Ctm { a: n[0], b: n[1], c: n[2], d: n[3], e: n[4], f: n[5] })
}

fn parse_color(s: String) -> Option<Color> {
    let n: Vec<u8> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    match n.len() {
        3 => Some(Color::Rgb(n[0], n[1], n[2])),
        _ => None, // non-RGB (CMYK/gray) -> skipped, render substitutes; v1 common subset
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}
