# rofd-editor

> Auto-loaded when you work in this subtree. The full annotation-semantics
> picture: docs/agents/editing.md.

Annotation selection, command pattern, Step/Transaction/History. Depends
only on dom.

- **Mutates only `.annotations`, never pages** (the body-read-only
  invariant).
- Command timestamps always come from the host-injected `set_clock` clock —
  the editor never reads the system time.
- Rich-text setters are selection-aware (since a6b8103): with a selection
  they change only the selection; without one, the whole box; select-all
  merges back to the flat fields whenever the result is flat-expressible;
  text edits preserve spans without collapsing.
