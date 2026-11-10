//! rofd-editor - OFD annotation editor (selection, commands, undo/redo).

pub mod commands;
pub mod cursor;
pub mod editor;
pub mod payload_util;
pub mod selection;
pub mod steps;
pub mod text_spans;

pub use commands::text_commands::normalize_newlines;
pub use cursor::TextCursor;
pub use editor::Editor;
pub use selection::AnnotationSelection;
pub use steps::{
    DeleteAnnotationStep, InsertAnnotationStep, ReplaceAnnotationStep, Step, Transaction,
};
