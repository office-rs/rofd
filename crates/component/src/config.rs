use rofd_render::{MAX_ZOOM, MIN_ZOOM, PX_PER_MM};
use std::sync::Arc;

use crate::zoom_mode::ZoomMode;

#[derive(Clone)]
pub struct OfdConfig {
    pub default_font_bytes: Arc<Vec<u8>>,
    pub page_gap: f64,
    /// Initial viewport zoom (viewport px per OFD mm). Defaults to
    /// [`PX_PER_MM`]; clamped into the supported range.
    pub zoom: f64,
    pub zoom_mode: ZoomMode,
}

impl OfdConfig {
    pub fn new(default_font_bytes: Arc<Vec<u8>>) -> Self {
        Self {
            default_font_bytes,
            page_gap: 20.0,
            zoom: PX_PER_MM,
            zoom_mode: ZoomMode::default(),
        }
    }

    /// Set the initial viewport zoom (builder chainer). Non-finite input
    /// falls back to [`PX_PER_MM`]; otherwise clamped to the supported range.
    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.zoom = if zoom.is_finite() {
            zoom.clamp(MIN_ZOOM, MAX_ZOOM)
        } else {
            PX_PER_MM
        };
        self
    }

    /// Set the initial zoom mode (builder chainer). Fit modes apply as soon
    /// as the viewport is laid out; until then the explicit `zoom` (or
    /// `PX_PER_MM`) is the pending initial value. Once active this takes
    /// precedence over [`Self::with_zoom`] (spec D5).
    pub fn with_zoom_mode(mut self, mode: ZoomMode) -> Self {
        self.zoom_mode = mode;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_zoom_to_baseline() {
        let c = OfdConfig::new(Arc::new(vec![]));
        assert_eq!(c.zoom, PX_PER_MM);
        assert_eq!(c.page_gap, 20.0);
    }

    #[test]
    fn with_zoom_sets_and_clamps() {
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom(PX_PER_MM * 2.0);
        assert_eq!(c.zoom, PX_PER_MM * 2.0);
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom(f64::MAX);
        assert_eq!(c.zoom, MAX_ZOOM);
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom(0.0);
        assert_eq!(c.zoom, MIN_ZOOM);
    }

    #[test]
    fn with_zoom_non_finite_falls_back_to_baseline() {
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom(f64::NAN);
        assert_eq!(c.zoom, PX_PER_MM);
    }

    #[test]
    fn with_zoom_mode_sets_initial_mode() {
        let c = OfdConfig::new(Arc::new(vec![])).with_zoom_mode(ZoomMode::FitWidth);
        assert_eq!(c.zoom_mode, ZoomMode::FitWidth);
        assert_eq!(OfdConfig::new(Arc::new(vec![])).zoom_mode, ZoomMode::Custom);
    }
}
