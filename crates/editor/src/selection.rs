//! Re-export of [`AnnotationSelection`] from `rofd-dom`.
//!
//! The type lives in dom so that `rofd-render` (which depends on dom but not
//! editor) can consume it for handle drawing and hit-testing without creating
//! a reverse dependency edge. Editor re-exports it for backward compatibility
//! with existing `use rofd_editor::AnnotationSelection` imports.

pub use rofd_dom::AnnotationSelection;
