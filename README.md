# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#平台支持)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，native + WASM 双平台。

---

## 使用

Web 端通过 npm 包 `@rofd/sdk` 集成：

```ts
import { Editor } from '@rofd/sdk';

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

## 平台支持

| 平台 | 渲染后端 | 状态                                     |
|---|---|------------------------------------------|
| Desktop（Windows / Linux） | wgpu（Vulkan/Metal/DX12） | ✅                                       |
| Web | WebGPU | ✅（Chrome/Edge 113+，无 Canvas2D 回退） |

---

## 许可

本项目基于 [GNU 通用公共许可证 v3 或更高版本（GPL-3.0-or-later）](LICENSE) 开源。使用、修改与分发均须遵守该协议条款；分发（含作为 larger program 的一部分）时必须以相同协议开源并附带源代码。

版权所有 © 2026 rofd 贡献者。
