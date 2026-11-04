//! Minimal first-light shell: the editor fills the window and a status
//! line reports the CLI load outcome. Toolbar, file dialogs, and event
//! mirrors arrive with the later gpui-line tasks.

use gpui::{div, prelude::*, px, rgb, Context, Entity, Render, Window};
use rofd_gpui_view::OfdGpuiView;

pub struct AppShell {
    /// The editor entity (created by main, mounted here).
    pub editor: Entity<OfdGpuiView>,
    /// One-line host status (CLI load result; later, operation outcomes).
    pub status: String,
}

impl AppShell {
    pub fn new(editor: Entity<OfdGpuiView>, status: String) -> Self {
        Self { editor, status }
    }
}

impl Render for AppShell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // The editor wrapper MUST be flex_1 AND overflow_hidden together:
        // flex_1 without clipping lets the relative(1.0) element cover the
        // whole window; overflow_hidden without flex_1 collapses it to 0.
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(div().flex_1().overflow_hidden().child(self.editor.clone()))
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .bg(rgb(0xFAFAFA))
                    .border_t_1()
                    .border_color(rgb(0xDDDDDD))
                    .child(self.status.clone()),
            )
    }
}
