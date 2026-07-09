//! rofd-editor - OFD annotation editor (selection, commands, undo/redo).

pub mod commands;
pub mod cursor;
pub mod editor;
pub mod payload_util;
pub mod selection;
pub mod steps;

pub use cursor::TextCursor;
pub use editor::Editor;
pub use selection::AnnotationSelection;
