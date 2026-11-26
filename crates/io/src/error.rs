use rofd_dom::OfdDocument;

// OfdWarning and ResourceKind are defined in rofd-dom (not io) so that both io
// (parse) and rofd-component (on_warning callback) can reference them without
// component depending on io (AGENTS.md §4.1). They are re-exported here for
// backward compatibility.
pub use rofd_dom::{OfdWarning, ResourceKind};

#[derive(Debug, thiserror::Error)]
pub enum OfdError {
    #[error("zip error in {entry}: {source}")]
    Zip {
        entry: String,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("xml error in {entry} at {loc}: {source}")]
    Xml {
        entry: String,
        loc: String,
        #[source]
        source: quick_xml::Error,
    },

    #[error("schema error in {entry}: {reason}")]
    Schema { entry: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// What parsing a package gives: the document, which carries the package it
/// came from (`OfdDocument.package`, spec 2026-11-23 §17.11), and the
/// degradations met on the way.
#[derive(Debug, Clone)]
pub struct LoadReport {
    pub document: OfdDocument,
    pub warnings: Vec<OfdWarning>,
}

impl LoadReport {
    pub fn new(document: OfdDocument, warnings: Vec<OfdWarning>) -> Self {
        Self { document, warnings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_report_carries_document_and_warnings() {
        let report = LoadReport::new(OfdDocument::default(), vec![]);
        assert!(report.document.pages.is_empty());
        assert!(report.warnings.is_empty());
    }
}
