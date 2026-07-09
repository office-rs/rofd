//! rofd-render - Vello scene builder for OFD documents.

pub mod path;
pub mod text;

pub use path::path_to_bezpath;
pub use text::{shape_text, FontStore, ShapedGlyph};
