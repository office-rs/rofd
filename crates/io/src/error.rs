use crate::package::PackageHandle;
use rofd_dom::OfdDocument;

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

    #[error("resource not found: {kind} {id}")]
    ResourceNotFound { kind: ResourceKind, id: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceKind {
    Font,
    Image,
    DrawParam,
}

impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceKind::Font => write!(f, "font"),
            ResourceKind::Image => write!(f, "image"),
            ResourceKind::DrawParam => write!(f, "draw param"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum OfdWarning {
    MissingFeature {
        feature: String,
        entry: String,
    },
    SkippedObject {
        page: rofd_dom::PageId,
        reason: String,
    },
    FontSubstituted {
        requested: String,
        used: String,
    },
}

pub struct LoadReport {
    pub document: OfdDocument,
    pub package: PackageHandle,
    pub warnings: Vec<OfdWarning>,
}

impl LoadReport {
    pub fn new(document: OfdDocument, package: PackageHandle, warnings: Vec<OfdWarning>) -> Self {
        Self {
            document,
            package,
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_feature_warning_displays_feature() {
        let w = OfdWarning::MissingFeature {
            feature: "JBIG2".into(),
            entry: "Doc_0/Res/Img_0.xml".into(),
        };
        assert!(format!("{w:?}").contains("JBIG2"));
    }

    #[test]
    fn load_report_carries_document_and_package() {
        let report = LoadReport::new(OfdDocument::default(), PackageHandle::empty(), vec![]);
        assert!(report.document.pages.is_empty());
        assert!(report.warnings.is_empty());
    }
}
