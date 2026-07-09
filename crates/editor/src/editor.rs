use rofd_dom::OfdDocument;

use crate::cursor::TextCursor;
use crate::selection::AnnotationSelection;
use crate::steps::history::History;

/// Annotation editor. Owns the document; mutates only `.annotations` via commands.
/// No callbacks - the host/component layer queries state after commands.
pub struct Editor {
    pub(crate) document: OfdDocument,
    pub(crate) selection: AnnotationSelection,
    pub(crate) text_cursor: Option<TextCursor>,
    pub(crate) history: History,
    pub(crate) author: String,
    pub(crate) current_ts: i64,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            document: OfdDocument::default(),
            selection: AnnotationSelection::None,
            text_cursor: None,
            history: History::new(100),
            author: String::new(),
            current_ts: 0,
        }
    }

    pub fn load_document(&mut self, doc: OfdDocument) {
        self.document = doc;
        self.selection = AnnotationSelection::None;
        self.text_cursor = None;
        self.history = History::new(100);
    }

    /// Caller-supplied author + timestamp. The library never reads a system clock.
    pub fn set_clock(&mut self, author: String, ts: i64) {
        self.author = author;
        self.current_ts = ts;
    }

    pub fn document(&self) -> &OfdDocument { &self.document }
    pub fn selection(&self) -> &AnnotationSelection { &self.selection }
    pub fn text_cursor(&self) -> Option<&TextCursor> { self.text_cursor.as_ref() }
    pub fn can_undo(&self) -> bool { self.history.can_undo() }
    pub fn can_redo(&self) -> bool { self.history.can_redo() }
}

impl Default for Editor {
    fn default() -> Self { Self::new() }
}
