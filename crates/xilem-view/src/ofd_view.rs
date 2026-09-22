//! Xilem `View` wrapping [`OfdWidget`].
//!
//! `ofd(queue)` embeds the widget in a xilem tree and exposes the
//! component callback surface as chainable handlers:
//!
//! ```ignore
//! ofd(state.commands.clone())
//!     .on_change(|state| state.mark_modified())
//!     .on_context_menu(|state, payload| state.open_menu(payload))
//! ```
//!
//! Host→widget imperative access (toolbar, file loads) goes through the
//! shared [`OfdCommandQueue`]: push closures from handlers; every rebuild
//! following `MessageResult::Action(())` drains them into
//! [`OfdWidget::with_component`].
//!
//! The `Action` type parameter is fixed to `()`: any handled callback
//! yields `MessageResult::Action(())`, rerunning the app logic.

use std::marker::PhantomData;

use rofd_component::{
    AnnotationId, AnnotationSelection, BodyTextSelection, ContextTarget, EditorConfig, OfdWarning,
    TextCursor,
};
use xilem::core::{MessageCtx, MessageResult, Mut, View, ViewMarker};
use xilem::{Pod, ViewCtx};

use crate::ofd_widget::{OfdCommandQueue, OfdWidget, OfdWidgetAction};

/// Callback handler: app state + event payload.
type Handler<State, Payload> = Box<dyn Fn(&mut State, Payload) + Send + Sync>;

/// Right-click context-menu payload: viewport position + target.
pub type OfdContextMenu = ((f64, f64), ContextTarget);

/// The xilem view for an OFD widget. Create with [`ofd`] / [`ofd_with_config`].
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct OfdView<State> {
    queue: OfdCommandQueue,
    config: EditorConfig,
    on_change: Option<Handler<State, ()>>,
    on_selection_change: Option<Handler<State, AnnotationSelection>>,
    on_cursor_change: Option<Handler<State, Option<TextCursor>>>,
    on_save_request: Option<Handler<State, ()>>,
    on_context_menu: Option<Handler<State, OfdContextMenu>>,
    on_annotation_focus: Option<Handler<State, AnnotationId>>,
    on_annotation_interact: Option<Handler<State, AnnotationId>>,
    on_page_change: Option<Handler<State, usize>>,
    on_zoom_change: Option<Handler<State, f64>>,
    on_text_selection_change: Option<Handler<State, Option<BodyTextSelection>>>,
    on_warnings: Option<Handler<State, Vec<OfdWarning>>>,
    phantom: PhantomData<fn() -> State>,
}

/// Embed an OFD widget with the default config (no registered fonts).
pub fn ofd<State: 'static>(queue: OfdCommandQueue) -> OfdView<State> {
    ofd_with_config(queue, EditorConfig::new(std::sync::Arc::new(vec![])))
}

/// Embed an OFD widget with a custom [`EditorConfig`].
pub fn ofd_with_config<State: 'static>(
    queue: OfdCommandQueue,
    config: EditorConfig,
) -> OfdView<State> {
    OfdView {
        queue,
        config,
        on_change: None,
        on_selection_change: None,
        on_cursor_change: None,
        on_save_request: None,
        on_context_menu: None,
        on_annotation_focus: None,
        on_annotation_interact: None,
        on_page_change: None,
        on_zoom_change: None,
        on_text_selection_change: None,
        on_warnings: None,
        phantom: PhantomData,
    }
}

impl<State: 'static> OfdView<State> {
    /// The document changed (query on demand via the command queue).
    pub fn on_change(mut self, f: impl Fn(&mut State) + Send + Sync + 'static) -> Self {
        self.on_change = Some(Box::new(move |state, ()| f(state)));
        self
    }

    pub fn on_selection_change(
        mut self,
        f: impl Fn(&mut State, AnnotationSelection) + Send + Sync + 'static,
    ) -> Self {
        self.on_selection_change = Some(Box::new(f));
        self
    }

    pub fn on_cursor_change(
        mut self,
        f: impl Fn(&mut State, Option<TextCursor>) + Send + Sync + 'static,
    ) -> Self {
        self.on_cursor_change = Some(Box::new(f));
        self
    }

    /// Ctrl+S save shortcut.
    pub fn on_save_request(mut self, f: impl Fn(&mut State) + Send + Sync + 'static) -> Self {
        self.on_save_request = Some(Box::new(move |state, ()| f(state)));
        self
    }

    /// Right-click context menu with viewport coordinates.
    pub fn on_context_menu(
        mut self,
        f: impl Fn(&mut State, OfdContextMenu) + Send + Sync + 'static,
    ) -> Self {
        self.on_context_menu = Some(Box::new(f));
        self
    }

    pub fn on_annotation_focus(
        mut self,
        f: impl Fn(&mut State, AnnotationId) + Send + Sync + 'static,
    ) -> Self {
        self.on_annotation_focus = Some(Box::new(f));
        self
    }

    pub fn on_annotation_interact(
        mut self,
        f: impl Fn(&mut State, AnnotationId) + Send + Sync + 'static,
    ) -> Self {
        self.on_annotation_interact = Some(Box::new(f));
        self
    }

    pub fn on_page_change(mut self, f: impl Fn(&mut State, usize) + Send + Sync + 'static) -> Self {
        self.on_page_change = Some(Box::new(f));
        self
    }

    pub fn on_zoom_change(mut self, f: impl Fn(&mut State, f64) + Send + Sync + 'static) -> Self {
        self.on_zoom_change = Some(Box::new(f));
        self
    }

    pub fn on_text_selection_change(
        mut self,
        f: impl Fn(&mut State, Option<BodyTextSelection>) + Send + Sync + 'static,
    ) -> Self {
        self.on_text_selection_change = Some(Box::new(f));
        self
    }

    pub fn on_warnings(
        mut self,
        f: impl Fn(&mut State, Vec<OfdWarning>) + Send + Sync + 'static,
    ) -> Self {
        self.on_warnings = Some(Box::new(f));
        self
    }
}

impl<State: 'static> ViewMarker for OfdView<State> {}

impl<State: 'static> View<State, (), ViewCtx> for OfdView<State> {
    type Element = Pod<OfdWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _state: &mut State) -> (Self::Element, Self::ViewState) {
        let widget = OfdWidget::new(self.config.clone());
        (ctx.with_action_widget(|ctx| ctx.create_pod(widget)), ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        _state: &mut State,
    ) {
        // Drain host commands queued since the last rerun. This is the
        // second half of the host→widget channel.
        let commands: Vec<_> = std::mem::take(&mut *self.queue.lock().unwrap());
        for command in commands {
            OfdWidget::with_component(&mut element, &command);
        }
    }

    fn teardown(&self, (): &mut Self::ViewState, _: &mut ViewCtx, _: Mut<'_, Self::Element>) {}

    fn message(
        &self,
        (): &mut Self::ViewState,
        message: &mut MessageCtx,
        _element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<()> {
        debug_assert!(
            message.remaining_path().is_empty(),
            "id path should be empty in OfdView::message"
        );
        let Some(action) = message.take_message::<OfdWidgetAction>() else {
            return MessageResult::Stale;
        };
        let mut handled = false;
        match *action {
            OfdWidgetAction::Changed => {
                if let Some(f) = &self.on_change {
                    f(app_state, ());
                    handled = true;
                }
            }
            OfdWidgetAction::SelectionChanged(sel) => {
                if let Some(f) = &self.on_selection_change {
                    f(app_state, sel);
                    handled = true;
                }
            }
            OfdWidgetAction::CursorChanged(cursor) => {
                if let Some(f) = &self.on_cursor_change {
                    f(app_state, cursor);
                    handled = true;
                }
            }
            OfdWidgetAction::SaveRequested => {
                if let Some(f) = &self.on_save_request {
                    f(app_state, ());
                    handled = true;
                }
            }
            OfdWidgetAction::ContextMenu { pos, target } => {
                if let Some(f) = &self.on_context_menu {
                    f(app_state, (pos, target));
                    handled = true;
                }
            }
            OfdWidgetAction::AnnotationFocus(id) => {
                if let Some(f) = &self.on_annotation_focus {
                    f(app_state, id);
                    handled = true;
                }
            }
            OfdWidgetAction::AnnotationInteract(id) => {
                if let Some(f) = &self.on_annotation_interact {
                    f(app_state, id);
                    handled = true;
                }
            }
            OfdWidgetAction::PageChanged(idx) => {
                if let Some(f) = &self.on_page_change {
                    f(app_state, idx);
                    handled = true;
                }
            }
            OfdWidgetAction::ZoomChanged(zoom) => {
                if let Some(f) = &self.on_zoom_change {
                    f(app_state, zoom);
                    handled = true;
                }
            }
            OfdWidgetAction::TextSelectionChanged(sel) => {
                if let Some(f) = &self.on_text_selection_change {
                    f(app_state, sel);
                    handled = true;
                }
            }
            OfdWidgetAction::Warnings(warnings) => {
                if let Some(f) = &self.on_warnings {
                    f(app_state, warnings);
                    handled = true;
                }
            }
            // Consumed internally by the widget drain; never submitted.
            OfdWidgetAction::PointerCursorChanged(_) => {}
        }
        if handled {
            MessageResult::Action(())
        } else {
            MessageResult::Nop
        }
    }
}
