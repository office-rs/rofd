use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Point,
    Rect,
};

use crate::error::OfdError;
use crate::parse::attr;

struct Pending {
    kind: AnnotationKind,
    color: Option<Color>,
    creator: String,
}

/// Parse a per-page Annotation.xml into annotations, tagged with the page id.
pub fn parse_annotation_xml(xml: &str, page: &PageId) -> Result<Vec<Annotation>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut current: Option<Pending> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().local_name().as_ref() == b"Annotation" => {
                let kind = match attr(&e, "Type").as_deref() {
                    Some("Highlight") => AnnotationKind::Highlight,
                    Some("Underline") => AnnotationKind::Underline,
                    Some("Strikeout") => AnnotationKind::Strikeout,
                    Some("Stamp") => AnnotationKind::Stamp,
                    Some("Watermark") => AnnotationKind::Watermark,
                    Some("Text") => AnnotationKind::TextBox,
                    Some("Note") | Some(_) | None => AnnotationKind::Note,
                };
                current = Some(Pending { kind, color: None, creator: String::new() });
            }
            Ok(Event::Empty(e)) if e.name().local_name().as_ref() == b"Color" => {
                if let Some(p) = current.as_mut() {
                    p.color = attr(&e, "Color").and_then(parse_color);
                }
            }
            Ok(Event::Text(t)) => {
                // v1 simplification: first non-empty text inside an Annotation is the Creator.
                // (Appearance geometry is not modelled yet; see hardening note in task brief.)
                if let Some(p) = current.as_mut() {
                    if p.creator.is_empty() {
                        let s = t.unescape().map(|s| s.into_owned()).unwrap_or_default();
                        if !s.is_empty() {
                            p.creator = s;
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name().local_name().as_ref() == b"Annotation" => {
                if let Some(p) = current.take() {
                    let color = p.color.unwrap_or(Color::Rgb(255, 255, 0));
                    let payload = match &p.kind {
                        AnnotationKind::Highlight | AnnotationKind::Underline | AnnotationKind::Strikeout => {
                            AnnotationPayload::Markup {
                                quad_points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
                                color,
                            }
                        }
                        _ => AnnotationPayload::Note {
                            rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                            color,
                            content: String::new(),
                            icon: NoteIcon::Note,
                        },
                    };
                    out.push(Annotation {
                        id: AnnotationId::new(),
                        kind: p.kind,
                        page: page.clone(),
                        creator: p.creator,
                        created: 0,
                        modified: 0,
                        reply_to: None,
                        payload,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Annotation.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(out)
}

fn parse_color(s: String) -> Option<Color> {
    let n: Vec<u8> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    match n.len() {
        3 => Some(Color::Rgb(n[0], n[1], n[2])),
        _ => None,
    }
}
