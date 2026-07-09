//! Viewport: scroll/zoom/size/page-gap describing the desk surface a
//! [`RenderEngine`](crate::RenderEngine) composites into.
//!
//! The viewport is the "desk" the paper pages sit on. `scroll` is the desk
//! offset (in device pixels), `zoom` scales page-local coordinates, `size` is
//! the viewport rectangle in device pixels, and `page_gap` is the vertical
//! spacing between stacked pages.

/// Paper-on-desk viewport state. All fields are in device pixels.
///
/// - `scroll`: (x, y) desk offset to apply (added to page positions; positive y
///   scrolls pages downward).
/// - `zoom`: uniform scale applied to every page's physical box.
/// - `size`: (width, height) of the viewport in device pixels - the gray desk
///   background fills this rectangle.
/// - `page_gap`: vertical gap between consecutive pages, in device pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct Viewport {
    pub scroll: (f64, f64),
    pub zoom: f64,
    pub size: (f64, f64),
    pub page_gap: f64,
}
