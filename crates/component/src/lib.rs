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

// Re-exported so adapters can name the body-text selection type (public API
// surface of `text_selection()`/`on_text_selection_change`) without taking a
// direct rofd-render dependency.
pub use rofd_render::BodyTextSelection;
