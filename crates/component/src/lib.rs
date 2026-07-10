//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod config;
pub mod editor_component;
pub mod event;
pub mod render_target;

pub use callbacks::Callbacks;
pub use config::EditorConfig;
pub use editor_component::EditorComponent;
pub use event::{EventOutcome, Key, Modifiers, MouseButton, ViewEvent};
pub use render_target::RenderTarget;
