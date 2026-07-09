//! rofd-dom - pure OFD data model. No ZIP/XML deps.

pub mod annotation;
pub mod document;
pub mod ids;
pub mod object;
pub mod page;
pub mod primitives;
pub mod resource;

pub use annotation::*;
pub use document::*;
pub use ids::*;
pub use object::*;
pub use page::*;
pub use primitives::*;
pub use resource::*;
