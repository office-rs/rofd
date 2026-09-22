# A0 依赖升级 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Linebender 栈从 xilem `bf81712d44e3` 升到 `271a27a6d4a930f7878d404f9014e3c50a3a9b88`，imaging/imaging_vello 从 git rev 切换到 crates.io `0.0.1`，并钉死 toolchain 1.98.1——为 A 的 masonry Widget 重写做前置准备。

**Architecture:** 纯依赖升级，不做任何架构变更：winit 桥（`WinitEventBridge` + `EditorApp`）原样保留。新 xilem rev 仍含 `masonry_winit` crate 且仍基于 winit 0.30，所以旧宿主只需要跟随两类 breaking changes：样式属性（`border_width`/`corner_radius`/`Padding::from_vh`）的参数从裸数值改为 `Length`，`MasonryState` 的生命周期参数移除。imaging 源切换对 rofd 的调用点实测零影响。本计划的全部修改已在一次 throwaway spike 中实测通过（build/test/clippy/fmt/wasm check 全绿），执行者按单照做即可。

**Tech Stack:** Rust 1.98.1（rust-toolchain.toml 钉版）、xilem/masonry git rev `271a27a6…`、imaging/imaging_vello crates.io 0.0.1、vello 0.8、parley 0.8、winit 0.30、wgpu 28。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §1（A0 表）、§6.5（阶段门禁）。as-built 参照 rword commit `5d05e32`。

## Global Constraints

- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- A0 **不删除旧架构、不做架构变更**：`EditorApp`/`WinitEventBridge`/`NativeApp` 全部保留，只做最小编译修正。
- 库内禁止 `Date::now()`/`SystemTime::now()`；本计划不引入任何时间调用。
- 手术刀字节保留测试（`-p rofd-io`）必须保持绿色。
- vello 0.8 / parley 0.8 / winit 0.30 / wgpu 28 / rfd 0.15 / kurbo 0.13.1 / peniko 0.6.1 维持不动。
- 代码注释/变量/文件名不得出现 "WPS" 字样。

---

### Task 1: 升级 Linebender 依赖钉版并修复调用点

**Files:**
- Modify: `Cargo.toml:33-36`（`[workspace.dependencies]` 四个钉版行）
- Create: `rust-toolchain.toml`
- Modify: `crates/native-app/src/main.rs:48`（BTN_PAD）
- Modify: `crates/native-app/src/main.rs:75-76`（tool_button 样式）
- Modify: `crates/native-app/src/main.rs:91-92`（markup_button 样式）
- Modify: `crates/native-app/src/main.rs:167-168`（btn_open 样式）
- Modify: `crates/native-app/src/main.rs:175-176`（btn_save 样式）
- Modify: `crates/native-app/src/main.rs:212`（menu_bar padding）
- Modify: `crates/native-app/src/main.rs:268`（MasonryState 生命周期）
- Generated: `Cargo.lock`（cargo 自动更新，不手改）

**Interfaces:**
- Consumes: 无（A0 是序列第一个计划）。
- Produces: `xilem::masonry::layout::Length::const_px(f64) -> Length`（新 rev 的样式长度类型，A 全程使用）；`masonry_winit::app::MasonryState`（无生命周期参数，A 重写宿主前的过渡形态）。

- [ ] **Step 1: 修改根 `Cargo.toml` 的四个钉版行**

把 `Cargo.toml` 中现有的：

```toml
xilem = { git = "https://github.com/linebender/xilem", rev = "bf81712d44e3" }
masonry_winit = { git = "https://github.com/linebender/xilem", rev = "bf81712d44e3" }
imaging = { git = "https://github.com/forest-rs/imaging.git", rev = "0eea0499d2666195103b9837ac4c3ee474176a5b" }
imaging_vello = { git = "https://github.com/forest-rs/imaging.git", rev = "0eea0499d2666195103b9837ac4c3ee474176a5b" }
```

替换为：

```toml
xilem = { git = "https://github.com/linebender/xilem", rev = "271a27a6d4a930f7878d404f9014e3c50a3a9b88" }
masonry_winit = { git = "https://github.com/linebender/xilem", rev = "271a27a6d4a930f7878d404f9014e3c50a3a9b88" }
masonry_testing = { git = "https://github.com/linebender/xilem", rev = "271a27a6d4a930f7878d404f9014e3c50a3a9b88" }
imaging = "0.0.1"
imaging_vello = "0.0.1"
```

说明：`masonry_winit` 在 A0 保留（旧宿主 main.rs 仍 import `MasonryState`/`AppDriver`），A 重写宿主时删除；`masonry_testing` 的 workspace 键此步先建好，A 的适配器测试才引用它。

- [ ] **Step 2: 创建仓库根 `rust-toolchain.toml`**

文件全文：

```toml
# Linebender git deps (xilem/masonry @ 271a27a6) declare rust-version 1.96.
[toolchain]
channel = "1.98.1"
```

保存后运行 `rustc --version`，Expected: `rustc 1.98.1`（rustup 自动下载并切换）。

- [ ] **Step 3: 构建并确认失败点与预期一致**

Run: `cargo build --workspace 2>&1 | grep -E "^error|-->"`

Expected: 恰好 11 个错误，全部在 `crates/native-app/src/main.rs`：L48、L75、L76、L91、L92、L167、L168、L175、L176、L212（`expected Length` / Padding 参数类型）、L268（`struct takes 0 lifetime arguments but 1 lifetime argument was supplied`）。其余所有 crate（dom/io/render/editor/component/native-view/web-view 的依赖）编译通过。若出现其它位置的错误，停止并核对 Step 1–2 是否照做。

- [ ] **Step 4: 修复 `BTN_PAD`（L48）**

把：

```rust
const BTN_PAD: Padding = Padding::from_vh(0.0, 6.0);
```

替换为：

```rust
const BTN_PAD: Padding = Padding::from_vh(
    xilem::masonry::layout::Length::const_px(0.0),
    xilem::masonry::layout::Length::const_px(6.0),
);
```

- [ ] **Step 5: 修复 `tool_button` 的两个样式属性（L75-76）**

把 `tool_button` 函数末尾的：

```rust
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0)
}
```

替换为：

```rust
    .padding(BTN_PAD)
    .border_width(xilem::masonry::layout::Length::const_px(0.0))
    .corner_radius(xilem::masonry::layout::Length::const_px(2.0))
}
```

- [ ] **Step 6: 修复 `markup_button` 的两个样式属性（L91-92）**

把 `markup_button` 函数末尾的：

```rust
    .disabled(disabled)
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0)
}
```

替换为：

```rust
    .disabled(disabled)
    .padding(BTN_PAD)
    .border_width(xilem::masonry::layout::Length::const_px(0.0))
    .corner_radius(xilem::masonry::layout::Length::const_px(2.0))
}
```

- [ ] **Step 7: 修复 `btn_open` 的两个样式属性（L167-168）**

把 `btn_open` 构造链末尾的：

```rust
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0);

    let btn_save = text_button("Save", |app: &mut AppState| {
```

替换为：

```rust
    .padding(BTN_PAD)
    .border_width(xilem::masonry::layout::Length::const_px(0.0))
    .corner_radius(xilem::masonry::layout::Length::const_px(2.0));

    let btn_save = text_button("Save", |app: &mut AppState| {
```

- [ ] **Step 8: 修复 `btn_save` 的两个样式属性（L175-176）**

把 `btn_save` 构造链末尾的：

```rust
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0);

    let file_row =
```

替换为：

```rust
    .padding(BTN_PAD)
    .border_width(xilem::masonry::layout::Length::const_px(0.0))
    .corner_radius(xilem::masonry::layout::Length::const_px(2.0));

    let file_row =
```

- [ ] **Step 9: 修复 `menu_bar` 的 padding（L212）**

把：

```rust
        .padding(Padding::from_vh(2.0, 4.0))
```

替换为：

```rust
        .padding(Padding::from_vh(
            xilem::masonry::layout::Length::const_px(2.0),
            xilem::masonry::layout::Length::const_px(4.0),
        ))
```

- [ ] **Step 10: 修复 `MasonryState` 生命周期参数（L268）**

把：

```rust
    masonry_state: MasonryState<'static>,
```

替换为：

```rust
    masonry_state: MasonryState,
```

（新 rev 的 `MasonryState` 已不再借用 event loop。其构造点 `MasonryState::new(...)` 签名不变，无需修改。）

- [ ] **Step 11: 全量构建通过**

Run: `cargo build --workspace`

Expected: `Finished`，零错误零警告。

- [ ] **Step 12: 全量测试通过（含手术刀字节保留测试）**

Run: `cargo test --workspace --exclude tauri-app --exclude rofd-web-view`

Expected: 所有 `test result: ok`，无 FAILED。关键计数：rofd-component 140 passed、rofd-io 81 passed、native-view 24 passed、io 的 round_trip 4 passed / save_surgical 5 passed。

- [ ] **Step 13: clippy / fmt 门禁**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clippy 零输出通过；fmt 零 diff 通过（若 fmt 报告 diff，直接运行 `cargo fmt --all` 采纳，然后重跑 check）。

- [ ] **Step 14: wasm 目标编译门禁**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`

Expected: `Finished`，imaging/imaging_vello 以 crates.io 0.0.1 编译，零错误。

- [ ] **Step 15: 提交**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml crates/native-app/src/main.rs
git commit -m "chore(deps): xilem 升级至 271a27a6，imaging 转正 crates.io 0.0.1，钉 toolchain 1.98.1"
```
