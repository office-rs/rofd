//! rofd native host — pure xilem (`Xilem::new_simple`).
//!
//! The OFD editor lives in an `OfdWidget` (rofd-xilem-view) embedded via
//! `ofd_with_config`. This
//! host owns only UI policy: the toolbar, the right-click context-menu
//! overlay, and file dialogs. Every editor interaction goes through the
//! command queue — buttons push closures run against the component at the
//! next rebuild — and component callbacks arrive as view handlers. There
//! is no winit layer, no shared `Arc<Mutex<_>>` editor, no MessageProxy
//! wake task, and no manual MasonryState/AppDriver.

mod preflight;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use office_rs_print::{JobHandle, JobState, PageRange};
use rfd::FileDialog;
use rofd_component::{
    ContextTarget, CreateKind, OfdConfig, PrintContent, Tool, ViewEvent, ZoomMode,
};
use rofd_dom::{AnnotationId, AnnotationKind, ShapeKind};
use rofd_io::PackageHandle;
use rofd_xilem_view::print_dialog::{print_dialog_view, HasPrintUi, PrintMaterial, PrintUi};
use rofd_xilem_view::print_dialog_state::{advance, DialogEvent, DiscoverState, PrintServices};
use rofd_xilem_view::print_services::NativePrintServices;
use rofd_xilem_view::{command_queue, ofd_with_config, OfdCommandQueue, OfdContextMenu};
use xilem::kurbo::Vec2;
use xilem::masonry::layout::{Length, UnitPoint};
use xilem::masonry::peniko::Color;
use xilem::style::{Padding, Style};
use xilem::view::{flex_col, flex_row, sized_box, text_button, transformed, zstack, FlexExt};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

use xilem_app::host;

const BTN_PAD: Padding = Padding::from_vh(Length::ZERO, Length::const_px(6.0));

/// viewport.zoom 的基数（96 DPI，mm→px）。xilem-app 不依赖 rofd-render，
/// 显示百分比在宿主本地换算（与 web App.vue 的 PX_PER_MM 同法）。
const PX_PER_MM: f64 = 96.0 / 25.4;

/// Push a host command onto the host→widget channel.
fn push(app: &mut AppState, f: impl Fn(&mut rofd_component::OfdComponent) + Send + Sync + 'static) {
    app.commands.lock().unwrap().push(Arc::new(f));
}

/// Toolbar tool button: sets the component's active tool.
fn tool_button(label: &str, tool: Tool) -> impl WidgetView<AppState> + use<'_> {
    text_button(label, move |app: &mut AppState| {
        let value = tool.clone();
        push(app, move |c| c.set_tool(value.clone()));
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0))
}

/// Markup button: an ACTION over the current body-text selection, not a
/// tool. Disabled without a selection.
fn markup_button(
    label: &str,
    kind: AnnotationKind,
    disabled: bool,
) -> impl WidgetView<AppState> + use<'_> {
    text_button(label, move |app: &mut AppState| {
        let value = kind.clone();
        push(app, move |c| {
            c.apply_markup(value.clone());
        });
    })
    .disabled(disabled)
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0))
}

/// 适应模式按钮：active 时底色高亮（复用 web 端选中蓝底）。
fn zoom_mode_button(
    label: &str,
    mode: ZoomMode,
    active: bool,
) -> impl WidgetView<AppState> + use<'_> {
    let bg = if active {
        Color::from_rgb8(227, 240, 252)
    } else {
        Color::from_rgba8(0, 0, 0, 0)
    };
    sized_box(
        text_button(label, move |app: &mut AppState| {
            let m = mode;
            push(app, move |c| c.set_zoom_mode(m));
        })
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0)),
    )
    .background_color(bg)
}

/// Open right-click overlay state, in ofd-widget-local (= zstack-local)
/// coordinates.
#[derive(Debug, Clone)]
struct ContextMenuState {
    x: f64,
    y: f64,
    id: AnnotationId,
}

/// What a queued host command reports back through `save_outcome`.
#[derive(Debug)]
enum HostOutcome {
    /// A save of the OFD document: success clears `modified` (and promotes
    /// a pending Save-As path); failure keeps it set.
    Save(Result<(), String>),
    /// A PDF export: never touches `modified` in either direction — an
    /// export does not persist annotations, and its failure must not
    /// dirty a clean document. Errors are logged like save errors.
    Export(Result<(), String>),
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
    /// Result of the latest queued file operation, written by the command
    /// closure and drained in `app_logic` before the next view build.
    /// `Save` outcomes drive the modified mirror (never cleared until a
    /// save reports success, so a failed save cannot be mistaken for a
    /// clean document); `Export` outcomes are logged only.
    save_outcome: Arc<Mutex<Option<HostOutcome>>>,
    /// Save-As target waiting for its queued save result. Promoted to
    /// `file` only when the save reports success.
    pending_save_as_path: Option<PathBuf>,
    /// Warnings collected from the last load/operation.
    warnings: Vec<rofd_dom::OfdWarning>,
    /// Open context-menu overlay, if any.
    context_menu: Option<ContextMenuState>,
    /// Current viewport zoom (mirror of on_zoom_change; drives the percent
    /// label and the 100% factor).
    zoom: f64,
    /// Active zoom mode (mirror of on_zoom_mode_change; button highlight).
    zoom_mode: ZoomMode,
    /// Open print Backstage dialog, if any (the overlay renders from it).
    print: Option<PrintUi>,
    /// Shared print preview material: filled by the queued materialize
    /// command (the component is only reachable at drain time), read by
    /// the dialog's preview canvas and the submit precheck.
    print_material: Arc<Mutex<PrintMaterial>>,
    /// Editor current page mirror, 1-based (on_page_change maintains it).
    /// Print "current page" semantics (range=Current, print_direct) and
    /// the dialog's opening preview page derive from it — never from the
    /// preview pager, which is a view control.
    current_page: usize,
    /// Document page-count mirror for the print dialog's 0-page guard.
    /// The component is only reachable at command-drain time, so the
    /// synchronous guard reads this host-side copy instead; it is exact
    /// because the host owns the document value at every swap point
    /// (new = the empty default document, load = the parsed file) and v1
    /// never changes a page count outside those points (body is
    /// read-only).
    doc_pages: usize,
    /// JobHandle write-back slot for the queued `print_direct` command:
    /// the command closure cannot reach AppState, so it stores the handle
    /// here and the next app_logic pass drains it into the dialog's live
    /// job (same one-pass-later contract as `save_outcome`).
    print_job_slot: Arc<Mutex<Option<JobHandle>>>,
}

impl HasPrintUi for AppState {
    fn print_ui(&mut self) -> Option<&mut PrintUi> {
        self.print.as_mut()
    }
}

impl AppState {
    fn new() -> Self {
        let mut state = Self {
            commands: command_queue(),
            file: None,
            package: None,
            modified: false,
            save_outcome: Arc::new(Mutex::new(None)),
            pending_save_as_path: None,
            has_selection: false,
            warnings: Vec::new(),
            context_menu: None,
            zoom: PX_PER_MM,
            zoom_mode: ZoomMode::Custom,
            print: None,
            print_material: Arc::new(Mutex::new(PrintMaterial::default())),
            current_page: 1,
            doc_pages: 0,
            print_job_slot: Arc::new(Mutex::new(None)),
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

/// Drain the queued file-operation outcome before building the view. A
/// successful save clears the modified mirror; a failed one leaves it set
/// (and logs the error), so the next close/save cannot treat the document
/// as clean. An export outcome never touches the mirror: export does not
/// persist annotations (success) and does not dirty the document
/// (failure); only its errors are logged.
fn consume_save_outcome(app: &mut AppState) {
    let Some(outcome) = app.save_outcome.lock().unwrap().take() else {
        return;
    };
    match outcome {
        HostOutcome::Save(result) => match result {
            Ok(()) => {
                app.modified = false;
                if let Some(path) = app.pending_save_as_path.take() {
                    app.file = Some(path);
                }
            }
            Err(e) => {
                app.modified = true;
                app.pending_save_as_path = None;
                eprintln!("[ERROR] save failed: {e}");
            }
        },
        HostOutcome::Export(result) => {
            if let Err(e) = result {
                eprintln!("[ERROR] export failed: {e}");
            }
        }
    }
}

fn do_new(app: &mut AppState) {
    push(app, |c| c.new_document());
    app.file = None;
    app.package = None;
    app.modified = false;
    app.context_menu = None;
    // new_document() resets to the empty default document (0 pages) and
    // the component's current-page cache to None; no on_page_change fires
    // for an empty document, so reset the mirrors here.
    app.doc_pages = 0;
    app.current_page = 1;
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
            // Page-count mirror for the print guard (the host owns the
            // document value here); the current-page mirror resets to 1 —
            // loads re-fire on_page_change, but 0-page files cannot.
            app.doc_pages = document.pages.len();
            app.current_page = 1;
            push(app, move |c| c.load_document(document.clone()));
            app.package = loaded.package;
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
///
/// The command reports its result through `save_outcome`; `modified` is
/// NOT cleared here. `consume_save_outcome` (start of each `app_logic`)
/// clears it on success and keeps it set — with a logged error — on
/// failure.
fn save_to(app: &mut AppState, path: PathBuf) {
    let package = app.package.clone();
    let outcome = app.save_outcome.clone();
    push(app, move |c| {
        let document = c.document().clone();
        *outcome.lock().unwrap() = Some(HostOutcome::Save(host::document_io::save_ofd(
            &document,
            package.as_ref(),
            &path,
        )));
    });
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
    app.pending_save_as_path = Some(path);
    // Spec §4.4: Save As does not mint a PackageHandle; saves stay
    // full-write until the file is opened again. The path is promoted to
    // `file` only after the queued save reports success.
}

/// Export the current document as a PDF. The snapshot runs at command
/// time against the live component (same rule as `save_to`); the result
/// rides `save_outcome` as `HostOutcome::Export`, which never touches the
/// modified mirror — an export does not persist annotations.
fn do_export_pdf(app: &mut AppState) {
    let Some(path) = FileDialog::new()
        .add_filter("PDF 文件", &["pdf"])
        .set_file_name("document.pdf")
        .save_file()
    else {
        return;
    };
    let outcome = app.save_outcome.clone();
    push(app, move |c| {
        *outcome.lock().unwrap() =
            Some(HostOutcome::Export(host::export_pdf::export_pdf(c, &path)));
    });
}

// --- Print Backstage (spec 2026-10-31 print phase 2 Task 5) ---

/// Poll the dialog's live job for a terminal state and refresh the status
/// line when one lands: Done → the submitted label ("已发送到打印机"),
/// Failed/Aborted → the failed label plus a reason. Returns whether the
/// status changed this poll; terminal states are idempotent (later polls
/// report no change). Non-terminal states (Spooling/Rendering) and "no
/// live job" report `false`.
fn poll_job_terminal(ui: &mut PrintUi) -> bool {
    let Some(snapshot) = ui.job.as_ref().map(|job| job.snapshot()) else {
        return false;
    };
    let status = match snapshot.state {
        JobState::Done => Some(ui.labels.submitted.clone()),
        JobState::Failed => Some(format!(
            "{}: {}",
            ui.labels.failed,
            // Worker error text rides along when present; the fallback is
            // zh, not an English fragment (the status line is zh UI copy).
            snapshot.error.as_deref().unwrap_or("未知错误")
        )),
        JobState::Aborted => Some(format!("{}: 已取消", ui.labels.failed)),
        JobState::Spooling | JobState::Rendering => None,
    };
    match status {
        Some(text) if text != ui.status => {
            ui.status = text;
            true
        }
        _ => false,
    }
}

/// Materialize the preview pages of the component's current document at
/// the print baseline: every page's scene, pixel dims (`page_rect` as
/// f32), and paper/orientation. The shared body of the open-time and the
/// content-dirty re-materialize commands.
fn materialize_print_material(
    c: &rofd_component::OfdComponent,
    content: PrintContent,
) -> PrintMaterial {
    let mut session = c.begin_print(content);
    let count = session.page_count();
    let mut material = PrintMaterial::default();
    for n in 0..count {
        let (w, h) = session.page_rect(n);
        material.dims.push((w as f32, h as f32));
        material.papers.push(session.page_parameters(n));
        material.scenes.push(session.page_scene(n));
    }
    material
}

/// Submit precheck (pure — the caller applies the side effect): an empty
/// material blocks the submit ("no printable pages"); otherwise the
/// settings' range/parity resolve against the material's page count using
/// the EDITOR's current page (the 1-based mirror), never the preview
/// pager's page. Returns the resolved 1-based page list; `Ok` is what
/// lets the caller fill `pending`.
fn submit_precheck(ui: &PrintUi, editor_page: usize) -> Result<Vec<usize>, String> {
    let total = ui.page_count();
    if total == 0 {
        return Err("no printable pages".to_string());
    }
    office_rs_print::resolve_page_list(&ui.settings.range, ui.settings.parity, total, editor_page)
}

/// Open the print Backstage. The 0-page guard runs synchronously BEFORE
/// anything else (fresh/new documents never open the dialog); the preview
/// material is materialized by a queued command (the component is only
/// reachable at drain time) and the dialog reads it through the shared
/// Arc, so the first rebuild after the drain fills the preview.
fn do_print(app: &mut AppState) {
    if app.doc_pages == 0 {
        return;
    }
    let content = PrintContent::default();
    let material = app.print_material.clone();
    // Reset the shared material BEFORE queueing the materialize command.
    // The command drains one rebuild later; until then the material would
    // still hold the PREVIOUS dialog's pages, and the drain-time defaults
    // sync (below) must not read those. Empty material = sync no-op; the
    // fresh pages land with the materialize drain and trigger the sync.
    if let Ok(mut target) = material.lock() {
        *target = PrintMaterial::default();
    }
    push(app, move |c| {
        let fresh = materialize_print_material(c, content);
        if let Ok(mut target) = material.lock() {
            *target = fresh;
        }
    });
    let mut ui = PrintUi::new(app.print_material.clone(), true);
    // Open on the editor's current page (1-based mirror, clamped into the
    // document), not page 0. Paper defaults are NOT synced here — the
    // material is empty until the materialize command drains; the host's
    // dialog drain syncs once it lands (drain_print_dialog step ③).
    ui.page = (app.current_page - 1).min(app.doc_pages - 1);
    // Discovery drain: the worker thread races the dialog open, so drain
    // bounded until the terminal message lands (rword do_print pattern).
    let rx = PrintServices::discover(&NativePrintServices);
    while let Ok(update) = rx.recv_timeout(Duration::from_millis(500)) {
        let terminal = !matches!(update, DiscoverState::Finding);
        ui.state = advance(
            &ui.state,
            &ui.settings,
            DialogEvent::DiscoveryUpdate(update),
        );
        if terminal {
            break;
        }
    }
    ui.printers = ui.state.printers.clone();
    // The submit precheck resolves "current page" against the open-time
    // editor page. The Backstage overlay does NOT take keyboard focus, so
    // the mirror CAN move while the dialog is open (narrow divergence
    // under keyboard scroll) — the submit closure therefore freezes
    // PageRange::Current into a Custom spec of the precheck-resolved page,
    // so the precheck and the spooled job always agree.
    let editor_page = app.current_page;
    ui.on_cancel = Some(Arc::new(|ui| {
        if let Some(job) = ui.job.as_ref() {
            job.cancel();
        }
        ui.job = None;
        ui.status.clear();
        ui.exit = true;
    }));
    ui.on_submit = Some(Arc::new(move |ui| {
        // In-flight guard: no second pending while one awaits its drain or
        // a job is live — a double submit would spool a duplicate job.
        if ui.pending.is_some() || ui.job.is_some() {
            return;
        }
        // Zero-printer gate: the discovered list is empty, so the async
        // default-printer failure path must not be reachable from submit.
        if ui.printers.is_empty() {
            ui.status = ui.labels.no_printers.clone();
            return;
        }
        match submit_precheck(ui, editor_page) {
            Ok(pages) => {
                // Freeze the settings snapshot: translate Current into a
                // Custom spec of the precheck-resolved page so
                // print_direct (which re-resolves Current against the
                // LIVE editor page) cannot diverge from the precheck.
                let mut frozen = ui.settings.clone();
                if frozen.range == PageRange::Current {
                    let page = pages.last().copied().unwrap_or(editor_page);
                    frozen.range = PageRange::Custom {
                        spec: page.to_string(),
                    };
                }
                ui.pending = Some((ui.settings.printer.clone(), frozen, ui.content));
            }
            Err(message) => ui.status = message,
        }
    }));
    app.print = Some(ui);
}

/// Print-dialog drain at the top of every `app_logic` pass, in a fixed
/// order: ① poll the live job for a terminal state; ② the exit flag
/// closes the overlay; ③ the first non-empty material syncs the
/// paper/orientation defaults (once per dialog); ④ a content-flag edit
/// re-materializes the preview; ⑤ a confirmed submit becomes a
/// `print_direct` command writing its JobHandle into the shared slot;
/// ⑥ a handle landed in the slot becomes the dialog's live job. ④/⑤'s
/// commands run at the next rebuild drain; ⑥ picks the handle up on the
/// HostCommandWake pass that follows (phase-1 contract 1).
fn drain_print_dialog(app: &mut AppState) {
    // ① terminal-state polling.
    if let Some(ui) = app.print.as_mut() {
        poll_job_terminal(ui);
    }
    // ② exit closes the overlay.
    if app.print.as_ref().is_some_and(|ui| ui.exit) {
        app.print = None;
        // A handle that landed in the slot but was never picked up (exit
        // raced the wake pass): the user cancelled a just-started job —
        // cancel it rather than letting it spool untracked.
        if let Some(handle) = app.print_job_slot.lock().unwrap().take() {
            handle.cancel();
        }
    }
    if app.print.is_none() {
        return;
    }
    // ③ defaults sync (Fix 1): the open-time materialize command drains
    // one rebuild after do_print, so the paper/orientation default can
    // only follow the document HERE, against the fresh material — syncing
    // at open time would read the empty (or previous document's) pages
    // and the physical job would carry a stale DEVMODE paper. Once per
    // dialog: papers are content-independent, so a content_dirty
    // re-materialize does not need another sync.
    if let Some(ui) = app.print.as_mut() {
        if !ui.defaults_synced && ui.page_count() > 0 {
            ui.sync_document_defaults();
            ui.defaults_synced = true;
        }
    }
    // ④ content-flag edit → re-materialize the preview material.
    let dirty_content = app.print.as_mut().and_then(|ui| {
        if ui.content_dirty {
            ui.content_dirty = false;
            Some(ui.content)
        } else {
            None
        }
    });
    if let Some(content) = dirty_content {
        let material = app.print_material.clone();
        push(app, move |c| {
            let fresh = materialize_print_material(c, content);
            if let Ok(mut target) = material.lock() {
                *target = fresh;
            }
        });
    }
    // ⑤ confirmed submit → one-call direct print in a command (the only
    // place the component is reachable); the handle rides the slot.
    let pending = app.print.as_mut().and_then(|ui| ui.pending.take());
    if let Some((printer, settings, content)) = pending {
        let slot = app.print_job_slot.clone();
        push(app, move |c| {
            let handle = c.print_direct(printer.as_deref(), &settings, content, "rofd");
            *slot.lock().unwrap() = Some(handle);
        });
    }
    // ⑥ handle landed in the slot → the dialog's live job.
    let landed = app.print_job_slot.lock().unwrap().take();
    if landed.is_some() {
        if let Some(ui) = app.print.as_mut() {
            ui.job = landed;
            ui.status = ui.labels.submitted.clone();
        }
    }
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
            app.doc_pages = document.pages.len();
            app.current_page = 1;
            push(app, move |c| c.load_document(document.clone()));
            app.package = loaded.package;
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
    consume_save_outcome(app);
    drain_print_dialog(app);
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
    let btn_export_pdf = text_button("导出 PDF", |app: &mut AppState| do_export_pdf(app))
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0));
    let btn_print = text_button("打印", |app: &mut AppState| do_print(app))
        .padding(BTN_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(2.0));
    let file_row = flex_row((btn_new, btn_open, btn_save, btn_export_pdf, btn_print))
        .gap(Length::const_px(2.0));

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

    // --- zoom row（缩放按钮组，spec D3）---
    let zoom_percent = ((app.zoom / PX_PER_MM) * 100.0).round() as i32;
    let btn_zoom_out = text_button("缩小", |app: &mut AppState| {
        push(app, |c| {
            c.handle_event(&ViewEvent::Zoom { factor: 0.9 });
        });
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let percent_label = format!("{zoom_percent}%");
    // text_button 接受 impl Into<ArcStr>：直接传 String 取得所有权——
    // 不要传 `percent_label.as_str()`（借用在 app_logic 返回后悬垂）。
    let btn_zoom_label = text_button(percent_label, |app: &mut AppState| {
        // 点击回 100%：按宿主镜像算倍率（Zoom 事件同样退出 fit 模式）。
        let factor = PX_PER_MM / app.zoom;
        push(app, move |c| {
            c.handle_event(&ViewEvent::Zoom { factor });
        });
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let btn_zoom_in = text_button("放大", |app: &mut AppState| {
        push(app, |c| {
            c.handle_event(&ViewEvent::Zoom { factor: 1.1 });
        });
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));
    let btn_actual_size = text_button("实际大小", |app: &mut AppState| {
        let factor = PX_PER_MM / app.zoom;
        push(app, move |c| {
            c.handle_event(&ViewEvent::Zoom { factor });
        });
    })
    .padding(BTN_PAD)
    .border_width(Length::ZERO)
    .corner_radius(Length::const_px(2.0));

    let btn_fit_width = zoom_mode_button(
        "适应宽度",
        ZoomMode::FitWidth,
        app.zoom_mode == ZoomMode::FitWidth,
    );
    let btn_fit_height = zoom_mode_button(
        "适应高度",
        ZoomMode::FitHeight,
        app.zoom_mode == ZoomMode::FitHeight,
    );
    let btn_fit_page = zoom_mode_button(
        "适应页面",
        ZoomMode::FitPage,
        app.zoom_mode == ZoomMode::FitPage,
    );
    let zoom_row = flex_row((
        btn_zoom_out,
        btn_zoom_label,
        btn_zoom_in,
        btn_actual_size,
        btn_fit_width,
        btn_fit_height,
        btn_fit_page,
    ))
    .gap(Length::const_px(2.0));

    let menu_bar = sized_box(flex_col((file_row, tool_row, zoom_row)))
        .padding(Padding::from_vh(
            Length::const_px(2.0),
            Length::const_px(4.0),
        ))
        .background_color(Color::from_rgb8(240, 240, 240));

    // --- editor: ofd view with the component callback surface mapped ---
    let editor = ofd_with_config(app.commands.clone(), OfdConfig::new(Arc::new(vec![])))
        .on_change(|app: &mut AppState| app.modified = true)
        .on_zoom_change(|app: &mut AppState, z| app.zoom = z)
        .on_zoom_mode_change(|app: &mut AppState, m| app.zoom_mode = m)
        // 0-based page index from the component -> 1-based mirror. Print
        // "current page" semantics and the dialog's opening page derive
        // from this mirror.
        .on_page_change(|app: &mut AppState, idx| app.current_page = idx + 1)
        .on_text_selection_change(|app, sel| app.has_selection = sel.is_some())
        .on_save_request(do_save)
        // Any cursor movement dismisses the popup. In rofd, right-click does
        // NOT fire on_cursor_change (only on_context_menu, verified in the
        // component), so opening isn't clobbered.
        .on_cursor_change(|app, _cursor| app.context_menu = None)
        .on_context_menu(open_context_menu)
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
        .padding(Padding::from_vh(
            Length::const_px(1.0),
            Length::const_px(8.0),
        ))
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

    let main_col = flex_col((menu_bar, editor_area.flex(1.0)));

    // --- print Backstage overlay (covers the whole window while open) ---
    // Same Option-overlay slot pattern as the context menu: the editor
    // tree stays mounted underneath.
    let print_overlay = app.print.as_mut().map(|ui| {
        sized_box(print_dialog_view::<AppState>(ui))
            .background_color(Color::from_rgb8(240, 240, 240))
    });

    zstack((main_col, print_overlay)).alignment(UnitPoint::TOP_LEFT)
}

fn main() -> Result<(), xilem::winit::error::EventLoopError> {
    // GPU 预检（spec 2026-10-15 §6）：无硬件 GPU 时弹原生错误框退出，
    // 而不是在 vello-on-WARP 上访问违例崩溃。必须在构建任何窗口之前。
    if let Err(msg) = preflight::preflight_gpu() {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("rofd")
            .set_description(&msg)
            .show();
        std::process::exit(1);
    }
    let mut app_state = AppState::new();
    maybe_load_cli_arg(&mut app_state);
    let result = Xilem::new_simple(
        app_state,
        app_logic,
        WindowOptions::new("rofd - OFD Editor"),
    )
    .run_in(EventLoop::with_user_event());
    // Print lifecycle: join in-flight spool jobs before process exit — a
    // process death mid-spool kills the spooler helper and the job with
    // it.
    office_rs_print::join_all();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_success_clears_modified() {
        let mut app = AppState::new();
        app.modified = true;
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Save(Ok(())));
        consume_save_outcome(&mut app);
        assert!(!app.modified);
        assert!(app.save_outcome.lock().unwrap().is_none());
    }

    #[test]
    fn save_failure_keeps_modified_and_logs() {
        let mut app = AppState::new();
        app.modified = true;
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Save(Err("disk full".to_string())));
        consume_save_outcome(&mut app);
        assert!(app.modified);
        assert!(app.save_outcome.lock().unwrap().is_none());
    }

    #[test]
    fn no_outcome_leaves_modified_untouched() {
        let mut app = AppState::new();
        app.modified = true;
        consume_save_outcome(&mut app);
        assert!(app.modified);
    }

    #[test]
    fn save_as_success_promotes_pending_path() {
        let mut app = AppState::new();
        app.modified = true;
        app.pending_save_as_path = Some(PathBuf::from("new-copy.ofd"));
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Save(Ok(())));
        consume_save_outcome(&mut app);
        assert_eq!(app.file, Some(PathBuf::from("new-copy.ofd")));
        assert!(app.pending_save_as_path.is_none());
        assert!(!app.modified);
    }

    #[test]
    fn save_as_failure_keeps_original_file() {
        let mut app = AppState::new();
        app.file = Some(PathBuf::from("original.ofd"));
        app.modified = true;
        app.pending_save_as_path = Some(PathBuf::from("new-copy.ofd"));
        *app.save_outcome.lock().unwrap() =
            Some(HostOutcome::Save(Err("access denied".to_string())));
        consume_save_outcome(&mut app);
        assert_eq!(app.file, Some(PathBuf::from("original.ofd")));
        assert!(app.pending_save_as_path.is_none());
        assert!(app.modified);
    }

    #[test]
    fn export_success_does_not_clear_modified() {
        // Export does not persist annotations: a successful export must
        // leave the unsaved-changes mirror exactly as it was.
        let mut app = AppState::new();
        app.modified = true;
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Export(Ok(())));
        consume_save_outcome(&mut app);
        assert!(app.modified, "export success must not clear modified");
        assert!(app.save_outcome.lock().unwrap().is_none());
    }

    #[test]
    fn export_failure_does_not_dirty_clean_document() {
        // A failed export is not a document change either: a clean
        // document stays clean (only the error is logged).
        let mut app = AppState::new();
        app.modified = false;
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Export(Err("disk full".to_string())));
        consume_save_outcome(&mut app);
        assert!(!app.modified, "export failure must not set modified");
    }

    #[test]
    fn export_outcome_never_promotes_pending_save_as_path() {
        // An export arriving while a Save-As is pending must not consume
        // the pending path: only a successful save promotes it.
        let mut app = AppState::new();
        app.modified = true;
        app.pending_save_as_path = Some(PathBuf::from("new-copy.ofd"));
        *app.save_outcome.lock().unwrap() = Some(HostOutcome::Export(Ok(())));
        consume_save_outcome(&mut app);
        assert_eq!(
            app.pending_save_as_path,
            Some(PathBuf::from("new-copy.ofd")),
            "export must not promote or clear a pending Save-As path"
        );
        assert!(app.modified);
    }

    // ---- Print Backstage wiring (spec 2026-10-31 print phase 2 Task 5) ----

    use office_rs_print::{job::submit_noop, Orientation, PageRange, Paper};

    /// One A4-portrait page (210x297mm) with an empty body layer.
    fn a4_page(id: &str) -> rofd_dom::Page {
        rofd_dom::Page {
            id: rofd_dom::PageId::new(id),
            physical_box: rofd_dom::Rect {
                x: 0.0,
                y: 0.0,
                w: 210.0,
                h: 297.0,
            },
            layers: vec![rofd_dom::Layer::default()],
            template: None,
        }
    }

    /// A document of `n` plain A4 pages (no annotations).
    fn pages_document(n: usize) -> rofd_dom::OfdDocument {
        let mut doc = rofd_dom::OfdDocument::default();
        for i in 0..n {
            doc.pages.push(a4_page(&format!("P{i}")));
        }
        doc
    }

    /// Component with an `n`-page document loaded (empty font config —
    /// plain pages shape no glyphs).
    fn component_with_pages(n: usize) -> rofd_component::OfdComponent {
        let mut c = rofd_component::OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
        c.load_document(pages_document(n));
        c
    }

    fn ui_with_material(material: PrintMaterial) -> PrintUi {
        PrintUi::new(Arc::new(Mutex::new(material)), true)
    }

    /// Poll until the helper reports a change: the noop job's worker
    /// thread races this thread, so a single poll may legally see a
    /// non-terminal state. Bounded so a regression cannot hang the test.
    fn poll_until_changed(ui: &mut PrintUi) -> bool {
        for _ in 0..500 {
            if poll_job_terminal(ui) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    /// A Done job flips the status line to the submitted label exactly
    /// once (terminal states are idempotent on later polls).
    #[test]
    fn poll_job_done_sets_status() {
        let mut ui = ui_with_material(PrintMaterial::default());
        assert!(!poll_job_terminal(&mut ui), "no job yet -> no change");
        ui.job = Some(submit_noop(1, None, 0));
        assert!(
            poll_until_changed(&mut ui),
            "job reaches Done and the poll reports it"
        );
        assert_eq!(ui.status, "已发送到打印机");
        assert!(!poll_job_terminal(&mut ui), "terminal status is idempotent");
    }

    /// A Failed job surfaces the worker's error behind the failed label.
    #[test]
    fn poll_job_failed_sets_error_status() {
        let mut ui = ui_with_material(PrintMaterial::default());
        ui.job = Some(submit_noop(1, Some(1), 0));
        assert!(poll_until_changed(&mut ui), "job reaches Failed");
        assert!(
            ui.status.starts_with("打印失败"),
            "failed jobs use the failed label: {}",
            ui.status
        );
        assert!(ui.status.contains("page 1"), "error text rides along");
    }

    /// No live job: the poll is a no-op and never touches the status.
    #[test]
    fn poll_job_none_is_noop() {
        let mut ui = ui_with_material(PrintMaterial::default());
        ui.status = ui.labels.submitted.clone();
        assert!(!poll_job_terminal(&mut ui));
        assert_eq!(ui.status, "已发送到打印机");
    }

    /// Materialization fills all three preview vectors for every page,
    /// with paper from the page geometry and dims at the print baseline.
    #[test]
    fn materialize_fills_dims_papers_scenes() {
        let c = component_with_pages(2);
        let m = materialize_print_material(&c, PrintContent::default());
        assert_eq!(m.scenes.len(), 2);
        assert_eq!(m.dims.len(), 2);
        assert_eq!(m.papers.len(), 2);
        assert!(!m.scenes[0].is_empty(), "page scene recorded");
        assert_eq!(m.papers[0], (Paper::A4, Orientation::Portrait));
        let zoom = rofd_component::PRINT_ZOOM as f32;
        assert!((m.dims[0].0 - 210.0 * zoom).abs() < 0.5);
        assert!((m.dims[0].1 - 297.0 * zoom).abs() < 0.5);
    }

    /// An empty material blocks the submit before any range grammar runs.
    #[test]
    fn empty_document_precheck_blocks_pending() {
        let ui = ui_with_material(PrintMaterial::default());
        assert_eq!(
            submit_precheck(&ui, 1),
            Err("no printable pages".to_string())
        );
    }

    /// The precheck resolves against the EDITOR's current page, never the
    /// preview pager's page: pager flipped to page 3 while the editor sits
    /// on page 1, range=Current must resolve [1]. An out-of-range editor
    /// page clamps to the last page (library semantics).
    #[test]
    fn precheck_uses_editor_page_not_preview_page() {
        let c = component_with_pages(3);
        let mut ui = ui_with_material(materialize_print_material(&c, PrintContent::default()));
        ui.page = 2; // preview pager on the third page
        ui.settings.range = PageRange::Current;
        assert_eq!(
            submit_precheck(&ui, 1),
            Ok(vec![1]),
            "editor page wins over the preview pager"
        );
        assert_eq!(
            submit_precheck(&ui, 9),
            Ok(vec![3]),
            "out-of-range editor page clamps to the last page"
        );
    }

    /// The 0-page guard: a fresh app (empty default document) never opens
    /// the dialog; a loaded document opens on the editor's current page,
    /// clamped into the document.
    #[test]
    fn do_print_guard_and_opening_page() {
        let mut app = AppState::new();
        do_print(&mut app);
        assert!(
            app.print.is_none(),
            "0-page document must not open the dialog"
        );

        app.doc_pages = 3;
        app.current_page = 2;
        do_print(&mut app);
        let ui = app
            .print
            .as_ref()
            .expect("dialog opens for a non-empty document");
        assert_eq!(ui.page, 1, "opens on the editor page (1-based 2)");
        assert!(ui.on_cancel.is_some() && ui.on_submit.is_some());

        app.current_page = 9; // stale mirror beyond the document
        do_print(&mut app);
        assert_eq!(
            app.print.as_ref().unwrap().page,
            2,
            "opening page clamps into the document"
        );
    }

    /// A component whose pages are landscape A4 (297x210mm) — a defaults
    /// sync against them must surface as `Orientation::Landscape`.
    fn landscape_component(n: usize) -> rofd_component::OfdComponent {
        let mut doc = rofd_dom::OfdDocument::default();
        for i in 0..n {
            doc.pages.push(rofd_dom::Page {
                id: rofd_dom::PageId::new(format!("L{i}")),
                physical_box: rofd_dom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 297.0,
                    h: 210.0,
                },
                layers: vec![rofd_dom::Layer::default()],
                template: None,
            });
        }
        let mut c = rofd_component::OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
        c.load_document(doc);
        c
    }

    /// Run every queued host command against `c` (what the ofd view's
    /// rebuild does one pass later).
    fn drain_commands(app: &mut AppState, c: &mut rofd_component::OfdComponent) {
        let commands: Vec<_> = app.commands.lock().unwrap().drain(..).collect();
        for command in commands {
            command(c);
        }
    }

    /// Fix 1: the paper/orientation default follows the FRESH material.
    /// The open-time materialize command drains one rebuild later, so the
    /// sync lives in the dialog drain — an empty material no-ops, the
    /// first non-empty drain syncs once, and later drains never re-sync.
    #[test]
    fn drain_syncs_paper_defaults_once_after_materialize() {
        let mut app = AppState::new();
        app.doc_pages = 2;
        app.current_page = 1;
        do_print(&mut app);

        // The materialize command has not run yet (it drains on the next
        // rebuild): the material is empty and the first drain must not
        // sync — the stale A4/Portrait default survives this pass.
        drain_print_dialog(&mut app);
        {
            let ui = app.print.as_ref().unwrap();
            assert!(!ui.defaults_synced, "empty material -> sync no-ops");
            assert_eq!(ui.settings.orientation, Orientation::Portrait);
        }

        // One rebuild later: the queued materialize command runs against
        // the component (landscape pages, so the sync is visible).
        let mut c = landscape_component(2);
        drain_commands(&mut app, &mut c);
        drain_print_dialog(&mut app);
        {
            let ui = app.print.as_ref().unwrap();
            assert!(ui.defaults_synced, "first non-empty drain syncs");
            assert_eq!(ui.settings.paper, Paper::A4);
            assert_eq!(
                ui.settings.orientation,
                Orientation::Landscape,
                "paper default follows the fresh material, not the stale one"
            );
        }

        // Second drain never re-syncs: a manual divergence survives.
        app.print.as_mut().unwrap().settings.orientation = Orientation::Portrait;
        drain_print_dialog(&mut app);
        assert_eq!(
            app.print.as_ref().unwrap().settings.orientation,
            Orientation::Portrait,
            "synced-once flag holds across later drains"
        );
    }

    /// Fix 2: a zero-printer dialog must NOT set pending — the async
    /// default-printer failure path is unreachable from an empty
    /// discovered list; the zh no-printers label lands in the status.
    #[test]
    fn submit_zero_printers_blocks_pending() {
        let mut app = AppState::new();
        app.doc_pages = 1;
        do_print(&mut app);
        let ui = app.print.as_mut().unwrap();
        ui.printers.clear();
        ui.status.clear();
        let submit = ui.on_submit.clone().expect("do_print wires on_submit");
        submit(ui);
        assert!(ui.pending.is_none(), "zero printers must not set pending");
        assert_eq!(ui.status, "未找到打印机");
    }

    /// Fix 2: an in-flight submission blocks a second one — a pending not
    /// yet drained (and a live job) must not be replaced or doubled.
    #[test]
    fn submit_while_in_flight_blocks_resubmit() {
        let mut app = AppState::new();
        app.doc_pages = 3;
        do_print(&mut app);
        let c = component_with_pages(3);
        let ui = app.print.as_mut().unwrap();
        ui.material = Arc::new(Mutex::new(materialize_print_material(
            &c,
            PrintContent::default(),
        )));
        ui.printers = vec!["Fake PDF".into()];
        let submit = ui.on_submit.clone().expect("do_print wires on_submit");

        // Pending in flight: a second submit must not replace it.
        ui.settings.printer = Some("second".into());
        ui.pending = Some((
            Some("first".into()),
            ui.settings.clone(),
            PrintContent::default(),
        ));
        submit(ui);
        let (printer, _, _) = ui.pending.as_ref().unwrap().clone();
        assert_eq!(printer.as_deref(), Some("first"), "pending is not replaced");

        // Live job: submit must not queue a new pending either.
        ui.pending = None;
        ui.job = Some(submit_noop(1, None, 0));
        submit(ui);
        assert!(ui.pending.is_none(), "live job blocks a new pending");
    }

    /// Fix 2 happy path + Carry C freeze: with printers discovered and a
    /// valid material, submit sets pending — freezing `PageRange::Current`
    /// into a Custom spec of the precheck-resolved editor page so the
    /// spooled job can never diverge from what the precheck validated.
    #[test]
    fn submit_freezes_current_page_into_custom_spec() {
        let mut app = AppState::new();
        app.doc_pages = 3;
        app.current_page = 2;
        do_print(&mut app);
        let c = component_with_pages(3);
        let ui = app.print.as_mut().unwrap();
        ui.material = Arc::new(Mutex::new(materialize_print_material(
            &c,
            PrintContent::default(),
        )));
        ui.printers = vec!["Fake PDF".into()];
        ui.settings.range = PageRange::Current;
        let submit = ui.on_submit.clone().expect("do_print wires on_submit");
        submit(ui);
        let (_, settings, _) = ui.pending.take().expect("valid submit sets pending");
        assert_eq!(
            settings.range,
            PageRange::Custom {
                spec: "2".to_string()
            },
            "Current freezes to the precheck-resolved editor page"
        );
        // Non-Current ranges pass through untouched.
        ui.settings.range = PageRange::All;
        submit(ui);
        let (_, settings, _) = ui
            .pending
            .take()
            .expect("submit works again after the drain consumed pending");
        assert_eq!(settings.range, PageRange::All);
    }
}
