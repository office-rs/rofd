//! rofd-render - imaging scene builder for OFD documents.
//!
//! Produces a backend-agnostic [`imaging::record::Scene`] from an
//! [`OfdDocument`](rofd_dom::OfdDocument) + [`Viewport`]. The native xilem host
//! consumes the scene via `Painter::replay`; the web host converts it to a
//! `vello::Scene` via `imaging_vello::VelloSceneSink`.

pub mod annotation_scene;
pub mod body_scene;
pub mod body_text;
pub mod caret_rect;
pub mod color;
pub mod composite;
pub mod ctm;
pub mod handles;
pub mod hit_test;
pub mod image;
pub mod path;
pub mod text;
pub mod viewport;

pub use annotation_scene::draw_annotations;
pub use body_scene::draw_body;
pub use body_text::{hit_test_body_text, BodyTextRange, BodyTextSelection, TextHit};
pub use caret_rect::caret_rect;
pub use composite::{page_origin, page_origins, DragPreview, RenderEngine};
pub use ctm::{compose_transform, ctm_to_affine};
pub use handles::{annotation_handle_positions, annotation_handles, handle_center_local};
pub use hit_test::{annotation_local_rect, hit_test, HandlePos, HitTarget};
pub use image::decode_image;
pub use path::path_to_bezpath;
pub use text::{shape_text, FontStore, ShapedGlyph};
pub use viewport::{clamp_scroll, Viewport, PX_PER_MM};

/// The backend-agnostic scene type this crate produces.
pub use imaging::record::Scene;
