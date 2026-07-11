use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{DocMeta, PageId, Rect};

use crate::error::OfdError;
use crate::parse::{attr, parse_rect_ws};

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
    // PhysicalBox carries its geometry as element text content ("x y w h"),
    // not attributes, so we capture the Text event inside it.
    let mut in_physical_box = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"PhysicalBox" => in_physical_box = true,
                b"Page" => handle_page(&e, &mut header),
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                // Self-closing <Page/> has no End event; register immediately.
                if e.name().local_name().as_ref() == b"Page" {
                    handle_page(&e, &mut header);
                }
            }
            Ok(Event::Text(t)) => {
                if in_physical_box {
                    let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                    header.page_area = Some(parse_rect_ws(&s));
                    in_physical_box = false;
                }
            }
            Ok(Event::End(e)) => {
                if in_physical_box && e.name().local_name().as_ref() == b"PhysicalBox" {
                    in_physical_box = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Document.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(header)
}

fn handle_page(e: &BytesStart, header: &mut DocHeader) {
    let id = attr(e, "ID").unwrap_or_default();
    let base = attr(e, "BaseLoc").unwrap_or_default();
    header.pages.push(PageRef { id: PageId::new(id), base_loc: base });
}
