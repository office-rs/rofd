// Ported from rword@4e0b25b6 (crates/gpui-view/src/ime.rs) — pure core
// plus the gpui EntityInputHandler delegate on the view type. rofd
// divergence: the preedit field is `caret` (rword: `cursor`).

//! Pull->push IME bridge.
//!
//! gpui's `EntityInputHandler` is the NSTextInputClient-shaped PULL model;
//! the kernel is PUSH (`ViewEvent::ImePreedit`/`ImeCommit`, with
//! `ImePreedit{""}` cancelling and the kernel rendering preedit itself —
//! component `apply_preedit`/`commit_text`).
//!
//! [`ImeBridge`] answers pull queries from a COMPOSITION-ONLY shadow
//! store: the IME sees exactly the text it is composing (sufficient for
//! the standard Windows composition call sequence; reconversion over
//! arbitrary document text degrades to empty answers, per the spec's IME
//! ladder). Every mutation returns the `ViewEvent`s to push into the
//! kernel — the trait impl on the view is a thin delegate.

use std::ops::Range;

use rofd_component::ViewEvent;

/// What `unmark_text` means for the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmarkOutcome {
    /// The marked text is confirmed as-is -> commit the kernel preedit.
    CommitMarked,
    /// Nothing was composing.
    Idle,
}

/// Pure core of the gpui-side IME adapter (no gpui types — unit-testable).
#[derive(Default)]
pub struct ImeBridge {
    preedit: String,
}

impl ImeBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_composing(&self) -> bool {
        !self.preedit.is_empty()
    }

    /// `replace_and_mark_text_in_range` — composition update.
    /// The range argument addresses the previous composition only; the
    /// store REPLACES wholesale (that is what every observed Windows IME
    /// sequence does with our composition-only text).
    pub fn replace_and_mark(&mut self, _range: Option<Range<usize>>, text: &str) -> Vec<ViewEvent> {
        self.preedit.clear();
        self.preedit.push_str(text);
        vec![ViewEvent::ImePreedit {
            text: text.to_string(),
            caret: None,
        }]
    }

    /// `replace_text_in_range` — commit of the final string. Kernel
    /// contract (component `commit_text`): while composing, ImeCommit
    /// inserts PREEDIT.text and ignores the event's own text (the browser
    /// compositionend contract). TSF's direct-commit flavor (MS Pinyin
    /// Space: replace_text(final) with no preceding replace_and_mark of
    /// the conversion) therefore needs the preedit SYNCED to the final
    /// string first — a lone ImeCommit would insert the stale pinyin.
    pub fn replace_text(&mut self, _range: Option<Range<usize>>, text: &str) -> Vec<ViewEvent> {
        if text.is_empty() {
            return self.cancel();
        }
        // Plain (non-composing) input — every ordinary keystroke rides
        // this channel: the kernel's ImeCommit fallback inserts the event
        // text directly when no preedit is active, so a lone commit
        // avoids per-char preedit churn.
        if self.preedit.is_empty() {
            return vec![ViewEvent::ImeCommit {
                text: text.to_string(),
            }];
        }
        let events = vec![
            ViewEvent::ImePreedit {
                text: text.to_string(),
                caret: None,
            },
            ViewEvent::ImeCommit {
                text: text.to_string(),
            },
        ];
        // The sync-preedit event carries the text; after the commit the
        // marked range is gone in TSF, so the shadow store empties.
        self.preedit.clear();
        events
    }

    /// `unmark_text` — TSF's confirm-as-is commit flavor: the final string
    /// is already marked and the IME only unmarks. Per TSF semantics that
    /// COMMITS the marked text (true cancels arrive as an empty
    /// replacement first, which already cleared the store). The caller
    /// commits the kernel's preedit via `force_commit_preedit()`.
    pub fn unmark(&mut self) -> UnmarkOutcome {
        if self.preedit.is_empty() {
            return UnmarkOutcome::Idle;
        }
        self.preedit.clear();
        UnmarkOutcome::CommitMarked
    }

    /// Composition cancelled without a commit: drop the composing text
    /// (xilem-line semantics: no residue).
    pub fn cancel(&mut self) -> Vec<ViewEvent> {
        if self.preedit.is_empty() {
            return Vec::new();
        }
        self.preedit.clear();
        vec![ViewEvent::ImePreedit {
            text: String::new(),
            caret: None,
        }]
    }

    pub fn text_length_utf16(&self) -> usize {
        self.preedit.encode_utf16().count()
    }

    /// The marked range is the whole composition.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.is_composing().then(|| 0..self.text_length_utf16())
    }

    /// Selection anchor for IME queries: caret at the composition's end.
    pub fn caret_utf16(&self) -> usize {
        self.text_length_utf16()
    }

    /// Slice the composition by UTF-16 offsets; `None` for ranges that do
    /// not fall on char boundaries (never tear surrogate pairs).
    pub fn slice_utf16(&self, range: Range<usize>) -> Option<String> {
        let s = self
            .preedit
            .get(self.offset_from_utf16(range.start)?..self.offset_from_utf16(range.end)?);
        s.map(str::to_string)
    }

    /// UTF-16 offset -> byte offset (`None` off a char boundary).
    fn offset_from_utf16(&self, utf16_offset: usize) -> Option<usize> {
        let mut units = 0usize;
        for (idx, ch) in self.preedit.char_indices() {
            if units == utf16_offset {
                return Some(idx);
            }
            units += ch.len_utf16();
        }
        (units == utf16_offset).then_some(self.preedit.len())
    }
}

// The window-side delegate: gpui's pull model answered by the bridge.
// Implemented here (not in view.rs) to keep all IME logic in one file;
// thin by construction — every method maps to a bridge call plus a
// text-changed notification.
impl gpui::EntityInputHandler for crate::view::OfdGpuiView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<String> {
        self.ime.slice_utf16(range)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        let caret = self.ime.caret_utf16();
        Some(gpui::UTF16Selection {
            range: caret..caret,
            reversed: false,
        })
    }

    fn set_selected_text_range(
        &mut self,
        _range: Range<usize>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        // Composition-only store: the kernel owns the real selection.
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<usize> {
        Some(self.ime.text_length_utf16())
    }

    fn marked_text_range(
        &self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<Range<usize>> {
        self.ime.marked_range()
    }

    fn unmark_text(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        match self.ime.unmark() {
            UnmarkOutcome::CommitMarked => {
                // Kernel preedit holds the confirmed string; commit it
                // directly (same entry the kernel's focus-loss path uses).
                self.component.force_commit_preedit();
                cx.notify();
            }
            UnmarkOutcome::Idle => {}
        }
        window.notify_text_input_changed(&self.focus_handle);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for event in self.ime.replace_text(range, new_text) {
            self.handle_view_event(event, cx);
        }
        window.notify_text_input_changed(&self.focus_handle);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for event in self.ime.replace_and_mark(range, new_text) {
            self.handle_view_event(event, cx);
        }
        window.notify_text_input_changed(&self.focus_handle);
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        element_bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        // Candidate-window anchor: the kernel caret rect is VIEWPORT-local;
        // the platform consumes client coordinates, so translate by the
        // element origin (without it the composition window anchors a
        // toolbar-height too high).
        let caret = self.component.caret_rect()?;
        Some(gpui::Bounds {
            origin: gpui::Point::new(
                element_bounds.origin.x + gpui::px(caret.x as f32),
                element_bounds.origin.y + gpui::px(caret.y as f32),
            ),
            size: gpui::size(gpui::px(caret.w.max(1.0) as f32), gpui::px(caret.h as f32)),
        })
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<usize> {
        None
    }
}
