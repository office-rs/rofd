use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::DocMeta;

use crate::error::OfdError;

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
