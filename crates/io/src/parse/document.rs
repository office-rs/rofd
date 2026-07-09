use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{DocMeta, PageId, Rect};

use crate::error::OfdError;

pub struct PageRef {
    pub id: PageId,
    pub base_loc: String,
}

pub struct DocHeader {
    pub page_area: Option<Rect>,
    pub pages: Vec<PageRef>,
    pub meta: DocMeta,
}

pub fn parse_document(doc_xml: &str) -> Result<DocHeader, OfdError> {
    let mut reader = Reader::from_str(doc_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut header = DocHeader { page_area: None, pages: vec![], meta: DocMeta::default() };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"PhysicalBox" => header.page_area = Some(parse_rect(&e)),
                b"Page" => {
                    let id = attr(&e, "ID").unwrap_or_default();
                    let base = attr(&e, "BaseLoc").unwrap_or_default();
                    header.pages.push(PageRef { id: PageId::new(id), base_loc: base });
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Document.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(header)
}

fn parse_rect(e: &BytesStart) -> Rect {
    Rect {
        x: attr(e, "x").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        y: attr(e, "y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        w: attr(e, "w").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        h: attr(e, "h").and_then(|s| s.parse().ok()).unwrap_or(0.0),
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}
