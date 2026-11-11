//! rofd-io - parse / full-write for .ofd packages.
//!
//! Saving is a full rewrite with passthrough (spec 2026-11-11 D12):
//! `write_ofd_with_text_layout` serializes the model under canonical entry
//! names and transports the original package's unconsumed entries verbatim.

pub mod abbreviated;
pub mod annotation_geom;
pub mod dateutil;
pub mod error;
pub mod package;
pub mod parse;
pub mod serialize;
pub mod zip_util;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
pub use parse::parse_ofd;
pub use serialize::package::{write_ofd, write_ofd_with_text_layout};
