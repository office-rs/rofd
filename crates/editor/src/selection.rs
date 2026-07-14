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
