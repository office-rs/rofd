# rofd-editor

> Auto-loaded when you work in this subtree. The full annotation-semantics
> picture: docs/agents/editing.md.

Annotation selection, command pattern, Step/Transaction/History. Depends
only on dom.

- **Commands mutate only `.annotations`, never pages** (the body-read-only
  invariant). Two document-level changes sit beside them: `load_document`
  (another document: selection, cursor and history start over) and
  `revise_document` (spec §17.11 — the component hands the document to a
  change that is no annotation command, today only a signature landing in
  the package the document carries; the caller follows with
  `invalidate_pages`, which raises `body_revision` because the seals are
  page content). A signature is no edit: the selection, the cursor and the
  history stay, and it is not an undo step.
- Command timestamps always come from the host-injected `set_clock` clock —
  the editor never reads the system time.
- On a document that declares signatures (`document.signatures.declared`)
  `create_annotation` numbers from the highest known ID upward — under the
  reserve written at signing time — and leaves `max_unit_id` alone (spec
  2026-11-23 M14); unsigned documents start from `max_unit_id + 1`.
- Rich-text setters are selection-aware (since a6b8103): with a selection
  they change only the selection; without one, the whole box; select-all
  merges back to the flat fields whenever the result is flat-expressible;
  text edits preserve spans without collapsing.
