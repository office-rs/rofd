//! Read-only projection of a package's signatures (GB/T 33190 chapter 18,
//! spec 2026-11-23 §8.1).
//!
//! io fills [`crate::OfdDocument::signatures`] while parsing, so render can draw
//! the seals and the editor can tell a signed document from an unsigned
//! one. Nothing edits this model: it is a view of the `Signs/` entries,
//! which are carried through a save byte for byte.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ids::PageId;
use crate::primitives::Rect;

/// `Signature@Type` (GB/T 33190 table 66): `Seal` is a secure seal, with
/// or without an appearance; `Sign` is a plain digital signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureKind {
    Seal,
    Sign,
}

/// One `StampAnnot` of a signature: where its seal picture appears.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StampAnnot {
    pub page_id: PageId,
    /// Page-local millimetres.
    pub boundary: Rect,
    pub clip: Option<Rect>,
}

/// The picture of an electronic seal (ESS_Seal), as PNG bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealPicture {
    pub png: Arc<Vec<u8>>,
    pub width: f64,
    pub height: f64,
}

/// One signature of the package, as the signature list and its files
/// declare it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// `Signature@ID` as written.
    pub id: String,
    pub kind: SignatureKind,
    /// `Signature@BaseLoc` exactly as written in the signature list.
    pub base_loc: String,
    pub stamps: Vec<StampAnnot>,
    /// `None` for a signature without a seal picture, and when the picture
    /// could not be read (a load warning says which).
    pub seal_picture: Option<SealPicture>,
}

/// Every signature of a package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SignatureSet {
    /// OFD.xml declares a `Signatures` entry, whether or not the signature
    /// list could be read.
    pub declared: bool,
    pub items: Vec<SignatureInfo>,
}

impl SignatureSet {
    /// The stamps on `page` that carry a picture.
    pub fn stamps_on<'a>(
        &'a self,
        page: &'a PageId,
    ) -> impl Iterator<Item = (&'a StampAnnot, &'a SealPicture)> + 'a {
        self.items
            .iter()
            .filter_map(|signature| {
                signature
                    .seal_picture
                    .as_ref()
                    .map(|picture| (signature, picture))
            })
            .flat_map(move |(signature, picture)| {
                signature
                    .stamps
                    .iter()
                    .filter(move |stamp| &stamp.page_id == page)
                    .map(move |stamp| (stamp, picture))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OfdDocument;
    use std::sync::Arc;

    fn stamp(page: &str, x: f64) -> StampAnnot {
        StampAnnot {
            page_id: PageId::new(page),
            boundary: Rect {
                x,
                y: 0.0,
                w: 40.0,
                h: 40.0,
            },
            clip: None,
        }
    }

    fn picture() -> SealPicture {
        SealPicture {
            png: Arc::new(vec![0x89, 0x50, 0x4E, 0x47]),
            width: 40.0,
            height: 40.0,
        }
    }

    #[test]
    fn signature_set_default_is_empty_and_not_declared() {
        let set = SignatureSet::default();
        assert!(!set.declared);
        assert!(set.items.is_empty());
        assert_eq!(OfdDocument::default().signatures, set);
    }

    #[test]
    fn stamps_on_yields_only_stamps_with_pictures() {
        let with_picture = SignatureInfo {
            id: "1".to_owned(),
            kind: SignatureKind::Seal,
            base_loc: "/Doc_0/Signs/Sign_0/Signature.xml".to_owned(),
            stamps: vec![stamp("7", 0.0), stamp("8", 50.0)],
            seal_picture: Some(picture()),
        };
        let without_picture = SignatureInfo {
            id: "2".to_owned(),
            kind: SignatureKind::Sign,
            base_loc: String::new(),
            stamps: vec![stamp("7", 90.0)],
            seal_picture: None,
        };
        let set = SignatureSet {
            declared: true,
            items: vec![with_picture, without_picture],
        };
        let on_page: Vec<(f64, f64)> = set
            .stamps_on(&PageId::new("7"))
            .map(|(stamp, picture)| (stamp.boundary.x, picture.width))
            .collect();
        assert_eq!(on_page, vec![(0.0, 40.0)]);
        assert_eq!(set.stamps_on(&PageId::new("8")).count(), 1);
        assert_eq!(set.stamps_on(&PageId::new("9")).count(), 0);
    }

    #[test]
    fn missing_signatures_field_deserializes_to_default() {
        let json = serde_json::to_string(&OfdDocument::default()).unwrap();
        let stripped = json.replace(r#","signatures":{"declared":false,"items":[]}"#, "");
        assert!(stripped != json, "the field is serialized: {json}");
        let parsed: OfdDocument = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed.signatures, SignatureSet::default());
    }

    #[test]
    fn signature_set_round_trips_serde() {
        let set = SignatureSet {
            declared: true,
            items: vec![SignatureInfo {
                id: "1".to_owned(),
                kind: SignatureKind::Seal,
                base_loc: "Sign_0/Signature.xml".to_owned(),
                stamps: vec![stamp("7", 10.0)],
                seal_picture: Some(picture()),
            }],
        };
        let json = serde_json::to_string(&set).unwrap();
        let parsed: SignatureSet = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, set);
    }
}
