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
    /// GB/T 33190 CommonData/MaxUnitID: highest ST_ID in the document.
    /// New IDs are allocated from max_unit_id + 1.
    pub max_unit_id: u64,
    /// GB/T 33190 Document/Annotations: path (relative to the doc root) of the
    /// annotation entry file (e.g. `Annots/Annotations.xml`). Consumed by T6 to
    /// locate the document-level annotation entry.
    pub annotations_loc: Option<String>,
}

pub fn parse_document(doc_xml: &str) -> Result<DocHeader, OfdError> {
    let mut reader = Reader::from_str(doc_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut header = DocHeader {
        page_area: None,
        pages: vec![],
        meta: DocMeta::default(),
        max_unit_id: 0,
        annotations_loc: None,
    };
    // PhysicalBox carries its geometry as element text content ("x y w h"),
    // not attributes, so we capture the Text event inside it.
    // MaxUnitID (an integer) and Annotations (a path string) likewise carry
    // their value as element text content, so we use the same flag pattern.
    let mut in_physical_box = false;
    let mut in_max_unit_id = false;
    let mut in_annotations = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"PhysicalBox" => in_physical_box = true,
                b"MaxUnitID" => in_max_unit_id = true,
                b"Annotations" => in_annotations = true,
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
                let s = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if in_physical_box {
                    header.page_area = Some(parse_rect_ws(&s));
                    in_physical_box = false;
                } else if in_max_unit_id {
                    // Fail-soft to 0 on malformed input (AGENTS.md §4.6: degraded
                    // input is downgraded, not fatal).
                    header.max_unit_id = s.trim().parse().unwrap_or(0);
                    in_max_unit_id = false;
                } else if in_annotations {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        header.annotations_loc = Some(trimmed.to_string());
                    }
                    in_annotations = false;
                }
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"PhysicalBox" => in_physical_box = false,
                b"MaxUnitID" => in_max_unit_id = false,
                b"Annotations" => in_annotations = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Document.xml".into(),
                    loc: String::new(),
                    source: e,
                })
            }
            _ => {}
        }
    }
    Ok(header)
}

fn handle_page(e: &BytesStart, header: &mut DocHeader) {
    let id = attr(e, "ID").unwrap_or_default();
    let base = attr(e, "BaseLoc").unwrap_or_default();
    header.pages.push(PageRef {
        id: PageId::new(id),
        base_loc: base,
    });
}
