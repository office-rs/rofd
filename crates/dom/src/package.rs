//! The package a document came from, as plain data (spec 2026-11-11 D12,
//! 2026-11-23 §7, §17.11).
//!
//! [`PackageHandle`] keeps every entry of the original package, so that a
//! save can return what the model does not regenerate byte for byte, and
//! records where the document's structural files live. io reads and writes
//! it; the document carries it, so no host keeps a second copy that could
//! drift from the document.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ids::PageId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Body content (Page.xml etc.) - preserved byte-identical by the preserving save.
    Body,
    /// Annotation entry - rewritten from AnnotationModel by the preserving save.
    Annotation,
    /// Signature entry - preserved byte-identical.
    Signature,
    /// Font/image/drawparam resource - preserved byte-identical.
    Resource,
    /// Manifest / unknown - preserved byte-identical.
    Other,
}

impl EntryKind {
    /// What an entry of a package is, by its name.
    pub fn of(name: &str) -> Self {
        if name.ends_with("Annotation.xml")
            || name.ends_with("Annotations.xml")
            || name.contains("/Annots/")
        {
            EntryKind::Annotation
        } else if name.ends_with("Page.xml") || name.ends_with("Document.xml") || name == "OFD.xml"
        {
            EntryKind::Body
        } else if name.starts_with("Doc_") && name.contains("/Signs/") {
            EntryKind::Signature
        } else if name.contains("/Res/") {
            EntryKind::Resource
        } else {
            EntryKind::Other
        }
    }
}

#[derive(Clone)]
pub struct PkgEntry {
    pub name: String,
    pub kind: EntryKind,
    pub bytes: Arc<Vec<u8>>,
}

// Not derived: a derived `Debug` would print every byte of every entry.
impl std::fmt::Debug for PkgEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PkgEntry")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Original package skeleton retained for passthrough transport
/// (spec 2026-11-11 D12): `consumed` records every entry parse actually
/// read into the model; the full-write transport copies the remaining
/// (unconsumed) entries verbatim.
///
/// The layout fields (spec 2026-11-23 §7) tell the preserving save where the
/// document's structural files live, under their original names.
#[derive(Debug, Clone, Default)]
pub struct PackageHandle {
    pub entries: Vec<PkgEntry>,
    /// Entry names parse consumed (pages/resources/annotations/...).
    pub consumed: BTreeSet<String>,
    /// Package path of the document entry (`DocRoot`), e.g.
    /// `Doc_0/Document.xml`.
    pub document_entry: Option<String>,
    /// Package path of the annotation entry file the document declares
    /// (`<Annotations>`), whether or not the package carries that file.
    pub annotations_entry: Option<String>,
    /// `(page id, package path)` of every per-page annotation file parse
    /// located, in entry-file order. Paths are package-absolute (no leading
    /// slash) and keep the original names.
    pub annotation_files: Vec<(PageId, String)>,
    /// OFD.xml's DocBody declares a `<Signatures>` entry, whether or not the
    /// signature structure could be read.
    pub signatures_declared: bool,
    /// Package path of the signature list OFD.xml declares, whether or not
    /// the package carries it.
    pub signatures_entry: Option<String>,
}

impl PackageHandle {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn find(&self, name: &str) -> Option<&PkgEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn annotation_entries(&self) -> impl Iterator<Item = &PkgEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Annotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_follows_the_name() {
        assert_eq!(EntryKind::of("OFD.xml"), EntryKind::Body);
        assert_eq!(EntryKind::of("Doc_0/Document.xml"), EntryKind::Body);
        assert_eq!(
            EntryKind::of("Doc_0/Annots/Annotations.xml"),
            EntryKind::Annotation
        );
        assert_eq!(
            EntryKind::of("Doc_0/Pages/Page_0/Annotation.xml"),
            EntryKind::Annotation
        );
        assert_eq!(
            EntryKind::of("Doc_0/Signs/Sign_0/Seal.esl"),
            EntryKind::Signature
        );
        assert_eq!(EntryKind::of("Doc_0/Res/a.png"), EntryKind::Resource);
        assert_eq!(
            EntryKind::of("Doc_0/Pages/Page_0/Content.xml"),
            EntryKind::Other
        );
        assert_eq!(EntryKind::of("Doc_0/Attachs/note.txt"), EntryKind::Other);
    }

    #[test]
    fn debug_shows_a_length_not_the_bytes() {
        let entry = PkgEntry {
            name: "a".to_owned(),
            kind: EntryKind::Other,
            bytes: Arc::new(vec![7u8; 4096]),
        };
        let text = format!("{entry:?}");
        assert!(text.contains("4096"), "{text}");
        assert!(!text.contains("7, 7, 7"), "{text}");
    }

    #[test]
    fn empty_handle_finds_nothing() {
        let h = PackageHandle::empty();
        assert!(h.find("anything").is_none());
        assert_eq!(h.annotation_entries().count(), 0);
    }
}
