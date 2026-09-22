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

// Re-exported so adapters can name the types in widget actions / callbacks
// without taking direct rofd-render / rofd-dom / rofd-editor dependencies.
pub use rofd_dom::{AnnotationId, OfdWarning, Rect};
pub use rofd_editor::{AnnotationSelection, TextCursor};
pub use rofd_render::BodyTextSelection;
