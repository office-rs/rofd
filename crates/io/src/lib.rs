//! rofd-io - parse / write for .ofd packages.
//!
//! Two writers share one serializer. The preserving save (`save_ofd`, the
//! editor's path, spec 2026-11-23 §7) keeps every entry of the loaded
//! package byte-identical except the annotation entries. The full write
//! (`write_ofd_with_text_layout`, spec 2026-11-11 D12) regenerates the model
//! under canonical entry names and transports the unmodelled entries.
//!
//! The document carries the package it came from (`OfdDocument.package`, spec
//! 2026-11-23 §17.11): parsing puts it there and the save reads it from
//! there, so no host keeps a second handle.

pub mod abbreviated;
pub mod annotation_geom;
pub mod dateutil;
mod der;
pub mod error;
pub mod package;
pub mod parse;
pub mod paths;
pub mod serialize;
pub mod zip_util;

pub use error::{LoadReport, OfdError, OfdWarning, ResourceKind};
pub use package::{EntryKind, PackageHandle, PkgEntry};
pub use parse::signature::seal_picture_from_der;
pub use parse::{generate_package, parse_ofd, refresh_document_level, GeneratedPackage};
pub use serialize::package::{write_ofd, write_ofd_with_text_layout};
pub use serialize::patch::{
    max_unit_id_of, with_annotations_loc, with_max_unit_id, with_signatures_loc,
};
pub use serialize::preserving::{
    annotations_unwritable, save_ofd, write_ofd_preserving, DocumentXmlChange, PreserveError,
    Preserved,
};
