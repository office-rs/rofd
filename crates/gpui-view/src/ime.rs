// Ported from rword@4e0b25b6 (crates/gpui-view/src/ime.rs) — pure core
// only; the gpui EntityInputHandler impl lands in view.rs with the view
// type. rofd divergence: the preedit field is `caret` (rword: `cursor`).

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
