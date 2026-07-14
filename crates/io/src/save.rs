use rofd_dom::{OfdDocument, PageId};

use crate::error::OfdError;
use crate::package::{EntryKind, PackageHandle};
use crate::serialize::annotation::serialize_page_annot;
use crate::serialize::annotation_entry::serialize_annotations_entry;
use crate::zip_util::write_zip;

/// Surgical save (invariant §4.3): the dirty set is -
/// - annotation entry file (`Annots/Annotations.xml`) and per-page files
///   (`Annots/Page_N/Annotation.xml`): re-serialized from `AnnotationModel`;
/// - `Document.xml`: `<MaxUnitID>` byte-patched to `doc.max_unit_id`;
/// - everything else (body `Content.xml`, resources, signatures, `OFD.xml`):
///   copied byte-identical from the retained `PackageHandle`.
///
/// If the model has annotations on a page whose package had no annotation
/// file (rofd added the first annotation to a previously bare page),
/// `ensure_annotation_entries` adds the missing entry + per-page files.
pub fn save_ofd(doc: &OfdDocument, pkg: &PackageHandle) -> Result<Vec<u8>, OfdError> {
    // Pages with annotations, in document order: (page_id, page_index).
    let pages_with_ann: Vec<(PageId, usize)> = doc
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| !doc.annotations.for_page(&p.id).is_empty())
        .map(|(i, p)| (p.id.clone(), i))
        .collect();

    let mut out: Vec<(String, Vec<u8>)> =
        Vec::with_capacity(pkg.entries.len() + pages_with_ann.len() + 1);

    for entry in &pkg.entries {
        match entry.kind {
            EntryKind::Annotation => {
                if is_annotation_entry(&entry.name) {
                    // Entry file `Annots/Annotations.xml` (not per-page `Annotation.xml`).
                    let xml = serialize_annotations_entry(&pages_with_ann);
                    out.push((entry.name.clone(), xml.into_bytes()));
                } else if let Some(idx) = page_index_from_name(&entry.name) {
                    // Per-page annotation file `Annots/Page_N/Annotation.xml`.
                    if let Some(page) = doc.pages.get(idx) {
                        let anns = doc.annotations.for_page(&page.id);
                        let xml = serialize_page_annot(&page.id, anns);
                        out.push((entry.name.clone(), xml.into_bytes()));
                    } else {
                        // Page index out of range - preserve original bytes.
                        out.push((entry.name.clone(), (*entry.bytes).clone()));
                    }
                } else {
                    // Unrecognized annotation entry - preserve original bytes.
                    out.push((entry.name.clone(), (*entry.bytes).clone()));
                }
            }
            EntryKind::Body if entry.name.ends_with("Document.xml") => {
                // Document.xml: byte-patch <MaxUnitID> only.
                let patched = match std::str::from_utf8(&entry.bytes) {
                    Ok(xml) => patch_max_unit_id(xml, doc.max_unit_id).into_bytes(),
                    Err(_) => {
                        // Non-UTF-8 Document.xml cannot be byte-patched; copy as-is
                        // (degraded, but never crash - the body is still preserved).
                        (*entry.bytes).clone()
                    }
                };
                out.push((entry.name.clone(), patched));
            }
            _ => {
                // Body Content.xml / resources / signatures / Other - byte-identical.
                out.push((entry.name.clone(), (*entry.bytes).clone()));
            }
        }
    }

    // Add entry + per-page files for pages that have annotations but no
    // corresponding package entry (rofd added the first annotation to a bare page).
    ensure_annotation_entries(&mut out, doc, &pages_with_ann, pkg);

    write_zip(&out)
}

/// True if `name` is the annotation entry file (`Annots/Annotations.xml`),
/// i.e. ends with `Annotations.xml` but NOT `Annotation.xml` (the per-page file).
fn is_annotation_entry(name: &str) -> bool {
    name.ends_with("Annotations.xml") && !name.ends_with("Annotation.xml")
}

/// Extract the `Page_<n>` index from an entry name like
/// `Doc_0/Annots/Page_0/Annotation.xml`. Returns `None` if no `Page_<n>` segment.
fn page_index_from_name(name: &str) -> Option<usize> {
    name.split('/').find_map(|seg| {
        seg.strip_prefix("Page_")
            .and_then(|n| n.parse::<usize>().ok())
    })
}

/// Byte-patch the `<...MaxUnitID>N</...MaxUnitID>` text to `new_val`.
///
/// Finds the first `MaxUnitID>` (end of the start tag, covering both
/// `<ofd:MaxUnitID>` and `<MaxUnitID>`), replaces the text up to the next `<`
/// (start of the end tag) with `new_val`. If the `MaxUnitID` element is
/// absent, the xml is returned unchanged (v1 does not insert the element).
fn patch_max_unit_id(xml: &str, new_val: u64) -> String {
    const KEY: &str = "MaxUnitID>";
    let Some(start) = xml.find(KEY) else {
        return xml.to_string();
    };
    let text_start = start + KEY.len();
    let Some(end_rel) = xml[text_start..].find('<') else {
        return xml.to_string();
    };
    let text_end = text_start + end_rel;
    let mut out = String::with_capacity(xml.len() + 8);
    out.push_str(&xml[..text_start]);
    out.push_str(&new_val.to_string());
    out.push_str(&xml[text_end..]);
    out
}

/// If the model has annotations but the package has no corresponding annotation
/// entry/per-page file (rofd added the first annotation to a previously bare page),
/// add the standard entry file `Doc_0/Annots/Annotations.xml` and per-page files
/// `Doc_0/Annots/Page_{idx}/Annotation.xml`. The doc root (e.g. `Doc_0`) is
/// inferred from the `Document.xml` entry's directory.
fn ensure_annotation_entries(
    out: &mut Vec<(String, Vec<u8>)>,
    doc: &OfdDocument,
    pages_with_ann: &[(PageId, usize)],
    pkg: &PackageHandle,
) {
    if pages_with_ann.is_empty() {
        return;
    }
    // Infer doc_root (e.g. "Doc_0") from the Document.xml entry's directory.
    let doc_root = pkg
        .entries
        .iter()
        .find(|e| e.name.ends_with("Document.xml"))
        .and_then(|e| e.name.rsplit_once('/').map(|(d, _)| d.to_string()))
        .unwrap_or_else(|| "Doc_0".into());

    let entry_name = format!("{doc_root}/Annots/Annotations.xml");
    if !out.iter().any(|(n, _)| n == &entry_name) {
        let xml = serialize_annotations_entry(pages_with_ann);
        out.push((entry_name, xml.into_bytes()));
    }

    for (pid, idx) in pages_with_ann {
        let name = format!("{doc_root}/Annots/Page_{idx}/Annotation.xml");
        if !out.iter().any(|(n, _)| n == &name) {
            let anns = doc.annotations.for_page(pid);
            let xml = serialize_page_annot(pid, anns);
            out.push((name, xml.into_bytes()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_annotation_entry_distinguishes_entry_from_per_page() {
        assert!(is_annotation_entry("Doc_0/Annots/Annotations.xml"));
        assert!(!is_annotation_entry("Doc_0/Annots/Page_0/Annotation.xml"));
        assert!(!is_annotation_entry("Doc_0/Annots/Page_12/Annotation.xml"));
    }

    #[test]
    fn page_index_from_name_extracts_index() {
        assert_eq!(
            page_index_from_name("Doc_0/Annots/Page_0/Annotation.xml"),
            Some(0)
        );
        assert_eq!(
            page_index_from_name("Doc_0/Annots/Page_7/Annotation.xml"),
            Some(7)
        );
        assert_eq!(page_index_from_name("Doc_0/Document.xml"), None);
    }

    #[test]
    fn patch_max_unit_id_replaces_value() {
        let xml = "<ofd:MaxUnitID>101</ofd:MaxUnitID>";
        let patched = patch_max_unit_id(xml, 999);
        assert_eq!(patched, "<ofd:MaxUnitID>999</ofd:MaxUnitID>");
    }

    #[test]
    fn patch_max_unit_id_preserves_surrounding_xml() {
        let xml = "<?xml version=\"1.0\"?>\n<ofd:Document><ofd:CommonData><ofd:MaxUnitID>5</ofd:MaxUnitID></ofd:CommonData></ofd:Document>";
        let patched = patch_max_unit_id(xml, 42);
        assert!(patched.contains("<ofd:MaxUnitID>42</ofd:MaxUnitID>"));
        assert!(patched.starts_with("<?xml version=\"1.0\"?>"));
        assert!(patched.ends_with("</ofd:Document>"));
    }

    #[test]
    fn patch_max_unit_id_absent_returns_unchanged() {
        let xml = "<ofd:Document><ofd:Pages/></ofd:Document>";
        let patched = patch_max_unit_id(xml, 42);
        assert_eq!(patched, xml);
    }

    #[test]
    fn patch_max_unit_id_works_without_namespace_prefix() {
        // GB/T 33190 allows <MaxUnitID> without the ofd: prefix.
        let xml = "<MaxUnitID>1</MaxUnitID>";
        let patched = patch_max_unit_id(xml, 100);
        assert_eq!(patched, "<MaxUnitID>100</MaxUnitID>");
    }
}
