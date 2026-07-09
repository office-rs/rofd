//! rofd-render - Vello scene builder for OFD documents.

pub mod ctm;
pub mod image;
pub mod path;
pub mod text;

pub use ctm::{compose_transform, ctm_to_affine};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{shape_text, FontStore, ShapedGlyph};
