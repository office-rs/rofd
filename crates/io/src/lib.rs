//! rofd-io - parse / surgical-save / full-write for .ofd packages.

pub mod abbreviated;
pub mod annotation_geom;
pub mod dateutil;
pub mod error;
pub mod package;
pub mod parse;
pub mod save;
pub mod serialize;
pub mod zip_util;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
pub use parse::parse_ofd;
pub use save::save_ofd;
pub use serialize::package::write_ofd;
