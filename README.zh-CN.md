# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#平台支持)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

[English](README.md) | **简体中文**

OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，native + WASM 双平台。

---

## 使用

Web 端通过 npm 包 `@office-rs/rofd` 集成：

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

本项目基于 [GNU 通用公共许可证 v3 或更高版本（GPL-3.0-or-later）](LICENSE) 开源。使用、修改与分发均须遵守该协议条款；分发（含作为 larger program 的一部分）时必须以相同协议开源并附带源代码。

版权所有 © 2026 rofd 贡献者。
