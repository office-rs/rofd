//! Annotation selection model (object-level).
//!
//! Pure data type over [`AnnotationId`](crate::AnnotationId). Lives in dom
//! (not editor) so that `rofd-render` can consume it for handle drawing and
//! hit-testing without depending on `rofd-editor` (which would be a reverse
//! dependency edge: render -> editor is forbidden).
//!
//! v1 UI produces only `None` and `Single`; `Multi` is reserved for future
//! Shift+click multi-select.

use crate::AnnotationId;

/// Which annotation(s) are currently selected.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationSelection {
    /// No selection.
    None,
    /// A single annotation is selected (handles drawn, draggable).
    Single(AnnotationId),
    /// Multiple annotations selected (v1: not produced by UI).
    Multi(Vec<AnnotationId>),
}

impl AnnotationSelection {
    /// Returns `true` if `id` is among the selected annotations.
    pub fn contains(&self, id: &AnnotationId) -> bool {
        match self {
            AnnotationSelection::None => false,
            AnnotationSelection::Single(s) => s == id,
            AnnotationSelection::Multi(ids) => ids.iter().any(|i| i == id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_contains_nothing() {
        assert!(!AnnotationSelection::None.contains(&AnnotationId::from_int(1)));
    }

    #[test]
    fn single_contains_its_id() {
        let id = AnnotationId::from_int(1);
        assert!(AnnotationSelection::Single(id.clone()).contains(&id));
        assert!(!AnnotationSelection::Single(id.clone()).contains(&AnnotationId::from_int(2)));
    }

    #[test]
    fn multi_contains_any_listed() {
        let a = AnnotationId::from_int(1);
        let b = AnnotationId::from_int(2);
        assert!(AnnotationSelection::Multi(vec![a.clone(), b.clone()]).contains(&a));
    }
}
