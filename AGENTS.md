# AGENTS.md

本文件为在本仓库工作的 AI 编程代理（以及人类协作者）提供上下文。先读这里，再动代码。

设计 spec 是事实的最终来源：最新一份 [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md)。本文档与代码现状对齐；二者冲突时以代码 + spec 为准，并回来修本文档。

---

## 1. 项目是什么

**rofd** 是一个 OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，双平台（native + WASM）。as-built 参照项目是 `D:/code/rword`（OOXML 编辑器库，同型改造已全部落地）。

- **v1 范围**：查看 + 批注。主文档（body）**只读渲染**；批注是**唯一可变层**。
- **库形态**：以 `OfdComponent` 为唯一集成入口（类比 `<textarea>`），宿主控制消息循环并转发事件。库本身不取系统时间、不直接依赖 GUI 框架。
- **平台边界**：库 = 平台无关核心（dom/io/render/editor/component）+ 两个平台适配器（xilem-view/web-view）；`crates/xilem-app`、`crates/web-app` 与 `crates/tauri-app` 是宿主应用，不是库的交付物（见 §4.9）。
- **非目标**：编辑 body 内容、创建/验签电子签名、全保真渲染（模板继承/JBIG2/瓦片图按需补，v1 桩处理）、实时协同。
- 为 B（后端生成）/ C（PDF→OFD）/ D（后端读改写）留门，但不实现。

---

## 2. 常用命令

> 工作目录始终为仓库根 `D:\code\rofd`，除非另说明。Shell 为 bash（Unix 语法）。

### 构建 / 测试

```bash
cargo build --workspace          # 全量编译
cargo test  --workspace --exclude tauri-app --exclude rofd-web-view
cargo test  -p rofd-io           # 单 crate 测试
cargo test  -p rofd-io surgical  # 按名过滤（手术刀字节保留测试）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

覆盖率目标 80%：`cargo llvm-cov --workspace`（需安装 `cargo-llvm-cov`）。

### 运行 native 宿主

```bash
cargo run -p xilem-app                           # 空编辑器
cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd  # 打开根目录 test/ 下的 .ofd
```

命令行路径参数经宿主 `host::document_io::load_ofd` 加载。native 宿主以空字体配置启动（`OfdConfig::new(Arc::new(vec![]))`，见 `crates/xilem-app/src/main.rs`），字形交给系统字体回退；仓库自带的 CJK 字体位于 `crates/web-app/public/fonts/`（git 跟踪，无需下载）。

### 构建 / 运行 web 宿主

```bash
rustup target add wasm32-unknown-unknown          # 一次性
cd crates/web-app
npm install
npm run build:sdk         # wasm-pack build crates/web-view -> sdk/dist
npm run dev               # vite 开发服务器
npm run build             # vite 生产构建
```

`build:sdk` 等价于 `cd ../web-view && wasm-pack build --target web --out-dir sdk/dist`。

### 构建 / 运行 tauri 桌面宿主

```bash
cd crates/tauri-app
npm install
npm run build:sdk         # 同 web-app：wasm-pack build crates/web-view -> sdk/dist
npm run tauri dev         # 开发：起 vite(1420) + 系统 WebView 窗口
npm run tauri build       # 打包：vite build -> tauri 打成桌面安装包
```

tauri-app 前端**复用 web-app 源码**（`main.ts` 从 `../web-app/src` 导入 App.vue）；仅在挂载前用 `setFileHost` 注入原生文件桥（`src/host/tauri.ts`，走 tauri-plugin-dialog + tauri-plugin-fs 的原生打开/保存对话框）。字体与 sample.ofd 从 web-app 的 `public/` 本地加载（vite `publicDir` 指向它），脱离 CDN。`src-tauri` 的 Rust 壳只启动窗口 + 注册插件，**不依赖任何 rofd crate**（§4.9）。平台支持：Windows（WebView2）开箱即用；macOS/Linux 受 WebKit 的 WebGPU 进度限制。

产物命名统一为 `rofd`（详见 release-tauri.yml）。

---

## 3. 仓库布局与分层

严格 5 层单向依赖，**反向边禁止**。每个 crate 的 `[dependencies]` 是依赖方向的唯一事实来源。**平台边界线画在 component 与适配器之间**：component 及以下五个 crate 平台无关（不依赖 winit/web-sys/wgpu/arboard 等平台 crate），xilem-view / web-view 是仅有的两层平台绑定。

```
crates/xilem-app      ─┐
                       ├─► xilem-view ─┐
                       │                ├─► component ─┬─► render ──┐
crates/web-app ─► web-view ─────────────┘              ├─► editor ──┴─► dom
   (JS + Vite,         (wasm-bindgen     │              │
    非 cargo 成员)      适配器)           │              └─► io ────────┘
```

| crate            | 路径                 | 职责 | 依赖（rofd 内部） |
| ---------------- | -------------------- | --- | ----------------- |
| `rofd-dom`       | `crates/dom`         | 纯数据模型 | — |
| `rofd-io`        | `crates/io`          | `parse_ofd` / `save_ofd`（手术刀）/ `write_ofd`（全量）+ `PackageHandle` | dom |
| `rofd-render`    | `crates/render`      | `imaging::record::Scene` 构建 + hit_test/caret_rect | dom |
| `rofd-editor`    | `crates/editor`      | 批注选区、命令模式、Step/Transaction/History | dom |
| `rofd-component` | `crates/component`   | **唯一集成入口** `OfdComponent`：ViewEvent、Callbacks、脏缓存 | dom + render + editor（**不依赖 io**） |
| `rofd-xilem-view`| `crates/xilem-view`  | masonry/xilem 薄适配器：`OfdWidget` + `ofd()`/`ofd_with_config()` | component |
| `rofd-web-view`  | `crates/web-view`    | WASM 薄适配器：`WasmOfd` + `WebGpuRenderTarget` + TS SDK | component + io + editor + dom + vello + imaging + imaging_vello + wgpu + web-sys |
| `xilem-app`      | `crates/xilem-app`   | xilem 宿主应用（文件对话框 + 工具栏 UI 策略） | xilem-view + component + io + dom + xilem + rfd |
| web-app          | `crates/web-app`     | Vite + TS 宿主应用（非 cargo 成员） | `@office-rs/rofd`（= `crates/web-view/sdk`） |
| `tauri-app`      | `crates/tauri-app`   | Tauri 桌面宿主应用 | tauri 壳不依赖 rofd crate；前端复用 web-app |

> **两条历史偏离（改造后的 as-built）**：
> 1. component 保持 io-free——io 依赖从不下沉到组件；
> 2. native 侧 io 调用已**上移到宿主**（`xilem-app/src/host/document_io.rs`），适配器 rofd-xilem-view 不依赖 io；web-view 侧仍持 io（wasm 宿主自己就是"宿主"）。

---

## 4. 不可违反的不变量

### 4.1 依赖严格向上，反向边禁止

### 4.2 body 只读；批注是唯一可变面

### 4.3 手术刀保存：未触碰条目字节级保留

`save_ofd(doc, pkg)`：批注条目从 `AnnotationModel` 重新序列化；`Document.xml` 字节级打补丁（`<MaxUnitID>` + 缺失时插入 `<Annotations>` loc，严格阅读器只经此引用发现批注）；其余条目原样拷字节。改 io 保存逻辑后，核心测试（`crates/io/tests/round_trip.rs`、`save_surgical.rs`）必须仍绿。宿主层另有 `crates/xilem-app/tests/c2_save.rs`（#[ignore]，真实样例）。

### 4.4 库不取系统时间

库内**绝不**调 `Date::now()` / `SystemTime::now()`。宿主通过 `set_clock(author, ts)` 注入；命令用注入时间填 `created`/`modified`。**例外（2026-09 明确）**：单调 `Instant`（非挂钟、不可持久化、不随系统时钟跳变）可用于动画计时——native 侧 caret blink 的 `tick_blink` 即此用途；wasm 侧不使用 `Instant`。

### 4.5 渲染产出 `imaging::record::Scene`，不是 `vello::Scene`

### 4.6 错误显式分层，绝不静默吞

### 4.7 没有 `Format` trait

### 4.8 ID 约定

### 4.9 平台边界：功能内聚核心层，适配器只做绑定

---

## 5. 各 crate 工作要点

### rofd-component

- 唯一入口 `OfdComponent`：`new_native()`/`new_wasm()`（`#[cfg(target_arch = "wasm32")]` 门控，非 feature）、`handle_event`。
- `OfdConfig`：`default_font_bytes`、`page_gap`（默认 20）、`zoom`（初始 PX_PER_MM，`with_zoom` 可覆盖）。
- 场景脏缓存（终态公共 API）：`update_scene()` 按脏标志重合成、`scene() -> &Scene`、`set_viewport_size(f64,f64)`、`mark_scene_dirty`（pub(crate)）。
- 文本编辑：`paste_text`、`delete_annotation`、`tick_blink`（native）、preedit 状态机（组件自持 `Option<PreeditState>`，简化 caret overlay，不做 ghost-document 回流）。

### rofd-xilem-view（as-built 九条契约）

见 §"Native Ofd Widget"。

### rofd-web-view

- `WasmOfd`（wasm-bindgen）+ `WebGpuRenderTarget` + JS 事件桥。工厂 `create_wasm_ofd`。wasm-pack --target web。
- SDK 在 `crates/web-view/sdk/`，入口 `Ofd.init(container, config?)`，发布为 npm 包 `@office-rs/rofd`。

### xilem-app

- 纯 `Xilem::new_simple` 宿主。AppState 为 plain data（命令队列 + file/package 路径 + modified + has_selection + warnings + context_menu），另持一个 `Arc<Mutex<Option<Result<(), String>>>>` 保存结果槽：保存命令回传落盘结果，`app_logic` 开头排空，成功才清 modified、失败保持未保存态。命令队列与结果槽的锁仅在瞬间持有。
- 文件 I/O 落 `src/host/document_io.rs`：`LoadedOfd{document, package, warnings}`、`load_ofd`、`save_ofd`（Some(pkg) 手术刀 / None 全量；原子写）。Save-As 不产生新 PackageHandle。

---

## Native Ofd Widget（masonry/xilem 适配器）

`rofd-xilem-view` 把编辑器嵌为一等 masonry widget。宿主不碰 winit——focus、pointer capture、IME session、clipboard 路由全部经 masonry：

```
ofd(queue) / ofd_with_config(queue, config)    xilem View (ofd_view.rs)
  │ build: OfdWidget::new(config); actions ↔ .on_change/.on_context_menu/… handlers
  │ rebuild: drains `queue` → OfdWidget::with_component (host→widget commands)
  ▼
OfdWidget (masonry Widget)                      owns OfdComponent, pending queue,
  │                                               effective focus, pointer cursor
  │ masonry events → masonry_events.rs → ViewEvent
  │ component callbacks → pending queue → ctx.submit_action (after every touch)
  │ paint: component.update_scene() + painter.replay(scene)
  ▼
OfdComponent (rofd-component)
```

Key contracts:

1. **Host→widget commands**：`OfdCommand = Arc<dyn Fn(&mut OfdComponent) + Send + Sync>`；`OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>`，rebuild 时经 `with_component` 排空。任何被处理的回调返回 `MessageResult::Action(())` → 重跑宿主逻辑 → rebuild → 排空。命令必须 `Fn`（按钮重复触发），捕获数据在闭包内 clone。
   命令排空非空时提交一次内部 `OfdWidgetAction::HostCommandWake`（不映射任何宿主 handler，仅触发再一拍 app_logic），使宿主能消费命令结果；下一拍队列已空、不再提交 wake，故有界、不产生无限重渲染。
2. **Widget→host events**：`OfdWidgetAction` 镜像组件回调面；`PointerCursorChanged` 内部消费（驱动 `get_cursor`），绝不 submit。
3. **有效焦点** = widget 键盘焦点 ∧ 窗口焦点；组件出生未聚焦（首次点击前 caret 隐藏）。`Ime::Disabled → FocusLost` 强制提交 preedit，带防御性 regain。
4. **IME**：`accepts_text_input` 自动起停 session；每个触点末尾从 `caret_rect()` 刷新 `set_ime_area`。
5. **剪贴板**：Ctrl+V = `TextEvent::ClipboardPaste` → `paste_text`；Ctrl+C 在 widget 拦截经 `ctx.set_clipboard` 写出。Ctrl+X 在 rofd 为 copy-only（运行期 body 只读、无 selection extent，绝不删除）；宿主菜单无 Cut 项。
6. **Zoom（rofd 有意差异，无镜像）**：组件 viewport 直接乘法缩放（`zoom *= factor`，基线 `PX_PER_MM = 96/25.4`）；ctrl+wheel 发 `ZoomAt{factor, center}`（×1.1 / ×0.9，center = 指针视口位置），钳制 `[PX_PER_MM×0.25, PX_PER_MM×3.0]`。宿主/widget 不保留 f32 zoom 镜像。
7. **Blink**：`on_anim_frame` 在聚焦期间保活动画；500ms 时序归组件 `tick_blink`（单调 Instant）。
8. **Scroll**：wheel 事件 set_handled——滚动与自绘滚动条归组件；无 portal、无 CANVAS_CONTENT_HEIGHT。
9. **无障碍**：`Role::Document`。

If you're tempted to reintroduce a winit-layer bridge、canvas-origin 记账、或每帧 force_repaint 排序——don't；widget 就是整个平台适配器。

---

## 6. 测试约定

- 目标 80% 覆盖，TDD（先红后绿）。
- 手术刀字节保留是核心测试；真实样例测试保持 `#[ignore]`：
  - `cargo test -p xilem-app --test c2_save -- --ignored`
- 渲染用场景结构断言（对象数/类型，非像素）。
- xilem-view 无头 harness（masonry_testing，7 个）：打字、双击选词、IME、滚轮、ctrl+wheel、Ctrl+C、窗口焦点门控。

---

## 7. 依赖钉版（动之前必读）

| crate | 源 | 版本/rev |
| ----- | -- | --------- |
| `xilem` | linebender/xilem git | `271a27a6d4a930f7878d404f9014e3c50a3a9b88` |
| `masonry_testing` | 同上（dev-dep） | 同 rev |
| `imaging` / `imaging_vello` | crates.io | `0.0.1` |
| vello / parley | crates.io | `0.8` |
| wgpu | crates.io | `28` |

Toolchain：`rust-toolchain.toml` 钉 `1.98.1`；新 pin 的包声明 `rust-version = 1.96`。已无 masonry_winit/winit 直接依赖（masonry_winit 内部使用 winit）。

---

## 8. 工作流

superpowers 工作流：spec → plan → TDD → 自审 → 提交，全部 conventional commits。

---

## 9. 常见陷阱

| ✅ Do | ❌ Don't |
| ----- | -------- |
| render 用 imaging Painter API | 在 render 里直接构造 vello::Scene |
| 宿主 set_clock（Instant 仅用于 blink 动画） | 库内调挂钟 |
| editor 只改 .annotations | 在 editor 里改 pages |
| xilem-view 只依赖 component | 往适配器塞 io/业务逻辑 |
| 改 io 后跑手术刀测试 | 只断言模型相等 |

---

## 10. 编码规范

- 设计文档可以保留 WPS 字样，但是代码注释、代码变量、代码文件命名不要存留 WPS 字样。

---

## 11. 参考项目

- **`D:/code/rword`** — Rust + GPU 的文档编辑器库，rofd 本次 masonry/xilem 改造与更名的 as-built 参照：适配器三文件（word_widget/masonry_events/word_view）、宿主（xilem-app + host/document_io）、更名 commit 序列。
- **`D:/code/reditor`** — 更早的 OOXML 编辑器库，rofd 分层骨架的历史来源。
