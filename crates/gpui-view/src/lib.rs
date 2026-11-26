//! rofd-gpui-view — GPUI adapter for the rofd viewer/annotator component.
//!
//! The adapter surface is complete per the gpui line plan
//! (docs/superpowers/specs/2026-10-31-gpui-view-gpui-app-design.md):
//! event translation (`events`), the kernel callback queue
//! (`events_surface`), the paint sink (`gpui_sink`) + font cache, the
//! pull->push IME bridge (`ime`), the host-facing view (`view`), the
//! host verb surface (`handle`), and the print dialog state machine +
//! native discovery services (`print_dialog_state`/`print_services`).

pub mod events;
pub mod events_surface;
pub mod font_cache;
mod format_panel;
pub mod gpui_sink;
pub mod handle;
pub mod ime;
pub mod print_dialog_state;
pub mod print_services;
pub mod view;

pub use events_surface::OfdGpuiEvent;
// The seal placement types, so a host needs no direct rofd-component
// dependency to enter the state and read the event.
pub use handle::GpuiOfdHandle;
pub use rofd_component::{SealPlacement, SealPreview};
pub use view::OfdGpuiView;
