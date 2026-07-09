use crate::cursor::TextCursor;
use crate::selection::AnnotationSelection;
use crate::steps::step_trait::Step;

pub struct Transaction {
    pub steps: Vec<Box<dyn Step>>,
    pub selection_before: AnnotationSelection,
    pub selection_after: AnnotationSelection,
    pub text_cursor_before: Option<TextCursor>,
    pub text_cursor_after: Option<TextCursor>,
}
