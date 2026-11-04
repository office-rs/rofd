# rofd gpui 线功能对齐逐 id 审计表（plan Task 14 产物）

**生成**: 2026-11-04，机读 `crates/web-app/src`（App.vue / FileMenu / HighlightColorPanel /
ShapeMenu / ZoomMenu / PrintDialog / host.ts）+ `crates/web-view/src/wasm_ofd.rs` 导出面，
逐条对照 gpui 线实际代码（HEAD 9f4e1c1）人工核对；所有 实装 行给出 file:line 证据。
**分母**: spec 2026-10-31 §8 的 24 行裁剪分母，展开为 33 个施工 id；表外行为 12 条逐条裁决
（A1–A12，采纳/挂账/豁免，无静默漂移）。
**用途**: 手测清单（`2026-11-01-gpui-app-windows-checklist.md`）与终核 review 的唯一依据。
**打印机制差异总注**: web 浏览器走 PDF iframe + 系统打印对话框（App.vue:865-881）、tauri 走
IPC spool；gpui 走 native spool（`print_direct` 一次调用）。分母按 spec R3 = 对话框全字段 +
结果呈现，非机制复刻。

## 状态词表

| 状态 | 含义 |
|---|---|
| 实装 | gpui 线已落地，证据列给出 file:line |
| 部分实装 | 主体落地，子项缺口已列入挂账（不静默） |
| 挂账 | 未落地，带理由与时机 |
| 采纳 | 功能等价，接受呈现/方式差异（spec 非目标：像素级视觉复刻） |
| 豁免 | web 侧同为桩（v1 暂未开放），零移植 |

## 实装表（spec §8 展开，按簇）

### 文件（spec 行 1/2/3/24 机制注）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| F1 打开文件 | FileMenu「打开」→ fileHost.open() → loadOfd（App.vue:720-723, 829-832） | 实装 | shell.rs:379-409 `open_document`（gpui 原生 `prompt_for_paths`，无 rfd 依赖）→ host/document_io.rs:27-35 `load_ofd`（含 PackageHandle + 警告收集）→ shell.rs:339-376 `install_loaded_session`；CLI 路径参数 main.rs:97-101 + shell.rs:198-206 | |
| F2 保存（Ctrl+S / 原地手术刀） | 「保存」→ fileHost.save(saveOfd())（App.vue:834-839）；Ctrl+S → onSaveRequest → save（App.vue:932） | 实装 | shell.rs:278-281 SaveRequested → `save_session`；shell.rs:414-447 有路径手术刀保存（失败保持 modified，xilem 结果槽契约）；document_io.rs:42-53 `save_ofd`（Some(pkg) 手术刀 / None 全量 + 原子写 69-81） | web 是浏览器下载；native 为原地写（平台差异，采纳） |
| F3 另存为 | v1 与保存同路径（App.vue:730-733 注释） | 实装 | shell.rs:452-499 `save_session_as`（`prompt_for_new_path` → 保存 → 重绑路径；不铸造 PackageHandle）；关闭确认的「保存」复用（shell.rs:903-907） | gpui 为真另存新路径（功能超集） |
| F4 导出为 PDF | 「导出为 PDF」→ exportPdf(净版预设) → Blob 下载（App.vue:843-860） | 实装 | shell.rs:506-539 `export_pdf`（prompt 前 capture editor、按身份回查防会话漂移）→ host/export_pdf.rs:19-33（begin_print 净版+图章 → rofd-export → 原子写） | |

### 工具（spec 行 4/7）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| T1 手型/文本 | 两个 ToolButton → setTool（App.vue:34-39） | 实装 | shell.rs:974-992 工具组按钮（选中态镜像 session.tool）→ shell.rs:803-810 `set_tool` → handle.rs:51-53 | 工具镜像每会话独立（sessions.rs:53-56） |
| T2 手绘 | 「手绘」→ setTool('freehand')（App.vue:198-199） | 实装 | shell.rs:993-1003 `Tool::Create(CreateKind::Freehand)` 按钮 | |
| T3 图形下拉 | ShapeMenu 五种（rect/ellipse/line/arrow/polygon，ShapeMenu.vue:34-40） | 实装 | shell.rs:1308-1340 `shape_menu`（kit Popover，受控 open）+ shell.rs:1770-1777 `SHAPE_ITEMS` → AppAction::PickShape → shell.rs:884-886 `set_tool(Create(Shape(kind)))` | gpui 六种（多折线 PolyLine，内核 ShapeKind 全集）= web 超集 |

### markup（spec 行 5/6）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| M1 markup 四种 | 高亮/下划线/删除线/波浪线按钮，action-disabled=!hasTextSelection（App.vue:155-195）→ applyMarkup | 实装 | shell.rs:1007-1043 四按钮（disabled=!has_selection）→ shell.rs:815-818 `apply_markup` → handle.rs:69-75；has_text_selection 镜像由 TextSelectionChanged 维护（shell.rs:266-269） | 一次点击=一条批注=一次撤销，与 web 同（内核语义） |
| M5 高亮色下拉 | HighlightColorPanel（自动+主题/标准色+其他颜色）→ setHighlightColor + 选区即应用 R4（App.vue:766-773） | 实装 | shell.rs:121-149 kit ColorPicker 订阅（Change → set_highlight_color + 有选区即 apply_markup）；handle.rs:56-58；新会话继承已选色（shell.rs:229-230） | kit 色板 ≠ web 63 色网格（采纳：chrome 用 kit 重绘）；web「自动」项展示色 #FFDD00 与内核默认 FFFF00（ofd_component.rs:3080）本就不一致——gpui 镜像取内核默认 FFFF00（shell.rs:189），内核行为两侧一致 |
| M6 线类各档颜色下拉 | 下划线/删除线/波浪线各自 HighlightColorPanel → setMarkupColor + R4（App.vue:353-373, 776-789） | 挂账 | 动词已备：handle.rs:62-64 `set_markup_color`；宿主未做各档取色 UI（工具栏仅一个高亮 ColorPicker） | 时机：需要线类配色时；纯宿主 UI，无内核改动 |

### 历史（spec 行 8）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| H1 撤销/恢复 | 按钮 disabled=!canUndo/canRedo，onChange 刷新（App.vue:207-212, 617-630）；Ctrl+Z/Y | 实装 | shell.rs:1057-1082 按钮（disabled=!can_undo/can_redo，每帧读 959-960）；handle.rs:111-130；快捷键 view.rs:237-259（Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z 拦截，rword 双通道防双写继承） | |

### 视图（spec 行 9/10/11/22/23）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| Z1 zoom 适应三模式 | 适应宽度/高度/页面按钮 + ZoomMenu 顶部三项 → setZoomMode（App.vue:55-82; ZoomMenu.vue:40-44） | 实装 | shell.rs:1375-1423 `zoom_menu` 三适应项（选中态对 zoom_mode 镜像）→ PickZoomMode → shell.rs:887-890 `set_zoom_mode`；handle.rs:134-140 | 翻页不重适配等内核语义自动继承 |
| Z2 zoom 预设 | ZoomMenu 预设 [10,25,50,75,100,125,150,200,300,400,500]（ZoomMenu.vue:37）→ setZoomPercent | 实装 | shell.rs:53 `ZOOM_PRESETS`（与 web 逐值一致）→ PickZoomPercent → shell.rs:822-831 倍率锚定缩放（与 App.vue:642-645 同式）→ handle.rs:150-161 `zoom_at` | 「实际大小」= 预设 100（裁决 A6） |
| S1 滚轮滚动 | SDK handleMouseScroll（DX/DY） | 实装 | view.rs:208-217 on_scroll → events.rs:169-199（Windows Lines 档×3 取反 ÷3 定标，20px/档对齐 xilem 手感）→ ViewEvent::Scroll | |
| S2 翻页 | 上一页/下一页按钮 → handleScrollPage（App.vue:94-100）；PageUp/PageDown 键 | 实装 | PageUp/PageDown 经 events.rs:61-87 `map_key` → 内核 ScrollPage；handle.rs:164-168 `scroll_page`（自动翻页/状态栏复用）；PageChanged → session.page 镜像（shell.rs:283-285） | web 的显式翻页按钮 → 裁决 A7 |
| S3 ctrl+wheel 缩放 | SDK 内部（指针位锚定 ZoomAt） | 实装 | events.rs:170-187 ctrl 分支 `ZoomAt{factor ×1.1/×0.9, center=指针视口位}` + events.rs:208-216 边界抑制（不动值不出内核，防误退 fit）；view.rs:208-217 | |
| V3 全屏 + Esc | 浏览器 fullscreen API + fullscreenchange 镜像（App.vue:122-124, 663-669, 913-915）；Esc 由浏览器原生退出 | 实装 | shell.rs:834-837 `toggle_fullscreen`（window.toggle_fullscreen）；按钮 shell.rs:1092-1100（active 态对 window.is_fullscreen）；Esc = main.rs:34 全局键绑定 ExitFullscreen → shell.rs:1118-1124（popover 捕获焦点时不干扰，双发残差与 web 同，main.rs:30-33 注） | |
| V4 自动翻页 | 3s setInterval → handleScrollPage('down')（App.vue:128-130, 671-680） | 实装 | shell.rs:42-43 `AUTO_PAGE_INTERVAL=3s` + shell.rs:842-865 定时循环（generation token 防陈旧循环）；会话切换/关闭即停（shell.rs:778-799 `on_active_changed` bump gen） | |

### 交互（spec 行 12/13/14/15/16）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| C1 右键菜单 | onContextMenu：仅批注命中出菜单、单项「删除批注」；点外关闭（App.vue:886-911, 404-408） | 实装 | events_surface.rs:104-107 ContextMenuRequested → shell.rs:305-325（仅 `ContextTarget::Annotation` 置菜单；+element_origin 转窗口坐标）；shell.rs:1527-1561 kit Popover 锚定 + 唯一项「删除批注」→ shell.rs:892-897 `delete_annotation`；CursorChanged 关闭（shell.rs:273-277）；overlay 点击关闭（ContextMenuDismiss） | R7 严格 web 对齐：Page/Empty 不出菜单 |
| C2 批注选中/删除 | Del 键删选中（内核）；工具栏「删除」按钮 → deleteSelected（App.vue:213-215, 656-661） | 实装 | 键盘 Del/Backspace 删选中经内核（component ofd_component.rs:2329-2342；events.rs:65-66 键映射直达）；SelectionChanged 事件面 events_surface.rs:80-83；handle.rs:92-106 `delete_selected` 动词已备 | web 的工具栏「删除」按钮 → 裁决 A4 |
| C3 批注交互提示 | onAnnotationFocus/onAnnotationInteract → console.log（App.vue:939-940） | 实装 | events_surface.rs:96-103 两事件入队列；shell.rs:331 v1 无 shell 镜像（与 web 的 console.log 等价：两侧均无用户可见面） | |
| E1 批注文本编辑 + IME | 浏览器组合事件（SDK preedit 状态机） | 实装 | ime.rs:34-155 ImeBridge 组合区影子仓（replace_and_mark→ImePreedit；replace_text(final) 先同步 preedit 再 commit 防 MS 拼音空格残留；unmark→force_commit_preedit；空替换=取消）；ime.rs:161-279 EntityInputHandler 委托（bounds_for_range 以 caret_rect+element_origin 锚候选窗，ime.rs:250-269）；裸可打印键只走输入通道（events.rs:93-115，双通道防双写） | rword C1 回归项继承 |
| E2 复制（Ctrl+C / Ctrl+X copy-only） | 内核 Ctrl+C → on_copy → SDK 剪贴板 | 实装 | view.rs:249-250 拦截（c 与 x 同路径 copy-only，body 只读契约）→ view.rs:149-154 `copy_selection` → `cx.write_to_clipboard`；内核 Ctrl+C → CopyRequested → shell.rs:328 回查重写剪贴板 | |
| E3 粘贴（Ctrl+V） | paste 事件 → pasteText | 实装 | view.rs:251 → view.rs:157-163 `paste_from_clipboard` → `component.paste_text`（handle.rs:185-187 同动词） | 纯文本，与 web v1 对齐（RichClipboard 挂账 spec §15） |

### 状态与宿主策略（spec 行 17/18/19/20/21）

| id | web-app 行为 | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| W1 警告呈现 | onWarning → antd message.warning toast ×4s（App.vue:936-938） | 实装 | shell.rs:292-299 Warned → `append_warnings`（批式追加不覆写载入警告）；shell.rs:1189-1227 可关闭警告条（计数 + 首条摘要 + × 关闭）；shell.rs:1800-1810 四类警告中文文案 | toast→bar 呈现差异 → 裁决 A2 |
| V1 指针形 | on_pointer_cursor → cursor 映射 | 实装 | shell.rs:300-304 镜像（仅活动会话）→ shell.rs:1185 编辑器包裹 div + shell.rs:1790-1797 `cursor_style`（Grab/Grabbing/Text/Default） | |
| V2 hover tooltip | wasm 侧默认装 default_tooltip_lines（wasm_ofd.rs:949-956），web 未关 → 悬停有批注 tooltip | 挂账 | 动词已备：handle.rs:199-209 `set_tooltip_enabled`；宿主未调用（内核默认无 formatter，callbacks.rs:136）→ gpui 悬停无 tooltip | 时机：宿主补一行启用即可；受 K1 时间戳影响（tooltip 显示 created，现为 0） |
| P1 页码/zoom 状态镜像 | 状态栏 第N页 + zoom%（App.vue:410-414） | 实装 | shell.rs:1229-1245 状态栏（page/zoom_percent 每会话镜像 shell.rs:949-954）+ ● 未保存指示 + 操作 status 行 | zoom% 归一化同式（/INITIAL_ZOOM×100，sessions.rs:23 定义基数） |
| D1 多文档页签 | （分母按改造后 web-app；改造线 A 未落地，rword sessionManager 先例） | 实装 | sessions.rs:87-174 SessionManager（每会话 editor/package/modified/pristine/事件镜像行）；shell.rs:1126-1159 TabBar 页签（modified 圆点、× 关闭、+ 新建）；关闭确认 shell.rs:736-772 + 1565-1630（保存/不保存/取消）；最后关闭自动开空白 shell.rs:752-772；跨会话模态复位 shell.rs:778-799 | spec §7：两线同推，本行分母按 rword 同型复核 |
| K1 时间戳/作者注入 | setClock('rofd', Date.now())（App.vue:961，宿主取挂钟） | 部分实装 | shell.rs:226-227 `set_clock("rofd", 0)`——注入通道实装（库不取钟契约保持），但 ts 为常量 0，未取系统时间 | 差异：批注 created/modified 落 epoch 0，保存往返与 tooltip 时间显示与 web 不一致；挂账：宿主改注 `SystemTime::now()`（宿主层允许） |

### 打印（spec 行 24；R3 入 v1）

| id | web-app 行为（tauri PrintDialog 同集） | 状态 | gpui 实装点（证据） | 备注 |
|---|---|---|---|---|
| PR1 打印对话框全字段 | tauri PrintDialog：打印机/份数/逐份/双面/彩色/纸型/方向/范围/奇偶 + 内容三开关（水印/批注/图章，浏览器面板仅三开关 App.vue:421-435） | 实装 | print_dialog.rs:359-378 枚举集（纸型 7 / 双面 3 / 方向 3 / 奇偶 3）+ 份数步进与逐份开关（print_dialog.rs:554-622, 838-848）+ 范围 All/Current/Custom 循环（171-180，Custom 输入框经 shell.rs:150-179 镜像）+ 内容三开关（PrintContent，print_dialog.rs:185-187）；纸型/方向默认取文档首页（shell.rs:549-586 开对话框时 `begin_print` 同步） | |
| PR2 打印机发现状态 | tauri IPC 发现；空/失败态 | 实装 | gpui-view print_services.rs:16-32 worker 线程 `office_rs_print::list_printers` → mpsc；print_dialog_state.rs:17-22 DiscoverState 四态（Finding/Printers/NoPrinters/Failed）+ 状态机推进 84-141；打开时有限排水（shell.rs:570-580，DISCOVER_TIMEOUT 500ms）+ 渲染拍排水（print_dialog.rs:121-137） | 无打印机/发现失败阻塞提交见 PR3 |
| PR3 单轨提交 + 终态轮询 + 取消 | web 无对应（系统对话框托管）；tauri IPC 单轨 | 实装 | print_dialog.rs:215-235 `try_submit`（Busy 单轨守卫 / NoPrinters 门 / precheck + Current→Custom 冻结 324-337）→ shell.rs:619-641 `print_direct` 一次调用（handle.rs:216-231）→ `submit_accepted`；终态轮询 shell.rs:657-684（200ms，Done/Failed/Aborted）；取消/关闭撤单 print_dialog.rs:252-260 + shell.rs:794-797（换页签也撤）；退出前 join_all main.rs:66（当前 pin 下为 no-op，真实保证是 cancel-on-close，main.rs:59-65 注） | 手测通道：`cargo run -p gpui-app --example gpui_print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out`（examples/gpui_print_to_file.rs） |

**主表合计**: 33 id = 实装 30 + 挂账 2（M6、V2）+ 部分实装 1（K1）。

## 事件面核对（14 内核回调 → OfdGpuiEvent）

events_surface.rs:33-48 一对一收窄：Changed / SelectionChanged / TextSelectionChanged /
CursorChanged / SaveRequested / AnnotationFocused / AnnotationInteracted /
ContextMenuRequested((f64,f64), ContextTarget) / PageChanged / ZoomChanged / ZoomModeChanged /
Warned / PointerCursorChanged / CopyRequested。引用参数事件不载文本（宿主回查组件，
events_surface.rs:25-31 payload 政策），与 xilem 线借阅式 handler 同型。无遗漏、无自造。

## 表外裁决（分母纪律：逐条登记，无静默漂移）

| id | web-app 行为 | 裁决 | 理由 |
|---|---|---|---|
| A1 状态栏保存按钮 | 模板中已不存在（footer 仅 页码+zoom%，App.vue:410-414）；App.vue:931-932 注释残留提及 | 无漂移 | 分母侧该按钮已移除；gpui 保存入口 = 文件菜单（保存/另存为）+ Ctrl+S，覆盖同等功能。（web 侧残留注释属 line A 清理项，非本线） |
| A2 警告 toast vs 常驻条 | antd toast ×4s 自动消失 | 采纳 | spec 非目标（像素级视觉复刻）；桌面惯例常驻可关闭条信息更持久，内容集一致（W1） |
| A3 操作反馈 toast（已保存/已删除 N 个/没有选中的批注） | message.success/info | 采纳 | gpui 以状态栏 status 行反馈（shell.rs:374, 436, 529, 730, 759-761, 895） |
| A4 工具栏「删除」按钮 | deleteSelected + toast（App.vue:213-215） | 挂账 | 键盘 Del/Backspace 已覆盖（C2）、右键单项删除已覆盖（C1）；按钮为显式入口。handle `delete_selected` 已备，补一个 tool_btn 即可。时机：交互打磨轮 |
| A5 缩小/放大按钮 | zoomBy(0.9/1.1) 两按钮（App.vue:43-51） | 挂账 | ctrl+wheel（S3）+ 预设（Z2）覆盖同功能；显式 ± 按钮缺。`zoom_at` 动词已备。时机：交互打磨轮 |
| A6 实际大小按钮 | setZoomPercent(100)（App.vue:55-57） | 采纳 | zoom 菜单预设 100% 等价入口（Z2，预设表逐值一致） |
| A7 上一页/下一页按钮 | 两按钮 → handleScrollPage（App.vue:94-100） | 挂账 | PageUp/PageDown 键盘等价（S2）；状态栏有页码镜像无翻页按钮。时机：交互打磨轮 |
| A8 阅读背景（4 桌面色） | 背景下拉（App.vue:125-127, 382-394, 537-542） | 挂账 | 纯装饰项；gpui 编辑区固定底色（shell.rs:1117）。时机：有桌面主题需求时 |
| A9 Ribbon 五页签（阅读/注释/编辑/签章/票据） | 页签切换工具行（App.vue:5-27） | 采纳 | chrome 组织差异：各页签**已启用**的功能（手型/文本/zoom/适应/翻页/全屏/自动翻页/markup/手绘图形/撤销恢复）在 gpui 平铺工具栏全部有对应行（本表 T/Z/S/V/M/H 簇）；禁用页签属 A10 豁免 |
| A10 启动自动加载 sample.ofd | fetch BASE sample.ofd（App.vue:962-968） | 采纳 | web 的 demo 便利行为；gpui 与 xilem 宿主同例（空编辑器起步 + CLI 路径参数），手测走显式路径 |
| A11 载入 loading 遮罩 | a-spin「正在初始化编辑器…」（App.vue:400-402） | 采纳 | gpui 启动即空编辑器（同步首帧），无长初始化窗口需要遮罩 |
| A12 品牌 logo / dev e2e seam | tabbar「rofd」品牌（App.vue:25）；`__ofd` dev 暴露（App.vue:958-960） | 豁免 | web 宿主专属装饰与测试缝，零移植 |

**表外合计**: 12 条 = 采纳 6（A2/A3/A6/A9/A10/A11）+ 挂账 4（A4/A5/A7/A8）+ 无漂移 1（A1）+ 豁免 1（A12）。

## 豁免清单（web 桩项，零移植）

以下 web 控件标注「v1 暂未开放」disabled，无行为，不进分母（rword 审计同型豁免）：

- **阅读页签**：顺时针/逆时针旋转、首页/尾页、单页/双页/连续阅读；
- **注释页签**：修订模式、隐藏注释、导入注释/导出注释；
- **编辑页签**：文档、水印、链接（v1 body 只读）；
- **签章页签**：签章、验签（v1 非目标）；**票据页签**：全部（注释/编辑/签章/票据/打印/数据查看/导出 PDF/导出图片）；
- **文件菜单**：关闭、导出 TXT、导出图片、属性、设置、帮助、最近（FileMenu.vue:16-80 disabled 项）。

## 挂账汇总（全部 7 项）

| id | 缺口 | 时机 | 工作量预估 |
|---|---|---|---|
| M6 | 线类 markup 各档取色 UI（动词已备） | 需要线类配色时 | 宿主 UI |
| V2 | hover tooltip 未启用（动词已备；依赖 K1） | K1 落地后随手启用 | 一行 + 验证 |
| K1 | set_clock ts 常量 0，未取系统时间 | 保存保真需要真实 created 时 | 宿主一行 |
| A4 | 工具栏「删除」按钮 | 交互打磨轮 | tool_btn |
| A5 | 缩小/放大按钮 | 交互打磨轮 | tool_btn ×2 |
| A7 | 上一页/下一页按钮 | 交互打磨轮 | tool_btn ×2 |
| A8 | 阅读背景色 | 桌面主题需求时 | 宿主 UI |

## 测试底账（headless，非本表义务，仅记录）

- gpui-view：7 个集成测试文件（event_mapping 11 / events_surface 6 / font_cache 3 /
  gpui_sink 5 / handle_verbs 6 / ime_bridge 9 / view_input 6）+ print_dialog_state 内联 9 +
  font_cache 内联 1；
- gpui-app：sessions 7 / print_dialog 18 / font_index 6 / host::document_io 3（内联）。
- `#[gpui::test]` + test-support；断言走录制式 sink（spec §11，无像素断言）。
