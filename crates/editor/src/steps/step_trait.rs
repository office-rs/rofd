use rofd_dom::AnnotationModel;

/// A reversible edit on the annotation model. Stores enough to apply AND revert.
pub trait Step: Send + std::fmt::Debug {
    fn apply(&self, anns: &mut AnnotationModel);
    fn revert(&self, anns: &mut AnnotationModel);
}
