//! Byte-level patches for the entry files the editor save may touch
//! (spec 2026-11-23 §7.3): Document.xml's `MaxUnitID` and `Annotations` loc,
//! and OFD.xml's `Signatures` loc.
//!
//! Each function locates its target through quick-xml event offsets (never by
//! guessing at strings), works with the `ofd:` prefix and with a default
//! namespace, and preserves every byte outside the one replaced or inserted
//! span. A new element reuses the root element's prefix, so it binds to the
//! namespace already declared there.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::serialize::annotation::xml_escape;

/// One element's place in the source text, as byte offsets into the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ElementSpan {
    /// Offset of the element's `<`.
    start: usize,
    /// Offset just past the start tag, where the content begins. Equal to
    /// `end` for an empty (`<.. />`) element.
    content_start: usize,
    /// Offset of the end tag's `<`. Equal to `end` for an empty element.
    content_end: usize,
    /// Offset just past the element.
    end: usize,
    /// Nesting depth; the root element is 1.
    depth: usize,
}

/// Qualified name of the root element (for example `ofd:Document`), empty
/// when the text has no element at all.
struct Scan {
    root_name: String,
}

impl Scan {
    /// The root's namespace prefix including its colon, or `""` when the
    /// document uses the default namespace.
    fn prefix(&self) -> &str {
        match self.root_name.find(':') {
            Some(i) => &self.root_name[..i + 1],
            None => "",
        }
    }

    fn root_local_name(&self) -> &str {
        self.root_name
            .split_once(':')
            .map_or(&self.root_name, |(_, l)| l)
    }
}

/// Full spans of every element whose local name is one of `wanted`, in
/// document order, paired with the index into `wanted`.
fn scan(xml: &str, wanted: &[&str]) -> (Vec<(usize, ElementSpan)>, Scan) {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    // Nesting depth of the next element (open elements count).
    let mut depth = 0usize;
    let mut hits = Vec::new();
    let mut root_name = String::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let local = e.name().local_name();
                let local = local.as_ref();
                if depth == 1 {
                    root_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                }
                let idx = wanted.iter().position(|w| w.as_bytes() == local);
                if let Some(idx) = idx {
                    hits.push((
                        idx,
                        ElementSpan {
                            start: pos_before,
                            content_start: reader.buffer_position() as usize,
                            // `end == 0` marks a hit still waiting for its end tag.
                            content_end: 0,
                            end: 0,
                            depth,
                        },
                    ));
                }
            }
            Ok(Event::End(_)) => {
                // The element closing here sits one level below the parent.
                let element_depth = depth;
                depth = depth.saturating_sub(1);
                // Close the innermost wanted element still open at this depth.
                if let Some((_, span)) = hits
                    .iter_mut()
                    .rev()
                    .find(|(_, s)| s.depth == element_depth && s.end == 0)
                {
                    span.content_end = pos_before;
                    span.end = reader.buffer_position() as usize;
                }
            }
            Ok(Event::Empty(e)) => {
                let depth = depth + 1;
                let local = e.name().local_name();
                let local = local.as_ref();
                if depth == 1 {
                    root_name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                }
                if let Some(idx) = wanted.iter().position(|w| w.as_bytes() == local) {
                    let pos_after = reader.buffer_position() as usize;
                    hits.push((
                        idx,
                        ElementSpan {
                            start: pos_before,
                            content_start: pos_after,
                            content_end: pos_after,
                            end: pos_after,
                            depth,
                        },
                    ));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (hits, Scan { root_name })
}

/// Byte range of the number inside the element's content, ignoring the
/// whitespace around it. `None` unless the content is one unsigned integer.
fn digits_span(xml: &str, span: &ElementSpan) -> Option<(usize, usize)> {
    let content = xml.get(span.content_start..span.content_end)?;
    let trimmed = content.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let lead = content.len() - content.trim_start().len();
    Some((
        span.content_start + lead,
        span.content_start + lead + trimmed.len(),
    ))
}

fn insert(xml: &str, at: usize, text: &str) -> String {
    let mut out = String::with_capacity(xml.len() + text.len());
    out.push_str(&xml[..at]);
    out.push_str(text);
    out.push_str(&xml[at..]);
    out
}

/// The value of Document.xml's `MaxUnitID`, when the element carries one.
pub fn max_unit_id_of(document_xml: &str) -> Option<u64> {
    let (hits, _) = scan(document_xml, &["MaxUnitID"]);
    let span = &hits.first()?.1;
    let (a, b) = digits_span(document_xml, span)?;
    document_xml[a..b].parse().ok()
}

/// Return `document_xml` with `MaxUnitID` raised to `value`. Values that are
/// not greater than the current one leave the text unchanged; a missing
/// element, or one without an integer, yields `None`.
pub fn with_max_unit_id(document_xml: &str, value: u64) -> Option<String> {
    let (hits, _) = scan(document_xml, &["MaxUnitID"]);
    let span = hits.first()?.1;
    let (a, b) = digits_span(document_xml, &span)?;
    let current: u64 = document_xml[a..b].parse().ok()?;
    if value <= current {
        return Some(document_xml.to_owned());
    }
    let mut out = String::with_capacity(document_xml.len() + 8);
    out.push_str(&document_xml[..a]);
    out.push_str(&value.to_string());
    out.push_str(&document_xml[b..]);
    Some(out)
}

/// Return `document_xml` with an `<Annotations>` child declaring `loc`,
/// unless it already has one. The element goes before an existing
/// CustomTags / Attachments / Extensions child and otherwise right before
/// the root's closing tag (GB/T 33190 appendix A child order). `None` when
/// there is no closable root element.
pub fn with_annotations_loc(document_xml: &str, loc: &str) -> Option<String> {
    let wanted = ["Annotations", "CustomTags", "Attachments", "Extensions"];
    let (hits, root) = scan(document_xml, &wanted);
    let root_local = root.root_local_name().to_owned();
    if root_local.is_empty() {
        return None;
    }
    if hits.iter().any(|(i, s)| *i == 0 && s.depth == 2) {
        return Some(document_xml.to_owned());
    }
    let (root_hits, _) = scan(document_xml, &[root_local.as_str()]);
    let root_span = root_hits.first()?.1;
    let root_close = (root_span.content_end < root_span.end).then_some(root_span.content_end)?;
    let anchor = hits
        .iter()
        .filter(|(i, s)| s.depth == 2 && (1..=3).contains(i))
        .map(|(_, s)| s.start)
        .min()
        .unwrap_or(root_close);
    let prefix = root.prefix();
    let element = format!(
        "<{prefix}Annotations>{}</{prefix}Annotations>",
        xml_escape(loc)
    );
    Some(insert(document_xml, anchor, &element))
}

/// Return `ofd_xml` with a `<Signatures>` child on DocBody declaring `loc`,
/// appended after DocBody's last child, unless DocBody already declares one.
/// `None` when there is no closable DocBody.
pub fn with_signatures_loc(ofd_xml: &str, loc: &str) -> Option<String> {
    let (hits, root) = scan(ofd_xml, &["Signatures", "DocBody"]);
    if hits.iter().any(|(i, s)| *i == 0 && s.depth == 3) {
        return Some(ofd_xml.to_owned());
    }
    let body = hits.iter().find(|(i, s)| *i == 1 && s.depth == 2)?.1;
    let body_close = (body.content_end < body.end).then_some(body.content_end)?;
    let prefix = root.prefix();
    let element = format!(
        "<{prefix}Signatures>{}</{prefix}Signatures>",
        xml_escape(loc)
    );
    Some(insert(ofd_xml, body_close, &element))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n\
  <ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID></ofd:CommonData>\n\
  <ofd:Pages />\n\
</ofd:Document>";

    const OFD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\n\
  <ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody>\n\
</ofd:OFD>";

    const INSERTED_ANNOTATIONS: &str = "<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>";
    const INSERTED_SIGNATURES: &str =
        "<ofd:Signatures>/Doc_0/Signs/Signatures.xml</ofd:Signatures>";

    #[test]
    fn with_max_unit_id_raises_only() {
        assert_eq!(max_unit_id_of(DOC), Some(9));
        let raised = with_max_unit_id(DOC, 20).expect("MaxUnitID present");
        assert!(
            raised.contains("<ofd:MaxUnitID>20</ofd:MaxUnitID>"),
            "{raised}"
        );
        assert_eq!(
            raised.replace(
                "<ofd:MaxUnitID>20</ofd:MaxUnitID>",
                "<ofd:MaxUnitID>9</ofd:MaxUnitID>"
            ),
            DOC,
            "only the digits change"
        );
        assert_eq!(
            with_max_unit_id(DOC, 5).as_deref(),
            Some(DOC),
            "never lowers"
        );
        assert_eq!(
            with_max_unit_id(DOC, 9).as_deref(),
            Some(DOC),
            "equal is a no-op"
        );
    }

    #[test]
    fn with_max_unit_id_missing_element_is_none() {
        let without = DOC.replace("<ofd:MaxUnitID>9</ofd:MaxUnitID>", "");
        assert_eq!(max_unit_id_of(&without), None);
        assert_eq!(with_max_unit_id(&without, 20), None);
        let self_closing = DOC.replace("<ofd:MaxUnitID>9</ofd:MaxUnitID>", "<ofd:MaxUnitID />");
        assert_eq!(max_unit_id_of(&self_closing), None);
        assert_eq!(with_max_unit_id(&self_closing, 20), None);
    }

    #[test]
    fn with_max_unit_id_replaces_the_digits_only_around_whitespace() {
        let padded = DOC.replace(
            "<ofd:MaxUnitID>9</ofd:MaxUnitID>",
            "<ofd:MaxUnitID> 9 </ofd:MaxUnitID>",
        );
        let raised = with_max_unit_id(&padded, 20).unwrap();
        assert!(
            raised.contains("<ofd:MaxUnitID> 20 </ofd:MaxUnitID>"),
            "{raised}"
        );
    }

    #[test]
    fn with_annotations_loc_inserts_before_extensions() {
        let doc = DOC.replace(
            "</ofd:Document>",
            "  <ofd:Extensions>Doc_0/Extensions.xml</ofd:Extensions>\n</ofd:Document>",
        );
        let out = with_annotations_loc(&doc, "Annots/Annotations.xml").expect("root closable");
        let annotations = out.find("<ofd:Annotations>").expect("inserted");
        let extensions = out.find("<ofd:Extensions>").expect("anchor");
        assert!(
            annotations < extensions,
            "Annotations precedes Extensions: {out}"
        );
        assert_eq!(
            out.replace(INSERTED_ANNOTATIONS, ""),
            doc,
            "nothing else changes"
        );
    }

    #[test]
    fn with_annotations_loc_inserts_before_root_close_without_anchor() {
        let out = with_annotations_loc(DOC, "Annots/Annotations.xml").unwrap();
        assert!(out
            .contains("<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations></ofd:Document>"));
        assert_eq!(out.replace(INSERTED_ANNOTATIONS, ""), DOC);
    }

    #[test]
    fn with_annotations_loc_keeps_existing() {
        let doc = DOC.replace(
            "</ofd:Document>",
            "<ofd:Annotations>Annots/A.xml</ofd:Annotations></ofd:Document>",
        );
        assert_eq!(
            with_annotations_loc(&doc, "other").as_deref(),
            Some(doc.as_str())
        );
        let empty_form = DOC.replace("</ofd:Document>", "<ofd:Annotations/></ofd:Document>");
        assert_eq!(
            with_annotations_loc(&empty_form, "x").as_deref(),
            Some(empty_form.as_str())
        );
    }

    #[test]
    fn with_signatures_loc_appends_to_doc_body() {
        let out = with_signatures_loc(OFD, "/Doc_0/Signs/Signatures.xml").expect("DocBody present");
        assert!(
            out.contains(&format!(
                "</ofd:DocRoot>{INSERTED_SIGNATURES}</ofd:DocBody>"
            )),
            "inserted after the last DocBody child: {out}"
        );
        assert_eq!(out.replace(INSERTED_SIGNATURES, ""), OFD);
        // Already declared: unchanged (idempotent).
        assert_eq!(
            with_signatures_loc(&out, "other").as_deref(),
            Some(out.as_str())
        );
    }

    #[test]
    fn with_signatures_loc_needs_a_closable_doc_body() {
        assert_eq!(
            with_signatures_loc("<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"/>", "x"),
            None
        );
        assert_eq!(with_signatures_loc("<ofd:OFD xmlns:ofd=\"u\"/>", "x"), None);
        assert_eq!(with_signatures_loc("not xml at all", "x"), None);
    }

    #[test]
    fn patch_handles_default_namespace() {
        let doc = DOC.replace("ofd:", "");
        assert_eq!(max_unit_id_of(&doc), Some(9));
        let raised = with_max_unit_id(&doc, 20).unwrap();
        assert!(raised.contains("<MaxUnitID>20</MaxUnitID>"), "{raised}");
        let out = with_annotations_loc(&doc, "A.xml").unwrap();
        assert!(
            out.contains("<Annotations>A.xml</Annotations></Document>"),
            "{out}"
        );
        let ofd = OFD.replace("ofd:", "");
        let out = with_signatures_loc(&ofd, "S.xml").unwrap();
        assert!(
            out.contains("<Signatures>S.xml</Signatures></DocBody>"),
            "{out}"
        );
    }

    #[test]
    fn patch_escapes_the_loc_text() {
        let out = with_annotations_loc(DOC, "a&b.less").unwrap();
        assert!(
            out.contains("<ofd:Annotations>a&amp;b.less</ofd:Annotations>"),
            "{out}"
        );
    }
}
