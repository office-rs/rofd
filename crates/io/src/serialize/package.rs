use std::collections::HashMap;

use rofd_dom::{DocMeta, OfdDocument, TextLayoutHints};

use crate::error::OfdError;
use crate::serialize::annotation::{object_id_seed, serialize_page_annot, xml_escape};
use crate::serialize::annotation_entry::serialize_annotations_entry;
use crate::serialize::body;
use crate::serialize::resource;
use crate::serialize::{interleave, Child};
use crate::zip_util::write_zip;
use rofd_dom::RawFragment;

/// Synthesize OFD.xml: DocBody with an optional DocInfo (GB/T 33190 places
/// CT_DocInfo on the OFD.xml DocBody face - the parse side reads it from
/// there, `parse/ofd_xml.rs`) followed by DocRoot, with passthrough
/// fragments interleaved at their captured positions (spec 2026-11-11 D3).
/// DocInfo is emitted when a modelled field is set OR a captured DocInfo
/// fragment exists; field order is fixed for determinism. Body raws'
/// positions count DocRoot as the only known child (capture-side
/// convention), so DocInfo is emitted before the merged sequence.
fn ofd_xml(meta: &DocMeta, body_raws: &[RawFragment]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    s.push_str("  <ofd:DocBody>\n");
    let fields: Vec<(&str, &String)> = [
        ("DocID", &meta.doc_id),
        ("Title", &meta.title),
        ("Author", &meta.author),
        ("CreationDate", &meta.creation_date),
        ("LastModDate", &meta.mod_date),
    ]
    .into_iter()
    .filter_map(|(tag, v)| v.as_ref().map(|v| (tag, v)))
    .collect();
    if !fields.is_empty() || !meta.raw_info_children.is_empty() {
        s.push_str("    <ofd:DocInfo>\n");
        for child in interleave(fields.len(), &meta.raw_info_children) {
            match child {
                Child::Known(i) => {
                    let (tag, v) = fields[i];
                    s.push_str(&format!("      <ofd:{tag}>{}</ofd:{tag}>\n", xml_escape(v)));
                }
                Child::Raw(f) => s.push_str(&format!("      {}\n", f.xml)),
            }
        }
        s.push_str("    </ofd:DocInfo>\n");
    }
    for child in interleave(1, body_raws) {
        match child {
            Child::Known(_) => {
                s.push_str("    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\n");
            }
            Child::Raw(f) => s.push_str(&format!("    {}\n", f.xml)),
        }
    }
    s.push_str("  </ofd:DocBody>\n");
    s.push_str("</ofd:OFD>");
    s
}

/// Full write: construct a fresh .ofd package from the model (generation /
/// conversion / save). Emits GB/T 33190 standard structure under canonical
/// entry names, then transports the original package's UNCONSUMED entries
/// verbatim (passthrough, spec 2026-11-11 D12): `pkg.consumed` records what
/// parse read into the model, so anything outside it - attachments,
/// signatures, unknown files - copies through byte-identical under its
/// original name. An unconsumed entry whose name collides with an emitted
/// canonical name is dropped (emitted wins; malformed-input boundary - the
/// write path has no warning channel).
///
/// Emits:
/// - `OFD.xml` (DocInfo + DocRoot + passthrough) -> `Doc_0/Document.xml`
/// - `Doc_0/Document.xml` with `<CommonData>` (PageArea + MaxUnitID +
///   DocumentRes loc + passthrough), `<Pages>`, `<Annotations>` loc
/// - `Doc_0/Res/DocumentRes.xml` + font/image byte files (when resources exist)
/// - `Doc_0/Pages/Page_N/Content.xml` per page
/// - `Doc_0/Annots/Annotations.xml` entry file (only if any page has annotations)
/// - `Doc_0/Annots/Page_N/Annotation.xml` per annotated page
///
/// `text_layout` carries the wrapped annotation-text line geometry computed
/// by the component (see the editor's save_snapshot); pass an empty
/// map for the legacy content-`\n` split.
pub fn write_ofd_with_text_layout(
    doc: &OfdDocument,
    pkg: Option<&crate::package::PackageHandle>,
    text_layout: &TextLayoutHints,
) -> Result<Vec<u8>, OfdError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    entries.push((
        "OFD.xml".into(),
        ofd_xml(&doc.meta, &doc.raw_ofd_body_children).into_bytes(),
    ));

    // Pages with annotations (doc order): (page_id, page_index).
    let pages_with_ann: Vec<(rofd_dom::PageId, usize)> = doc
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| !doc.annotations.for_page(&p.id).is_empty())
        .map(|(i, p)| (p.id.clone(), i))
        .collect();
    let has_annots = !pages_with_ann.is_empty();

    // Per-page annotation XML is serialized up front with one document-wide
    // ID allocator (ST_ID: object IDs are unique unsigned integers across the
    // whole file), so <MaxUnitID> below can be written from the final counter.
    let mut next_id = object_id_seed(doc);
    let mut page_annot_xml: HashMap<usize, String> = HashMap::new();
    for (i, page) in doc.pages.iter().enumerate() {
        let anns = doc.annotations.for_page(&page.id);
        if !anns.is_empty() {
            page_annot_xml.insert(
                i,
                serialize_page_annot(&page.id, anns, &mut next_id, text_layout),
            );
        }
    }

    // Document.xml. Known root children (CommonData / Pages / Annotations)
    // interleave with captured raw fragments at their original positions
    // (spec 2026-11-11 D3); inside CommonData, PageArea / MaxUnitID / the
    // canonical DocumentRes loc interleave with `raw_common_children`.
    let res = resource::res_entries(doc);
    let mut doc_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc_xml.push_str("<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    let pb = doc.pages.first().map(|p| p.physical_box);
    let mut root_known: Vec<String> = Vec::new();
    if let Some(r) = pb {
        let mut common_known: Vec<String> = vec![
            format!(
                "<ofd:PageArea><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:PageArea>",
                r.x, r.y, r.w, r.h
            ),
            format!("<ofd:MaxUnitID>{next_id}</ofd:MaxUnitID>"),
        ];
        if !res.is_empty() {
            common_known.push("<ofd:DocumentRes BaseLoc=\"Res/DocumentRes.xml\"/>".into());
        }
        let mut common_inner = String::new();
        for child in interleave(common_known.len(), &doc.raw_common_children) {
            match child {
                Child::Known(i) => common_inner.push_str(&common_known[i]),
                Child::Raw(f) => common_inner.push_str(&f.xml),
            }
        }
        root_known.push(format!(
            "  <ofd:CommonData>{common_inner}</ofd:CommonData>\n"
        ));
    }
    let mut pages_block = String::from("  <ofd:Pages>\n");
    for (i, page) in doc.pages.iter().enumerate() {
        pages_block.push_str(&format!(
            "    <ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{i}/Content.xml\"/>\n",
            xml_escape(&page.id.0)
        ));
    }
    pages_block.push_str("  </ofd:Pages>\n");
    root_known.push(pages_block);
    if has_annots {
        root_known.push("  <ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>\n".into());
    }
    for child in interleave(root_known.len(), &doc.raw_doc_children) {
        match child {
            Child::Known(i) => doc_xml.push_str(&root_known[i]),
            Child::Raw(f) => doc_xml.push_str(&format!("  {}\n", f.xml)),
        }
    }
    doc_xml.push_str("</ofd:Document>");
    entries.push(("Doc_0/Document.xml".into(), doc_xml.into_bytes()));

    // Resource entries (fonts / images / drawparams) + their byte files.
    entries.extend(res);

    // Annotations.xml entry file (GB/T 33190 §15.1).
    if has_annots {
        let xml = serialize_annotations_entry(&pages_with_ann);
        entries.push(("Doc_0/Annots/Annotations.xml".into(), xml.into_bytes()));
    }

    // Per-page Content.xml + Annotation.xml (§15.2). GB/T CT_Page orders
    // Template before Content; page-root raw fragments follow in capture
    // order (position approximation, content preserved).
    for (i, page) in doc.pages.iter().enumerate() {
        let mut page_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        page_xml.push_str("<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
        page_xml.push_str(&format!(
            "  <ofd:Area><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:Area>\n",
            page.physical_box.x, page.physical_box.y, page.physical_box.w, page.physical_box.h
        ));
        if let Some(loc) = &page.template {
            page_xml.push_str(&format!(
                "  <ofd:Template TemplateLoc=\"{}\"/>\n",
                xml_escape(loc)
            ));
        }
        page_xml.push_str("  <ofd:Content>\n");
        for layer in &page.layers {
            page_xml.push_str(&body::layer_xml(layer));
        }
        page_xml.push_str("  </ofd:Content>\n");
        for frag in &page.raw_children {
            page_xml.push_str(&format!("  {}\n", frag.xml));
        }
        page_xml.push_str("</ofd:Page>");
        entries.push((
            format!("Doc_0/Pages/Page_{i}/Content.xml"),
            page_xml.into_bytes(),
        ));

        if let Some(xml) = page_annot_xml.get(&i) {
            entries.push((
                format!("Doc_0/Annots/Page_{i}/Annotation.xml"),
                xml.clone().into_bytes(),
            ));
        }
    }

    // Package-layer passthrough (spec D12): unconsumed entries of the
    // original package copy through verbatim under their original names;
    // entries whose names collide with emitted canonical names lose.
    if let Some(pkg) = pkg {
        let emitted: std::collections::BTreeSet<String> =
            entries.iter().map(|(n, _)| n.clone()).collect();
        for e in &pkg.entries {
            if !pkg.consumed.contains(e.name.as_str()) && !emitted.contains(&e.name) {
                entries.push((e.name.clone(), e.bytes.as_ref().clone()));
            }
        }
    }

    write_zip(&entries)
}

/// Full write without a source package (pure generation) and without
/// wrapped-text line hints (legacy entry point): the appearance splits
/// content on explicit `\n` only.
pub fn write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError> {
    write_ofd_with_text_layout(doc, None, &TextLayoutHints::new())
}
