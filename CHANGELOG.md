# Changelog

All notable changes to this project are documented here. Desktop releases
follow the git tags published to GitHub Releases (`vX.Y.Z`); the npm SDK
`@office-rs/rofd` is versioned on its own track, documented under
`SDK X.Y.Z` headings.

## v0.1.6

This is the **masonry/xilem rewrite + final naming** release: the winit
bridge is gone, the native adapter is a standard masonry widget, and the
`Editor*` API family moves to its final `Ofd*` names. It ships together
with SDK 0.1.6 (below). The license changes to Apache-2.0.

### Changed
- **Native adapter rewrite:** the winit event bridge and `EditorApp` are
  deleted. The adapter is now a masonry `Widget` + xilem `View` in three
  files — `OfdWidget`, `masonry_events` (pure translation table), and
  `OfdView` (11 event chains). Focus, pointer capture, IME sessions,
  clipboard shortcuts, and ctrl+wheel zoom are handled inside the widget;
  the host never touches winit.
- **Host rewrite:** the desktop host is a pure `Xilem::new_simple` app
  with an `Arc<dyn Fn(&mut OfdComponent)>` command queue, a toolbar and a
  right-click overlay; file load/save routing and atomic writes live in
  `host/document_io`.
- **Crate renames:** `rofd-native-view` → `rofd-xilem-view`,
  `native-app` → `xilem-app`.
- **API renames:** `EditorComponent` → `OfdComponent`,
  `EditorConfig` → `OfdConfig`, `WasmEditor` → `WasmOfd`,
  `create_wasm_editor` → `create_wasm_ofd`; the SDK class
  `Editor` → `Ofd`. No aliases are kept.

### Added
- **CJK IME support:** preedit state machine (forced commit/cancel/navigation
  guards), a preedit overlay at the caret (Parley shaping, TextBox clipping),
  and a focus-gated blinking caret on both native and web.
- **Clipboard editing:** `paste_text(&str) -> bool` and
  `copy_selection() -> Option<String>` (Ctrl+X is copy-only).
- A README native integration example for `rofd-xilem-view`.

### Fixed
- **Queued save results are reported back:** a failed save no longer
  clears the modified indicator (internal bounded wake, no rerender loop).
- **Save-As** switches the file path only after the save succeeds; failure
  keeps the previous path.
- **Zoom defenses:** non-finite/non-positive inputs are rejected at the
  boundary, and Ctrl + pure horizontal scrolling no longer triggers zoom
  (native and web).
- Scrollbar thumb clamping no longer panics on very small windows; the
  preedit overlay is offset by the TextBox origin; direct annotation
  mutation now invalidates the scene cache.

### License
- Changed from GPL-3.0-or-later to **Apache-2.0**.

### Downloads
Windows installers (pick one):

| File | Installer | Notes |
|---|---|---|
| `rofd_0.1.6_x64-setup.exe` | NSIS | Standard install wizard, recommended |
| `rofd_0.1.6_x64_en-US.msi` | MSI | Suited for enterprise bulk deployment |

## SDK 0.1.6 (@office-rs/rofd)

Rename release: the SDK class and the wasm/Rust interface family move to
their final `Ofd*` names. Ships together with the native masonry/xilem
adapter rewrite and the crate renames (`rofd-xilem-view` / `xilem-app`).

Note: 0.1.6 is chronologically newer than 0.2.0 but sits lower in semver
numbering; it supersedes 0.2.0's API surface entirely.

### Breaking
- **`Editor` class → `Ofd`**: `Editor.init(...)` becomes `Ofd.init(...)`;
  no alias is kept.
- **`WasmEditor` → `WasmOfd`**; factory **`create_wasm_editor` →
  `create_wasm_ofd`**; **`EditorConfig` → `OfdConfig`**.
- **Rust core**: `EditorComponent` → `OfdComponent`, `EditorConfig` →
  `OfdConfig`.
- Crates `rofd-native-view` / `native-app` renamed `rofd-xilem-view` /
  `xilem-app`.

### Changed
- Native adapter is now a masonry `Widget` + xilem `View` (`OfdWidget`,
  `ofd()`/`ofd_with_config()`); the native host is a pure
  `Xilem::new_simple` app with a command queue, toolbar and context-menu
  overlay. Body zoom is component-owned multiplicative zoom (no host
  mirror); Ctrl+X is copy-only.
  - Queued saves report their result back to the host: a failed save no
    longer clears the modified indicator (internal bounded wake).

## SDK 0.2.0 (@office-rs/rofd)

Breaking release of the npm SDK: markup annotations (highlight / underline /
strikeout / squiggly) are no longer creation tools — you select body text
first, then apply.

Note: the Rust crates carry matching workspace-internal changes (the
`rofd-component` `Tool` enum tightening and the removal of
`create_highlight_from_selection`), but they are not published to
crates.io — the published, semver-tracked surface is the npm SDK alone.

### Breaking
- **`createHighlightFromSelection(color)` removed.** Use `applyMarkup(kind)`
  instead. Per-kind colors are configured through the existing
  `setMarkupColor(kind, color)` (and `setHighlightColor` for highlights).
- **`setTool` no longer accepts markup strings.** `highlight`, `underline`,
  `strikeout`, and `squiggly` are no longer tools; these legacy values now
  silently fall back to the text tool, like any unknown value.

### Added
- `applyMarkup(kind: MarkupKind): string | null` — convert the current
  body-text selection into a markup annotation. Returns the new annotation
  id, or `null` when there is no selection. The selection is kept so further
  markups can stack on the same range.
- `hasTextSelection(): boolean` — whether a body-text selection currently
  exists.
- `setOnTextSelectionChange(cb)` / `EditorConfig.onTextSelectionChange` —
  signal fired when the body-text selection appears, changes, or clears.
- `MarkupKind` type export: `'highlight' | 'underline' | 'strikeout' | 'squiggly'`.

### Changed
- Markup annotations are now created by selecting body text with the text
  tool, then clicking a toolbar button (or picking a color). Markup buttons
  disable while no text is selected.

### Fixed
- Host callbacks are now deferred to a microtask before invocation. A handler
  that called back into the editor synchronously (e.g. querying
  `hasTextSelection()` inside `onTextSelectionChange`) re-entered the wasm
  object's borrow, which wasm-bindgen rejects with "recursive use of an
  object detected" — and the error was swallowed, so markup buttons stayed
  permanently disabled after selecting text.

## v0.1.4

This release introduces a **Tauri desktop client** for Windows and restructures
the host applications into first-class workspace crates.

### Added
- **Desktop client (Tauri):** new **rofd** Windows desktop app that wraps the web
  editor inside the system WebView (WebView2), with no CDN dependency — fonts and
  the sample OFD load locally. The frontend reuses `crates/web-app` source
  verbatim; only a native file bridge is injected, and the Rust shell just opens
  the window and registers plugins (depends on no rofd crate). Native open/save
  dialogs via `tauri-plugin-dialog` + `tauri-plugin-fs`. Installer, intermediate
  exe, and installed program are all named **rofd**.
- **Automated release packaging:** on a published GitHub Release,
  `release-tauri.yml` builds the Windows installers on a Windows runner and
  uploads them to the release Assets. Version comes from the git tag.

### Changed
- **fileHost platform file bridge:** file open/save in web-app is now an
  injectable interface, so the same UI runs unchanged in the browser and under
  Tauri.
- Migrated `native-app` and `web-app` from `examples/` to `crates/` as proper
  workspace members; all path references (Cargo, CI, docs) updated accordingly.
- README split into English (`README.md`) and Chinese (`README.zh-CN.md`) with a
  language switcher, documenting how to download, use, and build the Tauri
  desktop client.

### Fixed
- CI: excluded `tauri-app` from the Linux workspace build — it is Windows-only
  and would otherwise fail `glib-sys` on Ubuntu runners. Its build is validated
  on Windows via the release workflow.

### Downloads
Windows installers (pick one):

| File | Installer | Notes |
|---|---|---|
| `rofd_0.1.4_x64-setup.exe` | NSIS | Standard install wizard, recommended |
| `rofd_0.1.4_x64_en-US.msi` | MSI | Suited for enterprise bulk deployment |

The web SDK is published to npm as `@office-rs/rofd@0.1.4`.

**Full changelog:** https://github.com/ravenq/rofd/compare/v0.1.3...v0.1.4
