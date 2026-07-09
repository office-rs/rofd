use rofd_dom::AnnotationId;

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationSelection {
    None,
    Single(AnnotationId),
    Multi(Vec<AnnotationId>),
}

impl AnnotationSelection {
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
        assert!(!AnnotationSelection::None.contains(&AnnotationId::new()));
    }

    #[test]
    fn single_contains_its_id() {
        let id = AnnotationId::new();
        assert!(AnnotationSelection::Single(id.clone()).contains(&id));
        assert!(!AnnotationSelection::Single(id.clone()).contains(&AnnotationId::new()));
    }

    #[test]
    fn multi_contains_any_listed() {
        let a = AnnotationId::new();
        let b = AnnotationId::new();
        assert!(AnnotationSelection::Multi(vec![a.clone(), b.clone()]).contains(&a));
    }
}
