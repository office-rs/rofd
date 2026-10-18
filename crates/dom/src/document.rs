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
    /// Body 内容修订号（缓存失效预留）。v1 body 只读（AGENTS §4.2），
    /// 恒为 0；将来"后端读改写"（spec D 门）修改 pages 时必须 `+= 1`，
    /// 页级 Scene/位图缓存按此自动失效（spec 2026-10-15 §7.3）。
    #[serde(default)]
    pub body_revision: u64,
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
        // 默认 0：v1 body 只读，永远不该有人 bump 它。
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
}
