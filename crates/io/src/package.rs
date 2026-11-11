use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Body content (Page.xml etc.) - preserved byte-identical on surgical save.
    Body,
    /// Annotation entry - rewritten from AnnotationModel on surgical save.
    Annotation,
    /// Signature entry - preserved byte-identical.
    Signature,
    /// Font/image/drawparam resource - preserved byte-identical.
    Resource,
    /// Manifest / unknown - preserved byte-identical.
    Other,
}

#[derive(Debug, Clone)]
pub struct PkgEntry {
    pub name: String,
    pub kind: EntryKind,
    pub bytes: Arc<Vec<u8>>,
}

/// Original package skeleton retained for passthrough transport
/// (spec 2026-11-11 D12): `consumed` records every entry parse actually
/// read into the model; the full-write transport copies the remaining
/// (unconsumed) entries verbatim.
#[derive(Debug, Clone, Default)]
pub struct PackageHandle {
    pub entries: Vec<PkgEntry>,
    /// Entry names parse consumed (pages/resources/annotations/...).
    pub consumed: BTreeSet<String>,
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
    fn empty_handle_finds_nothing() {
        let h = PackageHandle::empty();
        assert!(h.find("anything").is_none());
        assert_eq!(h.annotation_entries().count(), 0);
    }
}
