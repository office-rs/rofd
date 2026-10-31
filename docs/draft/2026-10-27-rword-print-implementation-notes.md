# rword 打印功能实施全记录 —— rofd 打印 spec 参考笔记

日期：2026-10-27
来源：rword 打印功能全周期（spec 2026-10-20 → 三计划落地 → 两轮手测 → 两层重构），供 rofd 打印 spec 起草时引用。rword 侧权威文档：`rword/docs/superpowers/specs/2026-10-20-print-design.md`（含 §14 重构修订）、`AGENTS.md` 打印分层段、`office-rs/print` 仓库 README。

## 0. 一句话架构（最终形态）

```
接口层（薄）                          默认 UI 层（可选组件）
├─ xilem-view: print_direct(          ├─ xilem 打印对话框组件
│    component, printer, settings)       （自含 dialog 状态机）
│    → 一步直打（无 UI）             ├─ js sdk: 无打印 UI
├─ js sdk: exportPdf() 唯一打印接口  └─ web/tauri 打印 UI = 上层业务自理
└─ tauri IPC: print_pdf(pdf_b64, settings, title)

底座（公共库，rofd 直接复用）：office-rs-print
  settings（钳制+纸型映射）/ page_plan（范围语法+collate）
  payload（SceneWire serde）/ job（取消+进度+JOBS 表）
  后端：Windows GDI 矢量回放 + Windows WinRT-PDF 光栅 + Linux CUPS
```

## 1. 决策史（rofd spec 起步时最值得读的部分）

| 决策 | 演变 | 对 rofd 的含义 |
|---|---|---|
| 共享库形态 | 独立公开仓 `office-rs/print`，crate 平铺在根（git rev 依赖，CI 拉取零鉴权） | rofd 直接 `office-rs-print = { git, rev }`，公共面零 rword 依赖（只 imaging + serde） |
| 大载荷跨进程 | 最初 Pages 矢量载荷过 IPC（SceneWire JSON 嵌字体字节，CJK 一份 16~24 MB）→ **tauri 打印慢的根因** → 全面废弃 | 跨进程一律走 PDF 字节（几 MB）；矢量 Pages 仅限**引擎同进程**路径（xilem） |
| Windows 直打 | D2 矢量 GDI 回放（保真）→ 跨进程宿主补充 **PDF→WinRT `Windows.Data.Pdf` 设备 DPI 光栅→GDI**（系统渲染器免打包，D2 修订） | rofd tauri/桌面壳用 `print_pdf` 即得 Windows/Linux 双路 |
| dialog 状态机 | 初版放 print 仓（"共享脑子"）→ 终版**移回 rword-xilem-view 作为默认对话框组件私有实现**（UI 状态模型不属于 spooling 库） | rofd 自己的打印对话框自己写状态（可抄 rword-xilem-view/print_dialog_state.rs 的形状：纯 advance(state,event) + PrintServices trait 注入） |
| js sdk 打印面 | 初版带 DOM 面板 + 8 方法 wasm 桥 → **全删**，只剩 exportPdf | 浏览器打印对话框自带打印机/份数/范围/奇偶——SDK 面板增量价值为零；上层一行 iframe print |
| 份数/逐份/双面 | 永远只是任务参数（DEVMODE / CUPS options / 提交端回放仿真），载荷永不翻倍 | rofd 直接沿用 office-rs-print 的 D10 纪律 |

## 2. 内核语义（rword 侧，rofd 需要等价物）

- **打印模式布局**：`LayoutMode::Print`——空 SDT 占位符**不进布局**（不 push 占位文本、不 push ZWSP、不记 placeholder_ranges）；屏幕模式字节级不变。打印 pass 跑在 **scratch 缓存**上（不读不写屏幕增量布局的 prev/memo），屏幕缓存零扰动。
- **守卫**：`needs_print_layout(doc)` 纯扫描——存在"配了 placeholder 且递归文本为空"的 SDT 才付第二次布局的钱，否则复用屏幕布局（Arc 同一性）。**rofd 若无占位符语义（OFD 表单域不同），等价守卫是"打印布局≠屏幕布局的任何差异点"**——没有差异就永远复用。
- **PrintSession**（component 层内核缝，上层不直接碰）：`begin_print()` → `page_count / page_rect / page_parameters(n)→(Paper,Orientation) / page_scene(n) / plan(settings,current_page) / payload_for(pages) / is_stale()`。纸型权威 = 文档分节的页面尺寸（mm→最近 Paper），不是面板。
- **页计划**：`"1-3,5"` 语法解析（≤500 字符、页数上限 5000、升序去重合并）+ range×parity 解析 + collate 展开——全在 office-rs-print 的 page_plan，rofd 白拿。
- **PDF 语义**：导出 PDF 本身就是打印产物（占位符收缩）；`pdf_from_page_scenes`（页局部 Scene 列表 → krilla PDF）是 adapter seam。

## 3. office-rs-print 复用面（rofd 拿到什么）

```rust
// 设置（钳制 1..=999、PWG/DMPAPER 纸型映射、Auto 方向解析、serde 校验）
PrintSettings { printer: Option<String>, copies, collate, duplex, color, paper, orientation, range, parity }

// 页计划
parse_page_spec("1-3,5") / resolve_page_list(range, parity, total, current) / collate_plan(pages, copies, collate)

// 载荷（serde 全套）
PrintPayload::Pages(Vec<PageScene>)   // 矢量：纸型+方向+页局部 imaging Scene（SceneWire）
PrintPayload::Pdf(Vec<u8>)           // PDF 字节

// 提交（JobHandle：snapshot 进度 / cancel / wait；进程级 JOBS 表 + join_all）
print_pages(payload, settings, title) -> JobHandle   // Windows GDI 矢量（同进程引擎路径）
print_pdf(bytes, settings, title)     -> JobHandle   // Linux=CUPS 原生；Windows=WinRT 光栅+GDI
list_printers() / printer_capabilities()
print_pages_to_file / print_pdf_to_file             // 打印到文件测试通道
```

Windows GDI sink 细节（rofd 不用重写但要懂）：DEVMODE 覆写（驱动没声明的字段不置位→提交端仿真）、`AddFontMemResourceEx`+sfnt name 解析选面、glyph-index `ExtTextOutW`、64× 亚像素 world transform、每页取消+AbortProc、驱动不支持 copies/collate 时按回放顺序仿真。

CUPS：`cupsPrintFile` + media/sides/ColorModel/copies/Collate options；手写 FFI 带 `#[link(name="cups")]`（曾漏——Windows 本地不编译所以 CI 才暴露）。

## 4. 宿主接线模式（rofd 对号入座）

| 宿主形态 | rword 做法 | rofd 等价 |
|---|---|---|
| 原生 GUI（同进程引擎） | `print_direct(component, printer, settings)`：begin_print→plan→payload_for→print_pages（矢量） | rofd 的 gpui/xilem 宿主同款一步 API；OFD 出纸若走 PDF（板式文档天然 PDF/OFD 产物）也可直接 print_pdf |
| 桌面壳（引擎在 WebView） | tauri：前端 `exportPdf()` → base64 → IPC `print_pdf` → 壳内 WinRT/CUPS | rofd-tauri 完全同构；**不要**试图把矢量载荷传过进程边界 |
| 浏览器 | `exportPdf()` + 上层自己的 iframe print（见 §5 教训） | rofd web 同款 |
| 打印 UI | 上层自理；rword-xilem-view 提供可选默认组件（状态机私有） | rofd 自画脸（WPS 实证：不同文档形态打印设置长相本就不同） |

## 5. 踩坑清单（都是真金白银换的，rofd spec 应逐条写成约束）

1. **GDI 字高×64**：坐标/笔宽都乘了 64× 亚像素因子，逻辑字体高度没乘——字形渲染成 1/64 大小（PDF 文本层查得到、肉眼看不见）。任何 GDI 自定义坐标体系下**字体高度必须与坐标同因子**。
2. **WinRT PDF 输出是 PNG 不是 BMP**（文档写 bitmap）；加载走临时文件（`InMemoryRandomAccessStream`+DataWriter 路线 `LoadFromStreamAsync` 见零长流）。
3. **iframe 打印三连坑**（浏览器路线）：① onload 赋值必须**先于** appendChild（about:blank 同步 load 会错过）；② Chromium 对 blob-PDF 插件**不再发 load**——需要固定延时兜底；③ **print() 后绝不能立即 remove/revoke**——预览异步从 iframe 渲染，源要常驻到下一次打印（用户断点定位的"iframe 瞬间消失"）。
4. **serde wire 形状**（Rust↔JS 往返）：unit 枚举变体是**纯字符串**（`"submit_pressed"` 非 `{submit_pressed:null}`）；枚举值默认 **PascalCase**（`All/Custom/Odd`）；Rust 桥的 error 兜底 JSON 必须在 JS 侧检测，否则坏状态当合法状态渲染崩掉。
5. **GUI 框架结构切换**：把常驻编辑器 widget 整棵从树上拆下再挂打印屏 → masonry(alpha) 对已移除 child `get_mut` panic。**编辑器常驻，打印屏做覆盖层**。
6. **会话 init 静默失败**：init promise 链无 catch 时挂死/拒绝都不可见——必须 catch + 报错。
7. **静默假绿**：字体目录 gitignored，worktree 里缺它时按"跳过"处理的测试全部空转通过——worktree 要显式拷 fonts。
8. **jsdelivr CDN 403**：新 worktree 的 headless 环境里默认 Noto manifest 可能拉不到导致 init 拒绝——验证环境要能确定性拿到字体。

## 6. 测试策略（rofd spec 的验收节可直接抄）

- 纯函数全单测：范围语法/钳制/collate/纸型映射/DEVMODE 覆写/GDI 坐标/渲染选项。
- 端到端手测通道：`print_pages_to_file`/`print_pdf_to_file` + "Microsoft Print to PDF" 出 PDF 目检（中文/表格/图片/占位符收缩/份数/范围/取消）。example host（`cargo run --example discover`）就是非 rword 复用性验收 + 手测载具。
- CI：win+ubuntu 矩阵 cargo test + clippy -D warnings（ubuntu 装 libcups2-dev）。
- 浏览器流 Playwright：断言到 PDF bytes/iframe 挂载/print 被调，不碰系统对话框。

## 7. rofd 落地建议（优先级序）

1. `office-rs-print` git rev 依赖（imaging 版本与 rword 同 pin）。
2. rofd 内核等价语义：打印变体布局差异点盘点（OFD 表单/域的屏幕专属内容）+ 守卫 + PrintSession 形状 session。
3. 桌面壳：print_pdf IPC 一条（base64 PDF）；若 rofd 产物本就是 OFD/PDF，print_pdf 即主路径。
4. 对话框：自画脸，状态机抄 rword-xilem-view/print_dialog_state.rs 模式（advance 纯函数 + PrintServices 注入）。
5. Linux 先行（CUPS 吃 PDF 原生，rofd 的 OFD/PDF 输出零转换）；Windows 直接复用 WinRT 光栅路线。
