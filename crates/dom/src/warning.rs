//! Degraded-load warnings. Pure data model types (no ZIP/XML deps).
//!
//! `OfdWarning` represents non-fatal issues encountered while parsing an OFD
//! package (e.g. unmodelled features, missing resources). The io layer emits
//! these into [`LoadReport::warnings`](crate::LoadReport); the component layer
//! fires them to the host via the `on_warning` callback.
//!
//! Defined in dom (not io) so that both io (parse) and component (callback)
//! can reference the type without component depending on io (AGENTS.md §4.1).

use crate::PageId;

/// Kind of resource referenced by the document.
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

/// A non-fatal issue encountered while parsing an OFD package. Parse continues;
/// the warning is surfaced to the host via the `on_warning` callback
/// (AGENTS.md §4.6 - degraded input path, distinct from `OfdError`).
#[derive(Debug, Clone)]
pub enum OfdWarning {
    /// An OFD feature not modelled in v1 (e.g. templates, JBIG2). Parse
    /// continues with the feature skipped.
    MissingFeature { feature: String, entry: String },
    /// An unknown/unmodelled object was skipped on a page (e.g. an unrecognized
    /// XML element). Parse continues without it.
    SkippedObject { page: PageId, reason: String },
    /// A requested font was not found in the package; a default font is used.
    FontSubstituted { requested: String, used: String },
    /// A resource (image/font) referenced by the document is missing from the
    /// package. Parse continues; the resource is simply unavailable.
    ResourceNotFound { kind: ResourceKind, id: String },
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
    fn resource_kind_display() {
        assert_eq!(ResourceKind::Font.to_string(), "font");
        assert_eq!(ResourceKind::Image.to_string(), "image");
        assert_eq!(ResourceKind::DrawParam.to_string(), "draw param");
    }
}
