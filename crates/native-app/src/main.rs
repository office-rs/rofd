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
use xilem::style::{Padding, Style};
use xilem::view::{flex_col, flex_row, sized_box, text_button, transformed, zstack, FlexExt};
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
        .padding(Padding::from_vh(
            Length::const_px(2.0),
            Length::const_px(4.0),
        ))
        .background_color(Color::from_rgb8(240, 240, 240));

    // --- editor: ofd view with the component callback surface mapped ---
    let editor = ofd_with_config(app.commands.clone(), EditorConfig::new(Arc::new(vec![])))
        .on_change(|app: &mut AppState| app.modified = true)
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
