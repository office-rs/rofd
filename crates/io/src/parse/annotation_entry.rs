//! GB/T 33190 §15.1 注释入口文件 Annotations.xml: <Annotations><Page PageID><FileLoc>。

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::OfdError;
use crate::parse::attr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnPageRef {
    pub page_id: String,
    pub file_loc: String,
}

/// Parse the GB/T 33190 §15.1 annotation entry file (`Annotations.xml`) into a
/// list of per-page annotation file references.
///
/// The entry file has the shape
/// `<Annotations><Page PageID="N"><FileLoc>Page_0/Annotation.xml</FileLoc></Page>...</Annotations>`.
/// `file_loc` is captured raw (relative to the entry file's own directory); T6
/// resolves it against the package. A self-closing `<Page/>` with no `FileLoc`
/// child yields an empty `file_loc`.
pub fn parse_annotations_entry(xml: &str) -> Result<Vec<AnnPageRef>, OfdError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut page_id: Option<String> = None;
    let mut in_file_loc = false;
    let mut file_loc = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().local_name().as_ref() {
                b"Page" => page_id = attr(&e, "PageID"),
                b"FileLoc" => {
                    in_file_loc = true;
                    file_loc.clear();
                }
                _ => {}
            },
            // Self-closing elements emit Empty (no following End): emit immediately.
            Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"Page" => {
                    if let Some(pid) = attr(&e, "PageID") {
                        out.push(AnnPageRef {
                            page_id: pid,
                            file_loc: String::new(),
                        });
                    }
                }
                b"FileLoc" => {
                    // Self-closing FileLoc has no text -> stays empty.
                    in_file_loc = false;
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_file_loc => {
                file_loc = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"FileLoc" => in_file_loc = false,
                b"Page" => {
                    if let Some(pid) = page_id.take() {
                        out.push(AnnPageRef {
                            page_id: pid,
                            file_loc: std::mem::take(&mut file_loc),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: "Annotations.xml".into(),
                    loc: String::new(),
                    source: e,
                })
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entry_pages() {
        let xml = r#"<?xml version="1.0"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>
  <ofd:Page PageID="497"><ofd:FileLoc>Page_1/Annotation.xml</ofd:FileLoc></ofd:Page>
</ofd:Annotations>"#;
        let pages = parse_annotations_entry(xml).unwrap();
        assert_eq!(
            pages,
            vec![
                AnnPageRef {
                    page_id: "1".into(),
                    file_loc: "Page_0/Annotation.xml".into()
                },
                AnnPageRef {
                    page_id: "497".into(),
                    file_loc: "Page_1/Annotation.xml".into()
                },
            ]
        );
    }

    #[test]
    fn self_closing_page_yields_empty_file_loc() {
        // A <Page/> with no FileLoc child -> file_loc is empty, no panic.
        let xml = r#"<Annotations><Page PageID="3"/></Annotations>"#;
        let pages = parse_annotations_entry(xml).unwrap();
        assert_eq!(
            pages,
            vec![AnnPageRef {
                page_id: "3".into(),
                file_loc: String::new()
            }]
        );
    }

    #[test]
    fn empty_annotations_yields_empty_vec() {
        let xml = r#"<Annotations></Annotations>"#;
        assert!(parse_annotations_entry(xml).unwrap().is_empty());
    }
}
