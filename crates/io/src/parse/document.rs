use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use rofd_dom::{DocMeta, PageId, RawFragment, Rect};

use crate::error::OfdError;
use crate::parse::attr;
use crate::parse::parse_rect_ws;
use crate::parse::raw::{capture_raw_empty, capture_raw_subtree, NsScope};

pub struct PageRef {
    pub id: PageId,
    pub base_loc: String,
}

#[derive(Default)]
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
    /// Unknown children of the Document root, position-preserved (spec
    /// 2026-11-11 §6.2).
    pub raw_children: Vec<RawFragment>,
    /// Unknown children of `<CommonData>`, position-preserved.
    pub raw_common_children: Vec<RawFragment>,
}

pub fn parse_document(doc_xml: &str) -> Result<DocHeader, OfdError> {
    let mut reader = Reader::from_str(doc_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut header = DocHeader::default();
    // PhysicalBox carries its geometry as element text content ("x y w h"),
    // not attributes, so we capture the Text event inside it.
    // MaxUnitID (an integer) and Annotations (a path string) likewise carry
    // their value as element text content, so we use the same flag pattern.
    let mut in_physical_box = false;
    let mut in_max_unit_id = false;
    let mut in_annotations = false;
    // Depth tracking: 1 = <ofd:Document>, 2 = root children (CommonData /
    // Pages / Annotations / unknown), 3 = CommonData children. `seen`
    // counters keep the interleaved positions of captured raw fragments.
    let mut depth = 0usize;
    let mut in_common_data = false;
    let mut root_seen = 0usize;
    let mut common_seen = 0usize;
    let mut ns = NsScope::default();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                ns.start(&e);
                let local = e.name().local_name();
                if local.as_ref() == b"PhysicalBox" {
                    in_physical_box = true;
                } else if local.as_ref() == b"MaxUnitID" {
                    in_max_unit_id = true;
                    if in_common_data && depth == 3 {
                        common_seen += 1;
                    }
                } else if local.as_ref() == b"Annotations" {
                    in_annotations = true;
                    if depth == 2 {
                        root_seen += 1;
                    }
                } else if local.as_ref() == b"CommonData" && depth == 2 {
                    in_common_data = true;
                    root_seen += 1;
                } else if local.as_ref() == b"Pages" && depth == 2 {
                    root_seen += 1;
                } else if local.as_ref() == b"PageArea" && in_common_data && depth == 3 {
                    common_seen += 1;
                } else if local.as_ref() == b"Page" {
                    handle_page(&e, &mut header);
                } else if in_common_data
                    && depth == 3
                    && matches!(local.as_ref(), b"PublicRes" | b"DocumentRes")
                {
                    // Resource loc elements are writer-owned (canonical
                    // emission, spec D12): the originals are superseded and
                    // must NOT pass through (duplicate locs would dangle).
                    common_seen += 1;
                } else if depth == 2 {
                    let frag = capture_raw_subtree(
                        &mut reader,
                        &mut buf,
                        doc_xml,
                        pos_before,
                        &ns,
                        "Document.xml",
                    )?;
                    // The capture consumed the subtree root's End event; undo
                    // this loop's depth increment for it.
                    depth = depth.saturating_sub(1);
                    ns.end(); // the subtree root's own declarations end with it
                    let position = root_seen + header.raw_children.len();
                    header.raw_children.push(RawFragment {
                        position,
                        xml: frag,
                    });
                } else if in_common_data && depth == 3 {
                    let frag = capture_raw_subtree(
                        &mut reader,
                        &mut buf,
                        doc_xml,
                        pos_before,
                        &ns,
                        "Document.xml",
                    )?;
                    depth = depth.saturating_sub(1);
                    ns.end();
                    let position = common_seen + header.raw_common_children.len();
                    header.raw_common_children.push(RawFragment {
                        position,
                        xml: frag,
                    });
                }
            }
            Ok(Event::Empty(e)) => {
                let local = e.name().local_name();
                let name = local.as_ref();
                if name == b"Page" {
                    handle_page(&e, &mut header);
                } else if matches!(
                    name,
                    b"Annotations"
                        | b"Pages"
                        | b"CommonData"
                        | b"PageArea"
                        | b"MaxUnitID"
                        | b"PhysicalBox"
                        | b"PublicRes"
                        | b"DocumentRes"
                        | b"Document"
                ) {
                    // Known-but-empty structural elements: nothing to capture.
                } else if depth == 1 {
                    // Unknown self-closing root child (typical form for
                    // attachment/outline references) - capture verbatim.
                    let frag = capture_raw_empty(&reader, doc_xml, pos_before, &ns);
                    let position = root_seen + header.raw_children.len();
                    header.raw_children.push(RawFragment {
                        position,
                        xml: frag,
                    });
                } else if in_common_data && depth == 2 {
                    let frag = capture_raw_empty(&reader, doc_xml, pos_before, &ns);
                    let position = common_seen + header.raw_common_children.len();
                    header.raw_common_children.push(RawFragment {
                        position,
                        xml: frag,
                    });
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
            Ok(Event::End(e)) => {
                depth = depth.saturating_sub(1);
                ns.end();
                match e.name().local_name().as_ref() {
                    b"PhysicalBox" => in_physical_box = false,
                    b"MaxUnitID" => in_max_unit_id = false,
                    b"Annotations" => in_annotations = false,
                    b"CommonData" => in_common_data = false,
                    _ => {}
                }
            }
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
