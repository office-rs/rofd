//! IME preedit state: composition text lives here (not in the dom) until
//! the IME commits. The component owns it independently of any platform
//! widget, so both adapters drive the same state machine.

use rofd_dom::AnnotationId;

/// Active IME composition: text plus the preedit caret range (byte offsets
/// into `text`) and the annotation/offset the composition started at.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreeditState {
    pub text: String,
    pub caret: Option<(usize, usize)>,
    pub annotation: AnnotationId,
    pub offset: usize,
}
