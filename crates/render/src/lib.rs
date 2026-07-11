//! rofd-render - imaging scene builder for OFD documents.
//!
//! Produces a backend-agnostic [`imaging::record::Scene`] from an
//! [`OfdDocument`](rofd_dom::OfdDocument) + [`Viewport`]. The native xilem host
//! consumes the scene via `Painter::replay`; the web host converts it to a
//! `vello::Scene` via `imaging_vello::VelloSceneSink`.

pub mod annotation_scene;
pub mod body_scene;
pub mod caret_rect;
pub mod color;
pub mod composite;
pub mod ctm;
pub mod hit_test;
pub mod image;
pub mod path;
pub mod text;
pub mod viewport;

pub use annotation_scene::draw_annotations;
pub use body_scene::draw_body;
pub use caret_rect::caret_rect;
pub use composite::RenderEngine;
pub use ctm::{compose_transform, ctm_to_affine};
pub use hit_test::{hit_test, HitTarget};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{FontStore, shape_text, ShapedGlyph};
pub use viewport::{Viewport, PX_PER_MM};

/// The backend-agnostic scene type this crate produces.
pub use imaging::record::Scene;
