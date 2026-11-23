# tauri-app

> Auto-loaded when you work in this subtree. Font channel:
> docs/agents/fonts.md §tauri desktop channel.

The Tauri desktop host (the cargo member is `src-tauri`). Two essentials:

- **The Rust shell has zero rofd deps**: `src-tauri` only starts the window
  + registers the plugins and the two commands (`listSystemFonts` +
  `readSystemFont`; `src-tauri/src/font_index.rs` scans `%WINDIR%\Fonts` +
  the user fonts directory, mmap + all-languages name table extraction).
- The frontend **reuses the web-app sources** (`main.ts` imports App.vue
  from `../web-app/src`); just before mount it injects the native file
  bridge via `setFileHost` (`src/host/tauri.ts`, native open/save dialogs
  through tauri-plugin-dialog + tauri-plugin-fs).

Fonts and sample.ofd load locally from web-app's `public/` (vite
`publicDir` points there), off CDN; the desktop manifest set is **empty**
(Noto slices stay off desktop; the vite plugin runs `families: []`), and
the SDK lazy-loads local bytes per declared family (a system-only channel:
zero redistribution, no allowlist); the default font is SimSun; the browser
path stays byte-identical.

Platforms: Windows (WebView2) works out of the box; macOS/Linux are gated
on WebKit's WebGPU progress. Product naming is uniformly `rofd`
(release-tauri.yml).
