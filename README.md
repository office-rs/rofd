# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#platform-support)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

**English** | [简体中文](README.zh-CN.md)

An OFD (GB/T 33190) **view + annotate** editor **library**, written in Rust, dual-platform (native + WASM).

---

## Live Demo

[https://office-rs.github.io/rofd/](https://office-rs.github.io/rofd/)

## Usage

### Web

Integrate on the web via the npm package `@office-rs/rofd`:

```ts
import { Ofd } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const ofd = await Ofd.init(container, {
    fonts: [
        { url: '/fonts/NotoSans-Regular.ttf' },
        { url: '/fonts/NotoSansCJKsc-Regular.otf' },
        { url: '/fonts/NotoSerifCJKsc-Regular.otf' }
    ],
    onContextMenu: (x, y, annotationId) => {
        ...
    }
});
ofd.setClock('rofd', Date.now());

ofd.loadOfd(bytes);
```

### Native (`rofd-xilem-view`)

Run the reference desktop host:

```bash
cargo run -p xilem-app
```

Embed rofd in a xilem (masonry) application:

```toml
# Cargo.toml
[dependencies]
rofd-xilem-view = { git = "https://github.com/office-rs/rofd", branch = "main" }
# xilem must be pinned to the same Linebender rev the workspace uses
xilem = { git = "https://github.com/linebender/xilem", rev = "271a27a6d4a9" }
```

```rust
use std::sync::Arc;
use rofd_xilem_view::{command_queue, ofd, OfdCommandQueue};
use xilem::view::{flex_col, text_button, FlexExt};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

struct AppState {
    commands: OfdCommandQueue,
    modified: bool,
}

fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let commands = state.commands.clone();
    flex_col((
        text_button("Undo", move |_: &mut AppState| {
            // Host → editor: push a command; the view rebuild executes it
            // against the live component.
            commands.lock().unwrap().push(Arc::new(|c| {
                c.undo();
            }));
        }),
        ofd(state.commands.clone())
            .on_change(|s: &mut AppState| s.modified = true) // editor → host
            .flex(1.0),
    ))
}

fn main() -> Result<(), xilem::winit::error::EventLoopError> {
    Xilem::new_simple(
        AppState {
            commands: command_queue(),
            modified: false,
        },
        app_logic,
        WindowOptions::new("rofd"),
    )
    .run_in(EventLoop::with_user_event())
}
```

The editor is a first-class masonry widget: focus, pointer capture, IME sessions, clipboard shortcuts, and ctrl+wheel zoom are handled inside — the host never touches winit. Two channels connect host and editor: component callbacks surface as chainable `.on_change` / `.on_context_menu` / `.on_save_request` / … handlers, and imperative calls (toolbar buttons, programmatic edits) are `Arc<dyn Fn(&mut OfdComponent)>` commands pushed onto a queue the view drains on every rebuild. The full reference implementation (toolbar, context menu, file I/O, zoom) is in [`crates/xilem-app`](crates/xilem-app).

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

---

## Platform support

| Platform                  | Rendering backend        | Status                                      |
| ------------------------- | ------------------------ | ------------------------------------------- |
| Desktop (Windows / Linux) | wgpu (Vulkan/Metal/DX12) | ✅                                          |
| Web                       | WebGPU                   | ✅ (Chrome/Edge 113+, no Canvas2D fallback) |

---

## License

Licensed under the **Apache License 2.0 (Apache-2.0)**. See [LICENSE](LICENSE) for the full license text.

Copyright © 2026 rofd contributors.
