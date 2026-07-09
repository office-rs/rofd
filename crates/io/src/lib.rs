//! rofd-io - parse / surgical-save / full-write for .ofd packages.

pub mod error;
pub mod package;
pub mod zip_util;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
