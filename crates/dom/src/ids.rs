//! Strongly-typed IDs. All OFD IDs are ST_ID (unsigned integer, GB/T 33190 表 2),
//! held as integer strings. New annotation IDs are allocated from
//! OfdDocument.max_unit_id + 1 (see editor::create_annotation), not uuid.

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
        }
    };
}

string_id!(ObjectId);
string_id!(PageId);
string_id!(FontId);
string_id!(ImageId);
string_id!(DrawParamId);
string_id!(AnnotationId);

impl AnnotationId {
    /// Construct from an integer (OFD ST_ID). New IDs come from
    /// OfdDocument.max_unit_id + 1, allocated by the editor.
    pub fn from_int(n: u64) -> Self {
        Self(n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_id_is_string_newtype_holding_integer() {
        let id = AnnotationId::from_int(1488);
        assert_eq!(id.0, "1488");
        let id2 = AnnotationId::new("1491");
        assert_eq!(id2.0, "1491");
    }

    #[test]
    fn annotation_id_round_trips_serde_json_as_string() {
        let id = AnnotationId::from_int(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "\"42\"");
        let back: AnnotationId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, id);
    }
}
