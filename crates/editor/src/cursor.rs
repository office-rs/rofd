use rofd_dom::AnnotationId;

#[derive(Debug, Clone, PartialEq)]
pub struct TextCursor {
    pub annotation: AnnotationId,
    pub offset: usize,
    pub preferred_x: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_clone_eq() {
        let c = TextCursor {
            annotation: AnnotationId::from_int(1),
            offset: 3,
            preferred_x: Some(1.0),
        };
        assert_eq!(c, c.clone());
    }
}
