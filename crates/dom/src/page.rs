use serde::{Deserialize, Serialize};

use crate::ids::PageId;
use crate::object::PageObject;
use crate::primitives::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayerType {
    #[default]
    Body,
    Foreground,
    Background,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Layer {
    pub layer_type: LayerType,
    pub objects: Vec<PageObject>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub physical_box: Rect,
    pub layers: Vec<Layer>,
    /// v1: stored raw; not expanded. Render skips with a warning.
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TemplateRef {
    pub page_id: PageId,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocMeta {
    pub doc_id: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
}
