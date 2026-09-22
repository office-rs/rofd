use rofd_render::{MAX_ZOOM, MIN_ZOOM, PX_PER_MM};
use std::sync::Arc;

#[derive(Clone)]
pub struct EditorConfig {
    pub default_font_bytes: Arc<Vec<u8>>,
    pub page_gap: f64,
    /// Initial viewport zoom (viewport px per OFD mm). Defaults to
    /// [`PX_PER_MM`]; clamped into the supported range.
    pub zoom: f64,
}

impl EditorConfig {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self {
            default_font_bytes,
            page_gap: 20.0,
            zoom: PX_PER_MM,
        }
    }

    /// Set the initial viewport zoom (builder chainer). Clamped to
    /// `[MIN_ZOOM, MAX_ZOOM]`.
    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_zoom_to_baseline() {
        let c = EditorConfig::new(Arc::new(vec![]));
        assert_eq!(c.zoom, PX_PER_MM);
        assert_eq!(c.page_gap, 20.0);
    }

    #[test]
    fn with_zoom_sets_and_clamps() {
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(PX_PER_MM * 2.0);
        assert_eq!(c.zoom, PX_PER_MM * 2.0);
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(f64::MAX);
        assert_eq!(c.zoom, MAX_ZOOM);
        let c = EditorConfig::new(Arc::new(vec![])).with_zoom(0.0);
        assert_eq!(c.zoom, MIN_ZOOM);
    }
}
