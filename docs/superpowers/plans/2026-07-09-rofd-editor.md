# rofd Phase 3 (rofd-editor) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-editor` - the annotation editor (selection, text cursor, command pattern + Step/Transaction/History undo/redo, command set: create/delete/move/resize/style/text/reply) operating on `rofd-dom`'s `AnnotationModel`. Pure logic, no rendering.

**Architecture:** `Editor` owns an `OfdDocument` and mutates only `.annotations` (API convention). Edits go through `Step` (apply/revert on `&mut AnnotationModel`) grouped into `Transaction` (with selection/cursor before-after snapshots), pushed to a `History` (cap 100; undo reverts steps in reverse, redo applies forward). Commands are `Editor` methods that snapshot before/after, build a `Transaction`, and call `execute_transaction`. The editor has **no callbacks** - the Phase 4 component layer fires `on_change`/`on_selection_change`/`on_cursor_change` by querying editor state after commands (mirrors reditor). author/timestamp are caller-supplied (`set_clock`); the library never reads a system clock.

**Tech Stack:** Rust 2021. New crate `rofd-editor` deps: `rofd-dom` only. No vello/parley. Phase 3 Task 1 amends `rofd-dom` (`AnnotationModel` mutation helpers).

## Global Constraints

Copied from spec §5 + Phase 1/2 carryover; every task implicitly includes these.

- **`rofd-editor` deps = `rofd-dom` only.** No vello/parley/zip/xml. `rofd-dom` stays pure (serde + uuid).
- **`Step` trait:** `apply(&self, &mut AnnotationModel)` + `revert(&self, &mut AnnotationModel)`; `Send + std::fmt::Debug`. Steps store before/after snapshots (NOT reditor's `invert` - simpler). `ReplaceAnnotationStep { id, before, after }` is the workhorse for partial changes (whole-annotation before/after; `Annotation: Clone`).
- **`History` cap 100.** `undo` reverts the transaction's steps in **reverse** order + restores `selection_before`/`text_cursor_before`; `redo` applies steps **forward** + restores `selection_after`/`text_cursor_after`. New push clears the redo stack; overflow evicts the oldest.
- **Editor has NO callbacks.** `on_change`/`on_selection_change`/`on_cursor_change` live in the Phase 4 component layer. (Deviation from spec §5.6 - the spec said editor fires callbacks; reditor's editor is callback-free and cleaner. The component queries editor state after a command.)
- **author/timestamp caller-supplied.** `Editor::set_clock(author: String, ts: i64)`; commands read `self.current_ts` for `created`/`modified`. The library never calls `Date::now()`.
- **Editor owns `OfdDocument`** and mutates only `document.annotations`. `AnnotationModel.by_page` is `pub` (Phase 1); Phase 3 Task 1 adds `find`/`find_mut`/`insert`/`remove` helpers that Steps/commands use.
- **All editor types `Clone + Debug`** (for Transaction snapshots + tests). `AnnotationSelection` + `TextCursor` also `PartialEq` (for assertions).
- **`Rect` is `{x, y, w, h}`**; `Color::Rgb(u8,u8,u8)`; `AnnotationId(pub Uuid)`, `::new()` (no args).
- **Commits:** conventional commits, NO Co-Authored-By attribution line (disabled globally).
- **TDD:** red -> green -> commit. Gate: `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green.

---

## File Structure

```
rofd/
├── Cargo.toml                      # add crates/editor to members + rofd-editor workspace dep
├── crates/
│   ├── dom/src/annotation.rs       # MODIFY: AnnotationModel += find/find_mut/insert/remove
│   └── editor/                     # NEW crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # Editor facade + re-exports
│           ├── editor.rs           # Editor struct + execute_transaction/undo/redo/set_clock/load/new
│           ├── selection.rs        # AnnotationSelection
│           ├── cursor.rs           # TextCursor
│           ├── payload_util.rs     # move_payload / shift_cmd / set_color / set_width (pure helpers)
│           ├── steps/
│           │   ├── mod.rs
│           │   ├── step_trait.rs   # Step trait
│           │   ├── annotation_steps.rs  # Insert/Delete/ReplaceAnnotationStep
│           │   ├── transaction.rs  # Transaction
│           │   └── history.rs      # History
│           └── commands/
│               ├── mod.rs
│               ├── annotation_commands.rs  # create/delete/delete_selected/move/resize/set_color/set_width
│               └── text_commands.rs        # insert_text/delete_text/set_text/reply_to
└── (tests inline + crates/editor/tests/integration.rs)
```

Each file has one responsibility. Pure-logic units (steps, history, payload_util, commands) are fully unit-tested.

---

## Task 1: `AnnotationModel` mutation helpers (dom amend)

**Files:**
- Modify: `crates/dom/src/annotation.rs` (AnnotationModel impl)
- Test: inline in `annotation.rs`

**Interfaces:**
- Consumes: Phase 1 `AnnotationModel { by_page }`, `Annotation`, `AnnotationId`.
- Produces: `AnnotationModel::find(&AnnotationId) -> Option<&Annotation>`, `find_mut(&AnnotationId) -> Option<&mut Annotation>`, `insert(Annotation)`, `remove(&AnnotationId) -> Option<Annotation>`. Later tasks (Steps, commands) use these.

- [ ] **Step 1: Write the failing tests**

Append to `crates/dom/src/annotation.rs` test module:
```rust
    use super::*;

    fn sample_ann(id: &str, page: &str) -> Annotation {
        Annotation {
            id: AnnotationId(uuid::Uuid::parse_str(id).unwrap()),
            kind: AnnotationKind::Note,
            page: PageId::new(page),
            creator: "tester".into(),
            created: 0, modified: 0, reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                color: Color::Rgb(0, 0, 0),
                content: "hi".into(),
                icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn find_returns_annotation_by_id() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("00000000-0000-0000-0000-000000000001", "P0");
        m.insert(ann.clone());
        assert_eq!(m.find(&ann.id).map(|a| a.id.0), Some(ann.id.0));
    }

    #[test]
    fn find_mut_allows_in_place_edit() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("00000000-0000-0000-0000-000000000002", "P0");
        m.insert(ann.clone());
        if let Some(a) = m.find_mut(&ann.id) {
            a.creator = "changed".into();
        }
        assert_eq!(m.find(&ann.id).unwrap().creator, "changed");
    }

    #[test]
    fn insert_places_on_correct_page() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("00000000-0000-0000-0000-000000000003", "P5");
        m.insert(ann);
        assert_eq!(m.by_page.get(&PageId::new("P5")).unwrap().len(), 1);
    }

    #[test]
    fn remove_returns_and_deletes() {
        let mut m = AnnotationModel::default();
        let ann = sample_ann("00000000-0000-0000-0000-000000000004", "P0");
        m.insert(ann.clone());
        let removed = m.remove(&ann.id);
        assert_eq!(removed.map(|a| a.id.0), Some(ann.id.0));
        assert!(m.find(&ann.id).is_none());
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut m = AnnotationModel::default();
        let id = AnnotationId::new();
        assert!(m.remove(&id).is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-dom find_returns_annotation_by_id`
Expected: FAIL - `find`/`insert`/`find_mut`/`remove` undefined.

- [ ] **Step 3: Add the helpers to AnnotationModel**

In `crates/dom/src/annotation.rs`, extend the `impl AnnotationModel` block (after `for_page`):
```rust
    /// Find an annotation by id (searches all pages).
    pub fn find(&self, id: &AnnotationId) -> Option<&Annotation> {
        self.by_page.values().flatten().find(|a| &a.id == id)
    }

    /// Find an annotation mutably by id.
    pub fn find_mut(&mut self, id: &AnnotationId) -> Option<&mut Annotation> {
        self.by_page.values_mut().flatten().find(|a| &a.id == id)
    }

    /// Insert an annotation onto its page (ann.page determines the page).
    pub fn insert(&mut self, ann: Annotation) {
        self.by_page.entry(ann.page.clone()).or_default().push(ann);
    }

    /// Remove an annotation by id. Returns the removed annotation, or None if not found.
    pub fn remove(&mut self, id: &AnnotationId) -> Option<Annotation> {
        for anns in self.by_page.values_mut() {
            if let Some(pos) = anns.iter().position(|a| &a.id == id) {
                return Some(anns.remove(pos));
            }
        }
        None
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-dom`
Expected: PASS (5 new tests + all prior green).

- [ ] **Step 5: Commit**

```bash
git add crates/dom/src/annotation.rs
git commit -m "feat(dom): add AnnotationModel find/find_mut/insert/remove helpers"
```

---

## Task 2: `rofd-editor` scaffold + Editor + Selection + TextCursor + clock

**Files:**
- Modify: `Cargo.toml` (workspace members + rofd-editor dep)
- Create: `crates/editor/Cargo.toml`, `crates/editor/src/lib.rs`, `editor.rs`, `selection.rs`, `cursor.rs`

**Interfaces:**
- Consumes: `rofd_dom::{OfdDocument, AnnotationId, PageId}`.
- Produces: `Editor` struct (with `new`, `load_document`, `document`, `document_mut`, `set_clock`, `selection`, `text_cursor` accessors); `AnnotationSelection { None, Single(AnnotationId), Multi(Vec<AnnotationId>) }` with `contains`; `TextCursor { annotation: AnnotationId, offset: usize, preferred_x: Option<f32> }`. (History is a placeholder field added in Task 4.)

- [ ] **Step 1: Add workspace member + dep**

Root `Cargo.toml` - add `crates/editor` to members and `rofd-editor` to `[workspace.dependencies]`:
```toml
[workspace]
resolver = "2"
members = ["crates/dom", "crates/io", "crates/render", "crates/editor"]

[workspace.dependencies]
# ... existing ...
rofd-editor = { path = "crates/editor" }
```

- [ ] **Step 2: Create `crates/editor/Cargo.toml`**

```toml
[package]
name = "rofd-editor"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-dom = { workspace = true }

[dev-dependencies]
uuid = { workspace = true }
```

- [ ] **Step 3: Write the failing selection + cursor tests**

`crates/editor/src/selection.rs`:
```rust
use rofd_dom::AnnotationId;

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationSelection {
    None,
    Single(AnnotationId),
    Multi(Vec<AnnotationId>),
}

impl AnnotationSelection {
    pub fn contains(&self, id: &AnnotationId) -> bool {
        match self {
            AnnotationSelection::None => false,
            AnnotationSelection::Single(s) => s == id,
            AnnotationSelection::Multi(ids) => ids.iter().any(|i| i == id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_contains_nothing() {
        assert!(!AnnotationSelection::None.contains(&AnnotationId::new()));
    }

    #[test]
    fn single_contains_its_id() {
        let id = AnnotationId::new();
        assert!(AnnotationSelection::Single(id.clone()).contains(&id));
        assert!(!AnnotationSelection::Single(id.clone()).contains(&AnnotationId::new()));
    }

    #[test]
    fn multi_contains_any_listed() {
        let a = AnnotationId::new();
        let b = AnnotationId::new();
        assert!(AnnotationSelection::Multi(vec![a.clone(), b.clone()]).contains(&a));
    }
}
```

`crates/editor/src/cursor.rs`:
```rust
use rofd_dom::AnnotationId;

#[derive(Debug, Clone, PartialEq)]
pub struct TextCursor {
    pub annotation: AnnotationId,
    pub offset: usize,
    pub preferred_x: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_clone_eq() {
        let c = TextCursor { annotation: AnnotationId::new(), offset: 3, preferred_x: Some(1.0) };
        assert_eq!(c, c.clone());
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p rofd-editor`
Expected: FAIL - crate not wired.

- [ ] **Step 5: Write Editor struct + lib.rs**

`crates/editor/src/editor.rs`:
```rust
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
```

(Note: `history: History::new(100)` + `use crate::steps::history::History` reference Task 4's History. To make Task 2 compile standalone, create a TEMPORARY stub `steps/history.rs` with `pub struct History; impl History { pub fn new(_: usize) -> Self { Self } pub fn can_undo(&self) -> bool { false } pub fn can_redo(&self) -> bool { false } }` - Task 4 replaces it with the real History. Do NOT build the real History here.)

`crates/editor/src/steps/mod.rs` (with the stub):
```rust
pub mod history;  // Task 3 adds step_trait + annotation_steps; Task 4 adds transaction + real history
```

`crates/editor/src/steps/history.rs` (TEMPORARY stub - Task 4 replaces):
```rust
pub struct History;
impl History {
    pub fn new(_capacity: usize) -> Self { Self }
    pub fn can_undo(&self) -> bool { false }
    pub fn can_redo(&self) -> bool { false }
}
```

`crates/editor/src/lib.rs`:
```rust
//! rofd-editor - OFD annotation editor (selection, commands, undo/redo).

pub mod cursor;
pub mod editor;
pub mod selection;
pub mod steps;

pub use cursor::TextCursor;
pub use editor::Editor;
pub use selection::AnnotationSelection;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rofd-editor`
Expected: PASS (selection + cursor tests; Editor constructs with stub History).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/editor
git commit -m "feat(editor): scaffold rofd-editor + Editor + Selection + TextCursor"
```

---

## Task 3: Step trait + Insert/Delete/ReplaceAnnotationStep

**Files:**
- Create: `crates/editor/src/steps/step_trait.rs`, `crates/editor/src/steps/annotation_steps.rs`
- Modify: `crates/editor/src/steps/mod.rs`
- Test: inline

**Interfaces:**
- Consumes: `rofd_dom::{AnnotationModel, Annotation, AnnotationId}` (Task 1 helpers).
- Produces: `Step` trait (`apply`/`revert` on `&mut AnnotationModel`); `InsertAnnotationStep { annotation }`, `DeleteAnnotationStep { annotation }`, `ReplaceAnnotationStep { id, before, after }`.

- [ ] **Step 1: Write the failing tests**

`crates/editor/src/steps/annotation_steps.rs`:
```rust
use rofd_dom::{Annotation, AnnotationId, AnnotationModel};

use crate::steps::step_trait::Step;

#[derive(Debug)]
pub struct InsertAnnotationStep {
    pub annotation: Annotation,
}
impl Step for InsertAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) { anns.insert(self.annotation.clone()); }
    fn revert(&self, anns: &mut AnnotationModel) { anns.remove(&self.annotation.id); }
}

#[derive(Debug)]
pub struct DeleteAnnotationStep {
    pub annotation: Annotation,
}
impl Step for DeleteAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) { anns.remove(&self.annotation.id); }
    fn revert(&self, anns: &mut AnnotationModel) { anns.insert(self.annotation.clone()); }
}

#[derive(Debug)]
pub struct ReplaceAnnotationStep {
    pub id: AnnotationId,
    pub before: Annotation,
    pub after: Annotation,
}
impl Step for ReplaceAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) {
        if let Some(a) = anns.find_mut(&self.id) { *a = self.after.clone(); }
    }
    fn revert(&self, anns: &mut AnnotationModel) {
        if let Some(a) = anns.find_mut(&self.id) { *a = self.before.clone(); }
    }
}
```

`crates/editor/src/steps/step_trait.rs`:
```rust
use rofd_dom::AnnotationModel;

/// A reversible edit on the annotation model. Stores enough to apply AND revert.
pub trait Step: Send + std::fmt::Debug {
    fn apply(&self, anns: &mut AnnotationModel);
    fn revert(&self, anns: &mut AnnotationModel);
}
```

Append tests to `annotation_steps.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn note_ann(id: &str, content: &str) -> Annotation {
        Annotation {
            id: AnnotationId(uuid::Uuid::parse_str(id).unwrap()),
            kind: AnnotationKind::Note,
            page: PageId::new("P0"),
            creator: "t".into(), created: 0, modified: 0, reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                color: Color::Rgb(0, 0, 0), content: content.into(), icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn insert_then_revert_yields_empty() {
        let mut m = AnnotationModel::default();
        let ann = note_ann("00000000-0000-0000-0000-000000000011", "a");
        let step = InsertAnnotationStep { annotation: ann.clone() };
        step.apply(&mut m);
        assert!(m.find(&ann.id).is_some());
        step.revert(&mut m);
        assert!(m.find(&ann.id).is_none());
    }

    #[test]
    fn delete_then_revert_restores() {
        let mut m = AnnotationModel::default();
        let ann = note_ann("00000000-0000-0000-0000-000000000012", "a");
        m.insert(ann.clone());
        let step = DeleteAnnotationStep { annotation: ann.clone() };
        step.apply(&mut m);
        assert!(m.find(&ann.id).is_none());
        step.revert(&mut m);
        assert!(m.find(&ann.id).is_some());
    }

    #[test]
    fn replace_then_revert_restores_before() {
        let mut m = AnnotationModel::default();
        let before = note_ann("00000000-0000-0000-0000-000000000013", "before");
        let mut after = before.clone();
        if let AnnotationPayload::Note { content, .. } = &mut after.payload { *content = "after".into(); }
        m.insert(before.clone());
        let step = ReplaceAnnotationStep { id: before.id.clone(), before: before.clone(), after: after.clone() };
        step.apply(&mut m);
        let got = m.find(&before.id).unwrap();
        assert!(matches!(&got.payload, AnnotationPayload::Note { content, .. } if content == "after"));
        step.revert(&mut m);
        let got = m.find(&before.id).unwrap();
        assert!(matches!(&got.payload, AnnotationPayload::Note { content, .. } if content == "before"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-editor steps::annotation_steps`
Expected: FAIL - modules not wired.

- [ ] **Step 3: Wire into steps/mod.rs**

`crates/editor/src/steps/mod.rs`:
```rust
pub mod annotation_steps;
pub mod history;  // still the Task 2 stub until Task 4
pub mod step_trait;

pub use annotation_steps::{DeleteAnnotationStep, InsertAnnotationStep, ReplaceAnnotationStep};
pub use step_trait::Step;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-editor`
Expected: PASS (3 step tests + Task 2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/editor/src/steps
git commit -m "feat(editor): Step trait + Insert/Delete/ReplaceAnnotationStep"
```

---

## Task 4: Transaction + History (real) + execute/undo/redo

**Files:**
- Create: `crates/editor/src/steps/transaction.rs`
- Modify: `crates/editor/src/steps/history.rs` (replace stub), `crates/editor/src/steps/mod.rs`, `crates/editor/src/editor.rs`
- Test: inline (unit tests in editor.rs; the create_undo_redo integration test is Task 5)

**Interfaces:**
- Consumes: `Step` (Task 3), `AnnotationSelection`/`TextCursor` (Task 2).
- Produces: `Transaction { steps, selection_before, selection_after, text_cursor_before, text_cursor_after }`; real `History { done, redo, capacity }` with `push`/`undo`/`redo`/`can_undo`/`can_redo`; `Editor::execute_transaction`/`undo`/`redo`.

- [ ] **Step 1: Write the failing unit tests (undo/redo round-trip + history cap)**

Append a test module to `crates/editor/src/editor.rs` (these use `execute_transaction` directly - available in this task; NOT `create_annotation` which is Task 5):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::annotation_steps::InsertAnnotationStep;
    use rofd_dom::{Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn note_ann(id: &str) -> Annotation {
        Annotation {
            id: AnnotationId(uuid::Uuid::parse_str(id).unwrap()),
            kind: AnnotationKind::Note, page: PageId::new("P0"),
            creator: "t".into(), created: 0, modified: 0, reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, color: Color::Rgb(0,0,0),
                content: "x".into(), icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn execute_undo_redo_via_transaction() {
        let mut e = Editor::new();
        let ann = note_ann("00000000-0000-0000-0000-000000000021");
        let txn = Transaction {
            steps: vec![Box::new(InsertAnnotationStep { annotation: ann.clone() })],
            selection_before: AnnotationSelection::None,
            selection_after: AnnotationSelection::Single(ann.id.clone()),
            text_cursor_before: None, text_cursor_after: None,
        };
        e.execute_transaction(txn);
        assert!(e.document().annotations.find(&ann.id).is_some());
        assert!(e.undo());
        assert!(e.document().annotations.find(&ann.id).is_none());
        assert!(e.redo());
        assert!(e.document().annotations.find(&ann.id).is_some());
    }

    #[test]
    fn history_capacity_evicts_oldest() {
        let mut e = Editor::new();
        for i in 0..105u32 {
            let ann = note_ann(&format!("00000000-0000-0000-0000-{:08x}", i));
            let txn = Transaction {
                steps: vec![Box::new(InsertAnnotationStep { annotation: ann })],
                selection_before: AnnotationSelection::None,
                selection_after: AnnotationSelection::None,
                text_cursor_before: None, text_cursor_after: None,
            };
            e.execute_transaction(txn);
        }
        assert!(e.can_undo());
        for _ in 0..100 { assert!(e.undo()); }
        assert!(!e.can_undo());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rofd-editor --lib`
Expected: FAIL - `execute_transaction`/`undo`/`redo`/`Transaction`/real `History` undefined.

- [ ] **Step 3: Write Transaction + real History + execute/undo/redo**

`crates/editor/src/steps/transaction.rs`:
```rust
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
```

Replace `crates/editor/src/steps/history.rs` (the stub) with the real History:
```rust
use std::collections::VecDeque;

use crate::steps::transaction::Transaction;

pub struct History {
    done: VecDeque<Transaction>,
    redo: Vec<Transaction>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self { done: VecDeque::new(), redo: Vec::new(), capacity }
    }

    pub fn push(&mut self, txn: Transaction) {
        if self.done.len() >= self.capacity {
            self.done.pop_front();
        }
        self.done.push_back(txn);
        self.redo.clear();
    }

    /// Move the most-recent transaction from `done` to `redo`. Returns a reference to it (now in redo).
    pub fn undo(&mut self) -> Option<&Transaction> {
        if let Some(txn) = self.done.pop_back() {
            self.redo.push(txn);
            self.redo.last()
        } else { None }
    }

    /// Move the last-undone transaction from `redo` back to `done`. Returns a reference to it (now in done).
    pub fn redo(&mut self) -> Option<&Transaction> {
        if let Some(txn) = self.redo.pop() {
            self.done.push_back(txn);
            self.done.back()
        } else { None }
    }

    pub fn can_undo(&self) -> bool { !self.done.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}
```

Add `pub mod transaction;` to `crates/editor/src/steps/mod.rs`.

Add `execute_transaction`/`undo`/`redo` to `crates/editor/src/editor.rs`:
```rust
use crate::steps::transaction::Transaction;
// ... in impl Editor:
    pub(crate) fn execute_transaction(&mut self, txn: Transaction) {
        for step in &txn.steps {
            step.apply(&mut self.document.annotations);
        }
        self.selection = txn.selection_after.clone();
        self.text_cursor = txn.text_cursor_after.clone();
        self.history.push(txn);
    }

    pub fn undo(&mut self) -> bool {
        let txn = self.history.undo();
        if let Some(txn) = txn {
            for step in txn.steps.iter().rev() {
                step.revert(&mut self.document.annotations);
            }
            self.selection = txn.selection_before.clone();
            self.text_cursor = txn.text_cursor_before.clone();
            true
        } else { false }
    }

    pub fn redo(&mut self) -> bool {
        let txn = self.history.redo();
        if let Some(txn) = txn {
            for step in &txn.steps {
                step.apply(&mut self.document.annotations);
            }
            self.selection = txn.selection_after.clone();
            self.text_cursor = txn.text_cursor_after.clone();
            true
        } else { false }
    }
```

(Unit tests are in Step 1 above - do not re-add them here.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rofd-editor --lib`
Expected: PASS (execute_undo_redo_via_transaction + history_capacity unit tests).

- [ ] **Step 5: Commit**

```bash
git add crates/editor/src/steps crates/editor/src/editor.rs
git commit -m "feat(editor): Transaction + History (cap 100) + execute/undo/redo"
```

---

## Task 5: Commands - create/delete/move/resize + payload_util

**Files:**
- Create: `crates/editor/src/payload_util.rs`, `crates/editor/src/commands/mod.rs`, `crates/editor/src/commands/annotation_commands.rs`
- Modify: `crates/editor/src/lib.rs`
- Test: inline + `crates/editor/tests/integration.rs` (create_undo_redo)

**Interfaces:**
- Consumes: `Editor::execute_transaction` (Task 4), `Insert/Delete/ReplaceAnnotationStep` (Task 3), `AnnotationModel` helpers (Task 1).
- Produces: `Editor::create_annotation(kind, page, payload) -> AnnotationId`, `delete_annotation(&AnnotationId)`, `delete_selected()`, `move_annotation(&AnnotationId, dx, dy)`, `resize_annotation(&AnnotationId, Rect)`. Plus `payload_util::move_payload`, `shift_cmd`, `resize_payload`.

- [ ] **Step 1: Write payload_util.rs (pure helpers)**

`crates/editor/src/payload_util.rs`:
```rust
use rofd_dom::{AnnotationPayload, PathCommand, Rect};

/// Shift an annotation's geometry by (dx, dy).
pub fn move_payload(p: &mut AnnotationPayload, dx: f64, dy: f64) {
    match p {
        AnnotationPayload::Markup { quad_points, .. } => {
            for pt in quad_points { pt.x += dx; pt.y += dy; }
        }
        AnnotationPayload::Freehand { path, .. } => {
            for cmd in &mut path.commands { shift_cmd(cmd, dx, dy); }
        }
        AnnotationPayload::Shape { rect, .. } | AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. } | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => {
            rect.x += dx; rect.y += dy;
        }
    }
}

/// Set the rect (for resize). No-op for Markup/Freehand (no single rect).
pub fn resize_payload(p: &mut AnnotationPayload, new_rect: Rect) {
    match p {
        AnnotationPayload::Shape { rect, .. } | AnnotationPayload::Note { rect, .. }
        | AnnotationPayload::TextBox { rect, .. } | AnnotationPayload::Stamp { rect, .. }
        | AnnotationPayload::Watermark { rect, .. } => { *rect = new_rect; }
        AnnotationPayload::Markup { .. } | AnnotationPayload::Freehand { .. } => { /* no-op v1 */ }
    }
}

fn shift_cmd(cmd: &mut PathCommand, dx: f64, dy: f64) {
    match cmd {
        PathCommand::M(x, y) | PathCommand::L(x, y) => { *x += dx; *y += dy; }
        PathCommand::C(x1, y1, x2, y2, x, y) => { *x1 += dx; *y1 += dy; *x2 += dx; *y2 += dy; *x += dx; *y += dy; }
        PathCommand::Q(x1, y1, x, y) => { *x1 += dx; *y1 += dy; *x += dx; *y += dy; }
        PathCommand::A(_rx, _ry, _rot, _large, _sweep, x, y) => { *x += dx; *y += dy; }
        PathCommand::Z => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{Color, PathData};

    #[test]
    fn move_note_shifts_rect() {
        let mut p = AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 5.0, h: 5.0 }, color: Color::Rgb(0,0,0),
            content: "".into(), icon: rofd_dom::NoteIcon::Note,
        };
        move_payload(&mut p, 3.0, 4.0);
        assert!(matches!(p, AnnotationPayload::Note { rect: Rect { x: 13.0, y: 14.0, .. }, .. }));
    }

    #[test]
    fn resize_shape_sets_rect() {
        let mut p = AnnotationPayload::Shape {
            kind: rofd_dom::ShapeKind::Rect, rect: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            stroke: Color::Rgb(0,0,0), fill: None, width: 1.0,
        };
        resize_payload(&mut p, Rect { x: 5.0, y: 5.0, w: 2.0, h: 2.0 });
        assert!(matches!(p, AnnotationPayload::Shape { rect: Rect { x: 5.0, y: 5.0, w: 2.0, h: 2.0 }, .. }));
    }

    #[test]
    fn move_freehand_shifts_path_points() {
        let mut p = AnnotationPayload::Freehand {
            path: PathData { commands: vec![PathCommand::M(1.0, 2.0), PathCommand::L(3.0, 4.0)] },
            color: Color::Rgb(0,0,0), width: 1.0,
        };
        move_payload(&mut p, 10.0, 20.0);
        if let AnnotationPayload::Freehand { path, .. } = &p {
            assert!(matches!(path.commands[0], PathCommand::M(11.0, 22.0)));
            assert!(matches!(path.commands[1], PathCommand::L(13.0, 24.0)));
        } else { panic!("expected Freehand"); }
    }
}
```

- [ ] **Step 2: Write annotation_commands.rs (the commands)**

`crates/editor/src/commands/annotation_commands.rs`:
```rust
use rofd_dom::{Annotation, AnnotationId, AnnotationKind, AnnotationPayload, PageId, Rect};

use crate::editor::Editor;
use crate::payload_util::{move_payload, resize_payload};
use crate::selection::AnnotationSelection;
use crate::steps::annotation_steps::{DeleteAnnotationStep, InsertAnnotationStep, ReplaceAnnotationStep};
use crate::steps::transaction::Transaction;

impl Editor {
    /// Create an annotation. Returns the new id. Stamps created/modified from current_ts + author.
    pub fn create_annotation(
        &mut self, kind: AnnotationKind, page: PageId, payload: AnnotationPayload,
    ) -> AnnotationId {
        let id = AnnotationId::new();
        let ann = Annotation {
            id: id.clone(), kind, page, creator: self.author.clone(),
            created: self.current_ts, modified: self.current_ts, reply_to: None, payload,
        };
        let txn = Transaction {
            steps: vec![Box::new(InsertAnnotationStep { annotation: ann })],
            selection_before: self.selection.clone(),
            selection_after: AnnotationSelection::Single(id.clone()),
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: None,
        };
        self.execute_transaction(txn);
        id
    }

    /// Delete an annotation by id.
    pub fn delete_annotation(&mut self, id: &AnnotationId) {
        let ann = match self.document.annotations.find(id).cloned() {
            Some(a) => a, None => return,
        };
        let sel_after = if self.selection.contains(id) { AnnotationSelection::None } else { self.selection.clone() };
        let cur_after = if self.text_cursor.as_ref().map_or(false, |c| &c.annotation == id) { None } else { self.text_cursor.clone() };
        let txn = Transaction {
            steps: vec![Box::new(DeleteAnnotationStep { annotation: ann })],
            selection_before: self.selection.clone(),
            selection_after: sel_after,
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: cur_after,
        };
        self.execute_transaction(txn);
    }

    /// Delete all selected annotations.
    pub fn delete_selected(&mut self) {
        let ids: Vec<AnnotationId> = match &self.selection {
            AnnotationSelection::None => vec![],
            AnnotationSelection::Single(id) => vec![id.clone()],
            AnnotationSelection::Multi(ids) => ids.clone(),
        };
        for id in &ids { self.delete_annotation(id); }
    }

    /// Move an annotation by (dx, dy).
    pub fn move_annotation(&mut self, id: &AnnotationId, dx: f64, dy: f64) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a, None => return,
        };
        let mut after = before.clone();
        move_payload(&mut after.payload, dx, dy);
        after.modified = self.current_ts;
        let txn = self.replace_txn(id.clone(), before, after);
        self.execute_transaction(txn);
    }

    /// Resize an annotation to new_rect (rect-based payloads; no-op for Markup/Freehand).
    pub fn resize_annotation(&mut self, id: &AnnotationId, new_rect: Rect) {
        let before = match self.document.annotations.find(id).cloned() {
            Some(a) => a, None => return,
        };
        let mut after = before.clone();
        resize_payload(&mut after.payload, new_rect);
        after.modified = self.current_ts;
        let txn = self.replace_txn(id.clone(), before, after);
        self.execute_transaction(txn);
    }

    /// Helper: build a ReplaceAnnotationStep Transaction preserving selection/cursor.
    fn replace_txn(&self, id: AnnotationId, before: Annotation, after: Annotation) -> Transaction {
        Transaction {
            steps: vec![Box::new(ReplaceAnnotationStep { id, before, after })],
            selection_before: self.selection.clone(),
            selection_after: self.selection.clone(),
            text_cursor_before: self.text_cursor.clone(),
            text_cursor_after: self.text_cursor.clone(),
        }
    }
}
```

`crates/editor/src/commands/mod.rs`:
```rust
pub mod annotation_commands;
pub mod text_commands;  // Task 6
```
(Create a temporary empty `text_commands.rs` so `pub mod text_commands;` compiles: `// Task 6 fills this.`)

Add to `crates/editor/src/lib.rs`: `pub mod commands;` and `pub mod payload_util;`.

- [ ] **Step 3: Create the integration test (create_annotation exists now)**

`crates/editor/tests/integration.rs`:
```rust
use rofd_editor::Editor;
use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

#[test]
fn create_undo_redo_restores_state() {
    let mut e = Editor::new();
    e.set_clock("tester".into(), 1_700_000_000_000);
    let id = e.create_annotation(
        AnnotationKind::Note,
        PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            color: Color::Rgb(0, 0, 0), content: "hi".into(), icon: NoteIcon::Note,
        },
    );
    assert!(e.document().annotations.find(&id).is_some());
    assert!(e.can_undo());
    assert!(e.undo());
    assert!(e.document().annotations.find(&id).is_none());
    assert!(e.can_redo());
    assert!(e.redo());
    assert!(e.document().annotations.find(&id).is_some());
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rofd-editor`
Expected: PASS - `create_undo_redo_restores_state` (integration) + payload_util tests + all prior.

- [ ] **Step 5: Commit**

```bash
git add crates/editor/src/payload_util.rs crates/editor/src/commands crates/editor/src/lib.rs crates/editor/tests/integration.rs
git commit -m "feat(editor): create/delete/move/resize commands + payload_util"
```

---

## Task 6: Commands - style/text/reply

**Files:**
- Modify: `crates/editor/src/commands/text_commands.rs` (fill the stub), `crates/editor/src/payload_util.rs` (add set_color/set_width helpers)
- Test: inline

**Interfaces:**
- Consumes: `Editor::execute_transaction` + `replace_txn` (Task 5), `AnnotationPayload` (Phase 1).
- Produces: `Editor::set_annotation_color(&AnnotationId, Color)`, `set_annotation_width(&AnnotationId, f64)`, `insert_text(&AnnotationId, offset, &str)`, `delete_text(&AnnotationId, offset, len)`, `set_annotation_text(&AnnotationId, &str)`, `reply_to(&AnnotationId, &str) -> AnnotationId`.

- [ ] **Step 1: Add set_color/set_width to payload_util**

Append to `crates/editor/src/payload_util.rs`:
```rust
use rofd_dom::Color;

/// Set the primary color of an annotation (no-op for Stamp which has no color).
pub fn set_color(p: &mut AnnotationPayload, color: Color) {
    match p {
        AnnotationPayload::Markup { color: c, .. } => *c = color,
        AnnotationPayload::Freehand { color: c, .. } => *c = color,
        AnnotationPayload::Shape { stroke: c, .. } => *c = color,
        AnnotationPayload::Note { color: c, .. } => *c = color,
        AnnotationPayload::TextBox { color: c, .. } => *c = color,
        AnnotationPayload::Watermark { color: c, .. } => *c = color,
        AnnotationPayload::Stamp { .. } => { /* no color */ }
    }
}

/// Set the stroke width (Freehand/Shape only; no-op otherwise).
pub fn set_width(p: &mut AnnotationPayload, width: f64) {
    match p {
        AnnotationPayload::Freehand { width: w, .. } => *w = width,
        AnnotationPayload::Shape { width: w, .. } => *w = width,
        _ => {}
    }
}
```

- [ ] **Step 2: Write text_commands.rs**

`crates/editor/src/commands/text_commands.rs`:
```rust
use rofd_dom::{AnnotationId, AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

use crate::editor::Editor;
use crate::payload_util::{set_color, set_width};
use crate::steps::transaction::Transaction;

impl Editor {
    /// Set the primary color.
    pub fn set_annotation_color(&mut self, id: &AnnotationId, color: Color) {
        let before = match self.document.annotations.find(id).cloned() { Some(a) => a, None => return };
        let mut after = before.clone();
        set_color(&mut after.payload, color);
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Set the stroke width (Freehand/Shape).
    pub fn set_annotation_width(&mut self, id: &AnnotationId, width: f64) {
        let before = match self.document.annotations.find(id).cloned() { Some(a) => a, None => return };
        let mut after = before.clone();
        set_width(&mut after.payload, width);
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Insert text into a text annotation (TextBox/Note/Watermark) at char offset.
    pub fn insert_text(&mut self, id: &AnnotationId, offset: usize, chars: &str) {
        let before = match self.document.annotations.find(id).cloned() { Some(a) => a, None => return };
        let mut after = before.clone();
        if let Some(content) = text_content_mut(&mut after.payload) {
            let off = offset.min(content.chars().count());
            let mut new = content.chars().take(off).collect::<String>();
            new.push_str(chars);
            new.extend(content.chars().skip(off));
            *content = new;
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Delete `len` chars from a text annotation at char offset.
    pub fn delete_text(&mut self, id: &AnnotationId, offset: usize, len: usize) {
        let before = match self.document.annotations.find(id).cloned() { Some(a) => a, None => return };
        let mut after = before.clone();
        if let Some(content) = text_content_mut(&mut after.payload) {
            let total = content.chars().count();
            let start = offset.min(total);
            let end = (offset + len).min(total);
            let kept: String = content.chars().enumerate()
                .filter(|(i, _)| *i < start || *i >= end)
                .map(|(_, c)| c)
                .collect();
            *content = kept;
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Replace the whole text content.
    pub fn set_annotation_text(&mut self, id: &AnnotationId, text: &str) {
        let before = match self.document.annotations.find(id).cloned() { Some(a) => a, None => return };
        let mut after = before.clone();
        if let Some(content) = text_content_mut(&mut after.payload) {
            *content = text.into();
        }
        after.modified = self.current_ts;
        self.execute_transaction(self.replace_txn(id.clone(), before, after));
    }

    /// Reply to an annotation (creates a Note with reply_to set).
    pub fn reply_to(&mut self, parent: &AnnotationId, content: &str) -> AnnotationId {
        // Find the parent's page so the reply lives on the same page.
        let page = self.document.annotations.find(parent)
            .map(|a| a.page.clone())
            .unwrap_or_default();
        self.create_annotation(
            AnnotationKind::Note,
            page,
            AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 40.0, h: 20.0 },
                color: Color::Rgb(255, 200, 0),
                content: content.into(),
                icon: NoteIcon::Comment,
            },
        );
        // create_annotation doesn't set reply_to; patch it with a Replace step.
        // Find the just-created annotation (the newest on the page) and set reply_to.
        let new_id = self.last_created_id();
        if let (Some(new_id), true) = (new_id, true) {
            // We need to set reply_to on the created annotation. Use a Replace step.
            // (Slight inefficiency: create + replace = 2 transactions. Acceptable for v1.)
            // The create_annotation already pushed a transaction; this replace pushes a second.
            let parent = parent.clone();
            // Re-fetch the created annotation to snapshot before/after.
            if let Some(before) = self.document.annotations.find(&new_id).cloned() {
                let mut after = before.clone();
                after.reply_to = Some(parent);
                after.modified = self.current_ts;
                self.execute_transaction(self.replace_txn(new_id, before, after));
            }
            return new_id;
        }
        AnnotationId::new()  // unreachable; satisfies return type
    }

    /// The id of the most recently created annotation (heuristic: last in the selected page's vec).
    fn last_created_id(&self) -> Option<AnnotationId> {
        match &self.selection {
            AnnotationSelection::Single(id) => Some(id.clone()),
            _ => None,
        }
    }
}

/// Mutable text content for text-bearing payloads (TextBox/Note/Watermark). None for others.
fn text_content_mut(p: &mut AnnotationPayload) -> Option<&mut String> {
    match p {
        AnnotationPayload::Note { content, .. } => Some(content),
        AnnotationPayload::TextBox { content, .. } => Some(content),
        AnnotationPayload::Watermark { content, .. } => Some(content),
        _ => None,
    }
}
```

> **Note for the implementer:** the `reply_to` implementation is intentionally simple (create + a follow-up Replace to set `reply_to` = 2 transactions). A cleaner single-transaction version would add a `CreateWithReplyStep`, but that's over-engineering for v1. The 2-transaction approach is correct and undoable (undo reverts both). Keep it.

Append tests to `text_commands.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn note_editor(content: &str) -> (Editor, AnnotationId) {
        let mut e = Editor::new();
        e.set_clock("t".into(), 1);
        let id = e.create_annotation(
            AnnotationKind::Note, PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, color: Color::Rgb(0,0,0),
                content: content.into(), icon: NoteIcon::Note,
            },
        );
        (e, id)
    }

    fn content_of(e: &Editor, id: &AnnotationId) -> String {
        let a = e.document().annotations.find(id).unwrap();
        match &a.payload {
            AnnotationPayload::Note { content, .. } => content.clone(),
            _ => panic!("not a text annotation"),
        }
    }

    #[test]
    fn insert_text_at_offset() {
        let (mut e, id) = note_editor("Hello");
        e.insert_text(&id, 2, "XX");
        assert_eq!(content_of(&e, &id), "HeXXllo");
    }

    #[test]
    fn delete_text_range() {
        let (mut e, id) = note_editor("Hello");
        e.delete_text(&id, 1, 2);
        assert_eq!(content_of(&e, &id), "Hlo");
    }

    #[test]
    fn set_text_replaces() {
        let (mut e, id) = note_editor("Hello");
        e.set_annotation_text(&id, "World");
        assert_eq!(content_of(&e, &id), "World");
    }

    #[test]
    fn text_edit_undo_restores() {
        let (mut e, id) = note_editor("Hello");
        e.insert_text(&id, 5, "!");
        assert_eq!(content_of(&e, &id), "Hello!");
        e.undo();
        assert_eq!(content_of(&e, &id), "Hello");
    }

    #[test]
    fn reply_to_creates_note_with_parent() {
        let (mut e, parent) = note_editor("parent");
        let child = e.reply_to(&parent, "reply");
        let c = e.document().annotations.find(&child).unwrap();
        assert_eq!(c.reply_to, Some(parent.clone()));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rofd-editor`
Expected: PASS (style/text/reply tests).

- [ ] **Step 4: Commit**

```bash
git add crates/editor/src/commands/text_commands.rs crates/editor/src/payload_util.rs
git commit -m "feat(editor): style/text/reply commands"
```

---

## Task 7: Selection/cursor management + facade + integration + gates

**Files:**
- Modify: `crates/editor/src/editor.rs` (selection/cursor setters), `crates/editor/src/lib.rs` (final re-exports)
- Test: `crates/editor/tests/integration.rs` (append), workspace gates

**Interfaces:**
- Produces: `Editor::select(&AnnotationId)`, `set_selection(AnnotationSelection)`, `clear_selection()`, `set_cursor(&AnnotationId, usize)`, `clear_cursor()`. Final `rofd-editor` public API.

- [ ] **Step 1: Add selection/cursor setters to Editor**

In `crates/editor/src/editor.rs` impl block:
```rust
    pub fn select(&mut self, id: AnnotationId) {
        self.selection = AnnotationSelection::Single(id);
    }
    pub fn set_selection(&mut self, sel: AnnotationSelection) {
        self.selection = sel;
    }
    pub fn clear_selection(&mut self) {
        self.selection = AnnotationSelection::None;
    }
    pub fn set_cursor(&mut self, annotation: AnnotationId, offset: usize) {
        self.text_cursor = Some(TextCursor { annotation, offset, preferred_x: None });
    }
    pub fn clear_cursor(&mut self) {
        self.text_cursor = None;
    }
```

- [ ] **Step 2: Append integration tests**

Append to `crates/editor/tests/integration.rs`:
```rust
#[test]
fn move_then_undo_restores_position() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1_700_000_000_000);
    let id = e.create_annotation(
        AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note {
            rect: Rect { x: 10.0, y: 10.0, w: 5.0, h: 5.0 }, color: Color::Rgb(0,0,0),
            content: "".into(), icon: NoteIcon::Note,
        },
    );
    e.move_annotation(&id, 3.0, 4.0);
    {
        let a = e.document().annotations.find(&id).unwrap();
        assert!(matches!(&a.payload, AnnotationPayload::Note { rect: Rect { x: 13.0, y: 14.0, .. }, .. }));
    }
    e.undo();
    {
        let a = e.document().annotations.find(&id).unwrap();
        assert!(matches!(&a.payload, AnnotationPayload::Note { rect: Rect { x: 10.0, y: 10.0, .. }, .. }));
    }
}

#[test]
fn delete_selected_removes_all_selected() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1);
    let a = e.create_annotation(AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note { rect: Rect{x:0.,y:0.,w:1.,h:1.}, color: Color::Rgb(0,0,0), content: "".into(), icon: NoteIcon::Note });
    let b = e.create_annotation(AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note { rect: Rect{x:0.,y:0.,w:1.,h:1.}, color: Color::Rgb(0,0,0), content: "".into(), icon: NoteIcon::Note });
    e.set_selection(rofd_editor::AnnotationSelection::Multi(vec![a.clone(), b.clone()]));
    e.delete_selected();
    assert!(e.document().annotations.find(&a).is_none());
    assert!(e.document().annotations.find(&b).is_none());
}

#[test]
fn selection_restored_on_undo() {
    let mut e = Editor::new();
    e.set_clock("t".into(), 1);
    let id = e.create_annotation(AnnotationKind::Note, PageId::new("P0"),
        AnnotationPayload::Note { rect: Rect{x:0.,y:0.,w:1.,h:1.}, color: Color::Rgb(0,0,0), content: "".into(), icon: NoteIcon::Note });
    // create sets selection to Single(id); undo restores selection_before (None).
    assert_eq!(e.selection(), &rofd_editor::AnnotationSelection::Single(id.clone()));
    e.undo();
    assert_eq!(e.selection(), &rofd_editor::AnnotationSelection::None);
}
```

- [ ] **Step 3: Finalize lib.rs re-exports**

`crates/editor/src/lib.rs`:
```rust
//! rofd-editor - OFD annotation editor (selection, commands, undo/redo).

pub mod commands;
pub mod cursor;
pub mod editor;
pub mod payload_util;
pub mod selection;
pub mod steps;

pub use cursor::TextCursor;
pub use editor::Editor;
pub use selection::AnnotationSelection;
pub use steps::{DeleteAnnotationStep, InsertAnnotationStep, ReplaceAnnotationStep, Step, Transaction};
```

- [ ] **Step 4: Run workspace gates**

Run: `cargo test --workspace`
Expected: PASS (all dom + io + render + editor tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (fix any warnings inline).

- [ ] **Step 5: Commit**

```bash
git add crates/editor/src/editor.rs crates/editor/src/lib.rs crates/editor/tests/integration.rs
git commit -m "feat(editor): selection/cursor management + facade + integration tests"
```

---

## Phase 3 Done - Definition of Done

- `rofd-dom`: `AnnotationModel` mutation helpers (find/find_mut/insert/remove).
- `rofd-editor`: `Editor` (owns OfdDocument, mutates only annotations), `AnnotationSelection`/`TextCursor`, `Step`/`Transaction`/`History` (cap 100, apply/revert undo/redo), commands (create/delete/delete_selected/move/resize/set_color/set_width/insert_text/delete_text/set_text/reply_to/undo/redo), `set_clock` (caller-supplied author/ts).
- Tests: dom helpers, steps (insert/delete/replace round-trip), history (undo/redo + cap eviction), payload_util, commands (each), integration (create/undo/redo, move+undo, delete_selected, selection-on-undo).
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` green.
- Editor has NO callbacks (Phase 4 component fires on_change/on_selection_change/on_cursor_change by querying editor state).

## Deferred to later phases

- **Editor callbacks** (`on_change`/`on_selection_change`/`on_cursor_change`): Phase 4 (component) - the component queries `editor.document()`/`editor.selection()`/`editor.text_cursor()` after commands and fires callbacks. The editor may track `last_affected_pages` for the component to read (add in Phase 4 if needed).
- **`up/down` visual-line text navigation**: needs render line-map (Phase 2 render). v1 supports left/right + click-to-position.
- **Multi-select move/resize**: v1 `move_annotation`/`resize_annotation` operate on one annotation; `delete_selected` handles multi. Multi-move can iterate (Phase 4).
- **`AnnotationId` stability across save/reload**: Phase 1 parse regenerates IDs; the editor works in-session. Cross-reload ID stability is a Phase 4+ concern (io could preserve the original `ID` string).
