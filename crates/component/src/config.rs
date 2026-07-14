use std::sync::Arc;

#[derive(Clone)]
pub struct EditorConfig {
    pub default_font_bytes: Arc<Vec<u8>>,
    pub page_gap: f64,
}

impl EditorConfig {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self {
            default_font_bytes,
            page_gap: 20.0,
        }
    }
}
