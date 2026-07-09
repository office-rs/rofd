//! rofd-io - parse / surgical-save / full-write for .ofd packages.

pub mod error;
pub mod package;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
