use rofd_dom::{AnnotationId, OfdDocument};
use rofd_editor::{AnnotationSelection, TextCursor};

// The 9 callback types. on_change passes &OfdDocument; on_selection_change
// passes &AnnotationSelection; on_cursor_change passes Option<&TextCursor>;
// on_save_request passes (); on_annotation_focus/on_annotation_interact pass
// &AnnotationId; on_context_menu passes ((f64,f64), ContextTarget);
// on_page_change passes usize; on_zoom_change passes f64.
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
#[cfg(not(target_arch = "wasm32"))]
pub type OnAnnotationFocus = dyn Fn(&AnnotationId) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnAnnotationInteract = dyn Fn(&AnnotationId) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnContextMenu = dyn Fn((f64, f64), ContextTarget) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnPageChange = dyn Fn(usize) + Send;
#[cfg(not(target_arch = "wasm32"))]
pub type OnZoomChange = dyn Fn(f64) + Send;

#[cfg(target_arch = "wasm32")]
pub type OnChange = dyn Fn(&OfdDocument);
#[cfg(target_arch = "wasm32")]
pub type OnSelectionChange = dyn Fn(&AnnotationSelection);
#[cfg(target_arch = "wasm32")]
pub type OnCursorChange = dyn Fn(Option<&TextCursor>);
#[cfg(target_arch = "wasm32")]
pub type OnSaveRequest = dyn Fn();
#[cfg(target_arch = "wasm32")]
pub type OnAnnotationFocus = dyn Fn(&AnnotationId);
#[cfg(target_arch = "wasm32")]
pub type OnAnnotationInteract = dyn Fn(&AnnotationId);
#[cfg(target_arch = "wasm32")]
pub type OnContextMenu = dyn Fn((f64, f64), ContextTarget);
#[cfg(target_arch = "wasm32")]
pub type OnPageChange = dyn Fn(usize);
#[cfg(target_arch = "wasm32")]
pub type OnZoomChange = dyn Fn(f64);

/// What a right-click landed on, passed to `on_context_menu`. The host uses
/// this to show a context menu tailored to the target (annotation actions vs.
/// page actions vs. nothing).
///
/// This is a component-level type (not dom): it collapses render's
/// `HitTarget` (Annotation/AnnotationText/Handle/Page/Empty) into the three
/// categories a context menu cares about. `Handle` maps to `Annotation` (the
/// user right-clicked on a selected annotation's resize grip -- still an
/// annotation context).
#[derive(Debug, Clone, PartialEq)]
pub enum ContextTarget {
    /// Right-click hit an annotation (or its selection handle).
    Annotation(AnnotationId),
    /// Right-click hit a page body (no annotation under the cursor).
    Page,
    /// Right-click hit the desk background (no page under the cursor).
    Empty,
}

#[derive(Default)]
pub struct Callbacks {
    pub on_change: Option<Box<OnChange>>,
    pub on_selection_change: Option<Box<OnSelectionChange>>,
    pub on_cursor_change: Option<Box<OnCursorChange>>,
    pub on_save_request: Option<Box<OnSaveRequest>>,
    pub on_annotation_focus: Option<Box<OnAnnotationFocus>>,
    pub on_annotation_interact: Option<Box<OnAnnotationInteract>>,
    pub on_context_menu: Option<Box<OnContextMenu>>,
    pub on_page_change: Option<Box<OnPageChange>>,
    pub on_zoom_change: Option<Box<OnZoomChange>>,
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
            on_change: Some(Box::new(move |_doc| {
                *fired_clone.lock().unwrap() = true;
            })),
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
            on_save_request: Some(Box::new(move || {
                *fired_clone.lock().unwrap() = true;
            })),
            ..Default::default()
        };
        (cbs.on_save_request.as_ref().unwrap())();
        assert!(*fired.lock().unwrap());
    }
}
