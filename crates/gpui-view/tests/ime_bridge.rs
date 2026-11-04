// Ported from rword@4e0b25b6 (crates/gpui-view/tests/ime_bridge.rs) —
// rofd divergence: the preedit field is `caret` (rword: `cursor`).

//! The pull->push IME bridge (composition-only store).
//!
//! gpui exposes the NSTextInputClient-shaped PULL model; the kernel is
//! PUSH (ViewEvent::ImePreedit / ImeCommit; kernel-side renders preedit,
//! and `ImePreedit{""}` cancels — component commit_text). The bridge
//! answers IME queries from a COMPOSITION-ONLY shadow store (the composing
//! text), which is sufficient for the standard Windows composition call
//! sequence; reconversion over arbitrary document text degrades (empty
//! answers) — acceptable per the spec's IME ladder.
//!
//! The pure core (`ImeBridge`) is unit-tested here; the EntityInputHandler
//! impl on the view is a thin delegate that lands with the view task.

use rofd_component::ViewEvent;
use rofd_gpui_view::ime::{ImeBridge, UnmarkOutcome};

#[test]
fn replace_and_mark_maps_to_preedit() {
    let mut ime = ImeBridge::new();
    let events = ime.replace_and_mark(None, "你");
    assert_eq!(
        events,
        vec![ViewEvent::ImePreedit {
            text: "你".to_string(),
            caret: None,
        }]
    );
    assert!(ime.is_composing());
    assert_eq!(ime.marked_range(), Some(0..1));
}

#[test]
fn mark_update_replaces_text() {
    let mut ime = ImeBridge::new();
    ime.replace_and_mark(None, "ni");
    let events = ime.replace_and_mark(None, "nihao");
    assert_eq!(
        events,
        vec![ViewEvent::ImePreedit {
            text: "nihao".to_string(),
            caret: None,
        }]
    );
    assert_eq!(ime.text_length_utf16(), 5);
}

#[test]
fn replace_text_maps_to_commit() {
    let mut ime = ImeBridge::new();
    ime.replace_and_mark(None, "nihao");
    // Kernel contract (component commit_text): while composing, ImeCommit
    // inserts the PREEDIT text and ignores the event text (the browser
    // compositionend contract). MS Pinyin's Space commit is
    // replace_text(final) WITHOUT a preceding replace_and_mark — so the
    // bridge must SYNC the preedit to the final string first, or the
    // pinyin gets committed instead of the conversion.
    let events = ime.replace_text(None, "你好");
    assert_eq!(
        events,
        vec![
            ViewEvent::ImePreedit {
                text: "你好".to_string(),
                caret: None,
            },
            ViewEvent::ImeCommit {
                text: "你好".to_string(),
            },
        ]
    );
    assert!(!ime.is_composing(), "commit clears the composition");
}

#[test]
fn plain_char_commit_is_single_event() {
    // No active composition (every ordinary keystroke lands here): one
    // ImeCommit, no preedit churn — the kernel's ImeCommit fallback
    // inserts the event text directly when no preedit is active.
    let mut ime = ImeBridge::new();
    let events = ime.replace_text(None, "b");
    assert_eq!(
        events,
        vec![ViewEvent::ImeCommit {
            text: "b".to_string(),
        }]
    );
    assert!(!ime.is_composing());
}

#[test]
fn unmark_commits_marked_text() {
    // TSF's other commit flavor: the final string is already MARKED
    // (replace_and_mark), then the IME only calls unmark_text — per TSF
    // semantics that CONFIRMS the marked text (true cancels arrive as an
    // empty replacement first, which clears the store before unmark).
    let mut ime = ImeBridge::new();
    ime.replace_and_mark(None, "你好");
    assert_eq!(ime.unmark(), UnmarkOutcome::CommitMarked);
    assert!(!ime.is_composing());
    // Unmark with nothing composing is a no-op.
    assert_eq!(ime.unmark(), UnmarkOutcome::Idle);
}

#[test]
fn empty_replacement_cancels() {
    let mut ime = ImeBridge::new();
    ime.replace_and_mark(None, "ni");
    let events = ime.replace_text(None, "");
    assert_eq!(
        events,
        vec![ViewEvent::ImePreedit {
            text: String::new(),
            caret: None,
        }]
    );
    assert!(!ime.is_composing());
    // Cancelling with nothing composing emits nothing.
    assert!(ime.cancel().is_empty());
}

#[test]
fn empty_commit_text_is_cancel_not_empty_commit() {
    let mut ime = ImeBridge::new();
    ime.replace_and_mark(None, "ni");
    let events = ime.replace_text(None, "");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ViewEvent::ImePreedit { .. }));
}

#[test]
fn utf16_round_trip() {
    let mut ime = ImeBridge::new();
    // 你 (1 utf16 unit, BMP) + 好 (1) + 👋 (surrogate pair, 2) = 4 units.
    ime.replace_and_mark(None, "你好👋");
    assert_eq!(ime.text_length_utf16(), 4);
    assert_eq!(ime.slice_utf16(1..4), Some("好👋".to_string()));
    assert_eq!(ime.slice_utf16(0..1), Some("你".to_string()));
    // Out-of-range slices answer None rather than panicking.
    assert_eq!(ime.slice_utf16(3..7), None);
    // A reversed range (start beyond end) answers None too; built from a
    // variable so clippy's reversed-empty-range lint lets the intent
    // (panic-freedom on garbage input) stand.
    let reversed_end = 2;
    assert_eq!(ime.slice_utf16(3..reversed_end), None);
    // Slicing inside the surrogate pair (units 2..3) answers None —
    // never tear a surrogate pair.
    assert_eq!(ime.slice_utf16(2..3), None);
    // The marked range is the whole composition.
    assert_eq!(ime.marked_range(), Some(0..4));
}
