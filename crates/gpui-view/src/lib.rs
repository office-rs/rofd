//! rofd-gpui-view — GPUI adapter for the rofd viewer/annotator component.
//!
//! Kernel-agnostic halves (font cache, paint sink) land first; the widget
//! surface (events, handle, view) follows per the gpui line plan
//! (docs/superpowers/specs/2026-10-31-gpui-view-gpui-app-design.md).

pub mod events;
pub mod font_cache;
pub mod gpui_sink;
pub mod view;

pub use view::OfdGpuiView;
