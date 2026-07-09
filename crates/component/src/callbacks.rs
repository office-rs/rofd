use rofd_dom::OfdDocument;
use rofd_editor::{AnnotationSelection, TextCursor};

// The 4 callback types. on_change passes &OfdDocument; on_selection_change passes
// &AnnotationSelection; on_cursor_change passes Option<&TextCursor>; on_save_request passes ().
//
// Target-gated `Send`: the host (Phase 4b native) requires `Send` callbacks. On native
// targets the aliases below add `+ Send`; on wasm they do not (wasm is single-threaded).
// Phase 4b can rely on these aliases directly when storing callbacks across threads.
#[cfg(not(target_arch = "wasm32"))]
pub type OnChange = dyn Fn(&OfdDocument) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnSelectionChange = dyn Fn(&AnnotationSelection) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnCursorChange = dyn Fn(Option<&TextCursor>) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnSaveRequest = dyn Fn() + Send;

#[cfg(target_arch = "wasm32")]
pub type OnChange = dyn Fn(&OfdDocument);
#[cfg(target_arch = "wasm32")]
pub type OnSelectionChange = dyn Fn(&AnnotationSelection);
#[cfg(target_arch = "wasm32")]
pub type OnCursorChange = dyn Fn(Option<&TextCursor>);
#[cfg(target_arch = "wasm32")]
pub type OnSaveRequest = dyn Fn();

#[derive(Default)]
pub struct Callbacks {
    pub on_change: Option<Box<OnChange>>,
    pub on_selection_change: Option<Box<OnSelectionChange>>,
    pub on_cursor_change: Option<Box<OnCursorChange>>,
    pub on_save_request: Option<Box<OnSaveRequest>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn on_change_fires() {
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = fired.clone();
        let cbs = Callbacks {
            on_change: Some(Box::new(move |_doc| { *fired_clone.lock().unwrap() = true; })),
            ..Default::default()
        };
        let doc = OfdDocument::default();
        (cbs.on_change.as_ref().unwrap())(&doc);
        assert!(*fired.lock().unwrap());
    }

    #[test]
    fn on_save_request_fires() {
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = fired.clone();
        let cbs = Callbacks {
            on_save_request: Some(Box::new(move || { *fired_clone.lock().unwrap() = true; })),
            ..Default::default()
        };
        (cbs.on_save_request.as_ref().unwrap())();
        assert!(*fired.lock().unwrap());
    }
}
