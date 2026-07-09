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
    /// Raw font bytes keyed by FontId. Empty when the package has no FontFile;
    /// renderers fall back to a registered default font.
    pub font_data: HashMap<FontId, Arc<Vec<u8>>>,
    pub draw_params: HashMap<DrawParamId, DrawParam>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn resources_default_has_empty_font_data() {
        let r = Resources::default();
        assert!(r.font_data.is_empty());
    }

    #[test]
    fn font_data_clone_shares_arc() {
        let mut r = Resources::default();
        let bytes = Arc::new(vec![0u8, 1, 2]);
        r.font_data.insert(FontId::new("F1"), bytes.clone());
        let cloned = r.clone();
        assert!(Arc::ptr_eq(r.font_data.get(&FontId::new("F1")).unwrap(), cloned.font_data.get(&FontId::new("F1")).unwrap()));
    }
}
