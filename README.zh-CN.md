# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#平台支持)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

[English](README.md) | **简体中文**

OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，native + WASM 双平台。

---

## 使用

### Web 端

Web 端通过 npm 包 `@office-rs/rofd` 集成：

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

### Native 端（`rofd-xilem-view`）

运行参考桌面宿主：

```bash
cargo run -p xilem-app
```

把 rofd 嵌入 xilem（masonry）应用：

```toml
# Cargo.toml
[dependencies]
rofd-xilem-view = { git = "https://github.com/office-rs/rofd", branch = "main" }
# xilem 必须钉到与工作区一致的 Linebender rev
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
        text_button("撤销", move |_: &mut AppState| {
            // 宿主 → 编辑器：推入命令，view rebuild 时对实时 component 执行
            commands.lock().unwrap().push(Arc::new(|c| {
                c.undo();
            }));
        }),
        ofd(state.commands.clone())
            .on_change(|s: &mut AppState| s.modified = true) // 编辑器 → 宿主
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

编辑器是一个标准 masonry widget：焦点、指针捕获、IME 会话、剪贴板快捷键、ctrl+wheel 缩放全部在内部处理——宿主无需接触 winit。宿主与编辑器之间有两条通道：component 回调体现为可链式调用的 `.on_change` / `.on_context_menu` / `.on_save_request` / … 处理器；命令式调用（工具栏按钮、程序化编辑）则是推入队列的 `Arc<dyn Fn(&mut OfdComponent)>` 命令，view 在每次 rebuild 时排空执行。完整参考实现（工具栏、右键菜单、文件 I/O、缩放）位于 [`crates/xilem-app`](crates/xilem-app)。

---

## 桌面客户端（Tauri）

无需自己搭前端，可直接下载打包好的 Windows 桌面客户端 **rofd**——它把上面的 Web 编辑器封装进系统 WebView。

### 下载安装

到本仓库的 [Releases](../../releases) 页面，从对应版本的 **Assets** 里下载安装包（二选一）：

| 文件 | 安装器 | 说明 |
|---|---|---|
| `rofd_<version>_x64-setup.exe` | NSIS | 常规安装向导，推荐 |
| `rofd_<version>_x64_en-US.msi` | MSI | 适合企业批量部署 |

`<version>` 为发布版本号（如 `0.1.0`）。安装后的程序名即 **rofd**。

> 仅提供 Windows 安装包：WebGPU 在 Windows 的 WebView2 上开箱即用；macOS/Linux 的系统 WebView 尚未完整支持 WebGPU，暂不构建。

---

## 平台支持

| 平台 | 渲染后端 | 状态 |
|---|---|---|
| Desktop（Windows / Linux） | wgpu（Vulkan/Metal/DX12） | ✅ |
| Web | WebGPU | ✅（Chrome/Edge 113+，无 Canvas2D 回退） |

---

## 许可

基于 **Apache License 2.0 (Apache-2.0)** 许可发布。完整许可证文本见 [LICENSE](LICENSE)。

版权所有 © 2026 rofd 贡献者。
