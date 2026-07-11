# rofd

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform: Desktop](https://img.shields.io/badge/platform-desktop%20%7C%20web-blue.svg)](#平台支持)
[![Status: WIP](https://img.shields.io/badge/status-v1%20WIP-yellow.svg)](#路线图)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

> OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，native + WASM 双平台。

rofd 把一个固定版式文档标准（OFD，中国版 PDF）做成一个可嵌入的编辑器组件，类比 HTML 的 `<textarea>`：宿主控制消息循环、转发事件，库负责文档解析、GPU 渲染、批注编辑与撤销/重做。主文档（body）只读渲染，**批注是唯一可编辑层**--这套"手术刀式"设计让 subset 渲染模型仍能对未建模内容做到**字节级保真保存**。

架构复刻自 [`reditor`](https://github.com/...)（Rust + GPU 的 OOXML 编辑器库），按 OFD 固定版式替换排版层。

---

## 目录

- [特性](#特性)
- [架构](#架构)
- [快速开始](#快速开始)
- [作为库集成](#作为库集成)
- [项目结构](#项目结构)
- [开发](#开发)
- [平台支持](#平台支持)
- [路线图](#路线图)
- [文档](#文档)
- [参考项目](#参考项目)

---

## 特性

**v1（进行中）**

- 📄 **OFD 渲染**：Text / Image / Path / Composite 对象 + CTM（坐标系变换）+ JPG/PNG；常见字体；CJK 支持。模板继承 / JBIG2 / 瓦片图等冷门特性按需补，v1 桩处理（能展开就展开，否则跳过 + warning）。
- ✏️ **批注编辑**：标注笔迹（高亮 / 下划线 / 删除线 / 自由绘制）、图形批注（矩形 / 椭圆 / 箭头 / 直线 / 文本框）、便签评论（带回复线程）、印章水印。电子签名 v1 只渲染已有签名（只读显示）。
- ↩️ **命令模式 + 撤销/重做**：Step / Transaction / History（cap 100），每命令 apply ↔ undo 可还原。
- 🖥️ **双平台**：native（xilem + winit + WebGPU/wgpu）+ Web（WASM + WebGPU）共享同一核心。
- 🔪 **手术刀保存**：`save_ofd` 仅重序列化批注条目，其余条目字节级保留--保签名、保未建模 body、零损耗。
- 🧩 **单一集成入口**：`EditorComponent` 门面 + `RenderTarget` trait，平台适配器极薄。

---

## 架构

严格 5 层单向依赖，反向边禁止。`dom` 是纯数据模型，无 ZIP/XML/GUI 依赖；向上依次是 io / render / editor，component 组合三者，native-view / web-view 是薄平台适配器。

```
examples/native-app        examples/web-app (JS)
        │                         │
   native-view ─────────────── web-view          ← 平台适配器（持 io: parse/save）
        └──────────┬──────────────┘
                 component                         ← 唯一集成入口 EditorComponent
        ┌──────────┼──────────────┐
     render      editor           (io)            ← render 产出 imaging Scene
        └──────────┴──────────────┘
                 dom (OfdDocument)                 ← 纯数据：PageModel(body 只读) + AnnotationModel(可变)
```

**三个核心设计**：

1. **body 只读 + 批注可变**：`OfdDocument` 分 `PageModel`（加载后只读）与 `AnnotationModel`（editor 唯一改动处）。渲染时 body 场景一次构建后稳定缓存，批注编辑只失效对应页的 overlay 场景。
2. **手术刀保存**：`parse_ofd` 时 body 页 XML / 签名 / 资源条目的原始字节以 `Arc` 保留在 `PackageHandle`；`save_ofd` 只重写批注条目，其余原样拷字节。subset 模型未建模的内容因此仍能保真。
3. **渲染产出 `imaging::record::Scene`**：backend-agnostic 的绘制 IR（来自 forest-rs/imaging），native 用 `Painter::replay` 回放，web 用 `imaging_vello` 转 `vello::Scene`。render 层与 vello API 变更解耦。

完整设计决策见 [设计 spec](docs/superpowers/specs/2026-07-08-ofd-editor-design.md)。在本仓库工作时请先读 [AGENTS.md](AGENTS.md)。

---

## 快速开始

> 工作目录为仓库根。Shell 用 bash（Unix 语法）。

### 先决条件

- Rust（edition 2021，建议最新 stable）+ `wasm32-unknown-unknown` target（Web 用）
- Node.js 18+ 与 npm（Web 示例用）
- `wasm-pack`（构建 Web SDK 用）：`cargo install wasm-pack`

### 运行 native 示例

```bash
cargo run -p native-app                           # 启动空编辑器
cargo run -p native-app -- test/ru-yuan-ji-lu.ofd  # 直接打开一个真实 OFD
```

打开后顶部工具栏的 **Open** 按钮可弹出原生文件对话框选择 `.ofd` 文件。

> native 示例按相对 CWD 的路径查找默认 CJK 字体（`examples/web-app/public/NotoSansSC-Regular.otf`）。若未下载，文字不渲染但程序不崩--先跑一次下面的 `npm run fetch:font` 即可。

### 运行 web 示例

```bash
rustup target add wasm32-unknown-unknown
cd examples/web-app
npm install
npm run fetch:font        # 下载 NotoSansSC 到 public/（文字渲染必需）
npm run build:sdk         # wasm-pack 构建 web-view -> sdk/dist
npm run dev               # 启动 Vite 开发服务器
```

浏览器打开 Vite 提示的地址，用左上角文件选择器加载 `.ofd`。

---

## 作为库集成

rofd 的核心入口是 `EditorComponent`。典型集成流（native 为例）：

```rust
use rofd_component::{EditorComponent, EditorConfig, ViewEvent, RenderTarget};
use rofd_io::parse_ofd;

// 1. 解析 OFD（io 层，返回文档 + 原始包句柄 + 降级警告）
let report = parse_ofd(&ofd_bytes)?;            // LoadReport { document, package, warnings }

// 2. 构造编辑器组件，载入文档
let mut editor = EditorComponent::new(EditorConfig::new(font_bytes));
editor.load_document(report.document);
editor.set_clock("author".into(), 1700000000);  // 库不取系统时间，宿主注入

// 3. 事件循环：转发平台事件 -> 命中测试 -> 命令 -> 重绘
let outcome = editor.handle_event(&view_event);  // ViewEvent: Pointer/Key/Scroll/Zoom/...
if outcome.needs_repaint {
    editor.render(&mut render_target);           // 实现 RenderTarget trait
}

// 4. 保存（手术刀：批注条目重序列化，其余字节级保留）
let new_bytes = rofd_io::save_ofd(editor.document(), &report.package)?;
```

Web 端通过 npm 包 `@rofd/sdk`（源在 `crates/web-view/sdk/`）集成：

```ts
import { Editor } from '@rofd/sdk';

const editor = await Editor.create(canvas, fontBytes);
editor.setClock('author', Date.now());
editor.loadOfd(ofdBytes);            // Uint8Array
editor.handlePointerDown(x, y, btn); // 返回是否需要重绘
editor.render();
```

---

## 项目结构

```
rofd/
├── crates/
│   ├── dom/          # 纯数据模型 OfdDocument（无 ZIP/XML 依赖）
│   ├── io/           # parse_ofd / save_ofd（手术刀）/ write_ofd（全量）+ PackageHandle
│   ├── render/       # imaging::record::Scene 构建 + text/(font/glyph/shape) + hit_test
│   ├── editor/       # 批注选区、命令、Step/Transaction/History
│   ├── component/    # EditorComponent 门面（唯一集成入口）+ ViewEvent/RenderTarget
│   ├── native-view/  # native 薄适配器：EditorApp + WinitEventBridge
│   └── web-view/     # WASM 薄适配器：WasmEditor + WebGpuRenderTarget + TS SDK
├── examples/
│   ├── native-app/   # xilem + masonry 宿主示例（cargo 成员）
│   └── web-app/      # Vite + TS 宿主示例（JS 项目）
├── test/             # 真实 .ofd 测试样本（如 ru-yuan-ji-lu.ofd）
└── docs/superpowers/ # 设计 spec + 实现 plans
```

每个 crate 保持小而聚焦（典型 200-400 行/文件，≤800 上限）。详细职责与依赖方向见 [AGENTS.md](AGENTS.md) 的"仓库布局与分层"。

---

## 开发

```bash
cargo build --workspace                            # 全量编译
cargo test  --workspace                            # 全量测试
cargo test  -p rofd-io surgical                    # 手术刀字节保留测试（核心）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo llvm-cov --workspace                         # 覆盖率，目标 80%
```

**测试约定**：TDD（先红后绿）。核心测试是 io 的手术刀字节保留（`parse -> save_ofd -> 未触碰条目逐字节相等`）；editor 每命令 apply ↔ undo 可还原；render 用场景结构断言（对象数/类型，非像素，避免 GPU 快照 flaky）。

新功能遵循 superpowers 工作流：先写/更新 `docs/superpowers/specs/` 设计 spec -> 写 `docs/superpowers/plans/` 实现 plan -> TDD -> 自审 -> 提交（conventional commits）。

---

## 平台支持

| 平台 | 渲染后端 | 状态 |
|---|---|---|
| Desktop（Windows / Linux） | wgpu（Vulkan/Metal/DX12） | ✅ v1 |
| Web | WebGPU | ✅ v1（Chrome/Edge 113+，无 Canvas2D 回退） |

Web 端首帧 shader 编译延迟靠 `warmup()` 前置；IME（中文输入）依赖上游 xilem plumbing，仍在完善。

---

## 路线图

| 子系统 | 部署 | 写入路径 | 状态 |
|---|---|---|---|
| **A. view + 批注编辑器** | 前端（native + WASM） | 手术刀（`save_ofd`） | 🚧 v1 进行中 |
| **B. 后端 OFD 生成** | 服务端 | 全量（`write_ofd`） | 🔜 后续 |
| **C. PDF → OFD 转换** | 服务端 | 全量（`write_ofd`） | 🔜 后续 |
| **D. 后端读 OFD → 改 → 输出** | 服务端 | 手术刀（`save_ofd`） | 🔜 后续 |

A/B/C/D 共享 `rofd-dom` + `rofd-io`；B/C/D 只用 dom + io（+ 未来的 `rofd-pdf`），不需 render/editor/component。架构已为 B/C/D 留门（见 spec §11）。

---

## 文档

- [设计 spec](docs/superpowers/specs/2026-07-08-ofd-editor-design.md) - 架构、数据模型、决策记录（事实来源）
- [实现 plans](docs/superpowers/plans/) - 每 crate/阶段的 TDD 任务分解
- [AGENTS.md](AGENTS.md) - 在本仓库工作时的规则、命令与不变量（给协作者与 AI 代理）

---

## 许可

本项目基于 [GNU 通用公共许可证 v3 或更高版本（GPL-3.0-or-later）](LICENSE) 开源。使用、修改与分发均须遵守该协议条款；分发（含作为 larger program 的一部分）时必须以相同协议开源并附带源代码。

版权所有 © 2026 rofd 贡献者。
