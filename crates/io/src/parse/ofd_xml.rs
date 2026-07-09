use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::OfdError;

/// Extract DocRoot path (e.g. "Doc_0/Document.xml") from OFD.xml.
pub fn parse_doc_root(ofd_xml: &str) -> Result<String, OfdError> {
    let mut reader = Reader::from_str(ofd_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_doc_root = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().local_name().as_ref() == b"DocRoot" => in_doc_root = true,
            Ok(Event::Empty(_)) => {}
            Ok(Event::Text(e)) if in_doc_root => {
                return Ok(e.unescape().map_err(|source| OfdError::Xml {
                    entry: "OFD.xml".into(),
                    loc: "DocRoot".into(),
                    source,
                })?.into_owned());
            }
            Ok(Event::End(_)) => in_doc_root = false,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml { entry: "OFD.xml".into(), loc: String::new(), source: e });
            }
            _ => {}
        }
    }
    Err(OfdError::Schema { entry: "OFD.xml".into(), reason: "DocRoot missing".into() })
}
