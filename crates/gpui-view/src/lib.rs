//! rofd-gpui-view — GPUI adapter for the rofd viewer/annotator component.
//!
//! The adapter surface is complete per the gpui line plan
//! (docs/superpowers/specs/2026-10-31-gpui-view-gpui-app-design.md):
//! event translation (`events`), the kernel callback queue
//! (`events_surface`), the paint sink (`gpui_sink`) + font cache, the
//! pull->push IME bridge (`ime`), and the host-facing view (`view`).

pub mod events;
pub mod events_surface;
pub mod font_cache;
pub mod gpui_sink;
pub mod ime;
pub mod view;

pub use events_surface::OfdGpuiEvent;
pub use view::OfdGpuiView;
