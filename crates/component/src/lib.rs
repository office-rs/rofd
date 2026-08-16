//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod config;
pub mod editor_component;
pub mod event;
pub mod render_target;

pub use callbacks::{Callbacks, ContextTarget, PointerCursor};
pub use config::EditorConfig;
pub use editor_component::{EditorComponent, Tool};
pub use event::{EventOutcome, Key, Modifiers, MouseButton, ScrollDirection, ViewEvent};
pub use render_target::RenderTarget;
