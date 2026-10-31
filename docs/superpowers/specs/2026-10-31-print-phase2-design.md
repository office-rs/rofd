# rofd 打印二期设计（xilem 原生直打 + 默认打印对话框）

日期：2026-10-31
状态：已实施
前置：[`2026-10-31-print-design.md`](2026-10-31-print-design.md)（一期，已实施；本文件是其 §5.3/§9 承诺的二期独立 spec）
参照：rword xilem 打印面 as-built（`rword/crates/xilem-view/src/{print.rs, print_dialog.rs, print_dialog_state.rs, print_services.rs}` + `rword/crates/xilem-app/src/main.rs` 打印段）；office-rs-print rev `c2f99d5`（钉版同一期）

## 0. 总览

### 0.1 目标与验收

| 能力 | 验收 |
|---|---|
| component `print_direct` 一步直打 | 宿主一个调用：begin_print → plan → payload_for → `print_pages`（GDI 矢量，同进程引擎路径）|
| rofd-xilem-view 默认打印对话框组件 | 可选组件：纯 advance 状态机 + PrintServices 注入 + Backstage 覆盖层（含预览翻页 + 内容三开关）|
| xilem-app 接线 | 工具栏「打印」→ 覆盖层对话框 → 确认直打；取消/失败可重试 |
| 打印到文件手测通道 | example 一条命令出文件目检 |
| print_gen 换文档失效 | `load_document`/`new_document` bump（清一期 TODO）|

### 0.2 已敲定决策

1. **继承一期**：内容三开关（水印✗/批注✗/图章✔默认）、纸型权威=文档、范围语法只在 office-rs-print 解析、份数/collate 恒为任务参数（载荷不翻倍）、跨进程纪律（本期全程同进程，无 IPC）。
2. **`print_direct` 下沉 component**（rword 2026-10-30 同款）：适配器/宿主不再各自拼 ~10 行胶水。签名 `print_direct(&mut self, printer: Option<&str>, settings: &PrintSettings, content: PrintContent, title: &str) -> JobHandle`。
3. **提交单轨**：对话框确认只产出 `pending` 快照 → 宿主 app_logic 排空 → 命令队列 → `component.print_direct`（组件命令期可达、内容开关在组件侧生效、payload 永远贴近提交时刻的文档）。rword 的 material-payload 直提路径**不移植**（其双轨并存是历史层积；rofd 单轨更干净）。`PrintMaterial` 只服务预览。
4. **原生对话框含内容三开关**（与 web/tauri 面一致）：开关变更 → 置 dirty → 经命令队列重物化 material（全页一次；v1 接受成本，惰性单页留优化点）。
5. 对话框 = Backstage **顶层覆盖层**（`zstack` Option 槽位，编辑器树常驻——rword 踩坑 5：整棵拆 widget 触发 masonry get_mut panic）。
6. 状态机纯函数移植（advance/Labels/PrintServices trait），去 serde 派生（rword 的 serde 服务 wasm bridge，rofd 的 wasm 打印走 exportPdf，无此用例）。

### 0.3 相对 rword 的修正（有据，非偏离）

| # | rword as-built | rofd 二期 | 依据 |
|---|---|---|---|
| 1 | `component.print_direct` **无 cfg 门控**，直接调 `office_rs_print::print_pages` | `#[cfg(not(target_arch = "wasm32"))]` 门控 | 同 pin rev 下 backend 在 wasm32 不导出 `print_pages`（print 仓 lib.rs:36 cfg）——rword 现状在 wasm 目标必编译失败（2026-10-30 刚下沉，其 wasm 冒烟未跑）。rofd CI wasm 冒烟每 push 必炸，必须门控 |
| 2 | `plan(&settings, 1)` 硬编码当前页 1；对话框预检用预览翻页页（`ui.page + 1`） | **「当前页」= 编辑器当前页**：`print_direct` 用 `self.current_page.map(\|i\| i+1).unwrap_or(1)`（0-based Option → 1-based）；对话框预检用宿主经 `on_page_change` 镜像的编辑器页；预览翻页只是预览，**不改变「当前页」语义**；对话框打开时预览初始页 = 编辑器当前页 | Word 系打印对话框惯例（Current page = 文档所在页，非预览所在页）；rword 的 pager 语义在用户翻预览后会打错页 |
| 3 | title 硬编码 `"Rword"` | `title: &str` 入参 | 打印队列名属宿主知识 |
| 4 | xilem-view `print.rs` 薄 shim | 不做（rofd 无既有调用者；宿主闭包直调 component verb） | YAGNI |
| 5 | `PrintServices { discover, submit }` 双方法 | trait 收窄 `{ discover }`（提交走 component verb，services.submit 无消费者） | 单轨决策的自然结论 |
| 6 | 对话框无内容开关（rword 无此语义） | 三 checkbox（默认✗✗✔） | 一期 owner 裁决的接口入参原则 |
| 7 | 提交后 JobHandle 丢弃，异步失败静默；进程退出不 join | ① `app_logic` 内对话框持有活 job 时每拍 `snapshot()` → 终态刷新 status/Failed（限制：完全无交互时延迟到下次交互/关闭时可见）；② `run()` 后 `office_rs_print::join_all()`（rword main.rs:364 同款；防止关窗杀 spooler 丢任务） | 异步失败完全静默不可接受；join_all 一行换任务不丢 |

### 0.4 一期挂账清偿

- **print_gen 换文档 bump**（一期终审 deferred + 代码 TODO(phase-2)）：本期实现 + 测试。对话框覆盖层挡住编辑器，dialog 期间批注编辑不可达，但语义正确性要求补上（任何未来跨帧持 session 的宿主依赖它）。

## 1. component：print_direct + print_gen 修复

```rust
impl OfdComponent {
    /// One-call direct print: begin_print(content) → plan(settings, current_page)
    /// → payload_for → print_pages. Native only (wasm hosts use exportPdf).
    /// Returns the async JobHandle — the component is NOT blocked.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn print_direct(
        &mut self,
        printer: Option<&str>,
        settings: &office_rs_print::PrintSettings,
        content: PrintContent,
        title: &str,
    ) -> office_rs_print::JobHandle;
}
```

- printer 覆写逻辑同 rword（Some → `settings.printer = Some(...)`，clone 后改，不动入参）。
- plan 失败（非法 spec）→ rword 是 `unwrap_or_default()`（空计划 → 空 payload → job 立即 Done，**静默**）。rofd 修正：对话框在 submit 前已 validate + resolve_page_list（见 §3），到达此处的 spec 必合法；防御性仍改为非法时 `resolve_page_list` 错误进 JobHandle 不可行（签名已定）——采用 rword 同款 `unwrap_or_default()` 并 doc 注明「caller must validate; invalid spec yields an empty job」。错误显式分层责任在对话框（提交前校验），组件 verb 契约如此记录。
- `print_gen`：`load_document`/`new_document` 各加 `self.print_gen.fetch_add(1, Ordering::Relaxed);`（与 `font_gen = 0` 同处，TODO 注释转为实现）。

## 2. rofd-xilem-view：三件套移植

新文件三个（`crates/xilem-view/src/`）：

- **`print_dialog_state.rs`**：rword 同名文件移植——`DiscoverState`/`DialogPhase`/`DialogState{initial, can_submit}`/`DialogEvent`/纯 `advance(state, settings, event)`/`Labels{zh_cn, en}`/`trait PrintServices`（收窄为 `{ fn discover(&self) -> mpsc::Receiver<DiscoverState> }`）。去 serde 派生。测试照抄（含 no-printers 阻断、非法 spec 阻断、失败可重试、reset）。
- **`print_services.rs`**：`NativePrintServices`（worker 线程 discover → mpsc；空列表 → NoPrinters）。33 行形状原样。
- **`print_dialog.rs`**：`PrintMaterial{scenes, dims, papers}` + `PrintUi{settings, state, labels, printers, page, material, job, status, content: PrintContent, content_dirty: bool, on_cancel/on_submit, exit, pending: Option<(Option<String>, PrintSettings, PrintContent)>}` + `HasPrintUi` + `print_dialog_view`。控件面沿用 rword（打印机循环切换 ◂▸、份数 ±、逐份 checkbox、双面/彩色/范围/奇偶循环 ▸、自定义 spec 输入、状态行、预览 canvas 翻页）**+ 新增内容三 checkbox 行**（水印/批注/图章，勾选即置 `content_dirty`）。`sync_document_defaults` 从 material.papers.first() 设默认纸型/方向（纸型权威=文档）。预览绘制函数 `draw_preview` 原样（xilem canvas 即 record::Scene，append_transformed 复用，灰桌白页 fit-contain）。

Cargo：rofd-xilem-view 加 `office-rs-print = { workspace = true }`（native 对话框服务 + PrintSettings 类型；xilem-view 本就 native-only，无 wasm 顾虑）。

## 3. xilem-app：接线

- `AppState` 增 `print: Option<PrintUi>` + `print_material: Arc<Mutex<PrintMaterial>>` + `current_page: usize` 镜像（经 xilem-view 既有 `on_page_change` 回调维护，1-based）；实现 `HasPrintUi`。
- `do_print(app)`：push 命令物化 material（begin_print(default content) → 全页 scenes/dims/papers）→ `PrintUi::new(material, zh=true)`（预览初始页 = 镜像编辑器当前页）→ `sync_document_defaults` → discover 阻塞 drain（rword 同款 `recv_timeout(500ms)` 循环至终态消息）→ 挂 on_cancel（取消活 job + 清 status、置 exit）/on_submit（校验 + 以镜像编辑器页 `resolve_page_list` 预检 + 置 `pending=(printer, settings, content)`，不直接 spool）。
- `app_logic` 排空段（仿 rword main.rs:296-305）：`ui.exit` → 关；`ui.pending.take()` → push 命令 `c.print_direct(printer, &settings, content, "rofd")`，JobHandle 回写 `ui.job`（实现期按宿主既有模式接线）。
- **job 终态观察**（§0.3 修正 7）：`app_logic` 开头若 `app.print` 持有活 `ui.job` 且非终态 → `snapshot()` → 终态时刷新 status（Done → 已发送；Failed/Aborted → 失败 + 可重试态）。刷新时机 = 宿主任一事件驱动的 app_logic 拍（点击/键盘/鼠标移动）；完全无交互时延迟可见——v1 接受，页级实时进度仍非目标（§6）。
- **content_dirty 重物化**：app_logic 见 dirty → push 命令重物化 material（同 do_print 物化闭包）→ 清 dirty。提交时若仍 dirty，pending 先重物化再直打（或直接依赖 dirty 分支——实现期选最短路径，语义 = 提交永远用最新开关）。
- 覆盖层：`zstack((main_col, print_overlay))`（rofd 宿主无 popover 层则按现有顶层结构适配），`print_overlay = sized_box(print_dialog_view).background_color(桌面 chrome 色)`——编辑器树常驻（§0.2 决策 5）。
- 工具栏「打印」按钮（「另存为 PDF」旁）。
- `main`：`run()` 之后 `office_rs_print::join_all()`（rword main.rs:364 同款注释：进程死亡会杀 spooler 助手与任务）。

## 4. 手测通道：print_to_file example

`crates/xilem-app/examples/print_to_file.rs`：`cargo run -p xilem-app --example print_to_file -- <file.ofd> <out>`——load_ofd → component（宿主同款构造）→ `begin_print(default)` → `payload_for(all)` → `office_rs_print::print_pages_to_file(payload, &settings, out)`。Windows 默认打印机语境下产 spool 文件目检（实现期以 print 仓 README 的 to-file 语义为准校准断言/输出提示）。

## 5. 测试与验收

- **纯函数单测**（状态机）：discovery 流转/no-printers 阻断/非法 spec 阻断+错误快照/提交成功态/失败可重试/reset/双语言标签齐全（照抄 rword 七测，去 serde 往返）。
- **component**：`print_direct` 契约测试 `#[ignore = "requires a printer system"]`（rword 同款：default 文档 → handle 非立即 Failed）；wasm32 冒烟（`cargo build -p rofd-component --target wasm32-unknown-unknown`）守门控正确性——**这是本期最重要的回归门**；print_gen：换文档后旧 session `is_stale()==true`。
- **PrintUi**：内容开关置 dirty、pending 快照含 content 三元组（纯 Rust 测，无 widget）。
- **手测**：`cargo run -p xilem-app` 开真实样例 → 打印 → 对话框（预览/开关/范围）→ 「Microsoft Print to PDF」出纸目检；`--example print_to_file` 通道。
- xilem-view harness（可选，实现期定）：对话框打开/关闭覆盖层不 panic。

## 6. 非目标

- 对话框实时页级进度（JobHandle.snapshot 轮询 UI 化）——v1 只 status 行 + 取消。
- 打印机能力感知（printer_capabilities 过滤双面/彩色选项）——同一期 tauri 面决策。
- material 惰性单页物化优化。
- wasm 打印（wasm 宿主走 exportPdf，永远不直打）。

## 7. 留给 plan/实现的次要决策（builder 可定）

- pending→JobHandle 回写的具体宿主模式（outcome 槽 vs 命令闭包旁路）。
- content_dirty 重物化与提交的合并策略（提交时强制重物化 vs 依赖 dirty 分支先行）。
- example 的输出格式断言细节（以 to-file 实际产物为准）。
- 对话框控件中文文案微调（沿 rword zh_cn 标签表）。
