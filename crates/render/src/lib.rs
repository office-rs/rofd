//! rofd-render - Vello scene builder for OFD documents.

pub mod annotation_scene;
pub mod body_scene;
pub mod cache;
pub mod color;
pub mod composite;
pub mod ctm;
pub mod image;
pub mod path;
pub mod text;
pub mod viewport;

pub use annotation_scene::build_annotation_scene;
pub use body_scene::build_body_scene;
pub use cache::PageSceneCache;
pub use color::to_peniko;
pub use composite::RenderEngine;
pub use ctm::{compose_transform, ctm_to_affine};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{shape_text, FontStore, ShapedGlyph};
pub use viewport::Viewport;
