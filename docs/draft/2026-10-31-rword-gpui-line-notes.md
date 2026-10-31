# rword gpui 适配线全记录 —— rofd gpui/host SDK 起草参考笔记

日期：2026-10-31
来源：rword gpui 线完整周期（头脑风暴补 spec → plan → Phase 0 spike 四门禁 → Phase 1 八任务 TDD → MVP 手测 → 整支 review 修复 → WPS ribbon 视觉对齐 → 多文档会话 → 打印下沉），供 rofd 实现 gpui 类宿主 SDK 或桌面壳时引用。rword 侧权威文档：`rword/docs/superpowers/specs/2026-10-27-gpui-view-gpui-app-design.md`（MVP）、`2026-10-29-gpui-app-parity-design.md`（parity）、`AGENTS.md` gpui 条目、PR #44/#46/#47/#50/#51/#53。

## 0. 一句话架构（最终形态）

```
layer-6 适配层 rword-gpui-view（SDK，给 gpui 类上层应用）
├─ view.rs       WordGpuiView（Entity）+ WordElement（自定义 Element）
│                paint = update_scene → GpuiSink 录制 → replay（offset=element origin）
├─ events.rs     gpui 事件 → ViewEvent 纯翻译（无窗口依赖，全 headless 可测）
├─ gpui_sink.rs  两半式：录制（imaging PaintSink → Vec<PaintCmd>）+ 重放（→ Window）
├─ ime.rs        ImeBridge：pull(InputHandler)→push(ViewEvent) 组合区影子仓
├─ font_cache.rs Arc 身份键 + ttf-parser name-table/OS2 解析 → gpui FontId
├─ handle.rs     GpuiWordHandle 宿主动词（~55 个：格式/表格/查找/页码/导出PDF…）
└─ events_surface.rs WordGpuiEvent（14 内核回调 → gpui 实体事件队列）

宿主 rword-gpui-app（只经 gpui-view 用内核；WPS 视觉 ribbon）
├─ shell.rs（状态+IO+菜单行）/ ribbon/mod.rs（页签组件）/ sessions.rs（多文档）
├─ print_dialog.rs / font_index.rs（系统字体扫描，tauri 先例移植）
```

依赖面（冻结契约，修正案已入册）：gpui-view = rword-component + imaging + gpui-pre =0.3.10 家族（gpui-kit =0.8.1 同批锁）+ rword-export（exportPdf，web 先例）+ rword-dom。office-rs-print 在 component 内（print_direct 已下沉）。

## 1. 基座事实（rofd 选型直接抄）

| 项 | 结论 | 原因 |
|---|---|---|
| gpui 本体 | `gpui-pre =0.3.10`（zed@08cbb88 快照） | 官方 crates.io gpui 停在 0.2.2 一年，丢 Windows/IME 修复 |
| 平台入口 | `gpui-pre-platform`（`application()`）**独立 crate** | gpui-pre 的 lib 名是 `gpui`；平台实现按 OS 拆（gpui-pre-windows 等） |
| 组件库 | `gpui-kit =0.8.1`（锁 gpui-pre =0.3.10 全家） | Button/TabBar/Popover/ColorPicker 现成；**无 Ribbon 组件**（分组标签/分隔线自组装） |
| 图标 | `gpui-kit-assets`（Lucide 全套 SVG）+ `application().with_assets(AllAssets)` | `IconName` 枚举 → `Icon::new(name).size(px)` |
| 升级纪律 | 精确锁 `=`，gpui-pre/platform/kit/base/component **整批 bump** | 快照重发节奏由 Longbridge 掌握 |
| headless 测试 | `#[gpui::test]` + dev-dep `features=["test-support"]`；`simulate_event` 单参、keystroke 需先 focus | **render_to_image 不可用**（无 HeadlessRenderer）→ 断言走录制式 sink |

## 2. 关键设计裁决（rofd 同类接缝直接引用）

1. **双输入通道防双写**（review C1）：gpui 对可打印字符有两条通道（原始 KeyDown + WM_CHAR→InputHandler.replace_text_in_range）。**裸可打印键不得走按键通道**——文本统一走 InputHandler；组合中 replace_text(final) 要先推 ImePreedit{final} 再 ImeCommit（内核以 preedit.text 为准，浏览器 compositionend 契约）；unmark=确认→force_commit_preedit；空替换=取消。
2. **画/鼠标双向坐标**：内核 Scene IR 是视口坐标（0,0=编辑器左上）——replay 必须 + element bounds.origin；鼠标事件必须 − origin（两方向都做，漏一边就是"点击位置与光标偏移、随工具栏高度变化"）。
3. **编辑器尺寸**：WordElement 声明 relative(1.0) + 宿主 wrapper `flex_1().overflow_hidden()` 成对出现——只声明不裁剪则盖满窗口，只裁剪不声明则塌缩为 0。
4. **下拉层级**：absolute 兄弟节点会被后绘的编辑器盖住——**必须用 kit Popover**（overlay 层、锚定、点外关闭）；受控 open+on_open_change 走 Global 桥（Popover content 的 cx 属于 PopoverState，够不到宿主实体）；条目点击推 [Action, SetOpen(false)] 保序。
5. **GpuiSink 两半式**：录制半（Scene IR→PaintCmd，clip 栈按命令求交、transform 全烘焙、图片解码 BGRA）100% headless 可测；重放半才碰 Window/TextSystem。faux bold **零逻辑**（IR 已在 scene_builder 双绘，x 偏移 1/24em）。
6. **font_cache Arc 身份键**：ptr+len+index 做键（layout 每 font 一个共享 Arc，免逐 run 哈希 18MB CJK）；族名/weight/italic 用 ttf-parser 从字体自身表解析（`add_fonts` 不回 FontId；headless 测试平台 TextSystem 是桩——真解析只能窗口态验）。
7. **打印/剪贴板下沉 kernel**：`print_direct` 在 component（component 已 dep office-rs-print，10 行胶水串 begin_print→plan→payload→print_pages）；RichClipboard trait+ClipboardWin+TestClipboard 在 component——适配层只持实例+接线，未来适配器免费获得。
8. **字色选区守卫**：ColorPicker Change 在滑条试色时无选区也会发——gpui 侧加 has_text_selection 守卫防误染。**已知内核问题（web 同现，暂未修）**：部分场景字色应用波及整段；repro 单测留存（editor set_text_color_single_run_mid_selection，证明纯单 run 场景选区路正确，问题涉 preedit/运行时选区交互）。

## 3. 踩坑清单（rofd 实现时逐条对照）

- **56 个链式 .child() 在 debug 模式爆栈**（STATUS_STACK_OVERFLOW）→ 拆成返回 Vec<AnyElement> 的组方法 + .children()。
- `tab_button` 在 render 期 `cx.entity().read(cx)` 重入读 → panic "already being updated"。active 态改传参。
- 字体下拉"透明/被遮挡"两问一源：absolute 兄弟层被盖 → 换 Popover。
- `| head` 管道截断会杀 GUI 进程；运行中的 exe 锁 target 需先 taskkill。
- `FocusHandle::new` 私有（用 `cx.focus_handle()`）；gpui Modifiers 的 win 键叫 `platform`；`Pixels` 内部私有（用 `f32::from`）；`Pixels` 是逻辑像素（滚轮不除 scale——与 masonry 裸设备像素不同）。
- Windows 滚轮：`Lines(档数 × OS wheel_scroll_lines)`、**+y=向上**（内核要 +y=下 → 取反）→ ÷3 定标对齐 xilem 20px/档手感。
- fontique 0.8 Collection **无公开 families() 枚举 API** → 系统字体清单走宿主侧扫描（%WINDIR%\Fonts + ttf-parser，tauri font_index.rs 先例）。
- workspace 成员间依赖用 **path 直连 + 连字符包名**，非 workspace 继承；dev-dep 里的 path 依赖不会透传到主 deps。
- kernel 无 ctrl+a 全选（web 的 selectAll 是桩）；Alignment 变体是 Left/Center/Right/Both（非 Justify）。
- worktree 构建目录：`.cargo/config.toml` 用 **`target-dir`** 键（不是 `target`，那是三元组）；gitignored 字体目录不随 worktree → NTFS junction 指回主检出。

## 4. 交付物清单（rword 侧 rofd 可对照的文件）

| 交付 | 位置 |
|---|---|
| MVP spec / parity spec | `rword/docs/superpowers/specs/2026-10-27-gpui-view-gpui-app-design.md`、`2026-10-29-…` |
| 实施 plan（标准头+审计表法） | `rword/docs/superpowers/plans/2026-10-27-…`（MVP）、`2026-10-29-…`（parity）、`2026-10-30-print-direct-sink.md` |
| spike 报告（四门禁+GO 判定模板） | `rword/docs/superpowers/plans/2026-10-27-gpui-spike-report.md` |
| 逐 id 审计表（60 实装项分母法） | `rword/docs/superpowers/plans/2026-10-29-parity-audit.md` |
| 适配器源码 | `rword/crates/gpui-view/src/{view,events,gpui_sink,ime,font_cache,handle,events_surface}.rs` + tests/ |
| 宿主（WPS ribbon 会话制） | `rword/crates/gpui-app/src/{shell,ribbon/mod,sessions,print_dialog,font_index}.rs` |
| 手测清单模板 | `rword/docs/superpowers/manual/2026-10-28-gpui-app-windows-checklist.md`、`2026-10-29-gpui-parity-checklist.md` |
| SDK 接口设计规则 | `rword/AGENTS.md` Conventions 首条（flw ruling 2026-10-30：宿主语言直觉/三线动词一致/不漏内核类型/英文注释） |

## 5. 对 rofd 的建议（按 rofd 场景过滤）

- rofd 若做 **gpui 桌面宿主**：直接复用 rword-gpui-view（它是通用 SDK）；抄 gpui-app 的 shell/ribbon 结构做 rofd 自己的 chrome。
- rofd 若做**其他 GUI 框架适配**：抄"两半式 sink + 事件纯翻译 + handle 动词"三件套模式；审计表法（先定分母再施工）与 spike 四门禁（字体锐度/路径/IME/事件 headless）流程同样适用。
- rofd 的打印/剪贴板/字色逻辑：全部走 rword-component 公共面（已下沉），不要进适配层复制。
- 已知未修问题先登记：字色整段（内核，web+gpui 同现）、capture_pointer 真指针捕获、内核 callback 面的完整暴露（on_save_request/右键菜单在 gpui 侧 v1 走 Global 桥）。
