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
