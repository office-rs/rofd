use rofd_dom::OfdDocument;

use crate::error::OfdError;
use crate::serialize::annotation::serialize_page_annotations;
use crate::zip_util::write_zip;

/// Full write: construct a fresh .ofd package from the model (generation / conversion).
pub fn write_ofd(doc: &OfdDocument) -> Result<Vec<u8>, OfdError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\n  <ofd:DocBody>\n    \
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\n  </ofd:DocBody>\n</ofd:OFD>";
    entries.push(("OFD.xml".into(), ofd_xml.as_bytes().to_vec()));

    let mut doc_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    doc_xml.push_str("<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
    let pb = doc.pages.first().map(|p| p.physical_box);
    if let Some(r) = pb {
        doc_xml.push_str(&format!(
            "  <ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData>\n",
            r.x, r.y, r.w, r.h
        ));
    }
    doc_xml.push_str("  <ofd:Pages>\n");
    for (i, page) in doc.pages.iter().enumerate() {
        doc_xml.push_str(&format!(
            "    <ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{i}/Page.xml\"/>\n",
            page.id.0
        ));
    }
    doc_xml.push_str("  </ofd:Pages>\n</ofd:Document>");
    entries.push(("Doc_0/Document.xml".into(), doc_xml.into_bytes()));

    for (i, page) in doc.pages.iter().enumerate() {
        let mut page_xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        page_xml.push_str("<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\n");
        page_xml.push_str(&format!(
            "  <ofd:Area><ofd:PhysicalBox>{} {} {} {}</ofd:PhysicalBox></ofd:Area>\n",
            page.physical_box.x, page.physical_box.y, page.physical_box.w, page.physical_box.h
        ));
        page_xml.push_str("  <ofd:Content>\n");
        for layer in &page.layers {
            let ty = match layer.layer_type {
                rofd_dom::LayerType::Body => "Body",
                rofd_dom::LayerType::Foreground => "Foreground",
                rofd_dom::LayerType::Background => "Background",
            };
            page_xml.push_str(&format!("    <ofd:Layer Type=\"{ty}\"/>\n"));
            // v1 full-write emits object skeleton only; full object serialization
            // is added as the render phase needs it. Body byte-fidelity is not
            // required for write_ofd (generation path).
        }
        page_xml.push_str("  </ofd:Content>\n");
        let anns = doc.annotations.for_page(&page.id);
        if !anns.is_empty() {
            page_xml.push_str(&format!("  <ofd:Annotation><ofd:File Loc=\"Page_{i}/Annotation.xml\"/></ofd:Annotation>\n"));
            let xml = serialize_page_annotations(&page.id, anns);
            entries.push((format!("Doc_0/Pages/Page_{i}/Annotation.xml"), xml.into_bytes()));
        }
        page_xml.push_str("</ofd:Page>");
        entries.push((format!("Doc_0/Pages/Page_{i}/Page.xml"), page_xml.into_bytes()));
    }

    write_zip(&entries)
}
