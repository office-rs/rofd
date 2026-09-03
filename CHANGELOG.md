# Changelog

All notable changes to this project are documented here. Versions follow the
git tags published to GitHub Releases (`vX.Y.Z`).

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
