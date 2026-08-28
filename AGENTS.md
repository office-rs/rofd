# AGENTS.md

本文件为在本仓库工作的 AI 编程代理（以及人类协作者）提供上下文。先读这里，再动代码。

设计 spec 是事实的最终来源：[`docs/superpowers/specs/2026-07-08-ofd-editor-design.md`](docs/superpowers/specs/2026-07-08-ofd-editor-design.md)。本文档与代码现状对齐；二者冲突时以代码 + spec 为准，并回来修本文档。

---

## 1. 项目是什么

**rofd** 是一个 OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，双平台（native + WASM）。参考项目是 `D:/code/reditor`（OOXML 编辑器库），复刻其分层架构。

- **v1 范围**：查看 + 批注。主文档（body）**只读渲染**；批注是**唯一可变层**。
- **库形态**：以 `EditorComponent` 为唯一集成入口（类比 `<textarea>`），宿主控制消息循环并转发事件。库本身不取系统时间、不直接依赖 GUI 框架。
- **平台边界**：库 = 平台无关核心（dom/io/render/editor/component）+ 两个平台适配器（native-view/web-view）；`examples/` 只是宿主接入的 demo，不是库的交付物（见 §4.9）。
- **非目标**：编辑 body 内容、创建/验签电子签名、全保真渲染（模板继承/JBIG2/瓦片图按需补，v1 桩处理）、实时协同。
- 为 B（后端生成）/ C（PDF→OFD）/ D（后端读改写）留门，但不实现。

---

## 2. 常用命令

> 工作目录始终为仓库根 `D:\code\rofd`，除非另说明。Shell 为 bash（Unix 语法）。

### 构建 / 测试

```bash
cargo build --workspace          # 全量编译
cargo test  --workspace          # 全量测试
cargo test  -p rofd-io           # 单 crate 测试
cargo test  -p rofd-io surgical  # 按名过滤（手术刀字节保留测试）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

覆盖率目标 80%：`cargo llvm-cov --workspace`（需安装 `cargo-llvm-cov`）。

### 运行 native 示例

```bash
cargo run -p native-app                           # 空编辑器
cargo run -p native-app -- test/ru-yuan-ji-lu.ofd  # 打开根目录 test/ 下的 .ofd
```

native-app 按相对 CWD 的候选路径查找默认 CJK 字体（`examples/web-app/public/NotoSansSC-Regular.otf`）。若未下载，文字不渲染但程序不崩——先跑下面 web-app 的 `npm run fetch:font` 即可获得该字体文件。

### 构建 / 运行 web 示例

```bash
rustup target add wasm32-unknown-unknown          # 一次性
cd examples/web-app
npm install
npm run fetch:font        # 下载 NotoSansSC 到 public/（文字渲染必需）
npm run build:sdk         # wasm-pack build crates/web-view -> sdk/dist
npm run dev               # vite 开发服务器
npm run build             # vite 生产构建
```

`build:sdk` 等价于 `cd ../../crates/web-view && wasm-pack build --target web --out-dir sdk/dist`。

---

## 3. 仓库布局与分层

严格 5 层单向依赖，**反向边禁止**。每个 crate 的 `[dependencies]` 是依赖方向的唯一事实来源。**平台边界线画在 component 与适配器之间**：component 及以下五个 crate 平台无关（不依赖 winit/web-sys/wgpu/arboard 等平台 crate），native-view / web-view 是仅有的两层平台绑定。

```
examples/native-app   ─┐
                       ├─► native-view ─┐
                       │                ├─► component ─┬─► render ──┐
examples/web-app ─► web-view ───────────┘              ├─► editor ──┴─► dom
   (JS + Vite,         (wasm-bindgen     │              │
    非 cargo 成员)      适配器)           │              └─► io ────────┘
                                          │
                                          └─ native-view / web-view 另直接依赖 io（parse_ofd / save_ofd）
```

| crate | 路径 | 职责 | 依赖（rofd 内部） |
|---|---|---|---|
| `rofd-dom` | `crates/dom` | 纯数据模型 `OfdDocument`（PageModel 只读 body + AnnotationModel 可变批注 + 元数据）。无 ZIP/XML。public 字段 + `Default` | — |
| `rofd-io` | `crates/io` | `parse_ofd` / `save_ofd`（手术刀）/ `write_ofd`（全量）+ `PackageHandle` | dom |
| `rofd-render` | `crates/render` | `imaging::record::Scene` 构建（body + CTM + 批注 overlay）+ `text/`（font/glyph/shape）+ `hit_test`/`caret_rect` | dom |
| `rofd-editor` | `crates/editor` | 批注选区、命令模式、Step/Transaction/History（cap 100） | dom |
| `rofd-component` | `crates/component` | **唯一集成入口** `EditorComponent`：`ViewEvent`、`RenderTarget`、Callbacks、脏缓存 | dom + render + editor（**不依赖 io**） |
| `rofd-native-view` | `crates/native-view` | native 薄适配器：`EditorApp` + `WinitEventBridge` | component + render + io + dom + winit |
| `rofd-web-view` | `crates/web-view` | WASM 薄适配器：`WasmEditor` + `WebGpuRenderTarget` + TS SDK | component + io + dom + vello + imaging + imaging_vello + wgpu + web-sys |
| `native-app` | `examples/native-app` | xilem + masonry 宿主示例 | native-view + component + xilem + masonry_winit + rfd |
| web-app | `examples/web-app` | Vite + TS 宿主示例（非 cargo 成员） | `@office-rs/rofd`（= `crates/web-view/sdk`） |

> **关键偏离 spec**：spec 设想 `component` 依赖 io 做 load/save 便利方法；**实际实现把 io 依赖下放到适配器层**（native-view / web-view 持有 `PackageHandle`、调 `parse_ofd`/`save_ofd`），`EditorComponent` 保持 io-free、更易复用。改 component 时不要往里塞 io 调用。

---

## 4. 不可违反的不变量

动任何 crate 前先内化这些。违反任一条都应视为 bug。

### 4.1 依赖严格向上，反向边禁止
`dom` 不依赖任何 rofd crate；`io`/`render`/`editor` 只依赖 `dom`；`component` 不依赖 `io`/`native-view`/`web-view`。需要"从 dom 反查渲染信息"时，用 render 的查询 API（`hit_test`/`caret_rect`），不要让 dom/render 反向依赖 editor。

### 4.2 body 只读；批注是唯一可变面
`Editor` 拥有整个 `OfdDocument`，但**只通过命令改 `.annotations`**，绝不碰 `pages`。`pages` 在 editor 里只读是**运行约定**（由 io 的 save 逻辑编码"重写批注、保留 body 原样"保证），类型不锁死——生成/D 场景可自由构造 `Page`。

### 4.3 手术刀保存：未触碰条目字节级保留
`save_ofd(doc, pkg)`：**批注条目从 `AnnotationModel` 重新序列化；其余条目原样拷字节**（`PackageHandle` 以 `Arc` 保留原始字节）。这就是 subset 模型仍能保真的原因。核心测试：`parse_ofd → save_ofd → 未触碰条目字节逐字节相等`（见 `crates/io/tests/round_trip.rs`、`save_surgical.rs`）。改 io 的保存逻辑后，此测试必须仍绿。

### 4.4 库不取系统时间
库内**绝不**调 `Date::now()` / `SystemTime`。宿主通过 `editor.set_clock(author, ts)` 注入 author + 时间戳；命令用 `self.current_ts` 填 `created`/`modified`。wasm 宿主传 `Date.now()`，native 宿主自定。

### 4.5 渲染产出 `imaging::record::Scene`，不是 `vello::Scene`
`rofd-render` 产出 backend-agnostic 的 `imaging::record::Scene`（来自 forest-rs/imaging）。native 侧用 `Painter::replay` 回放进 masonry canvas；web 侧用 `imaging_vello::VelloSceneSink` 转 `vello::Scene` 再渲染。**改 render 时用 imaging Painter API（fill/stroke/glyphs/draw_image）**，不要直接构造 `vello::Scene`。imaging 无 transform-aware 子场景回放，需把 `page_origin + zoom + CTM` 烘焙进每个 draw call（不缓存 body/annotation 子场景）。

### 4.6 错误显式分层，绝不静默吞
- 硬错（ZIP 损坏/XML 畸形）→ `Result<_, OfdError>`（结构化 enum：`Zip`/`Xml`/`Schema`/`Io`）。
- 可降级问题（模板未展开/JBIG2/未知对象/字体替换/缺资源）→ `OfdWarning`，继续加载，经 `on_warning` 回调上抛。
- 所有 `?` 带 context；无裸 `unwrap`/`ignore`；输入校验在 io 边界 fail-fast。

### 4.7 没有 `Format` trait
单格式项目，YAGNI。JSON 测试 fixture 由模型 `derive serde` 后用 `serde_json` 直接获得，不引入多格式抽象。

### 4.8 ID 约定
`ObjectId`/`PageId` = OFD ID 字符串 newtype；`AnnotationId` = uuid v4。

### 4.9 平台边界：功能内聚核心层，适配器只做绑定
- **component 及以下平台无关**：不依赖任何平台 crate（winit/web-sys/wgpu/arboard/文件对话框等），必须同时可编译 native 与 wasm32。平台差异只允许出现在 native-view / web-view。
- **功能内聚**：状态机、几何/命中、命令等业务逻辑一律落在 component 及以下；适配器**不实现功能**，只做事件映射、渲染目标对接、平台能力默认装配。
- **平台能力经回调下沉**：功能需要平台能力（剪贴板/保存路径/时钟/光标）时，component 定义回调（如 `on_copy`/`on_save_request`），适配器默认接好（可配置关闭），宿主零配置。库自身绝不直接触碰平台设施。
- **API 以宿主易用为先**：适配器与 SDK 对外 API 遵循"默认开箱即用、可选覆盖"；宿主接入代码量最小化是 API 设计的验收标准，examples 即用量样板。

---

## 5. 各 crate 工作要点

### rofd-dom
- 所有结构 `#[derive(Debug, Clone, Default)]` + public 字段。
- `AnnotationModel` 是唯一可变面；批注的 Appearance **不存模型**——overlay 几何由 render 从 `payload` 实时算。
- 字体/图片以 `Arc<Vec<u8>>` 共享，避免 CJK 字体大对象重复拷贝。

### rofd-io
- 三个入口：`parse_ofd`（→ `LoadReport{document, package, warnings}`）、`save_ofd`（手术刀，A+D）、`write_ofd`（全量，B+C）。
- `PackageHandle` 对外不透明：原始条目字节（`Arc`）+ name→位置 + 条目分类索引。
- 改 parse 时：未建模对象（模板/JBIG2/冷门）→ 跳过 + warning，**不 fatal**。
- 改 save 时：保持"只重写批注条目"语义；为 D 的 body 重写留路（条目驱动选择性重写）。

### rofd-render
- 产出 `imaging::record::Scene`。`text/` 子模块：`font`（OTF/TTF 解析 + `FontCache`）、`glyph`（按字形 ID + delta 画轮廓，body 文字用此、不整形）、`shape`（Parley 整形，仅批注文本用）。
- 脏缓存：body_scene 一次构建后稳定缓存（body 只读永不失效）；批注编辑只让对应页的 annotation_scene 失效。
- 交互 API：`hit_test` / `caret_rect` 算几何（像素→逻辑量 `HitTarget`），喂给 editor；editor 只存 `(AnnotationId, offset)`，不碰像素。

### rofd-editor
- 命令模式 + Step/Transaction/History。每命令 apply→undo 必须可还原。文本编辑产生 `ReplaceAnnotationStep`（before/after 整个 annotation）。
- editor **无回调**——宿主/component 层在命令后查询状态。变更经 `on_change(affected_pages)` 在 component 层失效缓存。
- left/right = ±1 char（纯逻辑）；up/down 视觉行导航 v1 降级（见 spec §10）。

### rofd-component
- 唯一入口 `EditorComponent`：`new_native()`/`new_wasm()`（构造目标用 `#[cfg(target_arch = "wasm32")]` 门控，非 feature）、`handle_event`、`render`、`register_font`。
- `RenderTarget` trait：native 由 `VelloRenderTarget`、wasm 由 `WebGpuRenderTarget` 实现。
- `ViewEvent`：Key/Pointer/Scroll/Zoom/Resize/Ime/Focus 等，平台无关。
- **不依赖 io**——load/save 的字节→文档转换由上层适配器完成。

### rofd-native-view
- 薄适配器：只做 winit/masonry 事件映射与平台能力默认装配（剪贴板等），不承载功能逻辑（§4.9）。
- 三层：Host（examples/native-app，masonry/xilem 状态 + 渲染循环）、`WinitEventBridge`（modifiers/cursor/scale_factor/canvas_origin）、`EditorApp`（EditorComponent + 文件路径 + modified）。
- Bridge **不进** EditorApp（框架无关、host 可前置拦截、生命周期不同）。
- 输入在 **winit 层**路由到 editor（不经 masonry widget 事件系统）；canvas widget 仅渲染。
- 坐标链：`winit CursorMoved(物理px) ÷ scale_factor − canvas_origin → 画布逻辑px → ViewEvent::PointerMove`。
- `EditorApp` 持 `Rc<RefCell<parley::FontContext>>`（经 FontStore），**非 `Send`**；单线程，`Arc<Mutex>` 镜像 reditor 模式（见 `#[allow(clippy::arc_with_non_send_sync)]`）。

### rofd-web-view
- 薄适配器：只做 DOM 事件桥、WebGPU 对接与平台能力默认装配（剪贴板等），不承载功能逻辑（§4.9）。
- `WasmEditor`（wasm-bindgen）+ `WebGpuRenderTarget` + JS 事件桥。`wasm-pack --target web`。
- SDK 在 `crates/web-view/sdk/`，入口 `Editor.create(canvas, fontBytes)`，发布为 npm 包 `@office-rs/rofd`。
- 默认字体 NotoSans + NotoSansCJKsc，`Arc<Vec<u8>>` 共享，`warmup()` 预编译 shader。
- **WebGPU only**，无 Canvas2D 回退（Chrome/Edge 113+）。

---

## 6. 测试约定

- **目标 80% 覆盖，TDD（先红后绿）**。
- **手术刀字节保留是核心测试**（`crates/io/tests/round_trip.rs`、`save_surgical.rs`）：`parse → save_ofd → 未触碰条目逐字节相等`。
- **每命令 apply→undo→还原**（`crates/editor/tests/integration.rs`）；History cap=100 溢出丢最旧。
- Fixture：模型 derive serde → `serde_json` fixture（免费跨 crate）；真实 .ofd fixture 放仓库根 `test/`（如 `test/ru-yuan-ji-lu.ofd`），生成 fixture 放 `crates/io/tests/fixtures/`。
- 渲染用**场景结构断言**（对象数/类型，非像素）；GPU 快照易 flaky，v1 不作阻塞 CI。
- Integration：真实 .ofd → 加载 → 批注 → 保存 → 重开 → 断言批注保留 + body 字节一致。

---

## 7. 依赖钉版（动之前必读）

`xilem` + `masonry_winit` + `imaging` + `imaging_vello` 是 **git rev 钉版**，定义在根 `Cargo.toml` `[workspace.dependencies]`：

| crate | git 源 | rev |
|---|---|---|
| `xilem`, `masonry_winit` | linebender/xilem | `bf81712d44e3` |
| `imaging`, `imaging_vello` | forest-rs/imaging | `0eea0499d2666195103b9837ac4c3ee474176a5b` |

**这四个 rev 必须作为整体一起 bump**。原因：xilem@`bf81712d44e3` 内部 pin 了同一个 imaging git rev，所以 xilem + imaging 统一到单一 imaging 源；published crates.io 的 xilem/masonry 0.4.0 仍 ship vello 0.6（与 rofd 的 vello 0.8 不兼容），故 xilem **必须来自 git**。

升级 Linebender 栈时（vello/parley/xilem），预期 alpha/beta 的 breaking change——bump 后可能需要调整 render/native-view/web-view 的调用代码。完整背景见记忆 `imaging-xilem-migration` 与 `docs/superpowers/plans/2026-07-10-rofd-xilem-imaging-migration.md`。

---

## 8. 工作流

本项目用 **superpowers 规划工作流**：

- 设计 spec：`docs/superpowers/specs/`（架构与决策记录，事实来源）。
- 实现 plan：`docs/superpowers/plans/`（每 crate/阶段一份，TDD 任务细化）。
- 新功能：先写/更新 spec → 写 plan → TDD（RED→GREEN→REFACTOR）→ 实现 → 自审 → 提交。
- 提交信息遵循 conventional commits（`feat`/`fix`/`refactor`/`docs`/`test`/`chore`/`perf`/`ci`）。

---

## 9. 常见陷阱（Do / Don't）

| ✅ Do | ❌ Don't |
|---|---|
| render 用 imaging Painter API | 在 render 里直接构造 `vello::Scene` |
| 宿主 `set_clock(author, ts)` | 库内调 `Date::now()` / `SystemTime::now()` |
| editor 只改 `.annotations` | 在 editor 里改 `pages`（body 只读） |
| 依赖只向上 | 给 dom/render 加反向依赖到 editor/io |
| 硬错 `OfdError`、降级 `OfdWarning` | 裸 `unwrap` / 静默 `ignore` 错误 |
| bump 四个 git rev 一起动 | 单独 bump xilem 或 imaging 之一 |
| 功能逻辑落 component 及以下 | 在适配器层写状态机/几何/业务逻辑 |
| component 出回调、适配器默认装配（宿主零配置） | 让每个宿主重复对接平台能力 |
| examples 只当接入 demo 参照 | 把库功能写进 examples |
| body_scene 稳定缓存、只失效批注页 | 每帧重建 body_scene |
| 适配器层（native-view/web-view）持 io | 往 component 里塞 io 调用 |
| 改 io save 后跑手术刀字节保留测试 | 只靠单元测试断言模型相等 |
| public 字段 + `Default` on dom 结构 | 用 private 字段 + setter 锁死构造能力 |

---

## 10. 编码规范
- 设计文档可以保留 WPS 字样，但是代码注释、代码变量、代码文件命名不要存留 WPS 字样。

## 11. 参考项目

- **`D:/code/reditor`** — Rust + GPU 的 OOXML 编辑器库，rofd 的分层骨架直接复刻自它。遇到"rofd 这里该怎么组织"时，先看 reditor 对应 crate（reditor-dom/reditor-ooxml/reditor-renderer/reditor-editor/reditor-component/reditor-native-view/reditor-web-view）如何处理。
- 关键差异见 spec §2.3：rofd 无独立 layout crate（OFD 固定版式无回流）、dom 双模型（PageModel + AnnotationModel）、io 双写入路径（手术刀 + 全量）、无 Format trait、editor 操作对象是批注而非文本光标。
