//! Passthrough storage for unknown XML content (spec 2026-11-11 D3).
//!
//! `RawFragment` holds the FINAL verbatim form of an unrecognized XML
//! subtree: the original source bytes with any inherited namespace
//! declarations already synthesized into the fragment root (all
//! capture-time processing; writers re-emit the string byte-for-byte).
//! `position` is the fragment's index within the parent's FULL child
//! sequence (known and unknown children interleaved), so writers can
//! re-insert it exactly where it was.

use serde::{Deserialize, Serialize};

/// One unrecognized XML subtree, position-preserved.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawFragment {
    /// Index within the parent's interleaved (known + unknown) child
    /// sequence.
    pub position: usize,
    /// Final verbatim XML of the subtree (namespace-complete).
    pub xml: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Page;

    #[test]
    fn raw_fragment_round_trips_serde() {
        let f = RawFragment {
            position: 3,
            xml: "<ext:Stamp ID=\"9\" xmlns:ext=\"urn:x\"/>".into(),
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: RawFragment = serde_json::from_str(&s).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn old_serialized_page_without_raw_children_still_loads() {
        // A Page serialized before the passthrough fields existed has no
        // `raw_children` key; serde(default) keeps those documents loadable.
        let legacy = r#"{
            "id": "P0",
            "physical_box": {"x": 0.0, "y": 0.0, "w": 210.0, "h": 297.0},
            "layers": [],
            "template": null
        }"#;
        let page: Page = serde_json::from_str(legacy).expect("legacy page loads");
        assert!(page.raw_children.is_empty());
    }
}
