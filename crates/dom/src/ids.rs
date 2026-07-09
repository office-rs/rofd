//! Strongly-typed IDs. Object/page IDs are OFD string IDs;
//! AnnotationId is a uuid v4 (no system time needed).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub Uuid);

impl AnnotationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
impl Default for AnnotationId {
    fn default() -> Self {
        Self::new()
    }
}
