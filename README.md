# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#platform-support)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

**English** | [简体中文](README.zh-CN.md)

An OFD (GB/T 33190) **view + annotate** editor **library**, written in Rust, dual-platform (native + WASM).

---

## Usage

Integrate on the web via the npm package `@office-rs/rofd`:

```ts
import { Editor } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const editor = await Editor.init(container, {
    fonts: [
        { url: '/fonts/NotoSans-Regular.ttf' },
        { url: '/fonts/NotoSansCJKsc-Regular.otf' },
        { url: '/fonts/NotoSerifCJKsc-Regular.otf' }
    ],
    onContextMenu: (x, y, annotationId) => {
        ...
    }
});
editor.setClock('rofd', Date.now());

editor.loadOfd(bytes);
```

---

## Desktop client (Tauri)

No need to build a frontend yourself — download the prebuilt Windows desktop client **rofd**, which wraps the web editor above inside the system WebView.

### Download & install

Go to this repo's [Releases](../../releases) page and grab an installer from the **Assets** of the matching version (pick one):

| File                           | Installer | Notes                                 |
| ------------------------------ | --------- | ------------------------------------- |
| `rofd_<version>_x64-setup.exe` | NSIS      | Standard install wizard, recommended  |
| `rofd_<version>_x64_en-US.msi` | MSI       | Suited for enterprise bulk deployment |

`<version>` is the release version (e.g. `0.1.0`). The installed program is named **rofd**.

> Windows installers only: WebGPU works out of the box on Windows via WebView2; the system WebView on macOS/Linux does not yet fully support WebGPU, so those are not built.

### Usage

1. Launch **rofd**.
2. Use the toolbar "Open" to pick a local `.ofd` file (native system file dialog).
3. View and add annotations; "Save" writes back to `.ofd` via the native save dialog.

### Run / build from source

```bash
cd crates/tauri-app
npm install
npm run build:sdk         # wasm-pack build crates/web-view -> sdk/dist
npm run tauri dev         # dev: launch vite + system WebView window
npm run tauri build       # package: emit installers under src-tauri/target/release/bundle/
```

The frontend **reuses `crates/web-app` source**, only injecting a native file bridge; the `src-tauri` Rust shell merely launches the window and registers plugins.

---

## Platform support

| Platform                  | Rendering backend        | Status                                      |
| ------------------------- | ------------------------ | ------------------------------------------- |
| Desktop (Windows / Linux) | wgpu (Vulkan/Metal/DX12) | ✅                                          |
| Web                       | WebGPU                   | ✅ (Chrome/Edge 113+, no Canvas2D fallback) |

---

## License

This project is released under the [GNU General Public License v3 or later (GPL-3.0-or-later)](LICENSE). Use, modification, and distribution must comply with its terms; when distributing (including as part of a larger program) you must open-source under the same license and include the source code.

Copyright © 2026 rofd contributors.
