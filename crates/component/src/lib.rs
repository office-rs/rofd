//! rofd-component - EditorComponent facade. The sole integration entry point.

pub mod callbacks;
pub mod config;
pub mod editor_component;
pub mod event;
pub mod preedit;
pub mod render_target;
pub mod tooltip_text;

pub use callbacks::{Callbacks, ContextTarget, PointerCursor, TooltipFormatter};
pub use config::EditorConfig;
pub use editor_component::{CreateKind, EditorComponent, Tool};
pub use event::{EventOutcome, Key, Modifiers, MouseButton, ScrollDirection, ViewEvent};
pub use render_target::RenderTarget;
pub use tooltip_text::{default_tooltip_lines, format_tooltip_datetime};

// Re-exported so adapters can name the body-text selection type (public API
// surface of `text_selection()`/`on_text_selection_change`) without taking a
// direct rofd-render dependency.
pub use rofd_render::BodyTextSelection;
