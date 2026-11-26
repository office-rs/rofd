use serde::{Deserialize, Serialize};

use crate::annotation::AnnotationModel;
use crate::package::PackageHandle;
use crate::page::{DocMeta, Page};
use crate::raw::RawFragment;
use crate::resource::Resources;
use crate::signature::SignatureSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
    /// GB/T 33190 CommonData/MaxUnitID: the largest ST_ID of the document.
    /// New IDs are allocated from `max_unit_id + 1`.
    pub max_unit_id: u64,
    /// Body content revision (reserved for cache invalidation). The body is
    /// read-only in v1; when pages change in the future ("backend
    /// read-modify-write", spec gate D) it must be `+= 1`, and the page-level
    /// Scene/bitmap caches go stale by it (spec 2026-10-15 §7.3). Today only
    /// the component raises it, when a signature lands (a seal is static
    /// page content).
    #[serde(default)]
    pub body_revision: u64,
    /// Unknown children of the Document.xml root (Outlines/Attachments/
    /// Actions/CustomDatas/...), position-preserved (passthrough, spec
    /// 2026-11-11 §6.2).
    #[serde(default)]
    pub raw_doc_children: Vec<RawFragment>,
    /// Unknown children of Document.xml `<CommonData>`, position-preserved.
    #[serde(default)]
    pub raw_common_children: Vec<RawFragment>,
    /// Unknown children of OFD.xml `<DocBody>` (e.g. Version), position-
    /// preserved.
    #[serde(default)]
    pub raw_ofd_body_children: Vec<RawFragment>,
    /// Read-only projection of the package's signatures (spec 2026-11-23
    /// §8.1). Filled by io at parse time; never written from the model.
    #[serde(default)]
    pub signatures: SignatureSet,
    /// The package this document came from (spec 2026-11-23 §17.11): every
    /// entry as the file holds it, except what the model itself represents.
    /// A save returns those entries byte for byte and rewrites only the
    /// annotation entries; a signature is appended here, in place. `None`
    /// for a document that has no file behind it yet (a new one, or one
    /// that was generated): it saves by the full write until it has one.
    /// Never serialized: the bytes stay with the document in memory.
    #[serde(skip)]
    pub package: Option<PackageHandle>,
}

impl OfdDocument {
    /// The highest object ID the model knows is in use. Every ID of a
    /// document shares one space, so all of them count: page objects, page
    /// IDs, the `ID` attributes inside page, layer and document passthrough
    /// fragments, resource IDs, and annotation IDs.
    ///
    /// `max_unit_id` is deliberately not consulted. On a signed document it
    /// is the reserve written into Document.xml at signing time, which can
    /// no longer change, so it is a ceiling rather than the highest ID in
    /// use (spec 2026-11-23 M14).
    pub fn highest_known_object_id(&self) -> u64 {
        use crate::object::PageObject;
        use crate::page::Page;
        fn object_id(object: &PageObject) -> &crate::ids::ObjectId {
            match object {
                PageObject::Text(o) => &o.id,
                PageObject::Image(o) => &o.id,
                PageObject::Path(o) => &o.id,
                PageObject::Composite(o) => &o.id,
            }
        }
        let numeric = |id: &str| id.parse::<u64>().unwrap_or(0);
        let mut highest = 0u64;
        let scan = |page: &Page, highest: &mut u64| {
            for layer in &page.layers {
                for object in &layer.objects {
                    *highest = (*highest).max(numeric(&object_id(object).0));
                }
                for fragment in &layer.raw_children {
                    *highest = max_id_attr(&fragment.xml, *highest);
                }
            }
            for fragment in &page.raw_children {
                *highest = max_id_attr(&fragment.xml, *highest);
            }
        };
        for page in &self.pages {
            // A page with no content object still has an ID of its own.
            highest = highest.max(numeric(&page.id.0));
            scan(page, &mut highest);
        }
        for fragment in self
            .raw_doc_children
            .iter()
            .chain(&self.raw_common_children)
        {
            highest = max_id_attr(&fragment.xml, highest);
        }
        let resources = &self.resources;
        let resource_ids = resources
            .fonts
            .keys()
            .chain(resources.font_data.keys())
            .map(|id| numeric(&id.0))
            .chain(resources.images.keys().map(|id| numeric(&id.0)))
            .chain(resources.draw_params.keys().map(|id| numeric(&id.0)));
        highest = resource_ids.fold(highest, u64::max);
        for annotation in self.annotations.by_page.values().flatten() {
            highest = highest.max(numeric(&annotation.id.0));
        }
        highest
    }
}

/// Highest `ID="<u64>"` attribute value in a raw fragment (a plain scan, no
/// XML parse; single-quoted IDs are not covered, they are rare in the wild).
fn max_id_attr(xml: &str, floor: u64) -> u64 {
    let mut highest = floor;
    let mut rest = xml;
    while let Some(i) = rest.find("ID=\"") {
        rest = &rest[i + 4..];
        match rest.find('"') {
            Some(j) => {
                if let Ok(n) = rest[..j].parse::<u64>() {
                    highest = highest.max(n);
                }
                rest = &rest[j..];
            }
            None => break,
        }
    }
    highest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::ids::ImageId;

    #[test]
    fn default_document_is_empty() {
        let doc = OfdDocument::default();
        assert!(doc.pages.is_empty());
        assert!(doc.annotations.by_page.is_empty());
        assert_eq!(doc.max_unit_id, 0);
    }

    #[test]
    fn body_revision_defaults_to_zero_and_serde_round_trips() {
        // 默认 0：载入的文档没人动过它。
        let doc = OfdDocument::default();
        assert_eq!(doc.body_revision, 0);
        // 旧数据没有该字段（serde default 向后兼容）。
        let json = serde_json::to_string(&OfdDocument::default()).unwrap();
        assert!(json.contains("body_revision"));
        let parsed: OfdDocument = serde_json::from_str(&json.replace(",\"body_revision\":0", ""))
            .unwrap_or_else(|e| panic!("missing body_revision must parse via default: {e}"));
        assert_eq!(parsed.body_revision, 0);
    }

    #[test]
    fn a_new_document_has_no_package_and_serde_leaves_it_out() {
        let doc = OfdDocument::default();
        assert!(doc.package.is_none());
        let json = serde_json::to_string(&doc).unwrap();
        assert!(!json.contains("package"));
        let back: OfdDocument = serde_json::from_str(&json).unwrap();
        assert!(back.package.is_none());
    }

    #[test]
    fn clone_shares_media_via_arc() {
        let mut doc = OfdDocument::default();
        let bytes = Arc::new(vec![1u8, 2, 3]);
        doc.resources
            .images
            .insert(ImageId::new("I1"), bytes.clone());
        let cloned = doc.clone();
        let a = doc.resources.images.get(&ImageId::new("I1")).unwrap();
        let b = cloned.resources.images.get(&ImageId::new("I1")).unwrap();
        assert!(Arc::ptr_eq(a, b), "clone must share Arc, not copy bytes");
    }

    fn note(id: u64) -> crate::Annotation {
        use crate::{AnnotationKind, AnnotationPayload, NoteIcon, PageId, Rect};
        crate::Annotation {
            id: crate::AnnotationId::from_int(id),
            kind: AnnotationKind::Note,
            page: PageId::new("7"),
            creator: "tester".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect::default(),
                color: crate::Color::Rgb(0, 0, 0),
                content: "note".into(),
                icon: NoteIcon::Note,
            },
        }
    }

    fn page_with_object(object_id: &str, raw_children: Vec<crate::RawFragment>) -> crate::Page {
        use crate::{Layer, PageId, PathObject};
        let path = PathObject {
            id: crate::ObjectId::new(object_id),
            ..PathObject::default()
        };
        crate::Page {
            id: PageId::new("7"),
            layers: vec![Layer {
                objects: vec![crate::PageObject::Path(path)],
                ..Layer::default()
            }],
            raw_children,
            ..crate::Page::default()
        }
    }

    #[test]
    fn highest_known_object_id_ignores_max_unit_id() {
        let mut doc = OfdDocument {
            max_unit_id: 65_545,
            ..OfdDocument::default()
        };
        doc.pages.push(page_with_object("9", Vec::new()));
        assert_eq!(doc.highest_known_object_id(), 9, "page object ids count");
        doc.annotations.insert(note(12));
        assert_eq!(
            doc.highest_known_object_id(),
            12,
            "the highest id in use, not the MaxUnitID ceiling"
        );
        doc.max_unit_id = 3;
        assert_eq!(
            doc.highest_known_object_id(),
            12,
            "max_unit_id is not consulted"
        );
    }

    #[test]
    fn highest_known_object_id_scans_raw_fragment_ids() {
        let raw = |xml: &str| crate::RawFragment {
            position: 0,
            xml: xml.to_owned(),
        };
        let mut doc = OfdDocument::default();
        doc.pages.push(page_with_object(
            "5",
            vec![raw("<ext:Block ID=\"77\"/>"), raw("<ext:Other ID=\"70\"/>")],
        ));
        doc.pages[0].layers[0].raw_children = vec![raw("<ext:Stamp ID=\"80\"/>")];
        assert_eq!(
            doc.highest_known_object_id(),
            80,
            "page and layer raws are scanned"
        );
    }

    #[test]
    fn highest_known_object_id_is_zero_for_an_empty_document() {
        let doc = OfdDocument {
            max_unit_id: 65_545,
            ..OfdDocument::default()
        };
        assert_eq!(doc.highest_known_object_id(), 0);
    }

    #[test]
    fn highest_known_object_id_counts_pages_document_children_and_resources() {
        // Every ID of a document shares one space, not only the page
        // objects': a page, a resource and a child of Document.xml have IDs
        // too. On a signed document new annotations are numbered above the
        // highest ID in use, so a number missed here is a number two
        // objects share (final review).
        let raw = |xml: &str| crate::RawFragment {
            position: 0,
            xml: xml.to_owned(),
        };
        let mut doc = OfdDocument::default();
        doc.pages.push(crate::Page {
            id: crate::PageId::new("50"),
            ..crate::Page::default()
        });
        assert_eq!(doc.highest_known_object_id(), 50, "a page has an ID");
        doc.raw_doc_children
            .push(raw("<ofd:Actions><ofd:Action ID=\"60\"/></ofd:Actions>"));
        doc.raw_common_children
            .push(raw("<ofd:Template ID=\"61\"/>"));
        assert_eq!(
            doc.highest_known_object_id(),
            61,
            "children of Document.xml"
        );
        doc.resources
            .images
            .insert(ImageId::new("70"), Arc::new(Vec::new()));
        doc.resources
            .fonts
            .insert(crate::FontId::new("71"), crate::FontRef::default());
        doc.resources
            .draw_params
            .insert(crate::DrawParamId::new("72"), crate::DrawParam::default());
        assert_eq!(doc.highest_known_object_id(), 72, "resources");
        // IDs that are not numbers do not count, as before.
        doc.pages.push(crate::Page {
            id: crate::PageId::new("cover"),
            ..crate::Page::default()
        });
        doc.resources
            .fonts
            .insert(crate::FontId::new("title"), crate::FontRef::default());
        assert_eq!(doc.highest_known_object_id(), 72);
    }
}
