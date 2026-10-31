# rofd 打印二期实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地打印二期：component `print_direct`（cfg 门控）+ print_gen 换文档失效 + rofd-xilem-view 默认打印对话框三件套 + xilem-app 接线（单轨提交/终态轮询/join_all）+ print_to_file 手测通道。

**Architecture:** 直打动词下沉 component（rword 2026-10-30 同款但修三处：cfg 门控、编辑器当前页、title 入参）；对话框 = 纯状态机 + NativePrintServices + Backstage 覆盖层 view（移植 rword 三件套，trait 收窄 discover-only，加内容三开关）；宿主提交单轨 pending → 命令队列 → component verb，JobHandle 经槽回写。

**Tech Stack:** Rust（office-rs-print rev c2f99d5 native 后端、xilem/masonry 钉版同库、既有 PrintSession/PrintContent）。

**Spec:** [`docs/superpowers/specs/2026-10-31-print-phase2-design.md`](../specs/2026-10-31-print-phase2-design.md)（本计划从 spec 立论，执行者两份都读）

## Global Constraints

- `print_direct` 必须 `#[cfg(not(target_arch = "wasm32"))]`——同 pin rev 下 `office_rs_print::print_pages` 在 wasm32 不导出（spec §0.3 修正 1；rofd CI wasm 冒烟每 push 必炸）。
- 签名（spec §1）：`print_direct(&mut self, printer: Option<&str>, settings: &PrintSettings, content: PrintContent, title: &str) -> office_rs_print::JobHandle`；plan 失败 `unwrap_or_default()`（doc 注明 caller-must-validate 契约）。
- 「当前页」= 编辑器当前页：`self.current_page.map(|i| i + 1).unwrap_or(1)`（0-based Option → 1-based）；预览翻页不改变该语义（spec §0.3 修正 2）。
- 内容三开关默认 ✗/✗/✔（`PrintContent::default()`）；对话框开关变更置 `content_dirty`，提交永远用最新开关（spec §0.2 决策 4）。
- 提交单轨：对话框只产出 `pending: Option<(Option<String>, PrintSettings, PrintContent)>`，spool 只发生在命令队列的 `component.print_direct`（spec §0.2 决策 3）。
- `PrintServices` trait 收窄为 `{ fn discover(&self) -> mpsc::Receiver<DiscoverState> }`；状态机去 serde 派生（spec §0.3 修正 5/6）。
- 对话框 = 顶层覆盖层，编辑器树常驻（spec §0.2 决策 5）；`main` 末尾 `office_rs_print::join_all()`（spec §0.3 修正 7②）。
- commit message 与代码注释一律英文；conventional commits；UI 文案中文。
- 每任务后跑：`cargo build --workspace --exclude tauri-app && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`。

## Review Focus

1. **wasm32 编译炸（rword 同款地雷）**：print_direct 无门控 → web-view wasm 构建失败 —— Task 2 Step 4 以 `cargo build -p rofd-component --target wasm32-unknown-unknown` 为回归门。
2. **空文档打印**：0 页文档 + 打印机在场 → 提交产空任务出白纸/怪任务 —— Task 5 Step 1 预检测试：`material.page_count() == 0` → 预检失败不置 pending。
3. **current_page 为 None（未发生任何翻页事件）**：直接调用 print_direct → 默认第 1 页而非 panic/0 —— Task 2 Step 1 测试钉住。
4. **内容开关变更后提交**：dirty 未消费 → 提交用旧物化/旧开关 —— Task 5 Step 1：dirty 时 pending 携带新 content 三元组且消费 dirty；Task 4 钉 PrintUi 开关联动。
5. **换文档后旧 session 报 fresh**（一期 TODO）—— Task 1 Step 1：`load_document` 后 `is_stale()` 为 true。

---

### Task 1: print_gen 换文档失效（component）

**Files:**
- Modify: `crates/component/src/ofd_component.rs`（`load_document`/`new_document` 中 TODO(phase-2) 注释处，~:455/:482）、`crates/component/src/print_session.rs`（追加测试）

**Interfaces:**
- Consumes: 一期 `print_gen: Arc<AtomicU64>` 字段 + `notify_document_change` 的既有 bump；`PrintSession::is_stale`
- Produces: 行为变更（换文档使既有 session 失效）；无新 API

- [ ] **Step 1: 写失败测试（print_session.rs mod tests）**

```rust
#[test]
fn doc_swap_flags_session_stale() {
    // component_with_page() 式构造：begin_print(default) → !is_stale()
    // load_document(另一份两页文档) → is_stale() == true
    // new_document 同理（第二个测试或同测内二段）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component print_session`
Expected: FAIL（is_stale 仍 false）

- [ ] **Step 3: 实现**

`load_document`/`new_document` 的缓存清空块（`font_gen = 0` 旁）各加 `self.print_gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);`，把 TODO(phase-2) 注释改为实现说明（英文）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-component`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "fix(component): bump print_gen on document swap (phase-1 TODO cleared)"
```

---

### Task 2: component print_direct（cfg 门控一步直打）

**Files:**
- Modify: `crates/component/src/print_session.rs`（或 ofd_component.rs——放 `OfdComponent` impl 所在文件，与 begin_print 同文件为佳）、`crates/component/src/lib.rs`（如需 re-export）

**Interfaces:**
- Consumes: 一期 `begin_print(&self, PrintContent) -> PrintSession`、`session.plan/payload_for`、`self.current_page: Option<usize>`（0-based）
- Produces（Task 5 消费）:
  ```rust
  #[cfg(not(target_arch = "wasm32"))]
  impl OfdComponent {
      pub fn print_direct(&mut self, printer: Option<&str>, settings: &office_rs_print::PrintSettings,
                          content: PrintContent, title: &str) -> office_rs_print::JobHandle;
      /// Editor current page, 1-based（0-based Option 的唯一换算点；print_direct 与宿主预检共用）。
      pub(crate) fn current_page_1based(&self) -> usize;  // self.current_page.map(|i| i + 1).unwrap_or(1)
  }
  ```

- [ ] **Step 1: 写测试（同文件 mod tests；两测）**

```rust
#[test] #[ignore = "requires a printer system; run with -- --ignored on a desktop"]
fn print_direct_returns_job_handle() {
    // component_with_page 风格构造 + load；print_direct(None, &default, PrintContent::default(), "t")
    // snapshot 非立即 Failed（rword word_component.rs:15984 同款）
}
#[test]
fn current_page_converts_none_and_zero_based() {
    // 直接测 current_page_1based 纯换算：无翻页事件组件（current_page == None）→ 1；
    // 手工置 current_page = Some(0) → 1；Some(4) → 5
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component print_direct current_page`
Expected: FAIL（未定义）

- [ ] **Step 3: 实现（rword word_component.rs:2478-2490 适配移植）**

printer Some → clone settings 后覆写 `printer` 字段；`current_page_1based()` 纯换算；`session.plan(&settings, self.current_page_1based()).unwrap_or_default()` → `payload_for` → `office_rs_print::print_pages(payload, &settings, title)`。doc 注明 caller-must-validate 契约（非法 spec → 空任务）。整体 `#[cfg(not(target_arch = "wasm32"))]`。

- [ ] **Step 4: 双门验证**

Run: `cargo test -p rofd-component && cargo build -p rofd-component --target wasm32-unknown-unknown`
Expected: 测试 PASS（含新两测）；**wasm32 构建成功**（门控正确——这是对 rword 地雷的回归门）

- [ ] **Step 5: Commit**

```bash
git add crates/component
git commit -m "feat(component): one-call print_direct (native only) with editor current page"
```

---

### Task 3: xilem-view 状态机移植（print_dialog_state.rs）

**Files:**
- Create: `crates/xilem-view/src/print_dialog_state.rs`
- Modify: `crates/xilem-view/src/lib.rs`（`pub mod print_dialog_state;`）、`crates/xilem-view/Cargo.toml`（`office-rs-print = { workspace = true }`）

**Interfaces:**
- Consumes: office-rs-print `validate_settings/PrintSettings/PrintPayload` 类型层
- Produces（Task 4/5 消费，逐项 rword print_dialog_state.rs 对应）:
  ```rust
  pub enum DiscoverState { Finding, Printers(Vec<String>), NoPrinters, Failed(String) }
  pub enum DialogPhase { Idle, Finding, Ready, Submitting, Submitted, Failed }
  pub struct DialogState { pub phase, pub printers: Vec<String>, pub errors: Vec<SettingsError>, pub last_error: Option<String> }
  impl DialogState { pub fn initial() -> Self; pub fn can_submit(&self) -> bool; }
  pub enum DialogEvent { DiscoveryStarted, DiscoveryUpdate(DiscoverState), SettingsChanged(PrintSettings), SubmitPressed, SubmitAccepted, SubmitFailed(String), Reset }
  pub fn advance(state: &DialogState, settings: &PrintSettings, event: DialogEvent) -> DialogState;  // 纯函数
  pub struct Labels { /* 16 字段同 rword */ }  // zh_cn()/en()
  pub trait PrintServices { fn discover(&self) -> std::sync::mpsc::Receiver<DiscoverState>; }  // 收窄：仅 discover
  ```
  全部**去 serde 派生**（rword 的 serde 服务 wasm bridge，rofd 无此用例）。

- [ ] **Step 1: 写失败测试（rword 七测移植改造，同文件 mod tests）**

移植 `discovery_flow_reaches_ready` / `no_printers_blocks_submit` / `discovery_failure_lands_in_failed` / `settings_change_updates_validation`（非法 spec "1-3,x" → `SettingsError::InvalidRangeSpec`，且 SubmitPressed → Failed）/ `happy_submit_flow` / `submit_failure_keeps_dialog_usable` / `reset_returns_to_idle` / `labels_cover_both_languages`（去 serde 往返断言，保双语言字段断言）。trait-object 测试改写：FakeServices 只实现 `discover`（mpsc 预灌 Finding + Printers），断言 recv 序列。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-xilem-view print_dialog_state`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现**

逐段移植 rword `print_dialog_state.rs`（advance 全逻辑、Labels zh_cn/en 两表、can_submit），按 Interfaces 收窄 trait、去 serde。`PrintPayload`/`JobHandle` import 随 trait 收窄一并删除。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rofd-xilem-view`
Expected: PASS（含既有 harness 测试无回归）

- [ ] **Step 5: Commit**

```bash
git add crates/xilem-view
git commit -m "feat(xilem-view): port print dialog state machine (discover-only services trait)"
```

---

### Task 4: xilem-view 对话框面（print_services.rs + print_dialog.rs）

**Files:**
- Create: `crates/xilem-view/src/print_services.rs`、`crates/xilem-view/src/print_dialog.rs`
- Modify: `crates/xilem-view/src/lib.rs`（两 `pub mod`）

**Interfaces:**
- Consumes: Task 3 全部公共面；一期 `rofd_component::PrintContent`；xilem/masonry 视图 API（rword print_dialog.rs 用的 `canvas/checkbox/flex_col/flex_row/label/sized_box/text_button/text_input/zstack` 在同一 xilem 钉版 rev 均可用）
- Produces（Task 5 消费）:
  ```rust
  // print_services.rs
  pub struct NativePrintServices;
  impl PrintServices for NativePrintServices { /* worker 线程 discover → mpsc；空列表 → NoPrinters；rword 33 行形状 */ }

  // print_dialog.rs
  #[derive(Default)] pub struct PrintMaterial { pub scenes: Vec<imaging::record::Scene>, pub dims: Vec<(f32, f32)>, pub papers: Vec<(Paper, Orientation)> }
  impl PrintMaterial { pub fn page_count(&self) -> usize; }
  pub struct PrintUi {
      pub settings: PrintSettings, pub state: DialogState, pub labels: Labels, pub printers: Vec<String>,
      pub page: usize, pub material: Arc<Mutex<PrintMaterial>>,
      pub job: Option<office_rs_print::JobHandle>, pub status: String,
      pub content: PrintContent, pub content_dirty: bool,        // ← 新增（rword 无）
      pub on_cancel: Option<PrintAction>, pub on_submit: Option<PrintAction>,
      pub exit: bool,
      pub pending: Option<(Option<String>, PrintSettings, PrintContent)>,  // ← 三元组（rword 二元）
  }
  pub type PrintAction = Arc<dyn Fn(&mut PrintUi) + Send + Sync>;
  impl PrintUi { pub fn new(material: Arc<Mutex<PrintMaterial>>, lang_zh: bool) -> Self;
                 pub fn page_count(&self) -> usize;
                 pub fn sync_document_defaults(&mut self) { /* papers.first() → settings.paper/orientation */ } }
  pub trait HasPrintUi { fn print_ui(&mut self) -> Option<&mut PrintUi>; }
  pub fn print_dialog_view<State: HasPrintUi + 'static>(ui: &mut PrintUi) -> impl WidgetView<State> + use<State>;
  ```

- [ ] **Step 1: 写失败测试（print_dialog.rs mod tests，纯 Rust 无 widget）**

```rust
#[test] fn content_toggle_marks_dirty() { /* 改任一 content 字段经 toggle 辅助 → content_dirty == true */ }
#[test] fn submit_snapshot_carries_content_triple() { /* pending = (printer, settings, content) 三元组齐 */ }
#[test] fn sync_document_defaults_reads_first_paper() { /* papers[0]=(A4, Portrait) → settings 跟随 */ }
#[test] fn initial_page_zero_clamped_against_empty_material() { /* 空文档：page_count()==0，view 组装用的 page.min(total-1) 逻辑提为可测纯辅助 page_clamped(&ui) -> usize == 0 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-xilem-view print_dialog`
Expected: FAIL

- [ ] **Step 3: 实现（两文件移植）**

`print_services.rs`：rword 同名文件原样（discover worker 线程 + mpsc）。
`print_dialog.rs`：rword 移植 + 三处适配——① `PrintUi` 增 `content/content_dirty`，`pending` 三元组；② 设置列新增一行三 checkbox（水印/批注/图章，绑 `ui.content.*`，勾选回调置 `content_dirty = true`）；③ `page_clamped` 纯辅助供 view 与测试共用。`print_dialog_view`/`draw_preview`（灰桌白页 fit-contain + append_transformed）逻辑照抄。import 路径按 rofd（`xilem::masonry::imaging` 同款存在于同 rev——rword 用法可直接对齐）。

- [ ] **Step 4: 跑测试 + 构建门**

Run: `cargo test -p rofd-xilem-view && cargo build --workspace --exclude tauri-app`
Expected: PASS（view 编译过 = widget API 用法正确性的第一道门；交互手感归 Task 6 手测）

- [ ] **Step 5: Commit**

```bash
git add crates/xilem-view
git commit -m "feat(xilem-view): native print services and Backstage dialog view with content flags"
```

---

### Task 5: xilem-app 接线（do_print / 单轨提交 / 终态轮询 / join_all）

**Files:**
- Modify: `crates/xilem-app/src/main.rs`（AppState、app_logic、工具栏、main 末尾）、`crates/xilem-app/Cargo.toml`（无新增——office-rs-print 经 rofd-xilem-view 传递，若需点名 `join_all` 则加 `office-rs-print = { workspace = true }`）

**Interfaces:**
- Consumes: Task 2 `print_direct`、Task 3/4 全部、既有 `push`/命令队列/`on_page_change`（xilem-view `ofd_view.rs:140`）、`HostOutcome` 槽模式（一期 Task 9）
- Produces（宿主内部；可测辅助提为自由函数）:
  ```rust
  // 可测纯辅助（main.rs cfg(test) 或 host 模块）：
  fn poll_job_terminal(ui: &mut PrintUi) -> bool;   // 活 job snapshot 终态 → 刷 status（Done→已发送/Failed|Aborted→失败），返回是否变化
  fn materialize_print_material(c: &rofd_component::OfdComponent, content: PrintContent) -> PrintMaterial; // do_print/重物化共用闭包体
  fn submit_precheck(ui: &PrintUi, editor_page: usize) -> Result<(), String>;
  //   material.page_count()==0 → Err("no printable pages")；resolve_page_list(range, parity, total, editor_page)
  //   失败 → Err(原因)；通过 → Ok（副作用在调用方：置 pending）
  // AppState 增：print: Option<PrintUi>、print_material: Arc<Mutex<PrintMaterial>>、
  //             current_page: usize（1-based 镜像，on_page_change 维护）、
  //             print_job_slot: Arc<Mutex<Option<office_rs_print::JobHandle>>>（命令闭包回写）
  ```

- [ ] **Step 1: 写失败测试（main.rs `#[cfg(test)]` mod tests，纯函数面）**

```rust
#[test] fn poll_job_done_sets_status() { /* ui.job = submit_noop 句柄（office_rs_print::job::submit_noop 存在，rword 测试用过）→ poll → status 变已发送 */ }
#[test] fn poll_job_none_is_noop() { /* 无 job → false */ }
#[test] fn materialize_fills_dims_papers_scenes() { /* component_with_page 式构造 → materialize → 三 Vec 等长非空、papers[0] 纸型正确 */ }
#[test] fn empty_document_precheck_blocks_pending() { /* materialize 空 → 预检 fn submit_precheck(ui, current_page) -> Result<(…), String> 的 Err("no printable pages") —— 预检也提为可测辅助 */ }
#[test] fn precheck_uses_editor_page_not_preview_page() { /* ui.page 翻到 3、镜像 current_page=1、range=Current → 预检 resolve 出 [1] */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p xilem-app`
Expected: FAIL

- [ ] **Step 3: 实现**

`do_print(app)`：guard（无文档/0 页不打开）→ push `materialize_print_material` 命令 → `PrintUi::new(print_material.clone(), true)`（`ui.page` 初始 = 镜像当前页-1，钳 0）→ `sync_document_defaults` → discover 阻塞 drain（`while let Ok(update) = rx.recv_timeout(500ms)`，终态 break）→ 挂 on_cancel（`job.cancel()` + 清 status + exit）/ on_submit（`submit_precheck` 通过 → `pending = Some((printer.clone(), settings.clone(), content))`，不 spool）。
`app_logic`（排空段次序固定）：① `poll_job_terminal(ui)`；② `ui.exit` → 置 None；③ `content_dirty` → push 重物化命令 + 清 dirty；④ `ui.pending.take()` → push 命令闭包（捕获 `print_job_slot` 克隆）：`let h = c.print_direct(printer.as_deref(), &settings, content, "rofd"); *slot.lock() = Some(h);`；⑤ `print_job_slot` 排空 → `ui.job = handle`。每非空命令排空已由 `HostCommandWake` 保一拍（一期契约 1），⑤ 靠同一拍或下一拍生效。
覆盖层：现有顶层结构外套 `zstack((原内容, print_overlay))`；`print_overlay = sized_box(print_dialog_view::<AppState>(ui)).background_color(...)`，`app.print` 为 None 时无覆盖层。工具栏「打印」按钮（`do_print`，置于「另存为 PDF」旁）。`main`：`run()` 后 `office_rs_print::join_all()`（rword main.rs:364 注释语义：进程死亡会杀 spooler 助手与任务）。`on_page_change` 回调维护 `current_page` 镜像（1-based）。

- [ ] **Step 4: 跑测试 + 构建**

Run: `cargo test -p xilem-app && cargo build -p xilem-app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/xilem-app
git commit -m "feat(xilem-app): print Backstage wiring (single-track submit, terminal polling, join_all)"
```

---

### Task 6: print_to_file 手测通道 + 文档

**Files:**
- Create: `crates/xilem-app/examples/print_to_file.rs`
- Modify: `docs/superpowers/specs/2026-10-31-print-phase2-design.md`（状态 → 已实施）、`AGENTS.md`（§5 rofd-component 行补 `print_direct`；§2 常用命令补 example 一行）

**Interfaces:**
- Consumes: 一期 `host::document_io::load_ofd`、Task 2 `print_direct`（或直接 begin_print→payload_for→`print_pages_to_file`——按 D:/code/print README 的 to-file 语义选：载荷复用 session `payload_for`，入口 `office_rs_print::print_pages_to_file(payload, &settings, title, out_path)`，以实际签名为准）

- [ ] **Step 1: 实现 example**

`cargo run -p xilem-app --example print_to_file -- <file.ofd> <out_file>`：参数解析（缺失 → eprintln 用法 + exit 1）→ `host::document_io::load_ofd` → 构造 OfdComponent（宿主同款 `OfdConfig::new(Arc::new(vec![]))`）→ load → `begin_print(PrintContent::default())` → 全页 `payload_for` → `print_pages_to_file` → 打印结果摘要（页数/输出路径/JobState）。载荷路径（Pages 矢量）优先；若 to-file 语义要求 Pdf 载荷则改走 `rofd_export::pdf_from_page_scenes` + `print_pdf_to_file`（以 print 仓 README/签名实测为准，报告里注明所选通道）。

- [ ] **Step 2: 真实样例手测**

Run: `cargo run -p xilem-app --example print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out`
Expected: 进程退出、输出文件产生、摘要页数 == 文档页数（无打印机环境按 JobState 如实报告，不视为失败——通道本身可跑通即验收）

- [ ] **Step 3: 文档更新**

spec 状态行 → 已实施；AGENTS §5 component 行补 print_direct 短语 + §2 常用命令区补 example 行。

- [ ] **Step 4: 全量验证**

Run: `cargo build --workspace --exclude tauri-app && cargo test --workspace --exclude tauri-app --exclude rofd-web-view && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add crates/xilem-app/examples docs AGENTS.md
git commit -m "feat(xilem-app): print-to-file example as manual test channel; docs"
```
