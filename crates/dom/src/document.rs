use serde::{Deserialize, Serialize};

use crate::annotation::AnnotationModel;
use crate::page::{DocMeta, Page};
use crate::resource::Resources;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfdDocument {
    pub meta: DocMeta,
    pub pages: Vec<Page>,
    pub resources: Resources,
    pub annotations: AnnotationModel,
    /// GB/T 33190 CommonData/MaxUnitID: 文档内最大 ST_ID。新 ID 从 max_unit_id+1 分配。
    pub max_unit_id: u64,
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
    fn clone_shares_media_via_arc() {
        let mut doc = OfdDocument::default();
        let bytes = Arc::new(vec![1u8, 2, 3]);
        doc.resources.images.insert(ImageId::new("I1"), bytes.clone());
        let cloned = doc.clone();
        let a = doc.resources.images.get(&ImageId::new("I1")).unwrap();
        let b = cloned.resources.images.get(&ImageId::new("I1")).unwrap();
        assert!(Arc::ptr_eq(a, b), "clone must share Arc, not copy bytes");
    }
}
