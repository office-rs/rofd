# rofd gpui Adapter + Host Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rofd-gpui-view` (gpui adapter SDK wrapping `OfdComponent`) and `gpui-app` (Windows-first multi-document host at web-app feature parity, print included).

**Architecture:** Two new crates mirror rword's gpui line: the adapter translates gpui events ↔ `ViewEvent`, replays the kernel's `imaging::record::Scene` through a two-half `PaintCmd` sink, and forwards 14 kernel callbacks as entity events; the host owns file I/O, chrome, system fonts, sessions, and the print dialog (state machine copied from xilem-view, submission via kernel `print_direct`).

**Tech Stack:** Rust 1.98.1 (rust-version 1.96), gpui-pre =0.3.10 family + gpui-kit =0.8.1, imaging 0.0.1, office-rs-print (git c2f99d5), ttf-parser 0.25.

**Spec:** `docs/superpowers/specs/2026-10-31-gpui-view-gpui-app-design.md` (read it first; rulings R1-R8 and the §8 denominator table bind this plan)

## Global Constraints

- Pins (batch-bump only, never individually): `gpui-pre = "=0.3.10"`, `gpui-pre-platform = "=0.3.10"`, `gpui-kit = "=0.8.1"`, `gpui-kit-assets = "=0.8.1"`, `ttf-parser = "0.25"`, `image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }`, `smallvec = "1"`, `memmap2 = "0.9"`, `office-rs-print = { git = "https://github.com/office-rs/print.git", rev = "c2f99d5" }`.
- Copy sources are pinned: rword @ `4e0b25b6e6de79051004a757a39a9a604eb1cc60` (gpui_sink / font_cache / events / ime / view / handle / events_surface patterns, tests), rofd's own `crates/xilem-view/src/print_dialog_state.rs` + `print_services.rs`, `crates/xilem-app/src/host/{document_io,export_pdf}.rs`. File headers carry a `Ported from <repo>@<rev>` comment.
- Layering: `rofd-gpui-view` deps = rofd-component + rofd-dom + imaging + gpui family + ttf-parser/image/smallvec + office-rs-print (list_printers only). **No rofd-io, no rofd-export in gpui-view.** `gpui-app` holds io/export/rfd-free file prompts/print glue.
- English-only comments and commit messages; no WPS/rword strings in code (rename all identifiers during ports).
- No wall-clock calls in library crates (`Instant` for blink only); the host alone calls `SystemTime`/path prompts and injects `set_clock`.
- Every task ends green on: `cargo test -p <touched crate>`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- Package names: lib crate `rofd-gpui-view` (dir `crates/gpui-view`, lib name `rofd_gpui_view`), bin crate `gpui-app` (dir `crates/gpui-app`). Workspace dep entry: `rofd-gpui-view = { path = "crates/gpui-view" }`.

## Review Focus

1. **Bare printable char double-input** (rword C1): gpui delivers printable text on two channels — a bare Char key must NOT ride the key channel. Test in Task 5 (`key_down` drops bare Char) and Task 8 (typing inserts exactly once).
2. **Coordinate origin mismatch** (click offset by chrome height): replay adds `bounds.origin`, mouse subtracts `element_origin`. Both directions unit-pinned in Task 5 (`*_at` variants) and Task 8 (origin bookkeeping test); manual checklist re-verifies.
3. **Windows wheel sign/scale**: `Lines(notches × OS lines)`, +y = up → negate, ÷3, ×20px. Task 5 scroll tests pin all three.
4. **Font double-system desync**: kernel shapes with a font gpui's TextSystem never got → glyphs silently drop. Task 3 pins `font_cache` resolves any `Arc<FontData>`; Task 11 manual check pins CJK via system scan.
5. **IME commit with stale preedit** (lone `replace_text(final)` inserts pinyin): Task 6 pins the preedit-sync-then-commit pair while composing.
6. **Print double-submit spools duplicates**: Task 12 state-machine tests (copied) + Task 13 in-flight guard.

---

### Task 1: gpui-view crate scaffold

**Files:**
- Create: `crates/gpui-view/Cargo.toml`, `crates/gpui-view/src/lib.rs`
- Modify: `Cargo.toml` (root: members + `[workspace.dependencies]` gpui block)

**Interfaces:**
- Produces: crate `rofd-gpui-view` compiling empty; workspace deps `gpui-pre`, `gpui-pre-platform`, `gpui-kit`, `gpui-kit-assets` available to later tasks.

- [ ] **Step 1: Add workspace wiring** — root `Cargo.toml`: append `"crates/gpui-view"` to members (gpui-app joins in Task 4); add to `[workspace.dependencies]` the four gpui pins with the rationale comment ported from `D:/code/rword/Cargo.toml` (gpui-pre is the kit maintainer's snapshot of zed@08cbb88; official gpui is stale at 0.2.2; bump only as one batch).
- [ ] **Step 2: Create the crate** — `Cargo.toml` mirrors rword's `crates/gpui-view/Cargo.toml` structure: deps `gpui-pre`/`gpui-pre-platform` (workspace), `rofd-component`/`rofd-dom` (path `../component`, `../dom` — hyphen names, path-direct, NOT workspace entries, matching how rword wires gpui-view), `imaging` (workspace), `ttf-parser`, `image`, `smallvec`; dev-deps `gpui-pre` with `features = ["test-support"]`. Declare `rust-version = "1.96"`. `src/lib.rs` = doc comment only.
- [ ] **Step 3: Verify** — `cargo build -p rofd-gpui-view && cargo test -p rofd-gpui-view` (0 tests, exit 0), plus clippy/fmt gates.
- [ ] **Step 4: Commit** — `feat(gpui-view): crate scaffold on the pinned gpui-pre family`

### Task 2: gpui_sink.rs port (recording + replay)

**Files:**
- Create: `crates/gpui-view/src/gpui_sink.rs`, `crates/gpui-view/tests/gpui_sink.rs`
- Modify: `crates/gpui-view/src/lib.rs` (module)

**Interfaces:**
- Produces: `pub enum PaintCmd { Quad{bounds, radii, color, mask}, Path{bez: BezPath, width: Option<Pixels>, color, mask}, Glyph{origin, font: FontData, glyph_id: u32, size, color, mask}, Image{bounds, image: Arc<gpui::RenderImage>, mask} }`, `pub struct GpuiSink` (impl `imaging::record::PaintSink`), `pub fn replay(window: &mut Window, cmds: &[PaintCmd], fonts: &mut FontCache, origin: Point<Pixels>)`.

- [ ] **Step 1: Port the tests first** — copy `D:/code/rword/crates/gpui-view/tests/gpui_sink.rs` at pinned rev; rename `rword`→`rofd` in idents/paths. The recording half is exercised headlessly (clip intersection, transform baking, image decode) — no changes to assertions (imaging IR is identical between repos).
- [ ] **Step 2: Run to verify they fail** — `cargo test -p rofd-gpui-view --test gpui_sink` → FAIL (module missing).
- [ ] **Step 3: Port `gpui_sink.rs`** — copy from rword@pin, rename idents, keep every comment's technical content in English. Only change: the `degrade_log` prefix `[rword-gpui-view]` → `[rofd-gpui-view]`. Add the `Ported from` header.
- [ ] **Step 4: Run to verify pass** — same command → PASS; clippy/fmt.
- [ ] **Step 5: Commit** — `feat(gpui-view): two-half paint sink (record + replay), ported from rword`

### Task 3: font_cache.rs port

**Files:**
- Create: `crates/gpui-view/src/font_cache.rs`, `crates/gpui-view/tests/font_cache.rs`
- Modify: `crates/gpui-view/src/lib.rs`

**Interfaces:**
- Produces: `pub struct FontCache` with `pub fn new()`, `pub fn font_id(&mut self, data: &imaging::peniko::FontData, ts: &gpui::TextSystem) -> Option<gpui::FontId>` — Arc-identity key `(ptr, len, index)`.

- [ ] **Step 1: Port tests** — copy rword's `tests/font_cache.rs`; assertions about key semantics (same Arc hits, different Arc misses, parse failure → None) port unchanged.
- [ ] **Step 2: Verify fail** — `cargo test -p rofd-gpui-view --test font_cache` → FAIL.
- [ ] **Step 3: Port `font_cache.rs`** — verbatim copy + rename; ttf-parser name-table/OS2 parsing stays as-is. Note in a comment: headless platform TextSystem is a stub — real parsing is window-state verified (rword finding, spec §5).
- [ ] **Step 4: Verify pass + clippy/fmt.**
- [ ] **Step 5: Commit** — `feat(gpui-view): Arc-identity font cache, ported from rword`

### Task 4: view.rs core + gpui-app skeleton + first light

**Files:**
- Create: `crates/gpui-view/src/view.rs`
- Create: `crates/gpui-app/Cargo.toml`, `crates/gpui-app/src/main.rs`, `crates/gpui-app/src/shell.rs`, `crates/gpui-app/src/host/mod.rs`, `crates/gpui-app/src/host/document_io.rs`, `crates/gpui-app/src/host/export_pdf.rs`
- Modify: root `Cargo.toml` (member `crates/gpui-app`), `crates/gpui-view/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 `GpuiSink`/`replay`, Task 3 `FontCache`; kernel `OfdComponent::{new_native, update_scene, scene, set_viewport_size}`.
- Produces: `pub struct OfdGpuiView { pub component: OfdComponent, pub focus_handle: FocusHandle, pub(crate) fonts: RefCell<FontCache>, pub(crate) last_size: std::cell::Cell<(f32,f32)>, ... }` (fields `ime`, `events`, `element_origin`, `had_focus`, `last_mouse` are added by Tasks 7-8 — declare them here already, unused until then); `impl gpui::EventEmitter<OfdGpuiEvent>` is deferred to Task 7 (keep `Render` plain this task); host `host::document_io::{load_ofd, save_ofd}` and `host::export_pdf::export_pdf` with signatures identical to `crates/xilem-app/src/host/` (copy + rename imports).

- [ ] **Step 1: Port `host/` from xilem-app** — copy `document_io.rs` + `export_pdf.rs` verbatim (they are host code, no xilem types inside; fix `use` paths to the gpui-app module). Unit tests: none here (io round-trip is guarded by `crates/io` tests); compile gate only.
- [ ] **Step 2: Write `view.rs` reduced core** — port from rword `view.rs`: struct + `Focusable` + `WordElement`→`OfdElement` (`request_layout` = `relative(1.0)` sized, `prepaint` returns bounds, `paint` = update_scene → record via `imaging::record::replay(scene, &mut GpuiSink)` → `gpui_sink::replay(window, &cmds, &mut fonts, bounds.origin)`). Add size-change detection: on paint, if `bounds.size` ≠ `last_size` → `component.set_viewport_size(w, h)` (logical px — gpui `Pixels` are logical; no scale division). No event listeners yet (Task 8).
- [ ] **Step 3: gpui-app skeleton** — `Cargo.toml` mirrors rword's gpui-app (gpui-pre/-platform/-kit + assets feature, `rofd-gpui-view` path dep, `rofd-io`/`rofd-export`/`rofd-component` workspace, `office-rs-print`, `ttf-parser`, `memmap2` reserved for Task 11). `main.rs` ports rword's `main.rs` pattern: `application().with_assets(gpui_kit_assets::AllAssets).run(...)`, `gpui_kit::init(cx)`, centered 1000×760 window, focus the editor. `shell.rs`: minimal — `AppShell { editor: Entity<OfdGpuiView> }`, `Render` = `v_flex().child(editor clone)` with the wrapper `flex_1().overflow_hidden()` (spec pitfall 8). Load CLI arg 1 via `host::document_io::load_ofd` (xilem `maybe_load_cli_arg` precedent); default font bytes: read `%WINDIR%\Fonts\simsun.ttc` (fallback empty vec) into `OfdConfig::new(Arc::new(bytes))`; `set_clock("rofd", <host SystemTime secs>)` after load (host may read the clock; check `crates/xilem-app/src/main.rs` for the exact author/ts precedent and mirror it).
- [ ] **Step 4: First-light manual check** — `cargo run -p gpui-app -- test/ru-yuan-ji-lu.ofd` → window shows the grey desk + page content; screenshot saved to the task notes. If glyphs are missing, check Task 3's `FontCache` actually resolves (add an `eprintln!` temporarily — remove before commit).
- [ ] **Step 5: Gates + Commit** — `feat(gpui-app): first light — window, host io, scene replay pipeline`

### Task 5: events.rs pure translation

**Files:**
- Create: `crates/gpui-view/src/events.rs`, `crates/gpui-view/tests/event_mapping.rs`

**Interfaces:**
- Produces: `pub const SCROLL_LINE_PX: f32 = 20.0; pub const GPUI_LINES_PER_NOTCH: f32 = 3.0;` plus fns `modifiers_from(&gpui::Modifiers) -> rofd_component::Modifiers`, `map_key(&str) -> Key`, `key_down(&KeyDownEvent) -> Option<ViewEvent>`, `mouse_down_at(&MouseDownEvent, x: f32, y: f32) -> ViewEvent`, `mouse_up_at(...) -> ViewEvent`, `mouse_move_at(...) -> ViewEvent`, `pub enum ScrollOutcome { Scroll(ViewEvent), Zoom(ViewEvent), None }`, `pub fn scroll(event: &ScrollEvent, current_zoom: f64, pointer: (f32, f32)) -> ScrollOutcome`.
- rofd divergences from the rword source (apply during port): target variants are `PointerDown{button, x: f64, y: f64, modifiers, click_count: u8}` (rword: u16), `PointerUp{button, x, y}` (no modifiers), `PointerMove{x, y}`, `Scroll{dx: f64, dy: f64}`; ctrl+wheel returns `ViewEvent::ZoomAt { factor, center: (f64, f64) }` where `center` = the `pointer` arg (viewport px) — NOT rword's absolute `Zoom`.

- [ ] **Step 1: Write failing tests** (`tests/event_mapping.rs`, port + adapt rword's): (a) `modifiers_from` maps `platform`→`meta`; (b) `map_key` covers rofd's full `Key` set (Char single char, enter/backspace/delete/tab/escape/arrows/home/end/pageup/pagedown, longer strings → `Unidentified`); (c) `key_down` drops `is_held` repeats, drops bare printable Char (no ctrl/alt/platform), passes named keys and ctrl-shortcuts; (d) `mouse_down_at` produces `PointerDown` with `click_count: u8` clamp and given coords; (e) `scroll`: `Lines(0,3)` at zoom z → `Scroll{dy: -20.0}`; `Pixels` negate; ctrl+wheel up → `ZoomAt{factor: 1.1, center}`; ctrl+wheel at the zoom ceiling → `None`. Zoom ceiling constants: `PX_PER_MM * 3.0` (max) and `PX_PER_MM * 0.25` (floor) — import `PX_PER_MM` the same way `crates/xilem-view/src/ofd_widget.rs` gets it (mirror its import path); `factor` is multiplicative (`z*1.1` vs `z/1.1`) compared against those bounds for the no-op suppression only (the kernel owns final clamping).
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement `events.rs`** — port rword's file with the divergence table above; keep the header comment explaining the Windows wheel facts (Lines × OS multiplier, +y up, logical Pixels).
- [ ] **Step 4: Verify pass + clippy/fmt.**
- [ ] **Step 5: Commit** — `feat(gpui-view): pure gpui→ViewEvent translation with wheel calibration`

### Task 6: ime.rs ImeBridge

**Files:**
- Create: `crates/gpui-view/src/ime.rs`, `crates/gpui-view/tests/ime_bridge.rs`

**Interfaces:**
- Produces: `pub enum UnmarkOutcome { CommitMarked, Idle }`, `pub struct ImeBridge` with `new/is_composing/replace_and_mark(Option<Range<usize>>, &str) -> Vec<ViewEvent>/replace_text(...) -> Vec<ViewEvent>/unmark() -> UnmarkOutcome/cancel() -> Vec<ViewEvent>/text_length_utf16/marked_range/slice_utf16` (port rword's exact API).
- rofd divergence: preedit events are `ViewEvent::ImePreedit { text, caret: None }` (field name `caret`, rword used `cursor`).

- [ ] **Step 1: Port failing tests** — rword `tests/ime_bridge.rs`: composition update → `[ImePreedit]`; commit-while-composing → `[ImePreedit(final), ImeCommit(final)]` (stale-pinyin guard, Review Focus 5); plain commit with empty store → lone `[ImeCommit]`; empty replace → cancel; `unmark` idle vs `CommitMarked`; UTF-16 slicing never tears surrogate pairs.
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Port `ime.rs`** (pure core only — the `InputHandler` impl lands in view.rs, Task 8, because it needs the view type).
- [ ] **Step 4: Verify pass + clippy/fmt.**
- [ ] **Step 5: Commit** — `feat(gpui-view): pull-push IME bridge with composition shadow store`

### Task 7: events_surface.rs — 14 callbacks → entity events

**Files:**
- Create: `crates/gpui-view/src/events_surface.rs`, `crates/gpui-view/tests/events_surface.rs`

**Interfaces:**
- Consumes: kernel callback setters (native `+Send` variants) on `OfdComponent`.
- Produces: `#[derive(Debug, Clone, PartialEq)] pub enum OfdGpuiEvent { Changed, SelectionChanged(AnnotationSelection), TextSelectionChanged, CursorChanged, SaveRequested, AnnotationFocused(AnnotationId), AnnotationInteracted(AnnotationId), ContextMenuRequested((f64,f64), ContextTarget), PageChanged(usize), ZoomChanged(f64), ZoomModeChanged(ZoomMode), Warned(Vec<OfdWarning>), PointerCursorChanged(PointerCursor), CopyRequested }`; `pub struct EventQueue(Arc<Mutex<Vec<OfdGpuiEvent>>>)` with `push/drain`; `pub fn install(component: &mut OfdComponent, queue: &EventQueue)` wiring all 14. Payload types come from `rofd_component`/`rofd_dom` re-exports — mirror the payload types of the `on_*` builder methods in `crates/xilem-view/src/ofd_view.rs` (e.g. `TextSelectionChanged` carries no clone — hosts re-query, matching how xilem's `on_text_selection_change` handler receives a borrow).

- [ ] **Step 1: Write failing tests** — fresh `OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])))` + `install`; drive: `set_zoom_mode(FitWidth)` after `set_viewport_size` → drains exactly `[ZoomModeChanged(FitWidth), ZoomChanged(_)]` (order per kernel fire); `ViewEvent::KeyDown{Char('s'), ctrl}` → `[SaveRequested]` if the kernel owns ctrl+s (verify against `handle_event`; if the kernel does NOT handle it, the adapter intercepts in Task 8 instead and this test asserts the interception point instead); right-click `PointerDown{Right}` on empty desk → `[ContextMenuRequested(_, ContextTarget::Empty)]`; `load_document` of a doc built to warn (see `crates/io` warning fixtures) → `[Warned(_), Changed]`. Choose 4-5 deterministic cases; copy kernel-behavior expectations from `crates/component/tests/integration.rs` rather than inventing them.
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement** — queue + enum + `install` registering each callback to `queue.push(...)`. Note: `on_change` fires with `&OfdDocument` — the event carries no payload.
- [ ] **Step 4: Verify pass + clippy/fmt.**
- [ ] **Step 5: Commit** — `feat(gpui-view): kernel callback surface as entity event queue`

### Task 8: view.rs completion — input, IME, blink, shortcuts

**Files:**
- Modify: `crates/gpui-view/src/view.rs`
- Create: `crates/gpui-view/tests/view_input.rs`
- Modify: `crates/gpui-view/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 5-7 (`events`, `ImeBridge`, `EventQueue`/`OfdGpuiEvent`) + `OfdComponent::{tick_blink, caret_rect, paste_text, copy_selection, undo, redo, force_commit_preedit, handle_event}`.
- Produces: complete `OfdGpuiView`: `impl gpui::EventEmitter<OfdGpuiEvent>`, `pub(crate) fn handle_view_event(&mut self, event: ViewEvent, cx: &mut Context<Self>)`, `impl gpui::InputHandler for OfdGpuiView` (composition-only shadow via `ImeBridge`), element listeners for mouse/scroll/keys.

- [ ] **Step 1: Write failing headless tests** (`#[gpui::test]`, port rword `tests/view_input.rs` patterns; keystrokes need prior `window.focus(&view.focus_handle)`): (a) `simulate_mouse_down` at window coords offset from a forced `element_origin` selects the annotation under the translated viewport point (assert `view.component.selection()`); (b) typing a printable char inserts exactly once (double-input guard, Review Focus 1); (c) `replace_and_mark_text_in_range` then `unmark_text` commits the marked text into the annotation (assert content via `component.document()`); (d) ctrl+z after an insert reverts it; (e) ctrl+c with a body selection writes the clipboard (`cx.read_from_clipboard`).
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement** — port from rword `view.rs`: `on_mouse_down/up/move` listeners using the `_at` variants with `window_pos − element_origin`; `on_scroll` → `events::scroll(event, component.zoom(), pointer−origin)`; `on_key_down` → shortcut interception FIRST (`ctrl+z`→`component.undo()`, `ctrl+y`/`ctrl+shift+z`→`redo()`, `ctrl+c`→`copy_selection()`→`cx.write_to_clipboard(ClipboardItem::new_string(text))`, `ctrl+v`→`cx.read_from_clipboard`→`paste_text`) then `events::key_down` passthrough; focus transitions (`paint` compares `focus_handle.is_focused(window)` vs `had_focus` → `FocusGained/Lost`; Lost force-commits preedit — kernel behavior on `FocusLost` per `handle_event`); `window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)` every paint; `InputHandler` methods delegate to `ImeBridge` (port rword `ime.rs` impl half: `replace_text_in_range`/`replace_and_mark_text_in_range`/`unmark_text`→`force_commit_preedit`/`bounds_for_range` = `caret_rect()` + origin); `tick_blink()` + `request_animation_frame()` only while focused (spec anti-goal: no always-on frame loop); events drain → `cx.emit` FIRST in paint.
- [ ] **Step 4: Verify pass + clippy/fmt + re-run Task 4 first light manually (regression).**
- [ ] **Step 5: Commit** — `feat(gpui-view): input channels, IME handler, blink, clipboard shortcuts`

### Task 9: handle.rs host verbs

**Files:**
- Create: `crates/gpui-view/src/handle.rs`, `crates/gpui-view/tests/handle_verbs.rs`
- Modify: `crates/gpui-view/src/lib.rs`

**Interfaces:**
- Consumes: `Entity<OfdGpuiView>`; kernel verbs `set_tool/set_highlight_color/set_markup_color/apply_markup/delete_annotation/delete_selected/undo/redo/can_undo/can_redo/set_zoom_mode/zoom_mode/zoom/new_document/load_document/selected_text/has_text_selection/paste_text/set_clock/set_tooltip_enabled/print_direct` (names verified against `ofd_component.rs`).
- Produces: `pub struct GpuiOfdHandle { entity }` with `new/entity/from<Entity>` and methods: `load_document(OfdDocument, &mut App)`, `new_document(&mut App)`, `set_tool(Tool, &mut App)`, `set_highlight_color(rofd_dom::Color, &mut App)`, `set_markup_color(AnnotationKind, Color, &mut App)`, `apply_markup(AnnotationKind, &mut App) -> Option<String>`, `delete_annotation(AnnotationId, &mut App) -> bool`, `delete_selected(&mut App) -> u32`, `undo/redo(&mut App)`, `can_undo/can_redo(&App) -> bool`, `set_zoom_mode(ZoomMode, &mut App)`, `zoom_mode(&App) -> ZoomMode`, `zoom(&App) -> f64`, `zoom_at(factor: f64, &mut App)` (center = viewport center), `scroll_page(ScrollDirection, &mut App)`, `selected_text(&App) -> Option<String>`, `has_text_selection(&App) -> bool`, `paste_text(&str, &mut App) -> bool`, `set_clock(String, i64, &mut App)`, `set_tooltip_enabled(bool, &mut App)`, `print_direct(Option<&str>, &PrintSettings, PrintContent, &str, &mut App) -> JobHandle`.

- [ ] **Step 1: Write failing tests** (`#[gpui::test]`): each verb mutates/queries component state — e.g. `set_tool(Tool::Hand)` then `component.tool` is Hand (expose via behavior: hand drag pans — or assert via `zoom_mode` roundtrip, `can_undo` flips after `apply_markup` on a selected doc built like `component/tests` fixtures); keep to 5-6 representative verbs, not all 22.
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement** — every method is `entity.update(cx, |view, cx| { view.component.<verb>(...); cx.notify(); })` (port rword handle.rs shape).
- [ ] **Step 4: Verify pass + clippy/fmt.**
- [ ] **Step 5: Commit** — `feat(gpui-view): GpuiOfdHandle host verb surface`

### Task 10: gpui-app sessions, file flows, chrome

**Files:**
- Create: `crates/gpui-app/src/sessions.rs`
- Modify: `crates/gpui-app/src/shell.rs`, `crates/gpui-app/src/main.rs`
- Test: `crates/gpui-app/src/sessions.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 8 `OfdGpuiView` + `OfdGpuiEvent`; Task 9 `GpuiOfdHandle`; host `document_io::{LoadedOfd, load_ofd, save_ofd}`, `export_pdf::export_pdf`; gpui `cx.prompt_for_paths(PathPromptOptions)` / new-path prompt (rword `shell.rs:204` precedent — no rfd dep).
- Produces: `SessionManager { sessions: Vec<DocumentSession>, active: usize, blank_counter }`, `DocumentSession { file_name, path: Option<PathBuf>, package: Option<PackageHandle>, modified, pristine, editor: Entity<OfdGpuiView> }`, `new_blank/open_file/close/active/active_mut/active_editor`; `AppShell` renders TabBar + toolbar + editor + status bar.

- [ ] **Step 1: Write failing session tests** (pure logic): `new_blank` names 批注文稿N-style blanks and activates; `open_file` strips extension, stores path+package, activates; `close` adjusts `active` clamp and reports empty; last-close → shell auto-opens blank (test the manager flag, not the shell).
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement `sessions.rs`** — port rword `sessions.rs` shape + `package` field (io stays host-side; `PackageHandle` from `LoadedOfd`).
- [ ] **Step 4: Implement shell chrome** — `cx.subscribe` on each editor's `OfdGpuiEvent` mirrors state (modified via `Changed`, page/zoom status, `can_undo` refresh, warnings bar, `PointerCursorChanged` → window cursor, `CopyRequested` → clipboard). Toolbar groups (gpui-kit Button; **no Ribbon component exists** — groups are label + divider stacks, and any dropdown uses kit **Popover** with controlled open via Global bridge, spec pitfall 3): tools (手型/文本/手绘/图形 dropdown `ShapeKind::{Rect,Ellipse,Arrow,Line,Polygon,PolyLine}`), markup (高亮/下划线/删除线/波浪线 + highlight ColorPicker applying on selection), 撤销/恢复 (disabled from `can_undo/can_redo`), zoom menu (Custom% + FitWidth/FitHeight/FitPage), 全屏 toggle (window fullscreen, Esc exits), 自动翻页 toggle (3s timer → `handle.scroll_page(Down)` via `cx.spawn` loop with a cancellation flag). File menu Popover: 打开 (prompt_for_paths → `load_ofd` → new session + `register` fonts is Task 11's job — this task loads with default config), 保存 (`SaveRequested` event + menu → surgical `save_ofd` via session package, atomic; failure keeps `modified`), 另存为 (new-path prompt → `save_ofd` Some(pkg) then rebind path), 导出为 PDF (new-path prompt → `export_pdf(&component, path)` — clean-copy preset inside), 打印… (present but disabled; Task 13 enables). Context menu: on `ContextMenuRequested((x,y), ContextTarget::Annotation(_))` ONLY — one item 删除批注 → `handle.delete_selected()` or `delete_annotation(id)` (R7; Page/Empty → no menu; coords +element_origin → window anchor). Close-confirm on modified sessions (kit modal or Popover). Prevent chained `.child()` explosion: group builders return `Vec<AnyElement>` + `.children()` (spec pitfall 1).
- [ ] **Step 5: Manual pass** — open → annotate (each tool) → undo/redo → zoom modes → save → reopen (bytes preserve: verify with the io surgical tests' mental model — host just round-trips) → save-as → export PDF → multi-session open/close/switch → 全屏/自动翻页. Note deviations.
- [ ] **Step 6: Gates + Commit** — `feat(gpui-app): sessions, file flows, toolbar chrome`

### Task 11: font_index.rs — system font channel

**Files:**
- Create: `crates/gpui-app/src/font_index.rs`
- Modify: `crates/gpui-app/src/shell.rs` (load hook), `crates/gpui-app/Cargo.toml` (memmap2)
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `rofd_component::font_req::{body_requirements, annotation_requirements}` → `FontRequirements { families: Vec<String>, codepoints: Vec<u32> }` (families are normalized keys); `OfdComponent::register_font_data(Vec<u8>) -> bool`.
- Produces: `pub struct SystemFontIndex` with `scan() -> Self` (dirs: `%WINDIR%\Fonts`, `%LOCALAPPDATA%\Microsoft\Windows\Fonts`; memmap2 + ttf-parser name table, all-language family extraction — port from `crates/tauri-app/src-tauri/src/font_index.rs`), `pub fn load_families(&self, required: &FontRequirements) -> Vec<Vec<u8>>` matching normalized family keys.

- [ ] **Step 1: Write failing tests** — index over a fixture dir containing one copied font file (reuse `crates/render/tests/fixtures/fonts/TestFont.ttf`): family extracted from name table; `load_families` with a matching normalized key returns bytes, with an absent key returns empty; normalization folds case/whitespace (宋体 ≡ simsun style mapping only if ttf names allow — assert what the ttf actually exposes, don't invent mappings).
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement + wire** — on session open (Task 10's load hook): `body_requirements(doc)` + `annotation_requirements(doc)` → `index.load_families(...)` → for each bytes `handle`-side `entity.update` `component.register_font_data(bytes)` (kernel gen bumps, page caches invalidate; gpui `FontCache` picks the same Arcs up at replay automatically — Review Focus 4). Startup keeps the Task 4 SimSun default.
- [ ] **Step 4: Manual check** — open `test/ru-yuan-ji-lu.ofd`: declared families render in their real faces (compare glyph density vs Task 4 fallback screenshot).
- [ ] **Step 5: Gates + Commit** — `feat(gpui-app): system font scan and lazy family loading`

### Task 12: print state machine + services port ✧

**Files:**
- Create: `crates/gpui-view/src/print_dialog_state.rs`, `crates/gpui-view/src/print_services.rs`
- Modify: `crates/gpui-view/Cargo.toml` (office-rs-print workspace dep), `src/lib.rs`

**Interfaces:**
- Produces (verbatim from xilem-view, rename-free — already rofd code): `DiscoverState`, `DialogPhase`, `DialogState{phase, printers, errors, last_error}::{initial, can_submit}`, `DialogEvent`, `pub fn advance(&DialogState, &PrintSettings, DialogEvent) -> DialogState`, `Labels::{zh_cn, en}`, `trait PrintServices { fn discover(&self) -> mpsc::Receiver<DiscoverState> }`, `NativePrintServices`.

- [ ] **Step 1: Copy both files with their inline tests** — they are UI-agnostic (mpsc + office_rs_print only; no xilem/masonry imports — verify with a grep before copying). Add `Ported from crates/xilem-view` header.
- [ ] **Step 2: Verify green immediately** — `cargo test -p rofd-gpui-view print` (copied tests: discovery flow reaches Ready, no-printers blocks submit, validation updates, FakeServices trait-object works).
- [ ] **Step 3: Gates + Commit** — `feat(gpui-view): print dialog state machine, ported from xilem-view`

### Task 13: gpui-app print panel + single-track submit

**Files:**
- Create: `crates/gpui-app/src/print_dialog.rs`
- Modify: `crates/gpui-app/src/shell.rs` (menu enable, dialog mount, exit join), `crates/gpui-app/src/main.rs` (post-run `office_rs_print::join_all()`)

**Interfaces:**
- Consumes: Task 12 state machine + `NativePrintServices`; Task 9 `handle.print_direct`; xilem wiring contract (`crates/xilem-app/src/main.rs:480-590` `drain_print_dialog`).
- Produces: `pub struct PrintPanelState { dialog: DialogState, settings: PrintSettings, content: PrintContent, receiver: Option<mpsc::Receiver<DiscoverState>>, job: Option<JobHandle>, pending: Option<(Option<String>, PrintSettings, PrintContent)>, exit: bool }` + render/submit functions consuming `DialogEvent`.

- [ ] **Step 1: Write failing unit tests** for the host glue (pure parts): submit guard rejects when `pending.is_some() || job.is_some()` (double-submit, Review Focus 6) and when printers empty; Current-range freeze translates `PageRange::Current` → `Custom{spec: <resolved page>}` before `print_direct` (port xilem `submit_precheck` + freeze logic); exit with an unpicked handle cancels it.
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement** — panel UI (kit widgets): printer dropdown, copies, collate, duplex, color, paper, orientation, range (All/Current/Custom spec), parity, content flags (水印/批注/图章 → `PrintContent`), submit/cancel, phase-driven status (`Labels::zh_cn`). On open (once per dialog), sync the paper/orientation defaults from the document's first visible page (`begin_print(PrintContent::default())` → `session.page_parameters(0)`; xilem drain ③ precedent — syncing at open time against the fresh session, never against a stale one). Discovery receiver drained on notify ticks; submit executes inside `entity.update` (`component.print_direct(printer, &frozen, content, "rofd")`); terminal polling: a `cx.spawn` async loop polling `job.snapshot()` every ~200ms until terminal, then one `cx.notify` (rword `crates/gpui-app/src/print_dialog.rs` precedent for the gpui-shaped parts). Menu 打印… enabled; `main.rs` calls `office_rs_print::join_all()` after `application().run(...)` returns (process death mid-spool kills the job).
- [ ] **Step 4: Manual print-to-file pass** — same channel as xilem (commit `01e8c6f` print-to-file example): submit a 1-page job, verify spooled output; verify no-printer and failed-discovery states block submit.
- [ ] **Step 5: Gates + Commit** — `feat(gpui-app): print panel, single-track submit, terminal polling`

### Task 14: parity audit, manual checklist, AGENTS.md

**Files:**
- Create: `docs/superpowers/plans/2026-11-01-gpui-parity-audit.md`, `docs/superpowers/manual/2026-11-01-gpui-app-windows-checklist.md`
- Modify: `AGENTS.md` (§2 commands, §3 layout table + diagram, §7 pins)

- [ ] **Step 1: Write the audit table** — expand spec §8 rows 1-24 into per-id rows (id, web-app behavior, gpui status, evidence) — rword `2026-10-29-parity-audit.md` format. Any web-app behavior not in the table (status-bar save button, toast-vs-bar warning presentation…) gets a row marked 采纳 or 挂账 per the spec's denominator discipline — no silent drift.
- [ ] **Step 2: Write the manual checklist** — rword `2026-10-28` template: open/render sharpness/wheel/ctrl+wheel/IME (中文组合+取消)/right-click/multi-session/save-reopen/export PDF/print (to-file)/fullscreen/auto-page/close-confirm. Run it; file deviations as fixes or parked rows.
- [ ] **Step 3: Update AGENTS.md** — workspace layout gains gpui-view/gpui-app rows (dependency columns per Global Constraints), §2 gains `cargo run -p gpui-app -- test/*.ofd`, §7 gains the gpui pin family with the batch-bump note.
- [ ] **Step 4: Whole-branch review** — invoke the code-review skill over the branch; fix CRITICAL/HIGH.
- [ ] **Step 5: Commit** — `docs: gpui parity audit, manual checklist, AGENTS layout`

---

## Deferred (registered, not this plan)

- Line A (web-app multi-doc): own spec/plan per spec §7.
- RichClipboard sinking, shared-crate extraction of sink/font_cache, macOS/Linux: spec §15 挂账表.
