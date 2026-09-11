# Changelog

All notable changes to this project are documented here. Desktop releases
follow the git tags published to GitHub Releases (`vX.Y.Z`); the npm SDK
`@office-rs/rofd` is versioned on its own track, documented under
`SDK X.Y.Z` headings.

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
