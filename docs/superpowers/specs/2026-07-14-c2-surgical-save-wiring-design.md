# rofd Cluster 2：手术刀保存调用链 设计

- **日期**: 2026-07-14
- **状态**: Draft（待评审）
- **范围**: V1 收尾子项目 2/4 -- 适配器层（native-view `EditorApp`、web-view `WasmEditor`）保留 `PackageHandle`、改调 `rofd_io::save_ofd`（手术刀）；native-app 接 Save 按钮 + Ctrl+S；修复 `modified` 标志的 view-dirty/doc-dirty 混淆
- **前置**: Cluster 1（[`2026-07-13-io-annotation-fidelity-design.md`](./2026-07-13-io-annotation-fidelity-design.md)）已完成。C1 实现了 `rofd_io::save_ofd`（手术刀：批注入口+分页重序列化、Document.xml MaxUnitID byte-patch、body `Content.xml` 字节级保留）+ `write_ofd`（全量），但两个 app 适配器当前都调 `write_ofd`（全量），丢弃了 `LoadReport.package`
- **约束**: AGENTS.md "关键偏离"--component 保持 **io-free**，io 调用下放到适配器层（C1 brainstorm 已确认）

---

## 1. 背景与动机

C1 让 `rofd_io` 具备了手术刀保存能力（不变量 4.3：body 字节级保留），但**从未被 app 调用**：

- `EditorApp::save_ofd`（`crates/native-view/src/editor_app.rs`）：调 `write_ofd(self.component.document())`（全量）。doc comment 明言："Surgical save needs a `&PackageHandle`, but the component consumes the package on `load_document` and does not re-expose it."--`load_ofd` 解析后丢弃了 `report.package`。
- `WasmEditor::save_ofd`（`crates/web-view/src/wasm_editor.rs:260`）：同样调 `write_ofd`，丢弃 package。
- native-app `main.rs`：**完全无保存布线**--无 Save 按钮、无 `on_save_request` 处理器。component 的 Ctrl+S 路由到 `on_save_request` 回调，但 native 未注册，故 Ctrl+S 是空操作。native 端到端存不了盘。

后果：打开含未建模 body（模板/JBIG2/冷门对象）或签名的真实 OFD，批注后保存，**未建模 body 内容丢失**（全量 write_ofd 只发模型里有的）。手术刀的核心保真承诺（决策 #5）在 app 层未生效。

另：`EditorApp.modified` 在**任何** `needs_repaint`（含 scroll/zoom--视图变更）时置 true，混淆 view-dirty 与 doc-dirty（C1 审计标 Important）。导致滚动后 `is_modified()` 误报，触发不必要的保存提示。

**本 cluster**：适配器保留 `PackageHandle`、save 路由手术刀/全量；native-app 接 Save + Ctrl+S；修 modified 标志。这是 V1 收尾 4 子项目的第 2 个（C1 done；C3 交互式批注 UX；C4 次要缺口）。

---

## 2. 范围与成功标准

### 2.1 范围

- `EditorApp`（native-view）：加 `package: Option<PackageHandle>`，`load_ofd` 保留，`save_ofd` 路由；去 `self.modified`，`is_modified()` 委托 component；加 `clear_modified` 协作。
- `WasmEditor`（web-view）：加 `package`，`load_ofd` 保留，`save_ofd` 路由。
- `EditorComponent`：加 `pub fn clear_modified(&mut self)`（save 后复位）。**不加 io 调用**（io-free 不变）。
- `examples/native-app/src/main.rs`：Save 按钮 + Ctrl+S（经 `on_save_request` flag-poll）-> save action -> 写 `current_file`（覆盖；None 时 rfd SaveAs 对话框）。

**不动**：component 的 io-free 边界、`rofd_io`（C1 已完成）、editor/render（C3/C4）、web `main.ts` 的 download 流程（只是底层 save_ofd 改手术刀）。

### 2.2 成功标准

- **手术刀生效**：`EditorApp::load_ofd(真实.ofd)` -> 批注 -> `save_ofd` -> 重开 -> 批注保留 + **body `Content.xml` 字节级相等**（不变量 4.3 在 app 层兑现）。
- **native 存盘**：native-app 打开文件 -> 批注 -> Ctrl+S 或 Save 按钮 -> 写盘 -> 重开 -> 批注保留 + body 一致。
- **modified 正确**：scroll/zoom 后 `is_modified()`=false；批注编辑后 true；save 后 false；load 后 false。
- **新文档**：`new_document`（package=None）-> save -> `write_ofd`（全量，无 package 可手术刀）。

---

## 3. 设计

### 3.1 适配器持 PackageHandle + save 路由

```rust
// crates/native-view/src/editor_app.rs
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub package: Option<PackageHandle>,   // 新增；Some=从文件加载(surgical), None=新文档(full)
    // modified 去掉，委托 component.is_modified()
}

impl EditorApp {
    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), String> {
        let report = parse_ofd(bytes).map_err(|e| format!("parse failed: {e}"))?;
        self.package = Some(report.package);          // 保留
        self.component.load_document(report.document);
        Ok(())
    }

    pub fn save_ofd(&self) -> Result<Vec<u8>, String> {
        match &self.package {
            Some(pkg) => rofd_io::save_ofd(self.component.document(), pkg),
            None => rofd_io::write_ofd(self.component.document()),
        }.map_err(|e| format!("save failed: {e}"))
    }

    pub fn is_modified(&self) -> bool { self.component.is_modified() }
}
```

`WasmEditor` 同理：加 `package: Option<PackageHandle>`，`load_ofd` 保留 `report.package`，`save_ofd` 路由。component 不变（io-free，`document()` 已有）。

### 3.2 native save UI（只 Save 覆盖）

`examples/native-app/src/main.rs`：

- **Save 按钮**（xilem button，工具栏）+ **Ctrl+S**。
- **Ctrl+S 路由**：component 已把 Ctrl+S 路由到 `on_save_request` 回调。native 注册回调用 `Arc<AtomicBool>` flag（`Send`-safe，绕过 `EditorApp` 非 `Send` [parley FontContext] + `&mut self` 借用冲突）：

```rust
let save_requested = Arc::new(AtomicBool::new(false));
let flag = save_requested.clone();
app.component.on_save_request(move || { flag.store(true, Ordering::SeqCst); });
// 主循环每次 handle_event 后：
if save_requested.swap(false, Ordering::SeqCst) { do_save(&mut app); }
```

- **Save 按钮**：xilem `onClick` 闭包直接调 `do_save`（按钮闭包有状态访问，无需 flag）。
- **save action `do_save`**：

```rust
fn do_save(app: &mut EditorApp) {
    let bytes = match app.save_ofd() { Ok(b) => b, Err(e) => { show_error(e); return; } };
    if let Some(path) = app.current_file.clone() {
        if let Err(e) = fs::write(&path, &bytes) { show_error(format!("write failed: {e}")); }
    } else {
        // 新文档无 current_file -> rfd SaveAs 对话框选路径
        if let Some(path) = rfd::AsyncFileDialog::new().add_filter("OFD", &["ofd"]).save_file().blocking() {
            let path = PathBuf::from(path);
            if fs::write(&path, &bytes).is_ok() { app.current_file = Some(path); }
        }
    }
    app.component.clear_modified();   // save 成功后复位
}
```

- 签名 caveat（覆盖签名文档可能失效，spec §10）文档化，不阻塞。

### 3.3 modified 标志修复

- `EditorApp` 去 `self.modified`（needs_repaint 置位的那个）。`is_modified()` -> `self.component.is_modified()`。`handle_event` 不再置 modified（component 的 `after_annotation_change` 已在批注编辑/undo/redo/delete 时置位；scroll/zoom/resize 不调 `after_annotation_change`，故不置）。
- `EditorComponent` 加 `pub fn clear_modified(&mut self) { self.modified = false; }`（save 后复位；component 的 `modified` 是 `pub(crate)`，故需公开方法）。
- native title/关闭提示用 `app.is_modified()`（现正确反映 doc-dirty）。

### 3.4 web save

`WasmEditor`：加 `package`，`load_ofd` 保留，`save_ofd` 路由手术刀/全量。web `main.ts` 的 `onSaveRequest` -> `editor.saveOfd()` -> Blob download 流程不变（底层由 `write_ofd` 改 `save_ofd` 手术刀）。

### 3.5 错误处理

- `save_ofd` 返回 `Result<Vec<u8>, String>`（适配器层把 `OfdError` 转 `String`，匹配现状）。
- native save action：失败 -> 状态栏/对话框提示"保存失败：{reason}"；file write 错误同理。不 panic，不静默吞。
- 无裸 unwrap。

---

## 4. 测试

### 4.1 EditorApp 单元测试（`crates/native-view/src/editor_app.rs`）

- `load_ofd` 保留 package：load 后 `app.package.is_some()`。
- `save_ofd` 有 package 走手术刀：load 真实/合成 .ofd -> save_ofd -> 重 parse -> body `Content.xml` 字节级相等（不变量 4.3 在 app 层）。
- `save_ofd` 无 package 走全量：`new_document`（package=None）-> save_ofd -> 非空、可重 parse。
- `is_modified()` 委托：scroll 后 false；批注编辑后 true；`clear_modified` 后 false；load 后 false。

### 4.2 WasmEditor 测试

- native 可测部分（parse/retain package、save 路由逻辑）：能在 native target 跑的纯逻辑测试。
- wasm-only 部分（JS 桥、download）：手动验（`npm run dev` -> 加载 -> 批注 -> save -> download -> 重开）。

### 4.3 native-app 集成（`examples/native-app`）

- Save 按钮 + Ctrl+S 触发 save：集成冒烟（可 `#[ignore]` 需 GUI，或 mock 文件写入断言 save_ofd 被调）。
- 真实样本：`EditorApp::load_ofd(test/ru-yuan-ji-lu.ofd)` -> 批注 -> `save_ofd` -> body `Content.xml` 字节级保留（端到端经适配器层，区别于 C1 的 io 层 `real_sample.rs`）。`#[ignore]`（gitignored 样本）。

### 4.4 component 测试

- `clear_modified`：置 modified 后 `clear_modified` -> `is_modified()`=false。

---

## 5. caveat / 不在范围

- **签名完整性**：surgical save 保留签名条目字节，但若原签名覆盖批注层，编辑+覆盖保存可能使签名失效。v1 不校验，文档化（spec §10）。用户选"只 Save 覆盖"--接受覆盖原件的风险，不做 Save As 默认。
- **Save As**：仅新文档（`current_file=None`）时弹 rfd SaveAs 对话框选路径；已打开的文件 Save 直接覆盖，不提供 Save As 菜单项（YAGNI，用户选最小）。
- **关闭时未保存提示**：不在 C2 范围（native-app 无 close-prompt；`is_modified()` 修好后可作为后续）。
- **web download 文件名**：维持现状（`document.ofd`），不改成原文件名（YAGNI）。
- **Stamp/自定义字体新资源**：surgical save 不加新资源条目（C1 caveat 延续；C1 spec §11）。

---

## 6. 决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | 方案 A：适配器持 `Option<PackageHandle>`，save 路由；component io-free | AGENTS.md 关键偏离；component 复用性；C1 brainstorm 确认 |
| 2 | `save_ofd` 按 package 有无自动选手术刀/全量 | spec §6.1；package 有=从文件加载(手术刀保 body)，无=新文档(全量) |
| 3 | modified 委托 `component.is_modified()`，非 on_change 回调 | component 已正确跟踪 doc-dirty（`after_annotation_change`）；委托无 `&mut self` 借用冲突，比 on_change 回调干净 |
| 4 | component 加 `clear_modified()`（save 后复位） | component.modified 是 pub(crate)；save 成功后需复位"未保存"状态 |
| 5 | Ctrl+S 经 `on_save_request` + `Arc<AtomicBool>` flag-poll | EditorApp 非 Send（parley FontContext）+ 借用冲突，回调不能捕获 `&mut app`；AtomicBool flag 是 Send-safe 信号，主循环 poll 后执行 save |
| 6 | 只 Save 覆盖，新文档才弹 SaveAs | 用户选最小 UX；签名 caveat 文档化 |
| 7 | web download 流程不变，仅底层改手术刀 | web `main.ts` 已工作；只换 save_ofd 实现 |

---

## 7. 对 v1 spec 的修订

- **§6.1 EditorComponent**：`save_ofd` 的"package 有->手术刀, 无->全量"路由**实际在适配器层**（EditorApp/WasmEditor），非 component（component io-free 偏离）。spec §6.1 描述的 `EditorComponent::save_ofd` 不存在；改为适配器层 `EditorApp::save_ofd`/`WasmEditor::save_ofd` 路由。
- **§6.6 native-view / §6.7 web-view**：补"适配器持 `package: Option<PackageHandle>`"。
- **§10 已知边界**：modified 标志修复后，view-dirty/doc-dirty 混淆不再；签名 caveat 维持。

---

## 8. 后续 cluster（本 spec 不实现）

- **Cluster 3**：交互式批注 UX（选区手柄/拖拽/创建 UI/右键/光标）。
- **Cluster 4**：次要缺口收尾（ViewEvent 补全/回调/on_warning/错误用例测试）。
