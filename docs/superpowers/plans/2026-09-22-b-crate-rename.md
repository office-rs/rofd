# B Crate 改名（native-view→xilem-view、native-app→xilem-app）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把两个未发布 crate 改到终态名：`crates/native-view`（包 `rofd-native-view`）→ `crates/xilem-view`（包 `rofd-xilem-view`，lib `rofd_xilem_view`）；`crates/native-app`（包 `native-app`）→ `crates/xilem-app`（包 `xilem-app`，lib `xilem_app`）。行为零变化，纯机械改名。

**Architecture:** 单个任务、单个提交（spec §5.1）：目录级 `git mv`（rword commit `f977b40` 同型已验证）→ 根 Cargo.toml（members + workspace.dependencies 键）→ 两个包 manifest → 全部 use 路径与注释 → 旧名零命中审计 → 全部门禁。

**Tech Stack:** git/cargo；无新依赖。已发布的 rofd-dom/io/editor 不动。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §5.1（转换 B）、§6.4（ignored 测试命令）、§6.5（阶段门禁）。

## Global Constraints

- 前置条件：transform A（A0/A1/A2/A3）已全部提交且门禁绿色；`git status` 干净。
- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- 行为零变化：本计划不改任何业务逻辑、不改测试断言（只改路径/名称）。
- 手术刀字节保留测试（`-p rofd-io`）始终绿色。
- **文档切分决策（spec §7.1 的落地）**：AGENTS.md、README、README.zh-CN、CHANGELOG 的改名统一在 transform C 一次性编辑（同时携带 B 与 C 两套名称），B 不动这些文件；因此 B 提交后它们含旧名是**有意中间态**。历史文档（`docs/superpowers/`、`tmp/`）永不改名（spec §7.3；rword as-built 同）。B 只同步代码与 CI 注释。

## File Structure

| 文件 | 动作 | 内容 |
| --- | --- | --- |
| `crates/native-view/` | 目录 git mv → `crates/xilem-view/` | 全部文件随行 |
| `crates/native-app/` | 目录 git mv → `crates/xilem-app/` | 全部文件随行 |
| `Cargo.toml` | 改 | members 2 处 + workspace.dependencies 键 1 处 |
| `crates/xilem-view/Cargo.toml` | 改 | package name |
| `crates/xilem-app/Cargo.toml` | 改 | package name + 依赖键 |
| `crates/xilem-app/src/main.rs` | 改 | use 路径 + 1 处注释 |
| `crates/xilem-app/src/lib.rs` | 改 | 1 处注释 |
| `crates/xilem-app/tests/c2_save.rs` | 改 | use 路径 + 2 处注释 |
| `crates/xilem-view/tests/ofd_widget_harness.rs` | 改 | use 路径 |
| `crates/web-view/src/webgpu_render_target.rs` | 改 | 1 处注释（顺带删已失效的 intra-doc 链接） |
| `crates/web-view/src/wasm_editor.rs` | 改 | 2 处注释 |
| `.github/workflows/publish-crates.yml` | 改 | 1 处注释 |
| `Cargo.lock` | cargo 自动重写 | 不手改 |

---

### Task 1: 目录改名与全仓引用更新

**Interfaces:**
- Consumes: transform A 终态（A3 完成判据全部满足）。
- Produces: 包 `rofd-xilem-view`（lib `rofd_xilem_view`）、`xilem-app`（lib `xilem_app`）。

- [ ] **Step 1: 前置门禁确认**

```bash
git status --short
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
```

Expected: 工作区干净；A 的全部测试绿色。若不干净或失败，先解决，不进入改名。

- [ ] **Step 2: 目录级 git mv**

```bash
git mv crates/native-view crates/xilem-view
git mv crates/native-app crates/xilem-app
git status --short
```

Expected: 两个目录整体重命名（rword 同法成功）。若 Windows 上目录被占用（rust-analyzer/编辑器句柄）导致失败，关闭占用进程后重试；仍不行则退回文件级移动：

```bash
mkdir -p crates/xilem-view crates/xilem-app
git mv crates/native-view/Cargo.toml crates/native-view/src crates/native-view/tests crates/xilem-view/
git mv crates/native-app/Cargo.toml crates/native-app/src crates/native-app/tests crates/xilem-app/
rmdir crates/native-view crates/native-app
```

- [ ] **Step 3: 根 Cargo.toml——members**

把：

```toml
members = ["crates/dom", "crates/io", "crates/render", "crates/editor", "crates/component", "crates/native-view", "crates/web-view", "crates/native-app", "crates/tauri-app/src-tauri"]
```

替换为：

```toml
members = ["crates/dom", "crates/io", "crates/render", "crates/editor", "crates/component", "crates/xilem-view", "crates/web-view", "crates/xilem-app", "crates/tauri-app/src-tauri"]
```

- [ ] **Step 4: 根 Cargo.toml——workspace.dependencies 键**

把：

```toml
rofd-native-view = { path = "crates/native-view" }
```

替换为：

```toml
rofd-xilem-view = { path = "crates/xilem-view" }
```

- [ ] **Step 5: xilem-view Cargo.toml——package name**

把 `crates/xilem-view/Cargo.toml` 中：

```toml
name = "rofd-native-view"
```

替换为：

```toml
name = "rofd-xilem-view"
```

说明：不显式写 `[lib] name`——cargo 默认推断 `rofd_xilem_view`，正是目标名。

- [ ] **Step 6: xilem-app Cargo.toml——package name 与依赖键**

把 `crates/xilem-app/Cargo.toml` 中：

```toml
name = "native-app"
```

替换为：

```toml
name = "xilem-app"
```

把：

```toml
rofd-native-view = { workspace = true }
```

替换为：

```toml
rofd-xilem-view = { workspace = true }
```

- [ ] **Step 7: main.rs——use 路径与注释**

把 `crates/xilem-app/src/main.rs` 中：

```rust
use rofd_native_view::{command_queue, ofd_with_config, OfdCommandQueue, OfdContextMenu};
```

替换为：

```rust
use rofd_xilem_view::{command_queue, ofd_with_config, OfdCommandQueue, OfdContextMenu};
```

把：

```rust
//! The OFD editor lives in an `OfdWidget` (rofd-native-view; renamed
//! rofd-xilem-view in transform B) embedded via `ofd_with_config`. This
```

替换为：

```rust
//! The OFD editor lives in an `OfdWidget` (rofd-xilem-view) embedded via
//! `ofd_with_config`. This
```

- [ ] **Step 8: lib.rs——注释**

把 `crates/xilem-app/src/lib.rs` 中：

```rust
//! native-app library surface.
```

替换为：

```rust
//! xilem-app library surface.
```

- [ ] **Step 9: tests/c2_save.rs——use 路径与注释**

把 `crates/xilem-app/tests/c2_save.rs` 中：

```rust
use native_app::host::document_io::{load_ofd, save_ofd};
```

替换为：

```rust
use xilem_app::host::document_io::{load_ofd, save_ofd};
```

把：

```rust
//! Migrated from rofd-native-view after the winit bridge removal; the
//! binary crate's helpers are reached through the `native_app` library
//! surface. Exercises the full host path —
```

替换为：

```rust
//! After the winit bridge removal; the binary crate's helpers are reached
//! through the `xilem_app` library surface. Exercises the full host path —
```

把：

```rust
//! `cargo test -p native-app --test c2_save -- --ignored`.
```

替换为：

```rust
//! `cargo test -p xilem-app --test c2_save -- --ignored`.
```

- [ ] **Step 10: ofd_widget_harness.rs——use 路径**

把 `crates/xilem-view/tests/ofd_widget_harness.rs` 中：

```rust
use rofd_native_view::{OfdCommand, OfdWidget, OfdWidgetAction};
```

替换为：

```rust
use rofd_xilem_view::{OfdCommand, OfdWidget, OfdWidgetAction};
```

- [ ] **Step 11: webgpu_render_target.rs——注释**

把 `crates/web-view/src/webgpu_render_target.rs` 中：

```rust
//! vello's own `RenderSurface` in `vello::util` and the native-view's
//! [`VelloRenderTarget`](../../native_view/vello_render_target/struct.VelloRenderTarget.html).
```

替换为：

```rust
//! vello's own `RenderSurface` in `vello::util`; the rofd xilem adapter
//! (`OfdWidget`) paints the same backend-agnostic scene on native.
```

说明：原 intra-doc 链接在 transform A 后已失效（vello_render_target 模块已不存在），此处一并删除，不制造新坏链接。

- [ ] **Step 12: wasm_editor.rs——两处注释**

把 `crates/web-view/src/wasm_editor.rs` 中：

```rust
/// back to [`Tool::Text`] (safe default). Mirrors the native-app's toolbar
```

替换为：

```rust
/// back to [`Tool::Text`] (safe default). Mirrors the xilem-app's toolbar
```

把：

```rust
        /// Mirrors the native-app's toolbar buttons.
```

替换为：

```rust
        /// Mirrors the xilem-app's toolbar buttons.
```

- [ ] **Step 13: publish-crates.yml——注释**

把 `.github/workflows/publish-crates.yml` 中：

```yaml
# 不发布：rofd-render / rofd-component / rofd-native-view / rofd-web-view
```

替换为：

```yaml
# 不发布：rofd-render / rofd-component / rofd-xilem-view / rofd-web-view
```

- [ ] **Step 14: 构建并确认 Cargo.lock 已重写**

```bash
cargo build --workspace
rg -n "rofd-native-view|rofd_native_view|native-app|native_view" Cargo.lock; echo "audit exit: $?"
```

Expected: 构建零错误；rg 退出码 1（无命中——包 ID 已随 manifest 重写）。若仍有命中，cargo 未更新 lock；跑 `cargo update -p rofd-native-view --precise` 不可行（包已改名），则检查 manifest 是否还有旧键残留。

- [ ] **Step 15: 代码 + CI 旧名零命中审计**

Run:

```bash
! rg -n "rofd-native-view|rofd_native_view|crates/native-view|crates/native-app" \
  --glob '!docs/**' --glob '!tmp/**' --glob '!AGENTS.md' --glob '!README*.md' --glob '!CHANGELOG.md' \
  .
```

Expected: 无匹配（`!` 反转后成功）。AGENTS/README/CHANGELOG 的有意旧名由 transform C 处理；单独确认它们确实仍有旧名（sanity 对照，证明审计范围没写错）：

```bash
rg -n "native-view|native-app" AGENTS.md | head -3
```

Expected: 有命中（C 阶段清除）。

- [ ] **Step 16: 全部门禁**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

Expected: 全绿。若有 gitignored 真实样例，可另跑：

```bash
cargo test -p xilem-app --test c2_save -- --ignored
```

- [ ] **Step 17: 提交**

```bash
git add -A
git status --short
git commit -m "refactor: crate 更名 native-view→rofd-xilem-view、native-app→xilem-app"
```

---

## B 完成判据

- [ ] 包名终态：`cargo metadata --format-version 1 | rg "rofd-xilem-view|xilem-app"` 命中，无 `rofd-native-view`/`native-app` 包。
- [ ] 代码与 CI 内旧名零命中（AGENTS/README/CHANGELOG 的旧名为有意遗留，C 处理）。
- [ ] `cargo run -p xilem-app` 行为与 A3 完成时一致（改名不改性）。
- [ ] 工作区测试、clippy、fmt、wasm check 全绿；手术刀字节保留测试绿色。
