# Annotation editing

> When to read: any task touching annotation tools, text layout, rich text,
> selection, IME, or the format panel. Authoritative specs:
> `2026-11-09-annotation-text-wrap`, `2026-11-10-annotation-format-panel`,
> `2026-11-10-annotation-rich-text`.

The kernel lives in `rofd-component` (`OfdComponent`) + `rofd-editor` +
`rofd-render`; the body is read-only and annotations are the only mutable
face. Hosts drive through the `OfdCommand`/callback surface and never touch
kernel state directly.

## Creation tools (the text pair, 2026-11-07)

- `CreateKind::Typewriter`: click to place a **borderless** empty box; the
  caret drops in immediately.
- `CreateKind::TextBox`: drag to draw a **bordered** box.
- Foreign rich-text boxes are **converted to flowed spans at parse time**:
  per-segment styles stored as `TextBox.spans`, participating in wrapping
  and editing (synthetic bold/italic drawn as usual) — no more collapsing.

## Flowed text layout (spec 2026-11-09, extended to spans 2026-11-10)

- Every TextBox wraps at the rect width; line height is the **largest font
  size in the line**; the box only grows downward.
- Enter inserts a hard break; up/down arrows move the caret by **visual
  line**.
- The caret covers the full line-box height (`line.height`, not `size`).

## In-text selection (`AnnotTextSelection`)

In edit mode the Text tool drag-selects text inside the box: per-line
translucent highlight; typing/Enter/paste/IME replace the selection,
Backspace/Delete delete it (each a single Transaction); IME composition
start deletes the selection first. **The first drag on an unfocused box and
the Hand tool still move the box** (not select text).

## IME / composition-string preview

- The component owns the preedit state machine (`Option<PreeditState>`);
  wasm composition events arrive through the SDK IME bridge (see
  crates/web-view/AGENTS.md), native ones through masonry/gpui IME.
- On-screen preview of the composition string: **every** TextBox (flat or
  spans) inlines it into the content flow for wrapping/reflow — text after
  the composition point shifts along, no overlap; a span box's preview
  inherits the style of the character at the insertion point. The
  composition string **never enters the dom, save, or print**.

## Rich-text segments (`TextBox.spans`, spec 2026-11-10)

- `TextSpan { text/font/size/color/bold/italic }`; concatenated in order ===
  content; empty = flat box.
- Setters are **selection-aware**: with a selection they change only the
  selection; without one, the whole box; select-all styling merges back to
  the flat fields whenever the result is flat-expressible. Text edits
  preserve spans (no collapsing).
- Foreign rich boxes parse into spans (Remark-aligned; on mismatch, degrade
  to flat + warning).

## Save and reopen

- Saving goes through `save_snapshot()` producing `TextLayoutHints`; io uses
  it to write per-line/per-fragment TextObjects (real line geometry and
  advances; span boxes styled per fragment) + `Remark` + `GrowMode`. It
  **never writes** `AllRichText`/`ParagraphFormat` (neither is a reflow
  source and rofd reads neither — the drop list, see [io.md](io.md)).
- Reopening restores the logical text from `Remark`; widening reflows
  (flowed layout, not fixed-geometry replay).

## Format panel (spec 2026-11-10)

The kernel provides data and commands; **each adapter builds its own panel
UI** (gpui-view and the web SDK are done; xilem-view is pending, spec §5.3):

- `annotation_text_style`: family/size mm/color/rich, **selection-aware**
  (with text selected, reports the style of the selection's first
  character).
- `annotation_format_anchor`: the viewport rect the panel anchors to.
- `font_families` (available fonts: system ∪ declared ∪ SDK manifest —
  **not** "registered"), `set_available_font_families`.
- Three setters (size/family with measure; single Transaction; height only
  grows).
- The size table / 60-color theme palette / 10 standard colors live in
  `component::text_format` (exported to the SDK on wasm).

## Font family is not persisted in v1

`payload.font = FontId(display family name)` (the panel data hands out the
normalized key; the component maps it to that entry's display — the key
itself may be an alias the backend doesn't know, e.g. `times`), and no Font
resource is created. Drawing goes through `family_list_for`'s "unknown
FontId → canonical key → slice chain → registered family name → original id
fallback" chain; `font_req` counts the canonical keys of unknown annotation
FontIds as required families. The full font-channel picture:
[fonts.md](fonts.md).

## Misc kernel API

`paste_text`, `delete_annotation`, `tick_blink` (unified on both targets;
wasm is driven by `renderFrame` — see golden rule 4 in the root AGENTS.md),
undo/redo straight from KeyDown.
