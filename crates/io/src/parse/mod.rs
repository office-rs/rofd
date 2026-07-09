pub mod annotation;
pub mod document;
pub mod ofd_xml;
pub mod page;
pub mod resource;

use std::sync::Arc;

use quick_xml::events::BytesStart;

use rofd_dom::OfdDocument;

use crate::error::{LoadReport, OfdError, OfdWarning};
use crate::package::{EntryKind, PackageHandle, PkgEntry};
use crate::zip_util::read_all_entries;

pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

pub fn parse_ofd(bytes: &[u8]) -> Result<LoadReport, OfdError> {
    let raw = read_all_entries(bytes)?;
    let mut warnings = Vec::new();
    let mut entries: Vec<PkgEntry> = Vec::with_capacity(raw.len());
    let mut ofd_xml = String::new();
    for (name, data) in &raw {
        let kind = classify(name);
        if name == "OFD.xml" {
            ofd_xml = String::from_utf8_lossy(data).into_owned();
        }
        entries.push(PkgEntry { name: name.clone(), kind, bytes: Arc::new(data.clone()) });
    }
    let doc_root = ofd_xml::parse_doc_root(&ofd_xml)?;
    let doc_xml = entry_str(&entries, &doc_root)?;
    let header = document::parse_document(&doc_xml)?;
    let mut doc = OfdDocument { meta: header.meta.clone(), ..OfdDocument::default() };
    for pref in &header.pages {
        let page_path = join(&doc_root, &pref.base_loc);
        let page_xml = entry_str(&entries, &page_path)?;
        let page = page::parse_page(pref.id.clone(), &page_xml, &header)?;
        // Template handling: if page.template is Some, emit warning (v1 doesn't expand).
        if page.template.is_some() {
            warnings.push(OfdWarning::MissingFeature { feature: "Template".into(), entry: page_path.clone() });
        }
        doc.pages.push(page);
    }
    // Resources: Font.xml entries
    for e in &entries {
        if e.name.ends_with("/Res/Font.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            resource::parse_font_res(&xml, &mut doc.resources)?;
        }
    }
    // Annotations: per-page Annotation.xml; map each entry to its page by index.
    for e in &entries {
        if e.name.ends_with("/Annotation.xml") {
            let page_idx = e
                .name
                .split('/')
                .find_map(|seg| seg.strip_prefix("Page_").and_then(|n| n.parse::<usize>().ok()));
            if let Some(idx) = page_idx {
                if let Some(page) = doc.pages.get(idx) {
                    let xml = String::from_utf8_lossy(&e.bytes).into_owned();
                    let anns = annotation::parse_annotation_xml(&xml, &page.id)?;
                    if !anns.is_empty() {
                        doc.annotations.by_page.entry(page.id.clone()).or_default().extend(anns);
                    }
                }
            }
        }
    }
    let package = PackageHandle { entries };
    Ok(LoadReport::new(doc, package, warnings))
}

fn classify(name: &str) -> EntryKind {
    if name.ends_with("Annotation.xml") || name.contains("/Annotations/") {
        EntryKind::Annotation
    } else if name.ends_with("Page.xml") || name.ends_with("Document.xml") || name == "OFD.xml" {
        EntryKind::Body
    } else if name.starts_with("Doc_") && name.contains("/Signs/") {
        EntryKind::Signature
    } else if name.contains("/Res/") {
        EntryKind::Resource
    } else {
        EntryKind::Other
    }
}

fn entry_str(entries: &[PkgEntry], name: &str) -> Result<String, OfdError> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| String::from_utf8_lossy(&e.bytes).into_owned())
        .ok_or_else(|| OfdError::Schema { entry: name.into(), reason: "entry missing".into() })
}

fn join(doc_root: &str, base_loc: &str) -> String {
    // base_loc is relative to the document's directory.
    let dir = doc_root.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    if dir.is_empty() { base_loc.to_string() } else { format!("{dir}/{base_loc}") }
}
