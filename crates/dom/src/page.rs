use serde::{Deserialize, Serialize};

use crate::ids::PageId;
use crate::object::PageObject;
use crate::primitives::Rect;
use crate::raw::RawFragment;

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
    /// Unknown children of the Layer element, position-preserved
    /// (passthrough, spec 2026-11-11 D3).
    #[serde(default)]
    pub raw_children: Vec<RawFragment>,
    /// Unmodelled attributes on the Layer element, verbatim (e.g. ID).
    #[serde(default)]
    pub raw_attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub physical_box: Rect,
    pub layers: Vec<Layer>,
    /// v1: stored raw; not expanded. Render skips with a warning.
    pub template: Option<String>,
    /// Unknown children of the Page root element, position-preserved
    /// (passthrough, spec 2026-11-11 D3).
    #[serde(default)]
    pub raw_children: Vec<RawFragment>,
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
    /// Unmodelled DocInfo fields (Subject/Keywords/Creator/CustomDatas/...),
    /// position-preserved (passthrough, spec 2026-11-11 §6.2).
    #[serde(default)]
    pub raw_info_children: Vec<RawFragment>,
}
