use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ids::{DrawParamId, FontId, ImageId};
use crate::primitives::Color;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FontRef {
    pub id: FontId,
    pub family_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ImageRef {
    pub id: ImageId,
    /// "png" | "jpg" (v1 common subset).
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DrawParam {
    pub line_width: Option<f64>,
    pub stroke: Option<Color>,
    pub fill: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Resources {
    pub fonts: HashMap<FontId, FontRef>,
    pub images: HashMap<ImageId, Arc<Vec<u8>>>,
    pub draw_params: HashMap<DrawParamId, DrawParam>,
}
