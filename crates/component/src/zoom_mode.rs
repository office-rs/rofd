//! Viewport zoom mode (适应宽度/高度/页面 state machine carrier).

/// How the viewport zoom is currently governed.
///
/// `Custom` is ordinary multiplicative zoom (ctrl+wheel, zoom buttons,
/// percent presets). The three fit modes re-derive their zoom whenever the
/// viewport size or document changes; any manual zoom exits to `Custom`
/// (spec 2026-10-10-fit-zoom-modes §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Custom,
    /// 适应宽度: the reference page's width fills the content region.
    FitWidth,
    /// 适应高度: the reference page's height fills the content region.
    FitHeight,
    /// 适应页面: the whole reference page is visible.
    FitPage,
}

impl ZoomMode {
    /// The mode's fit axis target, or `None` for `Custom`.
    pub fn fit_target(self) -> Option<rofd_render::FitTarget> {
        match self {
            ZoomMode::Custom => None,
            ZoomMode::FitWidth => Some(rofd_render::FitTarget::Width),
            ZoomMode::FitHeight => Some(rofd_render::FitTarget::Height),
            ZoomMode::FitPage => Some(rofd_render::FitTarget::Page),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_target_maps_modes() {
        assert_eq!(ZoomMode::Custom.fit_target(), None);
        assert_eq!(
            ZoomMode::FitWidth.fit_target(),
            Some(rofd_render::FitTarget::Width)
        );
        assert_eq!(
            ZoomMode::FitHeight.fit_target(),
            Some(rofd_render::FitTarget::Height)
        );
        assert_eq!(
            ZoomMode::FitPage.fit_target(),
            Some(rofd_render::FitTarget::Page)
        );
    }
}
