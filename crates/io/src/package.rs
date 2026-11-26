//! The package types live in dom (spec §17.11): the document carries its
//! package, and dom cannot see io. They are re-exported here, so
//! `rofd_io::PackageHandle` and `rofd_io::package::*` keep resolving.

pub use rofd_dom::{EntryKind, PackageHandle, PkgEntry};
