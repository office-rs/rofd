use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationModel {
    pub by_page: std::collections::HashMap<crate::ids::PageId, Vec<crate::ids::AnnotationId>>,
}
