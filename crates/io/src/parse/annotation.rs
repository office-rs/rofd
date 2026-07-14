//! GB/T 33190 §15.2 `<PageAnnot><Annot>` parsing. Annot attributes
//! ID/Type/Creator/LastModDate/Subtype; child elements Remark/Parameters/
//! Appearance(CT_PageBlock). Appearance objects (PathObject/TextObject/
//! ImageObject) are mapped to a typed [`AnnotationPayload`] by Type+Subtype.
//!
//! Follows the `parse/page.rs` inline-state pattern: a single read loop with
//! local `mut` flag variables and a `handle_element_start` helper that takes
//! `&mut` refs to the state. No owned-event split (quick-xml borrows `buf`).

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, NoteIcon,
    PageId, PathData, Point, Rect, ShapeKind,
};

use crate::abbreviated::parse_abbreviated;
use crate::dateutil::parse_last_mod_date;
use crate::error::OfdError;
use crate::parse::{attr, parse_color_value, parse_rect_ws};

/// One appearance object collected inside `<Appearance>` (GB/T 33190 §15.2.2).
/// All fields model the OFD structure; some (Text/Image boundary) are not yet
/// consumed by payload building but are retained for T7's round-trip serializer.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AppearanceObject {
    Path {
        boundary: Rect,
        line_width: f64,
        stroke: Option<Color>,
        fill: Option<Color>,
        data: PathData,
    },
    Text {
        boundary: Rect,
        font: String,
        size: f64,
        fill: Option<Color>,
        content: String,
    },
    Image {
        boundary: Rect,
        resource_id: String,
    },
}

/// Collected state for the single `<Annot>` currently being parsed. Its fields
/// double as the local `mut` state the read loop operates on.
struct PendingAnnot {
    id: String,
    type_str: String,
    subtype: Option<String>,
    creator: String,
    last_mod: String,
    remark: String,
    params: Vec<(String, String)>,
    appearance_boundary: Rect,
    objects: Vec<AppearanceObject>,
    page: PageId,
    // Inner parse state
    in_remark: bool,
    in_param: Option<String>,
    in_appearance: bool,
    cur_obj: Option<AppearanceObject>,
    in_abbrev: bool,
    in_text_code: bool,
    text_body: String,
}

impl PendingAnnot {
    fn from_attrs(e: &BytesStart, page: &PageId) -> Self {
        Self {
            id: attr(e, "ID").unwrap_or_default(),
            type_str: attr(e, "Type").unwrap_or_default(),
            subtype: attr(e, "Subtype"),
            creator: attr(e, "Creator").unwrap_or_default(),
            last_mod: attr(e, "LastModDate").unwrap_or_default(),
            remark: String::new(),
            params: vec![],
            appearance_boundary: Rect::default(),
            objects: vec![],
            page: page.clone(),
            in_remark: false,
            in_param: None,
            in_appearance: false,
            cur_obj: None,
            in_abbrev: false,
            in_text_code: false,
            text_body: String::new(),
        }
    }

    fn finish(self) -> Annotation {
        let kind = map_type_subtype(&self.type_str, self.subtype.as_deref());
        let payload = build_payload(kind.clone(), &self);
        let created = self
            .params
            .iter()
            .find(|(k, _)| k == "CreationDate")
            .and_then(|(_, v)| parse_last_mod_date(v))
            .unwrap_or(0);
        let reply_to = self
            .params
            .iter()
            .find(|(k, _)| k == "InReplyTo")
            .map(|(_, v)| AnnotationId::new(v.clone()));
        let modified = parse_last_mod_date(&self.last_mod).unwrap_or(0);
        Annotation {
            id: AnnotationId::new(self.id),
            kind,
            page: self.page,
            creator: self.creator,
            created,
            modified,
            reply_to,
            payload,
        }
    }
}

/// Parse a per-page `Annotation.xml` (GB/T 33190 §15.2 `<PageAnnot>`) into
/// annotations tagged with the given page id. Unrecognized Type/Subtype combos
/// degrade to a sensible default payload (never fatal; spec §7.4).
pub fn parse_page_annot(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut ann: Option<PendingAnnot> = None;
    loop {
        let ev = reader.read_event_into(&mut buf);
        match &ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().local_name().as_ref() == b"Annot" =>
            {
                // Clone the start element so we can own it independently of `buf`.
                let e2 = e.clone();
                let p = PendingAnnot::from_attrs(&e2, page);
                // Self-closing <Annot/> has no End event: finish immediately.
                if matches!(ev, Ok(Event::Empty(_))) {
                    out.push(p.finish());
                } else {
                    ann = Some(p);
                }
                continue;
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Annot" => {
                if let Some(p) = ann.take() {
                    out.push(p.finish());
                }
                continue;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Annotation.xml".into(),
                    loc: String::new(),
                    source: e.clone(),
                })
            }
            _ => {}
        }
        if let Some(p) = ann.as_mut() {
            handle_event(p, &ev);
        }
    }
    Ok(out)
}

/// Dispatch one quick-xml event into the pending annot's state (inline helper,
/// mirroring `parse/page.rs`'s `handle_element_start`).
fn handle_event(p: &mut PendingAnnot, ev: &Result<Event, quick_xml::Error>) {
    match ev {
        Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
            handle_element_start(p, e);
            // Self-closing appearance objects (Empty event) have no End event:
            // push them immediately so they aren't lost when the next element
            // overwrites cur_obj. (ImageObject is commonly self-closing.)
            if matches!(ev, Ok(Event::Empty(_))) {
                let local = e.name().local_name();
                let is_appearance_obj = matches!(
                    local.as_ref(),
                    b"PathObject" | b"TextObject" | b"ImageObject"
                ) && p.in_appearance;
                if is_appearance_obj {
                    if let Some(o) = p.cur_obj.take() {
                        p.objects.push(o);
                    }
                }
            }
        }
        Ok(Event::Text(t)) => {
            let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
            if p.in_remark {
                p.remark.push_str(&s);
            } else if p.in_abbrev {
                if let Some(AppearanceObject::Path { data, .. }) = p.cur_obj.as_mut() {
                    *data = parse_abbreviated(&s);
                }
            } else if p.in_text_code {
                p.text_body.push_str(&s);
            } else if let Some(name) = p.in_param.take() {
                p.params.push((name, s.trim().to_string()));
            }
        }
        Ok(Event::End(e)) => match e.name().local_name().as_ref() {
            b"Remark" => p.in_remark = false,
            b"Parameter" => p.in_param = None,
            b"Appearance" => p.in_appearance = false,
            b"PathObject" | b"TextObject" | b"ImageObject" if p.in_appearance => {
                if let Some(o) = p.cur_obj.take() {
                    p.objects.push(o);
                }
            }
            b"AbbreviatedData" => p.in_abbrev = false,
            b"TextCode" => {
                // Transfer accumulated text body into the Text object's content
                // when TextCode closes (not when TextObject closes, because
                // in_text_code is cleared here).
                if let Some(AppearanceObject::Text { content, .. }) = p.cur_obj.as_mut() {
                    *content = std::mem::take(&mut p.text_body);
                }
                p.in_text_code = false;
            }
            _ => {}
        },
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_element_start(p: &mut PendingAnnot, e: &BytesStart) {
    match e.name().local_name().as_ref() {
        b"Remark" => p.in_remark = true,
        b"Parameter" => p.in_param = attr(e, "Name"),
        b"Appearance" => {
            p.in_appearance = true;
            p.appearance_boundary = parse_rect_attr(e, "Boundary");
        }
        b"PathObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Path {
                boundary: parse_rect_attr(e, "Boundary"),
                line_width: attr(e, "LineWidth")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0),
                stroke: None,
                fill: None,
                data: PathData::default(),
            });
        }
        b"TextObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Text {
                boundary: parse_rect_attr(e, "Boundary"),
                font: attr(e, "Font").unwrap_or_default(),
                size: attr(e, "Size").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                fill: None,
                content: String::new(),
            });
        }
        b"ImageObject" if p.in_appearance => {
            p.cur_obj = Some(AppearanceObject::Image {
                boundary: parse_rect_attr(e, "Boundary"),
                resource_id: attr(e, "ResourceID").unwrap_or_default(),
            });
        }
        b"StrokeColor" => {
            if let Some(o) = p.cur_obj.as_mut() {
                set_stroke(o, attr(e, "Value").as_deref());
            }
        }
        b"FillColor" => {
            if let Some(o) = p.cur_obj.as_mut() {
                set_fill(o, attr(e, "Value").as_deref());
            }
        }
        b"AbbreviatedData" => p.in_abbrev = true,
        b"TextCode" if p.in_appearance => p.in_text_code = true,
        _ => {}
    }
}

/// Map GB/T 33190 Annot Type+Subtype to [`AnnotationKind`]. Unknown combos
/// degrade to a sensible default without fatal (spec §7.4).
fn map_type_subtype(ty: &str, sub: Option<&str>) -> AnnotationKind {
    match (ty, sub) {
        (_, Some("Underline")) => AnnotationKind::Underline,
        (_, Some("Strikeout")) => AnnotationKind::Strikeout,
        (_, Some("Freehand")) => AnnotationKind::Freehand,
        (_, Some("Rectangle")) => AnnotationKind::Shape(ShapeKind::Rect),
        (_, Some("Ellipse")) => AnnotationKind::Shape(ShapeKind::Ellipse),
        (_, Some("Arrow")) => AnnotationKind::Shape(ShapeKind::Arrow),
        (_, Some("Line")) => AnnotationKind::Shape(ShapeKind::Line),
        (_, Some("Note")) => AnnotationKind::Note,
        (_, Some("TextBox")) => AnnotationKind::TextBox,
        ("Stamp", _) => AnnotationKind::Stamp,
        ("Watermark", _) => AnnotationKind::Watermark,
        // Squiggly maps to Highlight (no dedicated kind in v1 model).
        (_, Some("Squiggly")) | ("Highlight", _) | (_, None) => AnnotationKind::Highlight,
        // Unknown Subtype under Path -> Freehand (a path-like default);
        // any other unknown -> Highlight (markup default).
        ("Path", _) => AnnotationKind::Freehand,
        _ => AnnotationKind::Highlight,
    }
}

/// Build the typed payload from collected appearance objects, remark, and
/// boundary. Each kind extracts the relevant fields from `p.objects`.
fn build_payload(kind: AnnotationKind, p: &PendingAnnot) -> AnnotationPayload {
    let boundary = p.appearance_boundary;
    match kind {
        AnnotationKind::Highlight
        | AnnotationKind::Underline
        | AnnotationKind::Strikeout
        | AnnotationKind::Squiggly => {
            let color = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path { stroke, .. } => *stroke,
                    _ => None,
                })
                .unwrap_or(Color::Rgb(255, 255, 0));
            // Quad points from each path object's boundary corners.
            let quad: Vec<Point> = p
                .objects
                .iter()
                .filter_map(|o| match o {
                    AppearanceObject::Path { boundary: r, .. } => Some([
                        Point { x: r.x, y: r.y },
                        Point {
                            x: r.x + r.w,
                            y: r.y + r.h,
                        },
                    ]),
                    _ => None,
                })
                .flatten()
                .collect();
            let quad_points = if quad.is_empty() {
                vec![
                    Point {
                        x: boundary.x,
                        y: boundary.y,
                    },
                    Point {
                        x: boundary.x + boundary.w,
                        y: boundary.y + boundary.h,
                    },
                ]
            } else {
                quad
            };
            AnnotationPayload::Markup { quad_points, color }
        }
        AnnotationKind::Freehand => {
            let (color, width, data) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path {
                        stroke,
                        line_width,
                        data,
                        ..
                    } => Some((*stroke, *line_width, data.clone())),
                    _ => None,
                })
                .unwrap_or((None, 1.0, PathData::default()));
            AnnotationPayload::Freehand {
                path: data,
                color: color.unwrap_or(Color::Rgb(0, 0, 0)),
                width,
            }
        }
        AnnotationKind::Shape(sk) => {
            let (stroke, fill, width) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path {
                        stroke,
                        fill,
                        line_width,
                        ..
                    } => Some((*stroke, *fill, *line_width)),
                    _ => None,
                })
                .unwrap_or((None, None, 1.0));
            AnnotationPayload::Shape {
                kind: sk,
                rect: boundary,
                stroke: stroke.unwrap_or(Color::Rgb(0, 0, 0)),
                fill,
                width,
                points: vec![],
            }
        }
        AnnotationKind::Note => {
            // Color from the first PathObject's stroke (fallback: default yellow).
            let color = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Path { stroke, .. } => *stroke,
                    _ => None,
                })
                .unwrap_or(Color::Rgb(255, 200, 0));
            AnnotationPayload::Note {
                rect: boundary,
                color,
                content: p.remark.clone(),
                icon: NoteIcon::Note,
            }
        }
        AnnotationKind::TextBox => {
            let (content, font, size, color) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Text {
                        content,
                        font,
                        size,
                        fill,
                        ..
                    } => Some((content.clone(), font.clone(), *size, *fill)),
                    _ => None,
                })
                .unwrap_or_default();
            AnnotationPayload::TextBox {
                rect: boundary,
                content,
                font: FontId::new(font),
                size,
                color: color.unwrap_or_default(),
            }
        }
        AnnotationKind::Stamp => {
            let image = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Image { resource_id, .. } => {
                        Some(ImageId::new(resource_id.clone()))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            AnnotationPayload::Stamp {
                rect: boundary,
                image,
            }
        }
        AnnotationKind::Watermark => {
            let (content, font, size, color) = p
                .objects
                .iter()
                .find_map(|o| match o {
                    AppearanceObject::Text {
                        content,
                        font,
                        size,
                        fill,
                        ..
                    } => Some((content.clone(), font.clone(), *size, *fill)),
                    _ => None,
                })
                .unwrap_or_default();
            // Opacity and angle are stored as Parameters (lossless f64) rather
            // than derived from Alpha (u8, lossy) or CTM (atan2, precision risk).
            // Alpha + CTM are still emitted on the TextObject for rendering.
            let opacity = p
                .params
                .iter()
                .find(|(k, _)| k == "Opacity")
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            let angle = p
                .params
                .iter()
                .find(|(k, _)| k == "Angle")
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            AnnotationPayload::Watermark {
                rect: boundary,
                content,
                opacity,
                angle,
                font: FontId::new(font),
                size,
                color: color.unwrap_or(Color::Rgb(200, 200, 200)),
            }
        }
    }
}

fn set_stroke(o: &mut AppearanceObject, v: Option<&str>) {
    if let Some(c) = v.and_then(parse_color_value) {
        if let AppearanceObject::Path { stroke, .. } = o {
            *stroke = Some(c);
        }
    }
}

fn set_fill(o: &mut AppearanceObject, v: Option<&str>) {
    if let Some(c) = v.and_then(parse_color_value) {
        match o {
            AppearanceObject::Path { fill, .. } => *fill = Some(c),
            AppearanceObject::Text { fill, .. } => *fill = Some(c),
            _ => {}
        }
    }
}

fn parse_rect_attr(e: &BytesStart, name: &str) -> Rect {
    // OFD Boundary="x y w h"; absent -> default (zeros).
    match attr(e, name) {
        Some(s) => parse_rect_ws(&s),
        None => Rect::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_page_annot_yields_empty() {
        let xml = r#"<?xml version="1.0"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016"/>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert!(anns.is_empty());
    }

    #[test]
    fn self_closing_annot_finishes_immediately() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Note" ID="9" Subtype="Note"/>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("2")).unwrap();
        assert_eq!(anns.len(), 1);
        assert!(matches!(anns[0].kind, AnnotationKind::Note));
        assert_eq!(anns[0].id.0, "9");
    }

    #[test]
    fn unknown_subtype_degrades_to_highlight() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="1" Subtype="Mystery">
    <ofd:Appearance Boundary="0 0 10 10"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert_eq!(anns.len(), 1);
        assert!(matches!(anns[0].kind, AnnotationKind::Highlight));
    }

    #[test]
    fn parameter_creation_and_inreplyto_parsed() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" ID="2" Subtype="Highlight" LastModDate="2026-07-13">
    <ofd:Parameters>
      <ofd:Parameter Name="CreationDate">2026-07-10</ofd:Parameter>
      <ofd:Parameter Name="InReplyTo">100</ofd:Parameter>
    </ofd:Parameters>
    <ofd:Appearance Boundary="0 0 5 5"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].created, 1_783_641_600_000); // 2026-07-10 midnight UTC
        assert_eq!(anns[0].reply_to.as_ref().map(|i| i.0.as_str()), Some("100"));
        assert_eq!(anns[0].modified, 1_783_900_800_000); // 2026-07-13 midnight UTC
    }

    #[test]
    fn remark_captured_into_note_content() {
        let xml = r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Note" ID="3" Subtype="Note">
    <ofd:Remark>a sticky note</ofd:Remark>
    <ofd:Appearance Boundary="1 2 30 15"/>
  </ofd:Annot>
</ofd:PageAnnot>"#;
        let anns = parse_page_annot(xml, &PageId::new("1")).unwrap();
        match &anns[0].payload {
            AnnotationPayload::Note { content, rect, .. } => {
                assert_eq!(content, "a sticky note");
                assert_eq!(rect.x, 1.0);
            }
            other => panic!("expected Note, got {other:?}"),
        }
    }
}
