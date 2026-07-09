//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod event;
pub mod render_target;

pub use callbacks::Callbacks;
pub use event::*;
pub use render_target::RenderTarget;
