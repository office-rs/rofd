pub mod annotation;
pub mod document;
pub mod ofd_xml;
pub mod page;
pub mod resource;

use std::sync::Arc;

use quick_xml::events::BytesStart;

use rofd_dom::{Color, OfdDocument, Rect};

use crate::error::{LoadReport, OfdError, OfdWarning};
use crate::package::{EntryKind, PackageHandle, PkgEntry};
use crate::zip_util::read_all_entries;

pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// Parse a Rect from a whitespace-separated `"x y w h"` string.
///
/// Per GB/T 33190, the page-area box elements (`PhysicalBox`, `ApplicationBox`,
/// ...) carry their geometry as element **text content**
/// (e.g. `<ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>`), not attributes.
pub(crate) fn parse_rect_ws(s: &str) -> Rect {
    let n: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    Rect {
        x: n.first().copied().unwrap_or(0.0),
        y: n.get(1).copied().unwrap_or(0.0),
        w: n.get(2).copied().unwrap_or(0.0),
        h: n.get(3).copied().unwrap_or(0.0),
    }
}

/// Parse a color from a whitespace-separated `"r g b"` (or `"r g b a"`) string -
/// the format of the `Value` attribute on `FillColor`/`StrokeColor`/`Color`
/// (GB/T 33190). Alpha is accepted but currently dropped (`Color::Rgb`).
pub(crate) fn parse_color_value(s: &str) -> Option<Color> {
    let n: Vec<u8> = s.split_whitespace().filter_map(|t| t.parse::<u8>().ok()).collect();
    match n.len() {
        3 | 4 => Some(Color::Rgb(n[0], n[1], n[2])),
        _ => None,
    }
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
    let meta = ofd_xml::parse_doc_meta(&ofd_xml)?;
    let doc_xml = entry_str(&entries, &doc_root)?;
    let header = document::parse_document(&doc_xml)?;
    let mut doc = OfdDocument { meta, ..OfdDocument::default() };
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
    // Resources: DocumentRes.xml / PublicRes.xml carry DrawParams, MultiMedias
    // (images), and Fonts (GB/T 33190). Paths resolve relative to the Res
    // BaseLoc (itself relative to the resource entry's own directory).
    for e in &entries {
        let name = e.name.as_str();
        if name.ends_with("DocumentRes.xml") || name.ends_with("PublicRes.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            let parsed = resource::parse_res(&xml)?;
            let res_dir = e.name.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let base_dir = if parsed.base_loc.is_empty() {
                res_dir.to_string()
            } else if res_dir.is_empty() {
                parsed.base_loc.clone()
            } else {
                format!("{res_dir}/{}", parsed.base_loc)
            };
            for (id, dp) in parsed.draw_params {
                doc.resources.draw_params.insert(id, dp);
            }
            for (id, media_file, _fmt) in parsed.multimedias {
                let path = if base_dir.is_empty() { media_file } else { format!("{base_dir}/{media_file}") };
                if let Some(fe) = entries.iter().find(|x| x.name == path) {
                    doc.resources.images.insert(id, fe.bytes.clone());
                }
            }
            for (id, fref, font_file) in parsed.fonts {
                doc.resources.fonts.insert(id.clone(), fref);
                if let Some(rel) = font_file {
                    let path = if base_dir.is_empty() { rel } else { format!("{base_dir}/{rel}") };
                    if let Some(fe) = entries.iter().find(|x| x.name == path) {
                        doc.resources.font_data.insert(id, fe.bytes.clone());
                    }
                }
            }
        }
    }
    // Resources: Font.xml entries (+ font bytes via FontFile)
    for e in &entries {
        if e.name.ends_with("/Res/Font.xml") {
            let xml = String::from_utf8_lossy(&e.bytes).into_owned();
            let font_dir = e.name.rsplit_once('/').map(|(d, _)| d).unwrap_or(""); // .../Res
            for (id, fref, font_file) in resource::parse_font_res(&xml)? {
                doc.resources.fonts.insert(id.clone(), fref);
                if let Some(rel) = font_file {
                    // FontFile is relative to the Res dir.
                    let font_path = if font_dir.is_empty() { rel } else { format!("{font_dir}/{rel}") };
                    if let Some(fe) = entries.iter().find(|x| x.name == font_path) {
                        doc.resources.font_data.insert(id, fe.bytes.clone());
                    }
                }
            }
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
