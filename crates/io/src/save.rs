use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::package::{EntryKind, PackageHandle};
use crate::serialize::annotation::serialize_page_annotations;
use crate::zip_util::write_zip;

/// Surgical save: rewrite annotation entries from the model; copy every other
/// entry byte-identical from the retained package.
pub fn save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::with_capacity(pkg.entries.len());
    for entry in &pkg.entries {
        match entry.kind {
            EntryKind::Annotation => {
                if let Some(xml) = annotation_target(&entry.name, doc) {
                    out.push((entry.name.clone(), xml.into_bytes()));
                } else {
                    // No model entry for this annotation file - preserve original.
                    out.push((entry.name.clone(), (*entry.bytes).clone()));
                }
            }
            _ => {
                // Body / signature / resource / other - byte-identical copy.
                out.push((entry.name.clone(), (*entry.bytes).clone()));
            }
        }
    }
    write_zip(&out)
}

/// Map an annotation entry name back to its serialized XML.
///
/// `name` like `Doc_0/Pages/Page_0/Annotation.xml` is matched via the `Page_<n>`
/// segment to page index `n`. Returns the re-serialized `<ofd:Annotations>` XML
/// for that page, or `None` if no page resolves.
fn annotation_target(name: &str, doc: &OfdDocument) -> Option<String> {
    let seg = name.split('/').find(|s| s.starts_with("Page_"))?;
    let idx: usize = seg.trim_start_matches("Page_").parse().ok()?;
    let page = doc.pages.get(idx)?;
    let anns = doc.annotations.for_page(&page.id);
    Some(serialize_page_annotations(&page.id, anns))
}
