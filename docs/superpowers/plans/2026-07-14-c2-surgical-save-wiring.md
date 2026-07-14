# Cluster 2: 手术刀保存调用链 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 native-view `EditorApp` 和 web-view `WasmEditor` 保留 `PackageHandle`、`save_ofd` 路由手术刀/全量；native-app 接 Save 按钮 + Ctrl+S；修复 `modified` 标志的 view-dirty/doc-dirty 混淆。

**Architecture:** 适配器层持 `Option<PackageHandle>`（component 保持 io-free）；`save_ofd` 按 package 有无选 `io::save_ofd`（手术刀，body 字节保留）/ `write_ofd`（全量）；native Ctrl+S 经 `on_save_request` + `Arc<AtomicBool>` flag-poll（绕 EditorApp 非 Send + 借用冲突）-> `do_save` 写 `current_file`；`EditorApp.is_modified()` 委托 `component.is_modified()`（component 已正确跟踪 doc-dirty），component 加 `clear_modified()` 复位。

**Tech Stack:** Rust 2021, xilem/masonry (native-app), rfd (file dialog), wasm-bindgen (web-view). C1 已就绪（`rofd_io::save_ofd`/`write_ofd`/`PackageHandle`）。

**Spec:** [`docs/superpowers/specs/2026-07-14-c2-surgical-save-wiring-design.md`](../specs/2026-07-14-c2-surgical-save-wiring-design.md)

## Global Constraints

（从 spec §2/AGENTS.md 逐条复制）

- **component 保持 io-free**（AGENTS.md §4.1 偏离）：不在 `EditorComponent` 加 io 调用；`PackageHandle` 只在适配器层（EditorApp/WasmEditor）。
- **手术刀字节保留**（AGENTS.md §4.3）：app 层 `save_ofd`（package 有）后 body `Content.xml` 字节级保留。
- **库不取系统时间**（AGENTS.md §4.4）：无 `Date::now`/`SystemTime`。
- **错误显式分层**（AGENTS.md §4.6）：save 失败提示用户，不静默吞，无裸 unwrap/ignore。
- **commits**：conventional commits，无 attribution 行。单 main 分支直接提交。
- **TDD**：先红后绿，每任务结束 commit。
- **真实样本** `test/ru-yuan-ji-lu.ofd` gitignored，相关测试标 `#[ignore]`。
- **fmt**：baseline 已 clean（C1 chore `7a6a930`）。用 `cargo fmt -- <files>` 格式化只改的文件（不要 `cargo fmt -p <crate>` 或 `--all`，会重格式化全 crate 产生副作用）；stage ONLY 改的文件（`git add <path>`），勿 `git add -A`（docs PDF untracked，勿提交）。

---

## File Structure

| 文件 | 责任 | 任务 |
|---|---|---|
| `crates/component/src/editor_component.rs` | 加 `pub fn clear_modified(&mut self)` | T1 |
| `crates/native-view/src/editor_app.rs` | 加 `package: Option<PackageHandle>`；`load_ofd` 保留；`save_ofd` 路由；去 `self.modified`，`is_modified()` 委托；更新测试 | T2 |
| `crates/web-view/src/wasm_editor.rs` | 加 `package`；`load_ofd` 保留；`save_ofd` 路由（wasm32） | T3 |
| `examples/native-app/src/main.rs` | Save 按钮 + Ctrl+S（`on_save_request` flag-poll）+ `do_save` 写 `current_file` | T4 |
| `crates/native-view/tests/c2_save.rs`（新） | app 层真实样本 `#[ignore]` 集成测试 | T5 |

---

## Task 1: component -- clear_modified()

**Files:**
- Modify: `crates/component/src/editor_component.rs`
- Test: inline

**Interfaces:**
- Consumes: 无
- Produces: `EditorComponent::clear_modified(&mut self)`（复位 `self.modified = false`）。

- [ ] **Step 1: 写失败测试**

在 `crates/component/src/editor_component.rs` 的 `#[cfg(test)] mod tests` 加：

```rust
    #[test]
    fn clear_modified_resets_flag() {
        let mut c = component_with_note(); // 已有 helper，create_annotation 置 modified=true
        assert!(c.is_modified(), "note creation sets modified");
        c.clear_modified();
        assert!(!c.is_modified(), "clear_modified resets");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-component clear_modified`
Expected: FAIL（`clear_modified` 未定义）。

- [ ] **Step 3: 实现 clear_modified**

在 `EditorComponent` impl 里（`is_modified` 附近）加：

```rust
    /// Reset the modified flag (call after a successful save).
    pub fn clear_modified(&mut self) {
        self.modified = false;
    }
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cargo test -p rofd-component`
Expected: PASS（新测试 + 既有 component 测试）。

- [ ] **Step 5: clippy + fmt + commit**

```bash
cargo clippy -p rofd-component --all-targets -- -D warnings
cargo fmt -- crates/component/src/editor_component.rs
git add crates/component/src/editor_component.rs
git commit -m "feat(component): add clear_modified() for post-save reset"
```

---

## Task 2: EditorApp -- package + save 路由 + modified 委托

**Files:**
- Modify: `crates/native-view/src/editor_app.rs`
- Test: inline

**Interfaces:**
- Consumes: `EditorComponent::clear_modified()`（T1）；`rofd_io::{save_ofd, write_ofd, PackageHandle}`（C1）。
- Produces: `EditorApp.package: Option<PackageHandle>`；`save_ofd()` 路由手术刀/全量；`is_modified()` 委托 component。

- [ ] **Step 1: 写失败测试**

在 `crates/native-view/src/editor_app.rs` 的 `#[cfg(test)] mod tests` 加（保留并更新既有测试，见 Step 6）：

```rust
    use rofd_io::zip_util::read_all_entries;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    #[test]
    fn load_ofd_retains_package() {
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&bytes).unwrap();
        assert!(app.package.is_some(), "package retained after load");
    }

    #[test]
    fn save_ofd_with_package_preserves_body_bytes() {
        // 构造一个有 body 的包：write_ofd -> parse -> load -> save_ofd(surgical) -> body Content.xml 字节级保留
        let doc = OfdDocument::default();
        let original = rofd_io::write_ofd(&doc).unwrap();
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.load_ofd(&original).unwrap();
        let saved = app.save_ofd().unwrap();
        let orig_e = read_all_entries(&original).unwrap();
        let save_e = read_all_entries(&saved).unwrap();
        // body Content.xml 字节级相等（surgical 保留）
        for name in orig_e.iter().filter(|(n, _)| n.ends_with("Content.xml")).map(|(n, _)| n.as_str()) {
            let o = orig_e.iter().find(|(n, _)| n == name).unwrap();
            let s = save_e.iter().find(|(n, _)| n == name).unwrap();
            assert_eq!(o.1, s.1, "body {} byte-identical (surgical)", name);
        }
    }

    #[test]
    fn save_ofd_without_package_uses_full_write() {
        // new doc (package=None) -> save_ofd -> write_ofd (full), 非空且可重 parse
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.component.load_document(OfdDocument::default()); // package stays None
        assert!(app.package.is_none());
        let saved = app.save_ofd().unwrap();
        assert!(!saved.is_empty());
        // 可重 parse
        rofd_io::parse_ofd(&saved).expect("full-write output re-parses");
    }

    #[test]
    fn is_modified_delegates_to_component_not_view_changes() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.set_clock("t".into(), 1);
        // scroll/zoom (view changes) 不置 modified
        app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 0.0 });
        assert!(!app.is_modified(), "scroll does not set modified");
        app.handle_event(&ViewEvent::Zoom { factor: 2.0 });
        assert!(!app.is_modified(), "zoom does not set modified");
        // 批注编辑（command pass-through）置 modified
        app.component.create_annotation(
            AnnotationKind::Note, PageId::new("1"),
            AnnotationPayload::Note { rect: Rect{x:0.0,y:0.0,w:10.0,h:10.0}, color: Color::Rgb(0,0,0), content: "x".into(), icon: NoteIcon::Note },
        );
        assert!(app.is_modified(), "annotation edit sets modified");
        // clear_modified 复位
        app.component.clear_modified();
        assert!(!app.is_modified(), "clear_modified resets");
    }
```

（`ViewEvent` 需 import：`use rofd_component::ViewEvent;` 已在文件顶部 `use rofd_component::{EditorComponent, EditorConfig, EventOutcome, ViewEvent};`。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rofd-native-view`
Expected: FAIL（`package` 字段不存在；`save_ofd` 仍 write_ofd；`is_modified` 仍用 self.modified）。

- [ ] **Step 3: 改 EditorApp 结构 + import**

`crates/native-view/src/editor_app.rs` 顶部 import 改：

```rust
use std::path::PathBuf;

use rofd_component::{EditorComponent, EditorConfig, EventOutcome, ViewEvent};
use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle};
use rofd_render::Scene;
```

结构体改（去 `modified`，加 `package`）：

```rust
pub struct EditorApp {
    pub component: EditorComponent,
    pub current_file: Option<PathBuf>,
    pub package: Option<PackageHandle>,
}
```

`new`：

```rust
    pub fn new(config: EditorConfig) -> Self {
        Self {
            component: EditorComponent::new(config),
            current_file: None,
            package: None,
        }
    }
```

- [ ] **Step 4: 改 load_ofd + save_ofd + handle_event + is_modified**

```rust
    pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), String> {
        let report = parse_ofd(bytes).map_err(|e| format!("parse failed: {e}"))?;
        self.package = Some(report.package);
        self.component.load_document(report.document);
        Ok(())
    }

    /// Serialize the current document to .ofd bytes. Surgical save (preserves
    /// unmodelled body) when a package was loaded; full write for new documents.
    pub fn save_ofd(&self) -> Result<Vec<u8>, String> {
        match &self.package {
            Some(pkg) => save_ofd(self.component.document(), pkg),
            None => write_ofd(self.component.document()),
        }
        .map_err(|e| format!("save failed: {e}"))
    }

    pub fn handle_event(&mut self, event: &ViewEvent) -> EventOutcome {
        self.component.handle_event(event)
    }

    pub fn build_scene(&mut self) -> Scene {
        self.component.build_scene()
    }

    pub fn set_size(&mut self, width: f64, height: f64) {
        self.component.handle_event(&ViewEvent::Resize { width, height });
    }

    pub fn document(&self) -> &OfdDocument {
        self.component.document()
    }
    pub fn is_modified(&self) -> bool {
        self.component.is_modified()
    }
    pub fn set_clock(&mut self, author: String, ts: i64) {
        self.component.set_clock(author, ts);
    }
```

（`handle_event` 去掉 `if outcome.needs_repaint { self.modified = true; }`；`is_modified` 委托；`load_ofd` 去 `self.modified = false;`。）

- [ ] **Step 5: 更新既有测试（modified 语义变了）**

`handle_event_sets_modified_on_repaint` -> scroll 现在不置 modified，改名 + 改断言：

```rust
    #[test]
    fn handle_event_scroll_does_not_set_modified() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        let outcome = app.handle_event(&ViewEvent::Scroll { dx: 10.0, dy: 20.0 });
        assert!(outcome.needs_repaint);
        assert!(!app.is_modified(), "scroll is a view change, not a doc change");
    }
```

`load_ofd_resets_modified_flag` -> 改用批注编辑置 modified（scroll 不再置）：

```rust
    #[test]
    fn load_ofd_resets_modified_flag() {
        let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
        app.set_clock("t".into(), 1);
        app.component.create_annotation(
            rofd_dom::AnnotationKind::Note, rofd_dom::PageId::new("1"),
            rofd_dom::AnnotationPayload::Note {
                rect: rofd_dom::Rect{x:0.0,y:0.0,w:1.0,h:1.0}, color: rofd_dom::Color::Rgb(0,0,0),
                content: "x".into(), icon: rofd_dom::NoteIcon::Note,
            },
        );
        assert!(app.is_modified());
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        app.load_ofd(&bytes).unwrap();
        assert!(!app.is_modified(), "load resets modified");
    }
```

`load_ofd_then_document_has_pages` / `save_ofd_round_trips` / `new_app_is_unmodified_with_no_file` 仍应通过（`is_modified` 委托，load/reset 后 false）。若 `save_ofd_round_trips` 的 `app.component.load_document(doc)` 后 `package` 仍 None -> save_ofd 走 write_ofd -> 仍 OK。

- [ ] **Step 6: 跑测试确认绿**

Run: `cargo test -p rofd-native-view`
Expected: PASS（4 新测试 + 4 更新后既有测试）。

- [ ] **Step 7: clippy + fmt + commit**

```bash
cargo clippy -p rofd-native-view --all-targets -- -D warnings
cargo fmt -- crates/native-view/src/editor_app.rs
git add crates/native-view/src/editor_app.rs
git commit -m "feat(native-view): EditorApp retains PackageHandle, routes surgical/full save; modified delegates to component"
```

---

## Task 3: WasmEditor -- package + save 路由（wasm32）

**Files:**
- Modify: `crates/web-view/src/wasm_editor.rs`（`mod wasm_impl`，cfg wasm32）
- Test: 编译验证 + 手动 wasm（native 不能测 wasm32 struct）

**Interfaces:**
- Consumes: `rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle}`（C1）。
- Produces: `WasmEditor.package: Option<PackageHandle>`；`load_ofd` 保留；`save_ofd` 路由。

- [ ] **Step 1: 改 import + struct**

`crates/web-view/src/wasm_editor.rs` 的 `mod wasm_impl` 内 import 改：

```rust
    use rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle};
```

struct 加 `package`：

```rust
    #[wasm_bindgen]
    pub struct WasmEditor {
        component: EditorComponent,
        render_target: WebGpuRenderTarget,
        callbacks: JsCallbacks,
        package: Option<PackageHandle>,
    }
```

`create_wasm_editor` 工厂（在 `crates/web-view/src/lib.rs` 或 wasm_impl 内）初始化 `package: None`（找到构造 WasmEditor 的地方，加 `package: None`）。

- [ ] **Step 2: 改 load_ofd + save_ofd**

找到 `load_ofd`（约 line 252）和 `save_ofd`（约 line 260），改为：

```rust
        /// Load an OFD document from raw `.ofd` package bytes.
        #[wasm_bindgen(js_name = loadOfd)]
        pub fn load_ofd(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            let report = parse_ofd(bytes).map_err(|e| JsValue::from_str(&format!("parse failed: {e}")))?;
            self.package = Some(report.package);
            self.component.load_document(report.document);
            Ok(())
        }

        /// Serialize the current document to OFD package bytes. Surgical save
        /// (preserves unmodelled body) when a package was loaded; full write otherwise.
        #[wasm_bindgen(js_name = saveOfd)]
        pub fn save_ofd(&self) -> Result<Vec<u8>, JsValue> {
            match &self.package {
                Some(pkg) => save_ofd(self.component.document(), pkg),
                None => write_ofd(self.component.document()),
            }
            .map_err(|e| JsValue::from_str(&format!("save failed: {e}")))
        }
```

（确认 `load_ofd`/`save_ofd` 的 `#[wasm_bindgen(js_name = ...)]` 属性与现状一致；若现状无 js_name 则保持现状。）

- [ ] **Step 3: wasm32 编译验证**

Run: `cargo check -p rofd-web-view --target wasm32-unknown-unknown`
Expected: PASS（编译通过，无错误）。

若 `wasm32-unknown-unknown` target 未装：`rustup target add wasm32-unknown-unknown`（一次性）。

- [ ] **Step 4: native 编译验证（wasm_impl cfg-gated，native 不应受影响）**

Run: `cargo check -p rofd-web-view`
Expected: PASS（wasm_impl 是 cfg(wasm32)，native 编译跳过）。

- [ ] **Step 5: clippy（native target，wasm 部分跳过）+ fmt + commit**

```bash
cargo clippy -p rofd-web-view --all-targets -- -D warnings
cargo fmt -- crates/web-view/src/wasm_editor.rs
git add crates/web-view/src/wasm_editor.rs
git commit -m "feat(web-view): WasmEditor retains PackageHandle, routes surgical/full save"
```

- [ ] **Step 6: 手动 wasm 验证（非 CI，记录在 report）**

非阻塞：`cd examples/web-app && npm run build:sdk && npm run dev`，打开页面，加载 `test/ru-yuan-ji-lu.ofd`，加批注，Ctrl+S（或 Save 按钮，若 web-app 有）-> download -> 重开下载的文件 -> 批注保留 + body 一致。在 report 里记录手动验证结果（若环境不便，标 "manual verify deferred"）。

---

## Task 4: native-app -- Save 按钮 + Ctrl+S + do_save

**Files:**
- Modify: `examples/native-app/src/main.rs`
- Test: inline（`do_save` 单元测试，current_file=Some 临时文件）

**Interfaces:**
- Consumes: `EditorApp::{save_ofd, current_file, component.clear_modified}`（T1/T2）；`EditorComponent::on_save_request`（已有）。
- Produces: `do_save(&mut EditorApp)` free function；btn_save + Ctrl+S flag-poll 布线。

- [ ] **Step 1: 写失败测试**

在 `examples/native-app/src/main.rs` 加 `#[cfg(test)] mod tests`（若已有则加）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::OfdDocument;

    #[test]
    fn do_save_writes_to_current_file_and_clears_modified() {
        let mut app = EditorApp::new(rofd_component::EditorConfig::new(Arc::new(vec![])));
        // load 一个包（带 package）
        let bytes = rofd_io::write_ofd(&OfdDocument::default()).unwrap();
        app.load_ofd(&bytes).unwrap();
        // 设 current_file 为临时文件
        let tmp = std::env::temp_dir().join(format!("rofd_c2_test_{}.ofd", std::process::id()));
        app.current_file = Some(tmp.clone());
        // 制造一次批注编辑置 modified
        app.set_clock("t".into(), 1);
        app.component.create_annotation(
            rofd_dom::AnnotationKind::Note, rofd_dom::PageId::new("1"),
            rofd_dom::AnnotationPayload::Note {
                rect: rofd_dom::Rect{x:0.0,y:0.0,w:5.0,h:5.0}, color: rofd_dom::Color::Rgb(0,0,0),
                content: "x".into(), icon: rofd_dom::NoteIcon::Note,
            },
        );
        assert!(app.is_modified());
        do_save(&mut app);
        assert!(tmp.exists(), "file written");
        assert!(!app.is_modified(), "clear_modified after save");
        // 写出的文件可重 parse
        let written = std::fs::read(&tmp).unwrap();
        rofd_io::parse_ofd(&written).expect("written file re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p native-app do_save`
Expected: FAIL（`do_save` 未定义）。

- [ ] **Step 3: 实现 do_save**

在 `examples/native-app/src/main.rs`（`app_logic` 之前或之后，模块级 free fn）加：

```rust
/// Save the editor's document to `current_file` (overwrite), or prompt a
/// Save As dialog if no file is set. Resets modified on success. Errors
/// print to stderr (v1: no status-bar UI wired).
fn do_save(editor: &mut EditorApp) {
    let bytes = match editor.save_ofd() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] save failed: {e}");
            return;
        }
    };
    if let Some(path) = editor.current_file.clone() {
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("[ERROR] write {}: {}", path.display(), e);
            return;
        }
    } else {
        // New document, no current_file -> Save As dialog.
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("OFD document", &["ofd"])
            .set_file_name("untitled.ofd")
            .save_file()
        {
            if let Err(e) = std::fs::write(&path, &bytes) {
                eprintln!("[ERROR] write {}: {}", path.display(), e);
                return;
            }
            editor.current_file = Some(path);
        } else {
            return; // 用户取消
        }
    }
    editor.component.clear_modified();
}
```

- [ ] **Step 4: 加 Save 按钮 + Ctrl+S flag-poll 布线**

`app_logic` 里 btn_open 旁加 btn_save：

```rust
    let btn_save = text_button("Save", |app: &mut AppState| {
        let mut editor = app.editor.lock().unwrap();
        do_save(&mut editor);
    })
    .padding(BTN_PAD)
    .border_width(0.0)
    .corner_radius(2.0);
```

`menu_bar` 改成 `flex_row((btn_open, btn_save))`：

```rust
    let menu_bar =
        sized_box(flex_row((btn_open, btn_save)).gap(xilem::masonry::layout::Length::const_px(2.0)))
            .padding(Padding::from_vh(2.0, 4.0))
            .background_color(Color::from_rgb8(240, 240, 240));
```

Ctrl+S flag-poll：`NativeApp` 加 `save_requested: Arc<AtomicBool>` 字段。顶部 import 加 `use std::sync::atomic::{AtomicBool, Ordering};`。

`NativeApp` struct 加字段：

```rust
struct NativeApp {
    masonry_state: MasonryState<'static>,
    app_driver: Box<dyn AppDriver>,
    editor: SharedEditor,
    bridge: WinitEventBridge,
    canvas_widget_id: SharedCanvasId,
    save_requested: Arc<AtomicBool>,
}
```

`window_event` 里，`editor.handle_event` 后加 flag-poll（在 `if outcome.needs_repaint { ... }` 之后）：

```rust
        if let Some(view_event) = self.bridge.translate(&ev) {
            let mut editor = self.editor.lock().unwrap();
            let outcome = editor.handle_event(&view_event);
            drop(editor);
            if outcome.needs_repaint {
                self.request_canvas_render();
            }
        }
        // Ctrl+S routed through component's on_save_request -> flag -> poll here.
        if self.save_requested.swap(false, Ordering::SeqCst) {
            let mut editor = self.editor.lock().unwrap();
            do_save(&mut editor);
        }
```

`main()` 里，构造 `save_requested` + 注册 `on_save_request` 回调 + 传入 NativeApp。在 `let editor = Arc::new(Mutex::new(editor));` 之后、构造 `app` 之前加：

```rust
    let save_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    {
        let flag = save_requested.clone();
        editor.lock().unwrap().component.on_save_request(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }
```

`NativeApp { ... }` 构造加 `save_requested: save_requested.clone()`（或 move，看后续是否再用；main 末尾不再用，可 move）：

```rust
    let mut app = NativeApp {
        masonry_state,
        app_driver: Box::new(driver),
        editor,
        bridge: WinitEventBridge::new(),
        canvas_widget_id,
        save_requested,
    };
```

- [ ] **Step 5: 跑测试确认绿**

Run: `cargo test -p native-app`
Expected: PASS（do_save 测试 + 既有 native-app 测试）。

- [ ] **Step 6: clippy + fmt + commit**

```bash
cargo clippy -p native-app --all-targets -- -D warnings
cargo fmt -- examples/native-app/src/main.rs
git add examples/native-app/src/main.rs
git commit -m "feat(native-app): Save button + Ctrl+S (on_save_request flag-poll) write current_file"
```

---

## Task 5: app 层真实样本集成测试 + 全量绿

**Files:**
- Create: `crates/native-view/tests/c2_save.rs`
- Test: `#[ignore]` 集成 + 全量验证

**Interfaces:**
- Consumes: `EditorApp::{load_ofd, save_ofd}`（T2）；真实样本 `test/ru-yuan-ji-lu.ofd`。
- Produces: app 层端到端 body 字节保留验证（区别于 C1 io 层 `real_sample.rs`）。

- [ ] **Step 1: 写真实样本集成测试**

`crates/native-view/tests/c2_save.rs`：

```rust
use std::sync::Arc;

use rofd_component::EditorConfig;
use rofd_io::zip_util::read_all_entries;
use rofd_native_view::EditorApp;

#[test]
#[ignore = "needs local test/ru-yuan-ji-lu.ofd (gitignored)"]
fn app_layer_surgical_save_preserves_body() {
    let bytes = std::fs::read("../../test/ru-yuan-ji-lu.ofd").expect("test sample present");
    let mut app = EditorApp::new(EditorConfig::new(Arc::new(vec![])));
    app.load_ofd(&bytes).expect("load");
    assert!(app.package.is_some(), "package retained");
    let saved = app.save_ofd().expect("save");
    // body Content.xml 字节级保留（surgical at app layer）
    let orig_e = read_all_entries(&bytes).unwrap();
    let save_e = read_all_entries(&saved).unwrap();
    for name in orig_e.iter().filter(|(n, _)| n.ends_with("Content.xml")).map(|(n, _)| n.as_str()) {
        let o = orig_e.iter().find(|(n, _)| n == name).unwrap();
        let s = save_e.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(o.1, s.1, "body {} byte-identical via app-layer surgical save", name);
    }
}
```

- [ ] **Step 2: 跑 #[ignore] 测试（本地）**

Run: `cargo test -p rofd-native-view --test c2_save -- --ignored`
Expected: PASS（真实样本 body 字节级保留）。

- [ ] **Step 3: 全量验证**

Run: `cargo test -p rofd-dom -p rofd-io -p rofd-render -p rofd-editor -p rofd-component -p rofd-native-view -p native-app`
Expected: PASS（per-crate；`--workspace` 撞 Windows linker OOM 时用 per-crate）。

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean。

Run: `cargo fmt --all -- --check`
Expected: clean（baseline 已 clean）。

- [ ] **Step 4: Commit**

```bash
git add crates/native-view/tests/c2_save.rs
git commit -m "test(native-view): app-layer surgical save #[ignore] integration test"
```

---

## Definition of Done

- `EditorApp` / `WasmEditor` 持 `Option<PackageHandle>`；`load_ofd` 保留；`save_ofd` 按 package 有无路由手术刀/全量。
- native-app：Save 按钮 + Ctrl+S（`on_save_request` flag-poll）-> `do_save` 写 `current_file`（覆盖；None 时 rfd SaveAs）-> `clear_modified`。
- `EditorApp.is_modified()` 委托 `component.is_modified()`（scroll/zoom 不再误报）；`component.clear_modified()` 复位。
- app 层 body 字节保留（不变量 4.3 在 app 层兑现）：真实样本 `#[ignore]` 通过。
- `cargo test`（per-crate）绿；clippy clean；fmt clean。

## 后续（非本 plan）

Cluster 3（交互式批注 UX）、Cluster 4（次要缺口收尾）。
