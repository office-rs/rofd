# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#平台支持)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

[English](README.md) | **简体中文**

OFD（GB/T 33190）**查看 + 批注 + 签章**编辑器**库**，Rust 实现，native + WASM 双平台。

---

## 使用

### Web 端

Web 端通过 npm 包 `@office-rs/rofd` 集成：

```ts
import { Ofd } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const ofd = await Ofd.init(container, {
    // 字体为显式配置：裸 init 不再拉任何字体。可用 `fontSlices` 指向切片
    // manifest（按需字形子集），或用 `fonts` 传整字体文件。
    fontSlices: { manifests: ['/fonts/manifest.json'] },
    onContextMenu: (x, y, annotationId) => {
        ...
    }
});
ofd.setClock('rofd', Date.now());

ofd.loadOfd(bytes);
```

WebGPU 不可用时（如浏览器关闭硬件加速），SDK 自动回退 CPU 软渲染（vello_cpu + 页位图缓存）；可用 `cpuFallback: false` 关闭。

### Native 端（`rofd-xilem-view`）

运行参考桌面宿主：

```bash
cargo run -p xilem-app
```

> **图形要求：** native 宿主需要硬件 GPU（无 GPU 环境请使用[网页版](#web-端)，WebGPU 不可用时自动切换 CPU 软渲染）。

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

### 签章（`rofd-sign`）

打开文件即验签；用证书文件签章，库不读时钟、不持有密钥。不在编辑器里的文件，字节进、字节出：

```rust
use rofd_sign::{sign, verify, PfxSigner, SealImage, SignOptions, VerifyOptions};

let reports = verify(&ofd_bytes, &VerifyOptions::default()); // 每条签名一份报告

let mut signer = PfxSigner::open(&pfx_bytes, "口令")?;
let signed = sign(&ofd_bytes, &mut signer, &SignOptions {
    seal: Some(SealImage { png, size_mm: 40.0 }),
    ..SignOptions::new("2026-11-24 10:30:00") // 签章时间，由宿主读取
})?;
```

签过章的文档仍可加批注：签名不保护批注层（GB/T 48666-2026），存盘时其余条目逐字节不变。
Web 端对应 `@office-rs/rofd` 的 `verifySignatures` / `signOfd`。

给当前打开的文档验签、签章，都通过组件：签名就地落进文档——一次调用，不产生第二个文档、不重新载入，
视图、选区和撤销历史都留在原处。用户保存之前不写文件，保存就是一次普通保存。
要用的类型随组件一起给出：

```rust
// rofd-component 的 `sign` feature：密钥和时间由宿主提供。
use rofd_component::signing::{SealImage, SignOptions, VerifyOptions};

// 验当前打开的文档，存没存盘都行：不存盘，也不读文件。
let reports = component.verify_signatures(&VerifyOptions::default());

// 用证书文件签章；自己的签名器用 `component.sign(&mut signer, &options)`。
// 签之前先问一句 `component.sign_refusal()`。
let reports = component.sign_pfx(&pfx_bytes, "口令", &SignOptions {
    seal: Some(SealImage { png, size_mm: 40.0 }),
    ..SignOptions::new("2026-11-24 10:30:00") // 签章时间，由宿主读取
})?;
```

组件和 Web SDK 是这个库的两个出口，提供同样的能力：两边收的东西一样时用同样的名字
（`verify_signatures` / `ofd.verifySignatures()`、`PreparedSignature`、`SignStep`）；
Web 端收不了 Rust 签名器的地方则成对出现——`component.sign_pfx(pfx, password, options)`
对应 `ofd.sign(pfx, password, options)`（签之前先问一句 `ofd.isSignBlocked()`），
`component.sign(&mut signer, options)` 对应 `ofd.prepareSign` 加 `apply`。
对照表在 `docs/agents/sign.md`。
三个示例应用（web / xilem / gpui）有完整流程——证书导入、点击落位、状态栏、存盘预检。

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
| Web | WebGPU（CPU 软渲染自动回退） | ✅（Chrome/Edge 113+） |

---

## 许可

基于 **Apache License 2.0 (Apache-2.0)** 许可发布。完整许可证文本见 [LICENSE](LICENSE)。

版权所有 © 2026 rofd 贡献者。
