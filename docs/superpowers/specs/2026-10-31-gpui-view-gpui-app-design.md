# rofd gpui 适配层与宿主设计（rofd-gpui-view + rofd-gpui-app）

日期：2026-10-31（R3 修订 2026-11-01：打印入册）
状态：待批（brainstorming 产出，flw 裁决已入册）
参考：`docs/draft/2026-10-31-rword-gpui-line-notes.md`（rword gpui 线全记录）、rword 侧
`docs/superpowers/specs/2026-10-27-gpui-view-gpui-app-design.md` / `2026-10-29-gpui-app-parity-design.md`
（as-built 参照，实现时逐文件对照）。

---

## 1. 背景与动机

rword 已完整走通 gpui 适配线（spike 四门禁 → MVP → parity → review 修复 → 多文档 → 打印下沉），
基座事实（钉版、IME、sink、字体）全部钉死。rofd 作为同族项目（同 imaging IR、同 component 形态），
以**第三个宿主 + gpui 类上层应用 SDK** 的定位引入 gpui 线：

- 与 xilem / tauri 两条宿主线**并存**，不承担替代责任；
- 给桌面一个不依赖 WebView 的全原生形态（渲染/IME/文件 I/O 全原生）；
- 顺带成为 office-rs 家族的 gpui 适配 SDK 交付物。

**并行线**：rofd web-app 同时改造为多文档会话制（rword web-app sessionManager 先例）。
两线一起推、互不阻塞；gpui-app 从第一天就按多文档设计（分母按改造后的 web-app 计）。

## 2. 目标 / 非目标 / 反目标

### v1 目标

1. **rofd-gpui-view**（crates/gpui-view）：包 `OfdComponent` 的 gpui 适配 SDK，七文件结构
   照 rword，headless 可测；
2. **rofd-gpui-app**（crates/gpui-app）：Windows 优先宿主，功能对齐（改造后的）web-app：
   - 文件菜单：打开 / 保存 / 另存为 / 导出为 PDF / 打印…（全部接线，无置灰项）；
   - 工具组：手型 / 文本；
   - markup 组：高亮 / 下划线 / 删除线 / 波浪线 + 高亮色下拉（选色即应用）；
   - 批注工具：手绘 / 图形（ShapeKind 下拉）；
   - 撤销 / 恢复（canUndo/canRedo 使能）；
   - zoom 菜单：适应宽度/高度/页面 三模式 + 预设；
   - 右键菜单（严格 web 对齐：仅批注命中出菜单、唯一项"删除批注"，页面/空白处不出）、
     警告条、页码与 zoom 状态镜像、复制粘贴（纯文本）；
   - **全屏**（窗口全屏切换 + Esc 退出）与**自动翻页**（3s 定时驱动 `ScrollPage`），
     均纯宿主实现；
   - 批注文本编辑的 IME（中文组合输入）与 caret 闪烁；
   - **打印**（flw 裁决 2026-11-01：xilem 主线已落地，本期一起做）：native 直打对话框——
     打印机发现（discover-only `PrintServices` trait + worker 线程）、内容三开关（水印/批注/
     图章）、范围/奇偶/份数/逐份/双面/彩色/纸型/方向全字段（web-app PrintDialog 同集）、
     单轨提交（`print_direct` → JobHandle 终态轮询）、退出前 `office_rs_print::join_all`；
   - **多文档会话**：TabBar 页签、每会话独立实体、关闭确认、新建空白。

### 非目标（v1 明确不做）

- web-app 的**像素级视觉复刻**（功能对齐，chrome 用 gpui-kit 重绘）；
- macOS/Linux 承诺（Windows 优先；WebKit/gpui 平台差异记录不修）；
- RichClipboard 多格式下沉内核（v1 纯文本，与 web-app 对齐，见挂账）；
- CPU 回退（原生宿主，无 WebGPU 探测问题）；
- 多选批注（AnnotationSelection::Multi v1 无人产生）。

### 反目标（做了即错）

- 适配层依赖 rofd-io（平台边界 AGENTS §4.9：文件 I/O 只在宿主）；
- 库内取挂钟（§4.4；`Instant` 仅 caret blink，gpui 侧经 `request_animation_frame` 驱动）；
- 适配层构造 vello::Scene（§4.5：replay `imaging::record::Scene` IR）；
- 恒定 rAF 轮询 / 每帧 force-repaint（仅聚焦时保活动画帧）；
- 独立 Phase-0 spike 门禁（基座事实已被 rword 同版本钉死，first light 并入 Task 1）。

## 3. 基座事实（直接继承 rword 钉版，不重试）

| 项 | 结论 |
|---|---|
| gpui 本体 | `gpui-pre =0.3.10`（zed@08cbb88 快照），精确锁 `=` |
| 平台入口 | `gpui-pre-platform`（`application()`）独立 crate |
| 组件库 | `gpui-kit =0.8.1`（与 gpui-pre 整批锁）；Button/TabBar/Popover/ColorPicker 现成，无 Ribbon 组件（分组自组装） |
| 图标 | `gpui-kit-assets`（Lucide）+ `application().with_assets(AllAssets)` |
| 升级纪律 | gpui-pre/platform/kit/base/component **整批 bump** |
| headless 测试 | `#[gpui::test]` + dev-dep `features=["test-support"]`；`render_to_image` 不可用 → 断言走录制式 sink |
| workspace 依赖 | 成员间 path 直连 + 连字符包名；dev-dep 的 path 依赖不透传主 deps |

## 4. 架构与分层

```
crates/gpui-app（宿主，cargo bin）
  └─► rofd-gpui-view（crates/gpui-view，适配层 SDK）
        └─► rofd-component（+ rofd-dom 类型；不依赖 io/render/editor）
宿主另持：rofd-io（load/save/PackageHandle）、rofd-export（exportPdf）、
          rfd（文件对话框）、gpui 家族、gpui-kit
```

分层铁律：**适配层 = 内核翻面（事件进、Scene 出、回调转发），宿主 = 策略与 I/O**。
gpui-view 的 `[dependencies]` 是分层唯一事实来源（AGENTS §3）。

## 5. 适配层 rofd-gpui-view 逐文件

照 rword 七文件；标 ✦ 的两件从 rword **拷贝改写**（见裁决 R4），标 ✧ 的两件从
**xilem-view** 拷贝（打印状态机已在本仓 UI 无关落地）。

| 文件 | 职责 | rofd 特有差异 |
|---|---|---|
| `view.rs` | `OfdGpuiView`（Entity）+ `OfdElement`（Element）。字段：`component: OfdComponent`、`focus_handle`、`fonts: RefCell<FontCache>`、`ime: ImeBridge`、`last_mouse`、`had_focus`、`last_size`、`element_origin`、`events: Arc<EventQueue>`。paint 序：排空事件队列→cx.emit → 记 element_origin → 有效焦点变迁→FocusGained/Lost → `window.handle_input`(ElementInputHandler) → tick_blink+update_scene+录制 → replay(+origin) → 聚焦时 `request_animation_frame` | 无 clipboard_port（v1 纯文本走 gpui 剪贴板）；`Render` 用 `relative(1.0)`，宿主 wrapper 必须 `flex_1().overflow_hidden()` 成对 |
| `events.rs` | gpui 事件 → `rofd_component::ViewEvent` 纯翻译，无窗口依赖 | rofd ViewEvent 面：PointerDown 带 `click_count`（宿主供，组件无钟）；`ScrollPage{direction}`（PageUp/Down）；`ZoomAt{factor,center}`（ctrl+wheel，center=指针视口位）；`ImePreedit{text,caret:Option<(usize,usize)>}`/`ImeCommit{text}`（注意字段名是 `caret` 不是 rword 的 `cursor`）。Windows 滚轮：`Lines(档×OS wheel_scroll_lines)`、+y=上 → **取反**、÷3 定标 |
| `gpui_sink.rs` ✦ | 两半式：录制半（imaging `PaintSink` → `Vec<PaintCmd>`，clip 栈矩形求交、transform 全烘焙、图片解码 BGRA）100% headless 可测；重放半 `replay(window, cmds, fonts, origin)` 才碰 Window/TextSystem | imaging IR 同源，基本原样；rword 标识符改 rofd |
| `ime.rs` | ImeBridge 组合区影子仓：`replace_and_mark`→ImePreedit；`replace_text(final)`→先 ImePreedit{final} 再 ImeCommit（内核以 preedit 为准）；unmark=确认→`force_commit_preedit`；空替换=取消 | rofd 组件自持 preedit 状态机（无 ghost-document 回流），比 rword 更简单 |
| `font_cache.rs` ✦ | `FontCache`：Arc 身份键 `(ptr,len,index)→FontId`；族名/weight/italic 用 ttf-parser 从字体自身表解析（`add_fonts` 不回 FontId；headless 平台 TextSystem 是桩，真解析窗口态验） | 原样拷贝 |
| `handle.rs` | `GpuiOfdHandle`（包 `Entity<OfdGpuiView>`）宿主动词，动词面 = web SDK 导出面（分母表 §8），约 25 个 | 见 §8 动词列 |
| `events_surface.rs` | 组件回调 → `OfdGpuiEvent` 实体事件队列（paint 开头排空） | rofd 回调面 14 个（§8） |
| `print_dialog_state.rs` ✧ | UI 无关打印对话框状态机（DialogPhase/DiscoverState/全字段校验）+ discover-only `PrintServices` trait | **从 xilem-view 拷贝**（含 FakeServices 测试）；native 直打版 |
| `print_services.rs` ✧ | `NativePrintServices`：worker 线程 `office_rs_print::list_printers` → mpsc | 从 xilem-view 拷贝（~30 行） |

**双输入通道防双写**（rword review C1，必须继承）：裸可打印键不得走按键通道——文本统一走
InputHandler（ImeBridge）；Ctrl+V/Ctrl+C/Ctrl+Z/Ctrl+Y 等快捷键在按键通道拦截转 handle 动词。

## 6. 宿主 rofd-gpui-app

```
crates/gpui-app/src/
├─ main.rs        application() 入口 + AllAssets + 窗口
├─ shell.rs       AppShell：状态镜像（页码/zoom/warnings/canUndo…）+ 工具栏/文件菜单/
│                 zoom 菜单 + cx.subscribe(OfdGpuiEvent)
├─ sessions.rs    SessionManager 多文档（rword 同型，§7）
├─ print_dialog.rs gpui-kit 打印对话框面板（消费 gpui-view 的 PrintDialogState；
│                  单轨提交 + JobHandle 终态轮询；rword gpui-app 同型先例）
├─ host/
│  ├─ document_io.rs  load_ofd（警告收集）/ save_ofd（手术刀|全量，原子写）——xilem-app 同型搬移
│  └─ export_pdf.rs   begin_print 默认净版 + rofd-export 编码 + 原子写——xilem-app 同型搬移
└─ font_index.rs  系统字体扫描（§10）
```

- **chrome**：顶部 TabBar（多文档）+ 工具栏分组（gpui-kit Button/Popover/ColorPicker；无 Ribbon 组件，
  分组标签/分隔线自组装）。所有下拉必须用 **kit Popover**（overlay 层；absolute 兄弟会被后绘的
  编辑器盖住）；受控 open+on_open_change 走 Global 桥；条目点击推 [Action, SetOpen(false)] 保序。
- **右键菜单**（flw 裁决：严格 web 对齐）：组件 `on_context_menu((x,y), ContextTarget)` 给
  视口坐标 → +element_origin 转窗口坐标锚 Popover；**仅 `ContextTarget::Annotation(id)` 出菜单，
  唯一项"删除批注"**（`delete_annotation`），`Page`/`Empty` 不出菜单（宿主过滤）。内核零改动。
- **保存流**：`on_save_request`（Ctrl+S）→ 有路径手术刀保存；另存为 → rfd 对话框 + 全量/手术刀写；
  保存结果回写会话 modified 镜像（xilem-app 结果槽模式）。
- **导出 PDF**：`export_pdf` 同型于 xilem-app（净版预设：水印/批注关、图章开）。
- **打印流**（xilem phase 2 同思路）：菜单"打印…"开对话框 → `PrintServices::discover`
  （gpui-view）→ 用户确认 → 活动会话 `component.print_direct(printer, &settings, content, "rofd")`
  一次调用（内核已含 current-page 语义）→ JobHandle 终态轮询驱动对话框进度/完成/失败态；
  **单轨**（提交期间禁止二次提交）；退出前 `office_rs_print::join_all()`（进程死在 spool
  中会杀打印任务）。打印机为空/发现失败阻塞提交（状态机既有行为）。

## 7. 多文档会话（两线关系）

- **本线（B）**：gpui-app `sessions.rs` —— `SessionManager{sessions, active}`，每会话
  `Session{entity: Entity<OfdGpuiView>, file: Option<PathBuf>, package: Option<PackageHandle>,
  modified, pristine}`；关闭确认、最后一个关闭自动开空白、跨会话模态复位（find 无 → rofd 无此项）。
  PackageHandle 每会话独立持有（io 只在宿主）。
- **并行线（A）**：web-app 多文档改造 = rword `sessionManager.ts`/`appState.ts`（AppSessions/
  DocumentSession/patchSession）先例移植到 rofd 的 Vue 应用 + SDK 实例注册表。**独立 spec/plan**
  （本 spec 只记接口关系）；tauri 前端复用 web-app 源码自动受益。
- 两线互不阻塞：gpui 适配层（§5 events/sink/ime/font_cache/events_surface）不依赖会话形态；
  gpui-app 的会话实现与 parity 审计按 rword sessions 同型直接做，不等 web-app 改造落地。

## 8. 对齐分母（web-app 功能 → gpui 接缝映射表）

分母来源：`crates/web-app/src`（App.vue / FileMenu / HighlightColorPanel / ShapeMenu / ZoomMenu /
PrintDialog 置灰）+ `crates/web-view/src/wasm_ofd.rs` 导出面。**打印项整块排除**。

| # | web-app 功能 | 内核面 | gpui 落点 |
|---|---|---|---|
| 1 | 打开文件 | `parse_ofd`（io） | host document_io + rfd |
| 2 | 保存 / 另存为 | `save_ofd`/`write_ofd`（io） | host + on_save_request + 结果槽 |
| 3 | 导出为 PDF | `begin_print`+rofd-export | host export_pdf.rs |
| 4 | 手型/文本工具 | `set_tool(Tool)` | handle |
| 5 | markup 四种 | `apply_markup(AnnotationKind)` | handle（工具栏按钮，选中态镜像 on_text_selection_change） |
| 6 | 高亮色下拉 | `set_highlight_color` / `set_markup_color` | handle + kit ColorPicker |
| 7 | 手绘/图形工具 | `set_tool(Create(Freehand/Shape(k)))` | handle + ShapeMenu 同型 Popover |
| 8 | 撤销/恢复 | `undo/redo/can_undo/can_redo` + on_change 刷新 | handle + 按钮 disabled 态 |
| 9 | zoom 菜单/预设 | `set_zoom_mode`/`zoom_mode`/`zoom`、on_zoom_change/on_zoom_mode_change | handle + ZoomMenu 同型 Popover |
| 10 | 滚动/翻页 | `Scroll`/`ScrollPage`、on_page_change | events.rs + 状态栏 |
| 11 | ctrl+wheel 缩放 | `ZoomAt{factor,center}` | events.rs（指针位锚定） |
| 12 | 右键菜单 | on_context_menu + `ContextTarget`、`delete_annotation` | events_surface + Popover（仅 Annotation 命中、单项"删除批注"，flw 裁决） |
| 13 | 批注选中/删除 | on_selection_change、`delete_annotation`/`delete_selected` | events_surface + handle + 右键菜单 |
| 14 | 批注交互提示 | on_annotation_focus/on_annotation_interact | events_surface（状态栏/焦点环） |
| 15 | 批注文本编辑 + IME | preedit 状态机、ImePreedit/ImeCommit、`paste_text` | ime.rs + Ctrl+V 拦截 |
| 16 | 复制（选中文本） | `get_selected_text`/on_copy | 按键通道拦截 → gpui 剪贴板 |
| 17 | 警告条 | on_warning(s) | events_surface + shell 状态 |
| 18 | hover tooltip/指针形 | set_tooltip_enabled、on_pointer_cursor | handle + gpui cursor 映射 |
| 19 | 页码/zoom 状态镜像 | on_page_change/on_zoom_change | 状态栏 |
| 20 | 多文档页签 | （宿主概念） | sessions.rs + TabBar（分母按改造后 web-app） |
| 21 | 时间戳/作者 | `set_clock(author, ts)` | host 启动注入（宿主取系统时间，库不取） |
| 22 | 全屏 | （宿主概念：浏览器 fullscreen API） | gpui 窗口全屏切换 + Esc 退出（flw 裁决：进 v1） |
| 23 | 自动翻页 | `ScrollPage{direction}`（3s 定时） | 宿主定时器驱动（flw 裁决：进 v1） |
| 24 | 打印对话框 | `print_direct`（内核一次调用）+ PrintServices | gpui-view 状态机✧ + gpui-app 面板/单轨提交/轮询（flw 裁决 2026-11-01：入 v1；机制差异：web 走 PDF iframe，gpui 走 native spool，功能面=对话框全字段+结果） |

分母纪律：**本表为裁剪后的施工分母**——web-app 中存在但未入表的项（如状态栏保存按钮、toast
式警告呈现）在 plan 阶段展开审计表时逐条登记（采纳或明确挂账），不允许静默漂移。

plan 阶段按 rword `2026-10-29-parity-audit.md` 的**逐 id 审计表法**展开此表为施工分母。

## 9. 关键设计裁决（flw 已裁）

- **R1 并存定位**：不替代 tauri/xilem，功能债不背。
- **R2 v1 范围 = web-app 功能面**（功能对齐非视觉复刻）；打印除外（R3）。
- **R3 打印入 v1**（2026-11-01 修订，原裁决为推迟）：xilem 打印 phase 2 已落地
  （`print_direct` 下沉 component、xilem-view 状态机 + discover-only services），gpui 本期
  一起做——内核面零新增，对话框状态机/服务从 xilem-view 拷贝，面板与提交流程按 rword
  gpui-app 同型自建。
- **R4 拷贝不抽取**：`gpui_sink.rs`+`font_cache.rs` 从 rword-gpui-view 拷贝改写（文件头注释标
  来源 commit）；抽共享 crate 挂账至"出现第三消费者"。
- **R5 多文档**：gpui-app 第一天带会话制；web-app 并行改造多文档；两线同推、互不阻塞。
- **R7 右键菜单严格 web 对齐**（builder-check 裁决）：仅 Annotation 命中出菜单、单项"删除批注"；
  Page/Empty 不出。不做全选（内核无选区面，引入即内核改动，违反本线"适配层+宿主"范围）。
- **R8 全屏 + 自动翻页进 v1**（builder-check 裁决）：均纯宿主实现（窗口全屏/3s 定时 ScrollPage），
  不碰内核。
- **R6（我的调用，可否决）**：导出 PDF 进 v1；剪贴板 v1 纯文本；无独立 spike（first light 并入
  Task 1）；Windows 优先；crate 名 `crates/gpui-view`（包名 rofd-gpui-view）+ `crates/gpui-app`。

## 10. 字体通道（桌面）

- 宿主 `font_index.rs`：扫 `%WINDIR%\Fonts` + 用户字体目录（rword/tauri 同型：mmap + ttf-parser
  name 表全语言提取）建索引；文档声明族（`font_req::body_requirements`）**懒加载**本机字节 →
  `component.register_font_data`（内核 shaping）；同一批 Arc 字节经 `FontCache` 注册 gpui
  TextSystem（painting）——Arc 身份键保证两系统间零重复哈希。
- 默认字体宋体（tauri 桌面先例）；文档内嵌字体走 `Resources`（FontStore::from_resources）。
- 逐条对照 rword 踩坑：fontique 无 families() 枚举 → 清单永远宿主侧扫。

## 11. 测试策略

- **headless 单测**（适配层主体）：events.rs 映射表逐事件断言；gpui_sink 录制半（PaintCmd 序列/
  clip 求交/transform 烘焙）；ime.rs 影子仓四态机；font_cache 键语义；events_surface 队列序。
  `#[gpui::test]` + test-support；不断言像素。
- **组件级**：不重复 rofd-component 已有测试（工具/markup/undo 组件面已绿）。
- **手测清单**：`docs/superpowers/manual/2026-..-gpui-app-windows-checklist.md`（rword 模板）：
  打开样例、渲染锐度、滚轮/缩放、IME 中文、右键、多会话、保存回读（手术刀 `round_trip` 由 io 测试
  守护，宿主只验通路）、打印（print-to-file 通道，含打印机发现空/失败态）。
- **打印状态机**：拷贝自 xilem-view 的 FakeServices 单测随行即绿，不重写。
- **冒烟**：`test/ru-yuan-ji-lu.ofd` 作为 first light 与手测基准样例。

## 12. 依赖钉版

| crate | 版本 |
|---|---|
| gpui-pre / gpui-pre-platform | `=0.3.10` |
| gpui-kit / gpui-kit-assets | `=0.8.1`（整批锁，与 rword 同批） |
| imaging / imaging_vello | `0.0.1`（workspace 既有） |
| rfd | xilem-app 同版 |
| office-rs-print | git `c2f99d5`（workspace 既有 pin）：gpui-view（list_printers）+ gpui-app（join_all） |
| rofd 内部 | component + dom（gpui-view）；+ io + export（gpui-app） |

新 crate 声明 `rust-version = 1.96`；toolchain 1.98.1 不动。

## 13. 踩坑对照清单（rword 血泪，逐条适用）

1. 链式 `.child()` 数十层 debug 爆栈 → 组方法返回 `Vec<AnyElement>` + `.children()`；
2. render 期 `cx.entity().read(cx)` 重入 panic → active 态改传参；
3. 下拉必须 kit Popover（absolute 兄弟被编辑器盖）；受控 open 走 Global 桥；
4. `| head` 管道截断杀 GUI 进程；exe 锁 target 先 taskkill；
5. `FocusHandle::new` 私有 → `cx.focus_handle()`；Modifiers 的 win 键叫 `platform`；
   `Pixels` 私有内态 → `f32::from`；**Pixels 是逻辑像素**（滚轮不除 scale）；
6. Windows 滚轮 +y=上（取反）、`Lines(档×OS wheel_scroll_lines)`、÷3 对齐 xilem 手感；
7. 画/鼠标双向坐标：replay +origin、事件 −origin，**两方向都要**；
8. 编辑器尺寸 `relative(1.0)` 与宿主 `flex_1().overflow_hidden()` 成对；
9. dev-dep 的 path 依赖不透传主 deps；worktree 用 `target-dir` 键。

## 14. 阶段划分（plan 的输入，非施工单）

- **Phase 1 脚手架 + first light**：两 crate 立起、钉版落 Cargo、workspace 接线、打开
  ru-yuan-ji-lu.ofd 经 sink 渲染出图（手测截图，无 chrome）。
- **Phase 2 适配层 TDD**：events → sink 录制半 → ime → font_cache → events_surface，先红后绿；
  打印状态机 + services ✧ 拷贝随行（FakeServices 测试即绿）。
- **Phase 3 宿主**：shell + 工具栏/文件菜单/zoom 菜单 + handle 动词接线 + 剪贴板/右键/警告 +
  sessions/TabBar。
- **Phase 4 打印**：gpui-app 打印面板（全字段）+ 单轨提交（print_direct）+ JobHandle 终态
  轮询 + 退出 join_all；手测走 print-to-file 通道（xilem 先例 01e8c6f）。
- **Phase 5 parity 审计**：§8 分母展开逐 id 表 + 手测清单 + 整支 review。

并行线 A（web-app 多文档）独立 spec/plan，与本线 Phase 1-3 完全并行，Phase 5 审计时以
改造后 web-app 为分母复核第 20 行。

## 15. 挂账与未决

| 项 | 时机 |
|---|---|
| RichClipboard trait + 多格式下沉 rofd-component | 需要富格式时（rword 先例 A7） |
| gpui_sink/font_cache 抽共享 crate | 出现第三消费者 |
| capture_pointer 真指针捕获 | gpui 上游能力变化时 |
| macOS/Linux | 有需求时（WebKit/gpui 平台线） |
