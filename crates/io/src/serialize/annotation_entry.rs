//! GB/T 33190 §15.1 Annotations.xml 入口序列化。
//!
//! 入口文件 `<Annotations>` 列出每个有批注的页面，通过 `<FileLoc>` 指向
//! 对应的分页批注文件（`Page_{idx}/Annotation.xml`，相对入口目录）。
//! 这是 `parse::annotation_entry::parse_annotations_entry` 的逆操作。

use rofd_dom::PageId;

/// Serialize the Annotations.xml entry file.
///
/// `pages`: list of `(page_id, page_index)` pairs for pages that have
/// annotations, in document order. The `FileLoc` for each is
/// `Page_{index}/Annotation.xml` (relative to the entry file's directory).
pub fn serialize_annotations_entry(pages: &[(PageId, usize)]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">");
    for (pid, idx) in pages {
        s.push_str(&format!(
            "<ofd:Page PageID=\"{}\"><ofd:FileLoc>Page_{}/Annotation.xml</ofd:FileLoc></ofd:Page>",
            pid.0, idx
        ));
    }
    s.push_str("</ofd:Annotations>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pages_yields_empty_annotations() {
        let xml = serialize_annotations_entry(&[]);
        assert!(xml.contains("<ofd:Annotations"));
        assert!(xml.contains("</ofd:Annotations>"));
        assert!(!xml.contains("<ofd:Page"));
    }

    #[test]
    fn single_page_emits_fileloc() {
        let pages = vec![(PageId::new("1"), 0)];
        let xml = serialize_annotations_entry(&pages);
        assert!(xml.contains("PageID=\"1\""));
        assert!(xml.contains("Page_0/Annotation.xml"));
    }

    #[test]
    fn multiple_pages_emit_in_order() {
        let pages = vec![(PageId::new("1"), 0), (PageId::new("3"), 2)];
        let xml = serialize_annotations_entry(&pages);
        let p0 = xml.find("Page_0/Annotation.xml").unwrap();
        let p2 = xml.find("Page_2/Annotation.xml").unwrap();
        assert!(p0 < p2, "pages should appear in document order");
    }
}
