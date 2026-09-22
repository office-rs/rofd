# A3 宿主重写（纯 Xilem::new_simple）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `crates/native-app` 重写为纯 `Xilem::new_simple` 宿主：宿主只持有 plain-data `AppState`，工具栏/右键 overlay/文件对话框是宿主仅有的 UI 策略；与编辑器的全部交互走 `OfdCommandQueue` 命令队列。完成后删除 winit 旧桥（`EditorApp`/`WinitEventBridge`）与 native-view 的旧依赖/旧测试，迁移三个旧测试到 render/component，并删除 A1 的 `build_scene` 临时垫片。

**Architecture:** 三个任务顺序推进，每步独立可编译：(1) 新增库目标暴露 `host::document_io`（旧二进制不动）；(2) main.rs 全文重写为 new_simple 宿主，c2_save 迁移为宿主层外部测试；(3) 旧桥与旧测试迁移删除、两个 Cargo.toml 收敛到终态。事件排序事实（已核实 component 源码 L935-964）：rofd 右键**只**触发 `on_context_menu`、不改光标，因此 `on_cursor_change` 关闭菜单不会误伤刚打开的 overlay。

**Tech Stack:** `Xilem::new_simple` + `WindowOptions`/`EventLoop`（masonry_winit 内部驱动，run_in 自包含 winit 装配）；rofd-io 升为宿主正常依赖。同包 lib+bin 同名（已在本工具链实测可行）。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §4（A3 全部小节）、§3.7（旧测试迁移）、§6.4（ignored 测试命令）。

## Global Constraints

- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- 手术刀保存语义不变：`save_ofd(doc, pkg)` 路径必须保留，未触碰条目字节一致测试始终绿色。
- 写入失败不改动内存状态；错误/警告显式呈现（至少 stderr），不静默吞。
- 宿主默认装配保持现状：空默认字体（系统字体回退）、`set_clock("rofd".into(), 0)`、默认 UTC tooltip formatter。
- 代码注释/变量/文件名不得出现 "WPS" 字样。
- 无裸 `unwrap`（锁获取与测试代码沿用既有约定）。
- **标题决策（对 spec §4.6 末句的明确收敛）**：本 rev 的 xilem 无 `set_title` API（全仓 grep 零命中），动态标题无法经 `new_simple` 表达；旧宿主标题本来也是静态的，非回归。窗口标题固定 `"rofd - OFD Editor"`。

## File Structure

| 文件 | 动作 | 职责 |
| --- | --- | --- |
| `crates/native-app/src/lib.rs` | 新建 | 库目标：`pub mod host` |
| `crates/native-app/src/host/mod.rs` | 新建 | host 模块根 |
| `crates/native-app/src/host/document_io.rs` | 新建 | `LoadedOfd` + `load_ofd`/`save_ofd`（原子写） |
| `crates/native-app/Cargo.toml` | 改 | rofd-io 升正常依赖（Task 1） |
| `crates/native-app/src/main.rs` | 改（Task 2 全文重写） | AppState/app_logic/new_simple |
| `crates/native-app/tests/c2_save.rs` | 新建 | 迁移的宿主层真实样例测试（#[ignore]） |
| `crates/native-view/tests/hit_coverage.rs` | git mv → render/tests | 内容不变 |
| `crates/native-view/tests/sample_ctm_hit.rs` | git mv → render/tests | 内容不变 |
| `crates/native-view/tests/sample_drag_select.rs` | git mv → component/tests | build_scene 调用改为 update_scene/scene |
| `crates/component/Cargo.toml` | 改 | dev 增 rofd-io、kurbo |
| `crates/native-view/src/editor_app.rs` | 删 | 旧桥 |
| `crates/native-view/src/winit_bridge.rs` | 删 | 旧桥 |
| `crates/native-view/tests/c2_save.rs` | 删 | 已迁移 |
| `crates/native-view/src/lib.rs` | 改 | 终态导出 |
| `crates/native-view/Cargo.toml` | 改 | 终态：component + xilem |
| `crates/component/src/editor_component.rs` | 改 | 删除 build_scene 临时垫片 |
| `Cargo.toml` | 改 | 删除 winit/masonry_winit workspace 键 |

---

### Task 1: host::document_io（新增库目标）

**Files:**
- Create: `crates/native-app/src/lib.rs`
- Create: `crates/native-app/src/host/mod.rs`
- Create: `crates/native-app/src/host/document_io.rs`
- Modify: `crates/native-app/Cargo.toml`

**Interfaces:**
- Consumes: `rofd_io::{parse_ofd, save_ofd, write_ofd, PackageHandle}`、`rofd_dom::{OfdDocument, OfdWarning}`。
- Produces（`native_app::host::document_io`）：
  - `pub struct LoadedOfd { pub document: OfdDocument, pub package: Option<PackageHandle>, pub warnings: Vec<OfdWarning> }`
  - `pub fn load_ofd(path: &Path) -> Result<LoadedOfd, String>`
  - `pub fn save_ofd(document: &OfdDocument, package: Option<&PackageHandle>, path: &Path) -> Result<(), String>`

- [ ] **Step 1: Cargo.toml 把 rofd-io 升为正常依赖**

把 `crates/native-app/Cargo.toml` 全文替换为：

```toml
[package]
name = "native-app"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-native-view = { workspace = true }
rofd-component = { workspace = true }
rofd-dom = { workspace = true }
rofd-io = { workspace = true }
winit = { workspace = true }
xilem = { workspace = true }
masonry_winit = { workspace = true }
rfd = { workspace = true }
```

说明：旧 `[dev-dependencies] rofd-io` 删除——正常依赖同样服务测试。winit/masonry_winit 保留（旧 main.rs Task 2 才重写）。

- [ ] **Step 2: 新建 src/lib.rs**

`crates/native-app/src/lib.rs` 全文：

```rust
//! native-app library surface.
//!
//! The binary (`main.rs`) owns AppState/app_logic; the library exposes the
//! host-side helpers (`host`) so external integration tests can exercise
//! file load/save without duplicating the routing.

pub mod host;
```

- [ ] **Step 3: 新建 src/host/mod.rs**

`crates/native-app/src/host/mod.rs` 全文：

```rust
//! Host-side helpers for the native app.

pub mod document_io;
```

- [ ] **Step 4: 新建 src/host/document_io.rs**

`crates/native-app/src/host/document_io.rs` 全文：

```rust
//! Plain file I/O for the native host.
//!
//! The view layer owns file dialogs and path state; host commands snapshot
//! the component's document. Loading returns the parsed document together
//! with the `PackageHandle` surgical save needs. Commands cannot touch the
//! OS clipboard; that path stays in the widget itself.

use std::path::{Path, PathBuf};

use rofd_dom::OfdDocument;
use rofd_io::{parse_ofd, save_ofd as io_save_ofd, write_ofd, PackageHandle};

/// Result of loading an `.ofd` file from disk.
pub struct LoadedOfd {
    /// Parsed document model.
    pub document: OfdDocument,
    /// Original package skeleton for surgical save. Always Some for a
    /// successfully parsed file; Option keeps the host's "new document"
    /// state uniform.
    pub package: Option<PackageHandle>,
    /// Degraded-load warnings (templates/JBIG2/font substitution, ...).
    pub warnings: Vec<rofd_dom::OfdWarning>,
}

/// Read and parse an `.ofd` file.
pub fn load_ofd(path: &Path) -> Result<LoadedOfd, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let report = parse_ofd(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(LoadedOfd {
        document: report.document,
        package: Some(report.package),
        warnings: report.warnings,
    })
}

/// Serialize the document and write it back to `path`.
///
/// With a package handle: surgical save (untouched entries byte-preserved).
/// Without: full write for a new document. The write itself is atomic
/// (sibling temp file + rename).
pub fn save_ofd(
    document: &OfdDocument,
    package: Option<&PackageHandle>,
    path: &Path,
) -> Result<(), String> {
    let bytes = match package {
        Some(pkg) => io_save_ofd(document, pkg),
        None => write_ofd(document),
    }
    .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    write_atomic(path, &bytes)
}

/// Write bytes to a sibling `.ofd.tmp` file, then rename over destination.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp: PathBuf = path.to_path_buf();
    tmp.set_extension("ofd.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_and_save_round_trip() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_{}.ofd", std::process::id()));
        let bytes = write_ofd(&OfdDocument::default()).expect("write_ofd seeds a package");
        std::fs::write(&tmp, bytes).expect("seed file");

        let loaded = load_ofd(&tmp).expect("load_ofd");
        assert!(loaded.package.is_some(), "parsed file retains a package");
        assert!(loaded.warnings.is_empty(), "minimal doc has no warnings");

        save_ofd(&loaded.document, loaded.package.as_ref(), &tmp).expect("save_ofd");

        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("written file re-parses");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_missing_path_errors_with_read_context() {
        let err = load_ofd(Path::new("no-such-rofd-file.ofd")).unwrap_err();
        assert!(err.contains("read"), "error names the failing stage: {err}");
    }

    #[test]
    fn save_without_package_full_writes() {
        let tmp = std::env::temp_dir().join(format!("rofd_doc_io_full_{}.ofd", std::process::id()));
        save_ofd(&OfdDocument::default(), None, &tmp).expect("full write");
        let written = std::fs::read(&tmp).expect("file written");
        parse_ofd(&written).expect("full-write output re-parses");
        let _ = std::fs::remove_file(&tmp);
    }
}
```

- [ ] **Step 5: 验证与提交**

```bash
cargo test -p native-app
cargo clippy -p native-app --all-targets -- -D warnings
cargo fmt -p native-app
git add crates/native-app/Cargo.toml crates/native-app/src/lib.rs crates/native-app/src/host
git commit -m "feat(native-app): host document_io——load_ofd/save_ofd 落盘路由"
```

Expected: 3 个新库测试 + 旧 main 内联测试全绿；旧二进制行为不变。

---

### Task 2: main.rs 重写为纯 new_simple + c2_save 迁移

**Files:**
- Modify: `crates/native-app/src/main.rs`（全文替换）
- Modify: `crates/native-app/Cargo.toml`（删除 winit/masonry_winit——新 main 不再直接使用）
- Create: `crates/native-app/tests/c2_save.rs`

**Interfaces:**
- Consumes: Task 1 的 `native_app::host::document_io`；A2 的 `ofd_with_config`/`command_queue`/`OfdCommandQueue`/`OfdContextMenu`；组件公共方法（new_document/load_document/set_tool/apply_markup/delete_annotation/set_clock/set_tooltip_formatter/document/clear_modified）。
- Produces: `native-app` 二进制（new_simple 自运行）。

- [ ] **Step 1: main.rs 全文重写**

把 `crates/native-app/src/main.rs` 全文替换为：

```rust
//! rofd native host — pure xilem (`Xilem::new_simple`).
//!
//! The OFD editor lives in an `OfdWidget` (rofd-native-view; renamed
//! rofd-xilem-view in transform B) embedded via `ofd_with_config`. This
//! host owns only UI policy: the toolbar, the right-click context-menu
//! overlay, and file dialogs. Every editor interaction goes through the
//! command queue — buttons push closures run against the component at the
//! next rebuild — and component callbacks arrive as view handlers. There
//! is no winit layer, no shared `Arc<Mutex<_>>` editor, no MessageProxy
//! wake task, and no manual MasonryState/AppDriver.

use std::path::PathBuf;
use std::sync::Arc;

use rfd::FileDialog;
use rofd_component::{ContextTarget, CreateKind, EditorConfig, Tool};
use rofd_dom::{AnnotationId, AnnotationKind, ShapeKind};
use rofd_io::PackageHandle;
use rofd_native_view::{command_queue, ofd_with_config, OfdCommandQueue, OfdContextMenu};
use xilem::kurbo::Vec2;
use xilem::masonry::layout::{Length, UnitPoint};
use xilem::masonry::peniko::Color;
use xilem::style::Padding;
use xilem::view::{
    flex_col, flex_row, sized_box, text_button, transformed, zstack, FlexExt,
};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

use native_app::host;

const BTN_PAD: Padding = Padding::from_vh(Length::ZERO, Length::const_px(6.0));

/// Push a host command onto the host→widget channel.
fn push(
    app: &mut AppState,
    f: impl Fn(&mut rofd_component::EditorComponent) + Send + Sync + 'static,
) {
    app.commands.lock().unwrap().push(Arc::new(f));
}

/// Toolbar tool button: sets the component's active tool.
fn tool_button(label: &str, tool: Tool) -> impl WidgetView<AppState> + use<> {
    text_button(label, move |app: &mut AppState| {
        push(app, move |c| c.set_tool(tool.clone()));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0))
}

/// Markup button: an ACTION over the current body-text selection, not a
/// tool. Disabled without a selection.
fn markup_button(label: &str, kind: AnnotationKind, disabled: bool) -> impl WidgetView<AppState> + use<> {
    text_button(label, move |app: &mut AppState| {
        push(app, move |c| {
            c.apply_markup(kind.clone());
        });
    })
    .disabled(disabled)
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0))
}

/// Open right-click overlay state, in ofd-widget-local (= zstack-local)
/// coordinates.
#[derive(Debug, Clone)]
struct ContextMenuState {
    x: f64,
    y: f64,
    id: AnnotationId,
}

/// Combined host state — plain data. Every mutation flows through xilem
/// (button handlers, ofd-view handlers), so no locks guard the state.
struct AppState {
    /// Host→widget command channel, drained on every rebuild.
    commands: OfdCommandQueue,
    /// Path of the loaded document, if any.
    file: Option<PathBuf>,
    /// Package skeleton for surgical save (only ever comes from parsing).
    package: Option<PackageHandle>,
    /// Unsaved changes mirror (on_change sets it).
    modified: bool,
    /// Body-text selection mirror; gates the markup buttons.
    has_selection: bool,
    /// Warnings collected from the last load/operation.
    warnings: Vec<rofd_dom::OfdWarning>,
    /// Open context-menu overlay, if any.
    context_menu: Option<ContextMenuState>,
}

impl AppState {
    fn new() -> Self {
        let mut state = Self {
            commands: command_queue(),
            file: None,
            package: None,
            modified: false,
            has_selection: false,
            warnings: Vec::new(),
            context_menu: None,
        };
        // Default assembly (AGENTS §4.9): clock + UTC tooltip formatter,
        // zero extra host code. Empty default font: system fallback.
        push(&mut state, |c| {
            c.set_clock("rofd".into(), 0);
            c.set_tooltip_formatter(|ann| rofd_component::default_tooltip_lines(ann, 0));
        });
        state
    }
}

// --- File operations ---

fn do_new(app: &mut AppState) {
    push(app, |c| c.new_document());
    app.file = None;
    app.package = None;
    app.modified = false;
    app.context_menu = None;
}

fn do_open(app: &mut AppState) {
    let Some(path) = FileDialog::new()
        .add_filter("OFD document", &["ofd"])
        .pick_file()
    else {
        return;
    };
    match host::document_io::load_ofd(&path) {
        Ok(loaded) => {
            let document = loaded.document.clone();
            push(app, move |c| c.load_document(document.clone()));
            app.package = Some(loaded.package);
            app.file = Some(path);
            app.modified = false;
            app.warnings = loaded.warnings;
            for warning in &app.warnings {
                eprintln!("[warning] {warning:?}");
            }
        }
        Err(e) => eprintln!("[ERROR] {e}"),
    }
}

/// Save to `path`: the snapshot must run at command time against the live
/// component; the package rides along (Arc-backed, cheap to clone).
fn save_to(app: &mut AppState, path: PathBuf) {
    let package = app.package.clone();
    push(app, move |c| {
        let document = c.document().clone();
        if let Err(e) = host::document_io::save_ofd(&document, package.as_ref(), &path) {
            eprintln!("[ERROR] {e}");
        }
    });
    app.modified = false;
}

fn do_save(app: &mut AppState) {
    if let Some(path) = app.file.clone() {
        save_to(app, path);
    } else {
        do_save_as(app);
    }
}

fn do_save_as(app: &mut AppState) {
    let Some(path) = FileDialog::new()
        .add_filter("OFD document", &["ofd"])
        .set_file_name("untitled.ofd")
        .save_file()
    else {
        return;
    };
    save_to(app, path.clone());
    app.file = Some(path);
    // Spec §4.4: Save As does not mint a PackageHandle; saves stay
    // full-write until the file is opened again.
}

/// Load the command-line path argument if present.
fn maybe_load_cli_arg(app: &mut AppState) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return;
    }
    let path = PathBuf::from(&args[1]);
    match host::document_io::load_ofd(&path) {
        Ok(loaded) => {
            let document = loaded.document.clone();
            push(app, move |c| c.load_document(document.clone()));
            app.package = Some(loaded.package);
            app.file = Some(path);
            app.warnings = loaded.warnings;
        }
        Err(e) => eprintln!("failed to load {}: {e}", args[1]),
    }
}

/// Open the overlay only for an annotation target. Page/Empty: nothing to
/// offer (old host only logged these), so no overlay.
fn open_context_menu(app: &mut AppState, event: OfdContextMenu) {
    let (pos, target) = event;
    if let ContextTarget::Annotation(id) = target {
        app.context_menu = Some(ContextMenuState {
            x: pos.0,
            y: pos.1,
            id,
        });
    }
}

fn app_logic(app: &mut AppState) -> impl WidgetView<AppState> + use<> {
    // --- file row ---
    let btn_new = text_button("新建", |app: &mut AppState| do_new(app))
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0));
    let btn_open = text_button("打开", |app: &mut AppState| do_open(app))
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0));
    let btn_save = text_button("保存", |app: &mut AppState| do_save(app))
        .disabled(!app.modified)
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0));
    let file_row = flex_row((btn_new, btn_open, btn_save)).gap(Length::const_px(2.0));

    // --- tool row (same grouping as the old host) ---
    let btn_hand = tool_button("手型", Tool::Hand);
    let btn_text = tool_button("文本", Tool::Text);
    let group_tools = flex_row((btn_hand, btn_text)).gap(Length::const_px(2.0));

    let btn_highlight = markup_button("高亮", AnnotationKind::Highlight, !app.has_selection);
    let btn_underline = markup_button("下划线", AnnotationKind::Underline, !app.has_selection);
    let btn_strikeout = markup_button("删除线", AnnotationKind::Strikeout, !app.has_selection);
    let btn_squiggly = markup_button("波浪线", AnnotationKind::Squiggly, !app.has_selection);
    let btn_freehand = tool_button("手写", Tool::Create(CreateKind::Freehand));
    let btn_rect = tool_button("矩形", Tool::Create(CreateKind::Shape(ShapeKind::Rect)));

    let tool_row = flex_row((
        group_tools,
        btn_highlight,
        btn_underline,
        btn_strikeout,
        btn_squiggly,
        btn_freehand,
        btn_rect,
    ))
    .gap(Length::const_px(8.0));

    let menu_bar = sized_box(flex_col((file_row, tool_row)))
        .padding(Padding::from_vh(Length::const_px(2.0), Length::const_px(4.0)))
        .background_color(Color::from_rgb8(240, 240, 240));

    // --- editor: ofd view with the component callback surface mapped ---
    let editor = ofd_with_config(
        app.commands.clone(),
        EditorConfig::new(Arc::new(vec![])),
    )
    .on_change(|app: &mut AppState| app.modified = true)
    .on_text_selection_change(|app, sel| app.has_selection = sel.is_some())
    .on_save_request(|app| do_save(app))
    // Any cursor movement dismisses the popup. In rofd, right-click does
    // NOT fire on_cursor_change (only on_context_menu, verified in the
    // component), so opening isn't clobbered.
    .on_cursor_change(|app, _cursor| app.context_menu = None)
    .on_context_menu(|app, event| open_context_menu(app, event))
    .on_warnings(|app, warnings| {
        for warning in &warnings {
            eprintln!("[warning] {warning:?}");
        }
        app.warnings = warnings;
    });

    // --- context-menu overlay ---
    let menu_overlay = app.context_menu.as_ref().map(|menu| {
        let id = menu.id.clone();
        let item = text_button("删除批注", move |app: &mut AppState| {
            // Selecting the menu item is itself the confirm gesture; no
            // second Yes/No dialog (rword parity).
            let id = id.clone();
            push(app, move |c| c.delete_annotation(&id));
            app.context_menu = None;
        })
        .padding(Padding::from_vh(Length::const_px(1.0), Length::const_px(8.0)))
        .border_width(Length::ZERO)
        .corner_radius(Length::ZERO);
        let panel = sized_box(item)
            .fixed_width(Length::const_px(120.0))
            .background_color(Color::WHITE)
            .border_color(Color::from_rgb8(180, 180, 180))
            .border_width(Length::const_px(1.0))
            .corner_radius(Length::const_px(2.0))
            .padding(Padding::from_vh(Length::const_px(2.0), Length::ZERO));
        transformed(panel).translate(Vec2::new(menu.x, menu.y))
    });

    let editor_area = zstack((editor, menu_overlay)).alignment(UnitPoint::TOP_LEFT);

    flex_col((menu_bar, editor_area.flex(1.0)))
}

fn main() -> Result<(), xilem::winit::error::EventLoopError> {
    let mut app_state = AppState::new();
    maybe_load_cli_arg(&mut app_state);
    Xilem::new_simple(
        app_state,
        app_logic,
        WindowOptions::new("rofd - OFD Editor"),
    )
    .run_in(EventLoop::with_user_event())
}
```

- [ ] **Step 2: Cargo.toml 删除 winit/masonry_winit**

新 main.rs 只直接使用 xilem（`EventLoop` 经 xilem re-export）；winit 由 masonry_winit 在内部驱动。把 `crates/native-app/Cargo.toml` 全文替换为：

```toml
[package]
name = "native-app"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-native-view = { workspace = true }
rofd-component = { workspace = true }
rofd-dom = { workspace = true }
rofd-io = { workspace = true }
xilem = { workspace = true }
rfd = { workspace = true }
```

- [ ] **Step 3: 新建 tests/c2_save.rs（迁移自 native-view）**

`crates/native-app/tests/c2_save.rs` 全文：

```rust
//! Host-layer real-sample integration test for surgical save.
//!
//! Migrated from rofd-native-view after the winit bridge removal; the
//! binary crate's helpers are reached through the `native_app` library
//! surface. Exercises the full host path —
//! `document_io::load_ofd` (retains PackageHandle) →
//! `document_io::save_ofd` (routes to surgical save for a package) — and
//! asserts invariant 4.3 (body `Content.xml` byte-identical) at the host
//! layer.
//!
//! Marked `#[ignore]`: `test/ru-yuan-ji-lu.ofd` is gitignored (not in CI).
//! Run locally:
//! `cargo test -p native-app --test c2_save -- --ignored`.

use std::path::Path;

use native_app::host::document_io::{load_ofd, save_ofd};
use rofd_io::zip_util::read_all_entries;

#[test]
#[ignore = "requires the real OFD at ../../test/ru-yuan-ji-lu.ofd"]
fn host_layer_surgical_save_preserves_body() {
    // Integration tests run with the package dir as CWD; the workspace
    // root sample is at ../../test (same convention as the io/render tests).
    let source = Path::new("../../test/ru-yuan-ji-lu.ofd");
    let bytes = std::fs::read(source).expect("test sample present");

    let loaded = load_ofd(source).expect("load_ofd succeeds on real sample");
    assert!(loaded.package.is_some(), "package retained after load_ofd");

    // Save to a temp destination: keep the source untouched.
    let target = std::env::temp_dir().join(format!("rofd_c2_{}.ofd", std::process::id()));
    save_ofd(&loaded.document, loaded.package.as_ref(), &target)
        .expect("save_ofd succeeds on real sample");
    let saved = std::fs::read(&target).expect("saved file present");

    // Body Content.xml entries byte-identical before/after (invariant 4.3
    // at the host layer — surgical save preserves unmodelled body).
    let orig_entries = read_all_entries(&bytes).expect("read original entries");
    let saved_entries = read_all_entries(&saved).expect("read saved entries");

    let body_names: Vec<&str> = orig_entries
        .iter()
        .filter(|(name, _)| name.ends_with("Content.xml"))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        !body_names.is_empty(),
        "real sample has at least one body Content.xml entry"
    );

    for name in body_names {
        let orig = orig_entries
            .iter()
            .find(|(n, _)| n == name)
            .expect("orig entry exists");
        let saved_entry = saved_entries
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("saved body entry {name} missing"));
        assert_eq!(
            orig.1, saved_entry.1,
            "body {name} byte-identical via host-layer surgical save"
        );
    }
    let _ = std::fs::remove_file(&target);
}
```

- [ ] **Step 4: 验证与提交**

```bash
cargo build -p native-app
cargo test -p native-app
cargo clippy -p native-app --all-targets -- -D warnings
cargo fmt -p native-app
git add crates/native-app/Cargo.toml crates/native-app/src/main.rs crates/native-app/tests/c2_save.rs
git commit -m "feat(native-app): 纯 Xilem::new_simple 宿主（命令队列 + 右键 overlay）"
```

Expected: 库测试 3 个 + 二进制正常构建；旧的内联 `do_save_writes_to_current_file…` 随 main.rs 替换移除，其覆盖由 document_io 测试 + c2_save 承接。此时旧桥文件仍在 native-view 编译（下一任务删除）。

---

### Task 3: 删除旧桥、迁移旧测试、清单收敛

**Files:**
- Move: `crates/native-view/tests/hit_coverage.rs` → `crates/render/tests/hit_coverage.rs`
- Move: `crates/native-view/tests/sample_ctm_hit.rs` → `crates/render/tests/sample_ctm_hit.rs`
- Move: `crates/native-view/tests/sample_drag_select.rs` → `crates/component/tests/sample_drag_select.rs`
- Modify: `crates/component/tests/sample_drag_select.rs`（build_scene → update_scene/scene）
- Modify: `crates/component/Cargo.toml`（dev 增 rofd-io、kurbo）
- Delete: `crates/native-view/src/editor_app.rs`
- Delete: `crates/native-view/src/winit_bridge.rs`
- Delete: `crates/native-view/tests/c2_save.rs`
- Modify: `crates/native-view/src/lib.rs`（终态）
- Modify: `crates/native-view/Cargo.toml`（终态）
- Modify: `crates/component/src/editor_component.rs`（删除 build_scene 垫片）
- Modify: `Cargo.toml`（删除 winit/masonry_winit workspace 键）

**Interfaces:**
- Consumes: A1 Task 6 的公共 `update_scene()`/`scene()`。
- Produces: transform A 完成态——native-view 仅依赖 component + xilem。

- [ ] **Step 1: 迁移两个 render 测试（内容不变）**

```bash
git mv crates/native-view/tests/hit_coverage.rs crates/render/tests/hit_coverage.rs
git mv crates/native-view/tests/sample_ctm_hit.rs crates/render/tests/sample_ctm_hit.rs
```

说明：两个文件只引用 `rofd_render`/`rofd_io`/`rofd_dom`/`kurbo`，全部在 render crate 可达（dev rofd-io 已有）；`../../test` 路径从 crates/render 同样指向仓库根 test/，内容零修改。

- [ ] **Step 2: 迁移 sample_drag_select 到 component**

```bash
git mv crates/native-view/tests/sample_drag_select.rs crates/component/tests/sample_drag_select.rs
```

- [ ] **Step 3: 修改迁移后的 sample_drag_select（垫片 → 公共缓存 API）**

把 `crates/component/tests/sample_drag_select.rs` 末尾的：

```rust
    // The scene must contain the selection overlay: clearing the selection
    // must shrink the command list.
    let with_sel = c.build_scene().commands().len();
    c.set_tool(Tool::Text); // clears text_selection
    let without_sel = c.build_scene().commands().len();
```

替换为：

```rust
    // The scene must contain the selection overlay: clearing the selection
    // must shrink the command list. Selection changes mark the cache dirty;
    // update_scene recomposes, scene() is the permanent public API the
    // native widget paints from.
    c.update_scene();
    let with_sel = c.scene().commands().len();
    c.set_tool(Tool::Text); // clears text_selection
    c.update_scene();
    let without_sel = c.scene().commands().len();
```

- [ ] **Step 4: component Cargo.toml 增加 dev 依赖**

把 `crates/component/Cargo.toml` 全文替换为：

```toml
[package]
name = "rofd-component"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-dom = { workspace = true }
rofd-render = { workspace = true }
rofd-editor = { workspace = true }

# Test-only: inline scene-structure assertions (AGENTS §6) use imaging's
# Command/Draw record types, the same idiom as rofd-render's tests. Not part of
# the library's dependency surface (AGENTS §4.1/§4.9).
[dev-dependencies]
imaging = { workspace = true }
# Migrated sample_drag_select parses the real fixture and builds points.
rofd-io = { workspace = true }
kurbo = { workspace = true }
```

- [ ] **Step 5: 删除旧桥源文件与旧 c2_save**

```bash
git rm crates/native-view/src/editor_app.rs crates/native-view/src/winit_bridge.rs \
       crates/native-view/tests/c2_save.rs
```

- [ ] **Step 6: native-view lib.rs 终态**

把 `crates/native-view/src/lib.rs` 全文替换为：

```rust
//! rofd-native-view - masonry/xilem adapter for rofd.
//!
//! `OfdWidget` hosts an `EditorComponent`; embed it in a xilem tree via
//! `ofd()` / `ofd_with_config()`. Transform B renames this crate to
//! rofd-xilem-view; transform C renames the core family to Ofd*, making
//! `OfdCommand = Fn(&mut OfdComponent)` self-consistent.

pub mod masonry_events;
pub mod ofd_view;
pub mod ofd_widget;

pub use ofd_view::{ofd, ofd_with_config, OfdContextMenu, OfdView};
pub use ofd_widget::{
    command_queue, OfdCommand, OfdCommandQueue, OfdWidget, OfdWidgetAction,
};
```

- [ ] **Step 7: native-view Cargo.toml 终态**

把 `crates/native-view/Cargo.toml` 全文替换为：

```toml
[package]
name = "rofd-native-view"
version = "0.1.0"
edition = "2021"

[dependencies]
rofd-component = { workspace = true }
xilem = { workspace = true }

[dev-dependencies]
# Constructing ScrollDelta PixelDelta in masonry_events tests (dpi boundary).
dpi = "0.1"
# Headless TestHarness driving OfdWidget (same git pin as xilem).
masonry_testing = { workspace = true }
# Harness fixtures construct dom models.
rofd-dom = { workspace = true }
```

- [ ] **Step 8: 删除 build_scene 临时垫片并清理 render/component 三处旧引用**

在 `crates/component/src/editor_component.rs` 中删除 A1 插入的整块：

```rust
    /// Temporary public alias; removed once external tests migrate in A3.
    #[doc(hidden)]
    pub fn build_scene(&mut self) -> Scene {
        self.compose_scene()
    }

```

（删除后确认 `Scene` 导入仍被 scene_cache 等使用——它是 struct 字段类型，不会变成未用导入。）

`crates/render/src/tooltip.rs` 中把：

```rust
//! last (above scrollbars) by `EditorComponent::build_scene`.
```

替换为：

```rust
//! last (above scrollbars) when the component composites its scene.
```

`crates/render/src/scrollbar.rs` 的 `paint_appends_chrome_without_clearing_scene` 测试注释中把：

```rust
        // Append-not-clear contract relied on by
        // EditorComponent::build_scene: the composited page scene is passed
        // in already populated, and the chrome must be appended AFTER it with
        // the thumb last, never replacing it.
```

替换为：

```rust
        // Append-not-clear contract relied on by the component's scene
        // composition: the composited page scene is passed in already
        // populated, and the chrome must be appended AFTER it with the
        // thumb last, never replacing it.
```

`crates/component/src/callbacks.rs` 的 OnCopy 说明注释中把（旧桥的 clipboard 装配描述，EditorApp 已在本任务删除）：

```rust
// Exception: `OnCopy` is NOT Send on any target. The native adapter's default
// clipboard assembly captures `Rc<Cell<bool>>` (EditorApp is single-threaded,
// non-Send by design), and no existing usage relies on Send.
```

替换为：

```rust
// Exception: `OnCopy` is NOT Send on any target. The native widget adapter
// intercepts Ctrl+C itself and never installs on_copy (it writes the OS
// clipboard from component copy_selection); on_copy exists for the web
// adapter, which is single-threaded by design. No usage relies on Send.
```

`crates/component/src/editor_component.rs` 的 `fire_warnings` 文档注释中把：

```rust
    /// Fire `on_warning` with the given warnings. Called by the adapter layer
    /// (EditorApp/WasmEditor) after `parse_ofd` returns a `LoadReport` with
```

替换为：

```rust
    /// Fire `on_warning` with the given warnings. Called by the host / adapter
    /// layer after `parse_ofd` returns a `LoadReport` with
```

同文件 `on_copy` 安装器的文档注释中把：

```rust
    /// Fired on Ctrl+C while body text is selected (TextSelect tool). The
    /// component never touches the clipboard (AGENTS §4.9) - adapters wire
    /// the default platform clipboard behind this callback. Not Send-gated:
    /// the native adapter's default clipboard closure captures Rc state
    /// (single-threaded EditorApp), and no usage needs cross-thread Send.
```

替换为：

```rust
    /// Fired on Ctrl+C while body text is selected (TextSelect tool). The
    /// component never touches the clipboard (AGENTS §4.9) - adapters wire
    /// the default platform clipboard behind this callback. Not Send-gated:
    /// the native widget intercepts Ctrl+C without installing on_copy, and
    /// the web adapter is single-threaded; no usage needs cross-thread Send.
```

- [ ] **Step 9: 根 Cargo.toml 删除 winit/masonry_winit 键**

把根 `Cargo.toml` 中的两行删除：

```toml
winit = "0.30"
```

以及 masonry_winit 键所在的整行（A0 后形如）：

```toml
masonry_winit = { git = "https://github.com/linebender/xilem", rev = "271a27a6d4a930f7878d404f9014e3c50a3a9b88" }
```

说明：此时全工作区无任何 manifest 再引用这两个键（Step 5 删了旧桥，native-app Task 2 已重写）。

- [ ] **Step 10: 旧名/垫片零命中审计**

Run:

```bash
! rg -n "EditorApp|WinitEventBridge|build_scene" --glob '!docs/**' crates
```

Expected: 命令退出码 1（无匹配；`!` 反转后整体成功）。若仍有命中，逐个核对并处理后再继续。

- [ ] **Step 11: 全工作部门禁**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

Expected: 零错误零警告；手术刀字节保留测试（rofd-io）绿色；迁移后的 render/component 测试就位（sample_ctm_hit 非 ignored，随 CI 跑；其余真实样例测试保持 #[ignore]）。

- [ ] **Step 12: 提交**

```bash
git add -A
git status --short
git commit -m "refactor(native-view): 删除 winit 旧桥，适配器收敛为 component+xilem，旧测试迁移"
```

---

## A3（transform A）完成判据

- [ ] `cargo run -p native-app` 与 `cargo run -p native-app -- test/ru-yuan-ji-lu.ofd` 经 `Xilem::new_simple` 自运行：工具栏全部按钮、Ctrl+S、右键 overlay 删除批注、markup 按钮按选区启用。
- [ ] 全仓无 winit 直接依赖（masonry_winit 内部使用除外）、无 arboard、无 EditorApp/WinitEventBridge/build_scene 命中。
- [ ] `rofd-native-view` 依赖面终态：rofd-component + xilem；dev：masonry_testing/rofd-dom/dpi。
- [ ] 旧测试四个全部有归宿：c2_save→native-app/tests（#[ignore]），hit_coverage/sample_ctm_hit→render/tests，sample_drag_select→component/tests。
- [ ] transform A 的有意中间态存在且仅此一条：`OfdCommand = Fn(&mut EditorComponent)`，transform C 收敛。
