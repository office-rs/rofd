use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::{DocMeta, RawFragment};

use crate::error::OfdError;
use crate::parse::raw::{capture_raw_subtree, NsScope};

/// Extract DocRoot path (e.g. "Doc_0/Document.xml") from OFD.xml.
pub fn parse_doc_root(ofd_xml: &str) -> Result<String, OfdError> {
    let mut reader = Reader::from_str(ofd_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_doc_root = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().local_name().as_ref() == b"DocRoot" => {
                in_doc_root = true
            }
            Ok(Event::Empty(_)) => {}
            Ok(Event::Text(e)) if in_doc_root => {
                return Ok(e
                    .unescape()
                    .map_err(|source| OfdError::Xml {
                        entry: "OFD.xml".into(),
                        loc: "DocRoot".into(),
                        source,
                    })?
                    .into_owned());
            }
            Ok(Event::End(_)) => in_doc_root = false,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "OFD.xml".into(),
                    loc: String::new(),
                    source: e,
                });
            }
            _ => {}
        }
    }
    Err(OfdError::Schema {
        entry: "OFD.xml".into(),
        reason: "DocRoot missing".into(),
    })
}

/// Extract DocInfo (DocID/Title/Author/CreationDate/LastModDate) from OFD.xml.
///
/// Walks `<ofd:DocInfo>` children, tracking the currently-open element name so
/// that captured text is assigned to the matching `DocMeta` field. Mirrors the
/// annotation Creator-capture pattern.
pub fn parse_doc_meta(ofd_xml: &str) -> Result<DocMeta, OfdError> {
    let mut reader = Reader::from_str(ofd_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut meta = DocMeta::default();
    let mut in_doc_info = false;
    let mut current_field: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = e.name().local_name();
                let local_ref = local.as_ref();
                if local_ref == b"DocInfo" {
                    in_doc_info = true;
                } else if in_doc_info {
                    current_field = Some(String::from_utf8_lossy(local_ref).into_owned());
                }
            }
            Ok(Event::Empty(e)) if in_doc_info => {
                // Self-closing child element (no text) - nothing to capture.
                let _ = e.name().local_name();
            }
            Ok(Event::Text(t)) if in_doc_info && current_field.is_some() => {
                let s = t.unescape().map_err(|source| OfdError::Xml {
                    entry: "OFD.xml".into(),
                    loc: "DocInfo".into(),
                    source,
                })?;
                let s = s.trim();
                if !s.is_empty() {
                    let value = s.to_string();
                    match current_field.as_deref() {
                        Some("DocID") => meta.doc_id = Some(value),
                        Some("Title") => meta.title = Some(value),
                        Some("Author") => meta.author = Some(value),
                        Some("CreationDate") => meta.creation_date = Some(value),
                        Some("LastModDate") => meta.mod_date = Some(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = e.name().local_name();
                if local.as_ref() == b"DocInfo" {
                    in_doc_info = false;
                }
                current_field = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "OFD.xml".into(),
                    loc: String::new(),
                    source: e,
                });
            }
            _ => {}
        }
    }
    Ok(meta)
}

/// Capture passthrough fragments from OFD.xml (spec 2026-11-11 §6.2):
/// unknown `<DocBody>` children (Version, ...) and unknown `<DocInfo>`
/// children (Subject/Keywords/Creator/CustomDatas/...). Modelled elements
/// (DocInfo, DocRoot, DocID/Title/Author/CreationDate/LastModDate) are
/// excluded. Returns (body raws, info raws), each position-preserved.
pub fn parse_ofd_xml_raws(ofd_xml: &str) -> (Vec<RawFragment>, Vec<RawFragment>) {
    let mut reader = Reader::from_str(ofd_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut body = Vec::new();
    let mut info = Vec::new();
    let mut depth = 0usize;
    let mut in_doc_body = false;
    let mut in_doc_info = false;
    let mut body_seen = 0usize;
    let mut info_seen = 0usize;
    let mut ns = NsScope::default();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                ns.start(&e);
                let local = e.name().local_name();
                let name = local.as_ref();
                if depth == 2 && name == b"DocBody" {
                    in_doc_body = true;
                } else if in_doc_body && depth == 3 && name == b"DocInfo" {
                    in_doc_info = true;
                } else if in_doc_body && depth == 3 && name == b"DocRoot" {
                    body_seen += 1;
                } else if in_doc_info && depth == 4 {
                    if matches!(
                        name,
                        b"DocID" | b"Title" | b"Author" | b"CreationDate" | b"LastModDate"
                    ) {
                        info_seen += 1;
                    } else if let Ok(frag) =
                        capture_raw_subtree(&mut reader, &mut buf, ofd_xml, pos_before, &ns)
                    {
                        // The capture consumed the subtree root's End event;
                        // undo this loop's depth increment for it.
                        depth = depth.saturating_sub(1);
                        ns.end();
                        let position = info_seen + info.len();
                        info.push(RawFragment {
                            position,
                            xml: frag,
                        });
                    } else {
                        depth = depth.saturating_sub(1);
                        ns.end();
                    }
                } else if in_doc_body && depth == 3 {
                    if let Ok(frag) =
                        capture_raw_subtree(&mut reader, &mut buf, ofd_xml, pos_before, &ns)
                    {
                        depth = depth.saturating_sub(1);
                        ns.end();
                        let position = body_seen + body.len();
                        body.push(RawFragment {
                            position,
                            xml: frag,
                        });
                    } else {
                        depth = depth.saturating_sub(1);
                        ns.end();
                    }
                }
            }
            Ok(Event::End(e)) => {
                depth = depth.saturating_sub(1);
                ns.end();
                match e.name().local_name().as_ref() {
                    b"DocBody" => in_doc_body = false,
                    b"DocInfo" => in_doc_info = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    (body, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ofd_xml_raws_capture_version_and_subject() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocInfo><ofd:DocID>D1</ofd:DocID><ofd:Subject>s</ofd:Subject></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot><ofd:Version>1.1</ofd:Version></ofd:DocBody></ofd:OFD>";
        let (body, info) = parse_ofd_xml_raws(xml);
        assert_eq!(body.len(), 1, "body: {body:?}");
        assert_eq!(info.len(), 1, "info: {info:?}");
    }
}
