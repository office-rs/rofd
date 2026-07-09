//! rofd-render - Vello scene builder for OFD documents.

pub mod annotation_scene;
pub mod body_scene;
pub mod cache;
pub mod caret_rect;
pub mod color;
pub mod composite;
pub mod ctm;
pub mod hit_test;
pub mod image;
pub mod path;
pub mod text;
pub mod viewport;

pub use annotation_scene::build_annotation_scene;
pub use body_scene::build_body_scene;
pub use cache::PageSceneCache;
pub use caret_rect::caret_rect;
pub use composite::RenderEngine;
pub use ctm::{compose_transform, ctm_to_affine};
pub use hit_test::{hit_test, HitTarget};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{FontStore, shape_text, ShapedGlyph};
pub use viewport::Viewport;
