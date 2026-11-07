// Panel shape provenance: kit-button-via-bridge + listener-chip pattern
// ported from rword@4e0b25b6 (crates/gpui-app/src/print_dialog.rs — the
// modal chip panel and its kit Button routing); the dialog logic is the
// rofd xilem host's `drain_print_dialog` contract (see below), driving
// rofd's Task-12 DialogState machine instead of rword's own state.

//! Print panel host glue (Task 13, xilem `drain_print_dialog` contract
//! ported to the gpui shell): the pure panel state
//! ([`PrintPanelState`]) wrapping Task 12's `DialogState` machine plus the
//! single-track submit guards, and the centered modal overlay rendering
//! from it. Submission itself flows through `GpuiOfdHandle::print_direct`
//! (the component verb), never through this module.
//!
//! Divergences from the xilem wiring (both forced by gpui's direct
//! component access — the xilem host can only reach the component through
//! its command queue):
//!
//! - Paper/orientation defaults sync at OPEN time against a fresh
//!   `begin_print` session (xilem defers to its drain step ③ only because
//!   the open-time materialize command runs one rebuild later).
//! - Steps ⑤/⑥ (pending → `print_direct` → live job) run synchronously in
//!   the submit action: there is no job slot, so an "unpicked handle"
//!   cannot exist — the exit path (and every panel close, including the
//!   tab-switch reset) cancels whatever handle the panel holds.

use std::sync::mpsc;

use gpui::{div, prelude::*, px, rgb, AnyElement, ClickEvent, Context, Entity};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input as TextInput, InputState};
use gpui_kit::component::Disableable;
use office_rs_print::{Duplex, JobHandle, JobState, Orientation, PageRange, Paper, PrintSettings};
use rofd_component::PrintContent;
use rofd_gpui_view::print_dialog_state::{
    advance, DialogEvent, DialogPhase, DialogState, DiscoverState, Labels,
};

use crate::sessions::{AppAction, AppActionBridge};
use crate::shell::AppShell;

/// Why a submit press did not produce a pending job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitBlock {
    /// Single-track guard: a pending submit or a live job already owns the
    /// dialog (a second submit would spool a duplicate job).
    Busy,
    /// Discovery found no printer: the async default-printer failure path
    /// must not be reachable from submit.
    NoPrinters,
    /// Validation/plan failure (range spec, parity selection, no pages);
    /// carries the user-facing message.
    Invalid(String),
}

/// Host-side print panel state (pure — constructible in tests without a
/// Window): Task 12's dialog machine plus the host-owned settings, the
/// discovery receiver, and the single-track job slots.
pub struct PrintPanelState {
    /// Task 12 state machine state (phase, printers, validation errors).
    pub dialog: DialogState,
    /// The settings the panel edits (host-owned per the machine contract).
    pub settings: PrintSettings,
    /// Content flags (all-on defaults: watermark/annotations/seal on,
    /// convert spec D17).
    pub content: PrintContent,
    /// Discovery receiver kept after the open-time bounded drain; drained
    /// on render ticks until its terminal message lands.
    pub receiver: Option<mpsc::Receiver<DiscoverState>>,
    /// Live submitted job (terminal-state polling target; cancel handle).
    pub job: Option<JobHandle>,
    /// Confirmed submit awaiting execution: (printer override, frozen
    /// settings, content flags).
    pub pending: Option<(Option<String>, PrintSettings, PrintContent)>,
    /// Close request: the shell drain closes the overlay on it.
    pub exit: bool,
    /// Panel labels (zh_cn in this host).
    pub labels: Labels,
    /// Host status line (submit/poll outcomes; empty = derive from phase).
    pub status: String,
    /// Document page count captured at open (body is read-only in v1, so
    /// it cannot change while the dialog is open).
    pub page_count: usize,
    /// Whether the document paper/orientation defaults were applied.
    pub defaults_synced: bool,
}

impl PrintPanelState {
    pub fn new(page_count: usize) -> Self {
        Self {
            dialog: DialogState::initial(),
            settings: PrintSettings::default(),
            content: PrintContent::default(),
            receiver: None,
            job: None,
            pending: None,
            exit: false,
            labels: Labels::zh_cn(),
            status: String::new(),
            page_count,
            defaults_synced: false,
        }
    }

    /// Panel opened / discovery kicked off.
    pub fn open(&mut self) {
        self.dialog = advance(&self.dialog, &self.settings, DialogEvent::DiscoveryStarted);
    }

    /// One discovery update into the machine.
    pub fn apply_discovery(&mut self, update: DiscoverState) {
        self.dialog = advance(
            &self.dialog,
            &self.settings,
            DialogEvent::DiscoveryUpdate(update),
        );
    }

    /// Drain pending discovery messages (render-tick fallback for a
    /// receiver whose terminal message missed the open-time bounded
    /// drain). Returns how many updates landed.
    pub fn drain_discovery(&mut self) -> usize {
        let mut count = 0;
        while let Some(rx) = self.receiver.as_ref() {
            match rx.try_recv() {
                Ok(update) => {
                    self.apply_discovery(update);
                    count += 1;
                }
                Err(_) => break,
            }
        }
        // A terminal message ends discovery: release the dead channel.
        if !matches!(self.dialog.phase, DialogPhase::Idle | DialogPhase::Finding) {
            self.receiver = None;
        }
        count
    }

    /// Edit the settings and revalidate through the machine.
    pub fn edit_settings(&mut self, edit: impl FnOnce(&mut PrintSettings)) {
        edit(&mut self.settings);
        let snapshot = self.settings.clone();
        self.dialog = advance(
            &self.dialog,
            &snapshot,
            DialogEvent::SettingsChanged(snapshot.clone()),
        );
    }

    /// Cycle the printer selection through the discovered list plus the
    /// system default (xilem `print_dialog` semantics: position + 1 modulo
    /// list + 1; index == len selects `None` = system default).
    pub fn cycle_printer(&mut self) {
        if self.dialog.printers.is_empty() {
            return;
        }
        let idx = self
            .dialog
            .printers
            .iter()
            .position(|p| Some(p) == self.settings.printer.as_ref())
            .map(|i| i + 1)
            .unwrap_or(0);
        let picked = self.dialog.printers.get(idx).cloned();
        self.edit_settings(|s| s.printer = picked);
    }

    /// Cycle the range mode All → Current → Custom → All. Landing on
    /// Custom with no spec pre-fills `custom_prefill` (the caller passes
    /// the editor's current page) and RETURNS the landed spec so the
    /// caller can sync the Custom-range input's display to the backing
    /// value (final-review M-2: without the sync the field shows whatever
    /// it held the last time Custom was active).
    pub fn cycle_range(&mut self, custom_prefill: &str) -> Option<String> {
        let next = match &self.settings.range {
            PageRange::All => PageRange::Current,
            PageRange::Current => PageRange::Custom {
                spec: custom_prefill.to_string(),
            },
            PageRange::Custom { .. } => PageRange::All,
        };
        let landed_custom = match &next {
            PageRange::Custom { spec } => Some(spec.clone()),
            _ => None,
        };
        self.edit_settings(|s| s.range = next);
        landed_custom
    }

    /// Edit the content flags. Nothing the panel displays derives from
    /// them (page count is content-independent), so there is no
    /// re-materialize step (xilem drain ④ is a no-op here by design).
    pub fn edit_content(&mut self, edit: impl FnOnce(&mut PrintContent)) {
        edit(&mut self.content);
    }

    /// Paper/orientation defaults from the document's first page
    /// (xilem drain ③; here applied at open time from a fresh
    /// `begin_print` session).
    pub fn sync_document_defaults(&mut self, paper: Paper, orientation: Orientation) {
        self.settings.paper = paper;
        self.settings.orientation = orientation;
        self.defaults_synced = true;
    }

    /// Submit precheck (pure): page count guard, then the settings'
    /// range/parity resolved against the EDITOR's current page.
    pub fn submit_precheck(&self, editor_page: usize) -> Result<Vec<usize>, String> {
        if self.page_count == 0 {
            return Err("no printable pages".to_string());
        }
        office_rs_print::resolve_page_list(
            &self.settings.range,
            self.settings.parity,
            self.page_count,
            editor_page,
        )
    }

    /// Confirmed submit (xilem `on_submit` port): single-track guard,
    /// zero-printer gate, precheck, then the frozen settings snapshot
    /// lands in `pending`. `Ok(())` means `pending` is filled.
    pub fn try_submit(&mut self, editor_page: usize) -> Result<(), SubmitBlock> {
        // In-flight guard: no second pending while one awaits its drain or
        // a job is live — a double submit would spool a duplicate job.
        if self.pending.is_some() || self.job.is_some() {
            return Err(SubmitBlock::Busy);
        }
        // Zero-printer gate: the discovered list is empty, so the async
        // default-printer failure path must not be reachable from submit.
        if self.dialog.printers.is_empty() {
            self.status = self.labels.no_printers.clone();
            return Err(SubmitBlock::NoPrinters);
        }
        match self.submit_precheck(editor_page) {
            Ok(pages) => {
                let frozen = freeze_current_range(&self.settings, &pages, editor_page);
                self.pending = Some((self.settings.printer.clone(), frozen, self.content));
                Ok(())
            }
            Err(message) => Err(SubmitBlock::Invalid(message)),
        }
    }

    /// Take the confirmed submit for execution.
    pub fn take_pending(&mut self) -> Option<(Option<String>, PrintSettings, PrintContent)> {
        self.pending.take()
    }

    /// The submitted handle becomes the dialog's live job (xilem drain ⑥).
    pub fn submit_accepted(&mut self, job: JobHandle) {
        self.job = Some(job);
        self.dialog = advance(&self.dialog, &self.settings, DialogEvent::SubmitAccepted);
        self.status = self.labels.submitted.clone();
    }

    /// Close request: cancel the live job (an exit racing a just-started
    /// job cancels it rather than letting it spool untracked) and drop
    /// any un-executed pending. Returns the cancelled handle, if any.
    pub fn request_exit(&mut self) -> Option<JobHandle> {
        let job = self.job.take();
        if let Some(job) = &job {
            job.cancel();
        }
        self.pending = None;
        self.exit = true;
        job
    }

    /// Poll the live job for a terminal state and refresh the status line
    /// when one lands. Terminal states are idempotent (later polls report
    /// no change). Returns whether the status changed.
    pub fn poll_job_terminal(&mut self) -> bool {
        let Some(snapshot) = self.job.as_ref().map(|job| job.snapshot()) else {
            return false;
        };
        let status = match snapshot.state {
            JobState::Done => Some(self.labels.submitted.clone()),
            JobState::Failed => Some(format!(
                "{}: {}",
                self.labels.failed,
                // Worker error text rides along when present; the fallback
                // is zh, not an English fragment (zh UI copy).
                snapshot.error.as_deref().unwrap_or("未知错误")
            )),
            JobState::Aborted => Some(format!("{}: 已取消", self.labels.failed)),
            JobState::Spooling | JobState::Rendering => None,
        };
        match status {
            Some(text) if text != self.status => {
                self.status = text;
                true
            }
            _ => false,
        }
    }

    /// Whether the submit control is enabled (machine verdict + the
    /// single-track slots).
    pub fn can_submit(&self) -> bool {
        self.dialog.can_submit() && self.pending.is_none() && self.job.is_none()
    }

    /// The panel status line: validation errors first (explicit edit
    /// feedback), then the host status (submit/poll outcomes), then the
    /// phase-driven defaults.
    pub fn status_text(&self) -> String {
        if !self.dialog.errors.is_empty() {
            return errors_status_line(&self.dialog.errors, &self.labels.failed);
        }
        if !self.status.is_empty() {
            return self.status.clone();
        }
        match self.dialog.phase {
            DialogPhase::Finding => self.labels.finding.clone(),
            DialogPhase::Ready if self.dialog.printers.is_empty() => {
                self.labels.no_printers.clone()
            }
            DialogPhase::Failed => match &self.dialog.last_error {
                Some(error) => format!("{}: {}", self.labels.failed, error),
                None => self.labels.failed.clone(),
            },
            _ => String::new(),
        }
    }
}

/// Drop guard (final-review M-1): a panel dropped without an explicit
/// close still cancels its live job instead of letting it spool
/// untracked. Every explicit close already routed through `request_exit`,
/// which empties the slots — the drop-side call finds nothing and is a
/// quiet no-op, so there is no double-cancel noise on those paths.
impl Drop for PrintPanelState {
    fn drop(&mut self) {
        let _ = self.request_exit();
    }
}

/// Freeze the submit snapshot: translate `PageRange::Current` into a
/// `Custom` spec of the precheck-resolved page so `print_direct` (which
/// re-resolves Current against the LIVE editor page) cannot diverge from
/// the precheck (xilem freeze precedent).
pub fn freeze_current_range(
    settings: &PrintSettings,
    resolved: &[usize],
    editor_page: usize,
) -> PrintSettings {
    let mut frozen = settings.clone();
    if frozen.range == PageRange::Current {
        let page = resolved.last().copied().unwrap_or(editor_page);
        frozen.range = PageRange::Custom {
            spec: page.to_string(),
        };
    }
    frozen
}

/// Validation-error status line: each [`office_rs_print::SettingsError`]
/// mapped to a short zh string (xilem `errors_status_line` port — Rust
/// `Debug` output has no place in the zh UI). Exhaustive so a new
/// office-rs-print variant fails the build here.
fn errors_status_line(errors: &[office_rs_print::SettingsError], failed_label: &str) -> String {
    use office_rs_print::SettingsError;
    let detail = errors
        .iter()
        .map(|e| match e {
            SettingsError::InvalidRangeSpec { reason } => format!("页码范围无效：{reason}"),
            SettingsError::RangeSpecTooLong { len } => format!("页码范围过长（{len} 字符）"),
        })
        .collect::<Vec<_>>()
        .join("；");
    format!("{failed_label}: {detail}")
}

// -- Panel rendering (modal overlay in the shell's element tree) --

/// Enum value sets the cycle chips walk (order = display order).
const PAPERS: [Paper; 7] = [
    Paper::A3,
    Paper::A4,
    Paper::A5,
    Paper::B4,
    Paper::B5,
    Paper::Letter,
    Paper::Legal,
];
const DUPLEXES: [Duplex; 3] = [Duplex::Off, Duplex::LongEdge, Duplex::ShortEdge];
const ORIENTATIONS: [Orientation; 3] = [
    Orientation::Portrait,
    Orientation::Landscape,
    Orientation::Auto,
];
const PARITIES: [office_rs_print::Parity; 3] = [
    office_rs_print::Parity::All,
    office_rs_print::Parity::Odd,
    office_rs_print::Parity::Even,
];

fn next_in<T: PartialEq + Copy>(list: &[T], current: T) -> T {
    let idx = list
        .iter()
        .position(|t| *t == current)
        .map(|i| i + 1)
        .unwrap_or(0);
    list[idx % list.len()]
}

fn duplex_label(duplex: Duplex) -> &'static str {
    match duplex {
        Duplex::Off => "单面",
        Duplex::LongEdge => "双面（长边）",
        Duplex::ShortEdge => "双面（短边）",
    }
}

fn color_label(color: office_rs_print::ColorMode) -> &'static str {
    match color {
        office_rs_print::ColorMode::Color => "彩色",
        office_rs_print::ColorMode::Grayscale => "黑白",
    }
}

fn paper_label(paper: Paper) -> &'static str {
    match paper {
        Paper::A3 => "A3",
        Paper::A4 => "A4",
        Paper::A5 => "A5",
        Paper::B4 => "B4",
        Paper::B5 => "B5",
        Paper::Letter => "信纸",
        Paper::Legal => "法律",
    }
}

fn orientation_label(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Portrait => "纵向",
        Orientation::Landscape => "横向",
        Orientation::Auto => "自动",
    }
}

fn parity_label(parity: office_rs_print::Parity) -> &'static str {
    match parity {
        office_rs_print::Parity::All => "全部页",
        office_rs_print::Parity::Odd => "奇数页",
        office_rs_print::Parity::Even => "偶数页",
    }
}

fn range_label(range: &PageRange) -> &'static str {
    match range {
        PageRange::All => "全部",
        PageRange::Current => "当前页",
        PageRange::Custom { .. } => "自定义",
    }
}

/// The printer chip label: the selected printer, or the system-default
/// placeholder.
fn printer_display(panel: &PrintPanelState) -> String {
    panel
        .settings
        .printer
        .clone()
        .unwrap_or_else(|| "（系统默认）".to_string())
}

/// One label + control row.
fn field_row(label: &str, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(84.))
                .flex_shrink_0()
                .text_size(px(13.))
                .text_color(rgb(0x555555))
                .child(label.to_string()),
        )
        .child(control)
}

/// Cycle chip (div-based, listener-routed): shows the current value with
/// a cycle hint.
fn cycle_chip(
    id: &'static str,
    text: String,
    on_click: impl Fn(&mut AppShell, &ClickEvent, &mut gpui::Window, &mut Context<AppShell>) + 'static,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let chip = plain_chip(id, text, on_click, cx);
    chip.child(
        div()
            .text_size(px(10.))
            .text_color(rgb(0x999999))
            .child("◂▸"),
    )
    .into_any_element()
}

/// Chip without the cycle hint (e.g. the copies +/− steppers).
fn plain_chip(
    id: &'static str,
    text: String,
    on_click: impl Fn(&mut AppShell, &ClickEvent, &mut gpui::Window, &mut Context<AppShell>) + 'static,
    cx: &mut Context<AppShell>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .h(px(26.))
        .rounded(px(4.))
        .text_size(px(13.))
        .bg(rgb(0xF0F2F5))
        .cursor_pointer()
        .hover(|d| d.bg(rgb(0xE2E8F0)))
        .on_click(cx.listener(on_click))
        .child(text)
}

/// Toggle chip: label + check state.
fn toggle_chip(
    id: &'static str,
    label: &str,
    checked: bool,
    on_click: impl Fn(&mut AppShell, &ClickEvent, &mut gpui::Window, &mut Context<AppShell>) + 'static,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .h(px(26.))
        .rounded(px(4.))
        .text_size(px(13.))
        .cursor_pointer()
        .when(checked, |d| d.bg(rgb(0xD9E7F9)).text_color(rgb(0x1E6CFF)))
        .when(!checked, |d| d.bg(rgb(0xF0F2F5)).text_color(rgb(0x555555)))
        .on_click(cx.listener(on_click))
        .child(label.to_string())
        .child(
            div()
                .text_size(px(11.))
                .child(if checked { "✓" } else { "" }),
        )
        .into_any_element()
}

/// The centered modal print panel overlay. Field chips mutate the panel
/// directly through shell listeners; the submit/cancel kit buttons ride
/// the action bridge (kit on_click contexts cannot reach the shell
/// entity).
pub fn print_panel_overlay(
    panel: &mut PrintPanelState,
    spec_input: &Entity<InputState>,
    current_page: usize,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let labels = panel.labels.clone();
    let page_count = panel.page_count;
    let status = panel.status_text();
    let can_submit = panel.can_submit();
    let printer_text = printer_display(panel);
    let copies = panel.settings.copies;
    let collate = panel.settings.collate;
    let duplex = panel.settings.duplex;
    let color = panel.settings.color;
    let paper = panel.settings.paper;
    let orientation = panel.settings.orientation;
    let range = panel.settings.range.clone();
    let parity = panel.settings.parity;
    let content = panel.content;

    // -- Control closures (each mutates the shell's panel, then notifies) --

    let printer_chip = cycle_chip(
        "pp-printer",
        printer_text,
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.cycle_printer();
            }
            cx.notify();
        },
        cx,
    );

    let copies_dec = plain_chip(
        "pp-copies-dec",
        "−".to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.copies = s.copies.saturating_sub(1).max(1));
            }
            cx.notify();
        },
        cx,
    )
    .into_any_element();
    let copies_value = div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(36.))
        .h(px(26.))
        .text_size(px(13.))
        .child(format!("{copies}"));
    let copies_inc = plain_chip(
        "pp-copies-inc",
        "+".to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| {
                    s.copies = s
                        .copies
                        .saturating_add(1)
                        .min(office_rs_print::PrintSettings::MAX_COPIES)
                });
            }
            cx.notify();
        },
        cx,
    )
    .into_any_element();

    let collate_chip = toggle_chip(
        "pp-collate",
        &labels.collate,
        collate,
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.collate = !s.collate);
            }
            cx.notify();
        },
        cx,
    );

    let duplex_chip = cycle_chip(
        "pp-duplex",
        duplex_label(duplex).to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.duplex = next_in(&DUPLEXES, s.duplex));
            }
            cx.notify();
        },
        cx,
    );

    let color_chip = cycle_chip(
        "pp-color",
        color_label(color).to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| {
                    s.color = if matches!(s.color, office_rs_print::ColorMode::Color) {
                        office_rs_print::ColorMode::Grayscale
                    } else {
                        office_rs_print::ColorMode::Color
                    }
                });
            }
            cx.notify();
        },
        cx,
    );

    let paper_chip = cycle_chip(
        "pp-paper",
        paper_label(paper).to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.paper = next_in(&PAPERS, s.paper));
            }
            cx.notify();
        },
        cx,
    );

    let orientation_chip = cycle_chip(
        "pp-orientation",
        orientation_label(orientation).to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.orientation = next_in(&ORIENTATIONS, s.orientation));
            }
            cx.notify();
        },
        cx,
    );

    let range_chip = cycle_chip(
        "pp-range",
        range_label(&range).to_string(),
        {
            let prefill_fallback = current_page.to_string();
            move |this, _e, window, cx| {
                let prefill = this
                    .sessions
                    .active()
                    .map(|s| s.page.to_string())
                    .unwrap_or_else(|| prefill_fallback.clone());
                let landed_custom = this
                    .print_panel
                    .as_mut()
                    .and_then(|panel| panel.cycle_range(&prefill));
                // Landing on Custom: mirror the just-prefilled spec into
                // the shared input so the DISPLAYED spec matches the
                // backing value (final-review M-2). set_value does not
                // emit Change, so the shell's input->settings mirror
                // cannot loop back.
                if let Some(spec) = landed_custom {
                    this.print_spec_input
                        .update(cx, |state, cx| state.set_value(spec, window, cx));
                }
                cx.notify();
            }
        },
        cx,
    );

    // Custom-range spec input: only mounted in Custom mode; the shell's
    // InputEvent::Change subscription mirrors its value into the settings.
    let spec_control: AnyElement = match &range {
        PageRange::Custom { .. } => div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .min_w(px(120.))
            .child(TextInput::new(spec_input).appearance(true))
            .into_any_element(),
        _ => div()
            .text_size(px(12.))
            .text_color(rgb(0x999999))
            .child(if matches!(range, PageRange::Current) {
                format!("第 {current_page} 页")
            } else {
                format!("共 {page_count} 页")
            })
            .into_any_element(),
    };

    let parity_chip = cycle_chip(
        "pp-parity",
        parity_label(parity).to_string(),
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_settings(|s| s.parity = next_in(&PARITIES, s.parity));
            }
            cx.notify();
        },
        cx,
    );

    let watermark_chip = toggle_chip(
        "pp-watermark",
        "水印",
        content.watermark,
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_content(|c| c.watermark = !c.watermark);
            }
            cx.notify();
        },
        cx,
    );
    let annotations_chip = toggle_chip(
        "pp-annotations",
        "批注",
        content.annotations,
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_content(|c| c.annotations = !c.annotations);
            }
            cx.notify();
        },
        cx,
    );
    let seal_chip = toggle_chip(
        "pp-seal",
        "图章",
        content.seal,
        |this, _e, _w, cx| {
            if let Some(panel) = this.print_panel.as_mut() {
                panel.edit_content(|c| c.seal = !c.seal);
            }
            cx.notify();
        },
        cx,
    );

    // -- Assemble --

    let cancel_button = Button::new("pp-cancel")
        .label(labels.cancel.clone())
        .compact()
        .on_click(move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(AppAction::PrintCancel, cx);
        });
    let submit_button = Button::new("pp-submit")
        .label(labels.submit.clone())
        .compact()
        .primary()
        .disabled(!can_submit)
        .on_click(move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(AppAction::PrintSubmit, cx);
        });

    div()
        .id("print-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.35))
        // Modal: swallow backdrop clicks (no close-on-outside; the panel
        // owns an explicit cancel).
        .on_click(|_e: &ClickEvent, _w, _cx| {})
        .child(
            div()
                .id("print-panel")
                .flex()
                .flex_col()
                .gap_2()
                .min_w(px(380.))
                .max_w(px(460.))
                .px_4()
                .py_4()
                .bg(rgb(0xFFFFFF))
                .rounded(px(8.))
                .shadow_lg()
                // Title row.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(labels.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(0x999999))
                                .child(format!("共 {page_count} 页")),
                        ),
                )
                // Fields.
                .child(field_row(&labels.printer, printer_chip))
                .child(field_row(
                    &labels.copies,
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(copies_dec)
                        .child(copies_value)
                        .child(copies_inc),
                ))
                .child(field_row(&labels.collate, collate_chip))
                .child(field_row(&labels.duplex, duplex_chip))
                .child(field_row(&labels.color, color_chip))
                .child(field_row(&labels.paper, paper_chip))
                .child(field_row(&labels.orientation, orientation_chip))
                .child(field_row(
                    &labels.range,
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(range_chip)
                        .child(spec_control),
                ))
                .child(field_row(&labels.parity, parity_chip))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(84.))
                                .flex_shrink_0()
                                .text_size(px(13.))
                                .text_color(rgb(0x555555))
                                .child("内容"),
                        )
                        .child(watermark_chip)
                        .child(annotations_chip)
                        .child(seal_chip),
                )
                // Status line (phase-driven).
                .child(
                    div()
                        .h(px(18.))
                        .text_size(px(12.))
                        .text_color(rgb(0x888888))
                        .child(status),
                )
                // Actions.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(cancel_button)
                        .child(submit_button),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_rs_print::job::submit_noop;
    use office_rs_print::JobState;

    /// Poll a (possibly cloned) handle until it reports a terminal state.
    /// A clone's `wait` does not join the worker thread, so tests poll the
    /// shared snapshot instead of blocking on it.
    fn wait_terminal(handle: &JobHandle) -> JobState {
        for _ in 0..500 {
            let state = handle.snapshot().state;
            if matches!(state, JobState::Done | JobState::Failed | JobState::Aborted) {
                return state;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!(
            "job never reached a terminal state: {:?}",
            handle.snapshot()
        );
    }

    /// A ready panel with `n` discovered printers over a 10-page document.
    fn panel_with_printers(n: usize) -> PrintPanelState {
        let mut panel = PrintPanelState::new(10);
        panel.open();
        panel.apply_discovery(DiscoverState::Printers(
            (0..n).map(|i| format!("printer-{i}")).collect(),
        ));
        panel
    }

    /// Single-track guard (Review Focus 6 port): a submit while a pending
    /// job awaits execution or a job is live is rejected silently — a
    /// double submit would spool a duplicate job.
    #[test]
    fn submit_rejected_while_pending_or_job_live() {
        let mut panel = panel_with_printers(2);
        panel.pending = Some((
            panel.settings.printer.clone(),
            panel.settings.clone(),
            panel.content,
        ));
        assert_eq!(panel.try_submit(1), Err(SubmitBlock::Busy));
        assert!(
            panel.pending.is_some(),
            "the guard must not consume or overwrite the in-flight pending"
        );

        let mut panel = panel_with_printers(2);
        panel.job = Some(submit_noop(3, None, 0));
        assert_eq!(panel.try_submit(1), Err(SubmitBlock::Busy));
        assert!(
            panel.pending.is_none(),
            "no new pending while a job is live"
        );
    }

    /// Zero-printer gate: with an empty discovered list the async
    /// default-printer failure path must not be reachable from submit.
    #[test]
    fn submit_rejected_without_printers() {
        let mut panel = PrintPanelState::new(10);
        panel.open();
        panel.apply_discovery(DiscoverState::NoPrinters);
        assert_eq!(panel.try_submit(1), Err(SubmitBlock::NoPrinters));
        assert!(panel.pending.is_none());
        assert_eq!(panel.status, panel.labels.no_printers);
        assert!(!panel.can_submit());
    }

    /// Current-range freeze (xilem freeze port): the submit snapshot
    /// translates `PageRange::Current` into a `Custom` spec of the
    /// precheck-resolved page BEFORE `print_direct`, so the live editor
    /// page cannot diverge from the spooled job; other modes pass through
    /// untouched.
    #[test]
    fn current_range_freezes_to_custom_spec() {
        let mut panel = panel_with_printers(1);
        panel.edit_settings(|s| s.range = PageRange::Current);
        assert_eq!(panel.try_submit(4), Ok(()));
        let (printer, frozen, content) = panel.take_pending().expect("pending filled");
        assert_eq!(printer, None, "system-default selection passes through");
        assert_eq!(
            frozen.range,
            PageRange::Custom {
                spec: "4".to_string()
            },
            "Current is frozen to the precheck-resolved editor page"
        );
        assert_eq!(frozen.copies, panel.settings.copies);
        assert_eq!(content, panel.content);
        // The panel's own settings stay unfrozen (a retry re-resolves).
        assert_eq!(panel.settings.range, PageRange::Current);

        // All and Custom pass through unchanged.
        let mut panel = panel_with_printers(1);
        assert_eq!(panel.try_submit(4), Ok(()));
        assert_eq!(panel.take_pending().unwrap().1.range, PageRange::All);
        let mut panel = panel_with_printers(1);
        panel.edit_settings(|s| {
            s.range = PageRange::Custom {
                spec: "1-3,5".into(),
            }
        });
        assert_eq!(panel.try_submit(4), Ok(()));
        assert_eq!(
            panel.take_pending().unwrap().1.range,
            PageRange::Custom {
                spec: "1-3,5".into()
            }
        );
    }

    /// Exit cancels an unpicked/live handle instead of letting it spool
    /// untracked (xilem drain ② contract; in this host `print_direct` runs
    /// synchronously, so the live job IS the "unpicked handle" case).
    /// Every panel close routes through `request_exit` — the cancel
    /// button, and the tab-switch reset (`on_active_changed`, whose
    /// panel-drop calls this same method before dropping the panel:
    /// join_all is a no-op at the pinned office-rs-print rev, so
    /// cancel-on-close is the only real guarantee).
    #[test]
    fn exit_with_live_handle_cancels_it() {
        let mut panel = panel_with_printers(1);
        let job = submit_noop(50, None, 20);
        let witness = job.clone();
        panel.job = Some(job);
        // An un-executed pending must not survive the exit either.
        panel.pending = Some((None, panel.settings.clone(), panel.content));
        let cancelled = panel.request_exit();
        assert!(cancelled.is_some());
        assert!(witness.is_cancelled(), "exit cancels the live job");
        assert_eq!(wait_terminal(&witness), JobState::Aborted);
        assert!(panel.pending.is_none());
        assert!(panel.job.is_none());
        assert!(panel.exit);

        // The tab-switch close path: request_exit then drop (the shell's
        // on_active_changed sequence).
        let mut panel = panel_with_printers(1);
        let job = submit_noop(50, None, 20);
        let witness = job.clone();
        panel.job = Some(job);
        let _ = panel.request_exit();
        drop(panel);
        assert!(witness.is_cancelled(), "active-change close cancels too");
    }

    /// The drop guard: a panel dropped without any explicit close path
    /// still cancels its live job (defense in depth for future assignment
    /// paths that forget `request_exit`; the shell's explicit closes are
    /// unchanged — their earlier `request_exit` leaves the slots empty so
    /// the drop-side cancel is a no-op there, no double-cancel noise).
    #[test]
    fn drop_cancels_live_job_without_explicit_exit() {
        let mut panel = panel_with_printers(1);
        let job = submit_noop(50, None, 20);
        let witness = job.clone();
        panel.job = Some(job);
        drop(panel);
        assert!(witness.is_cancelled(), "drop alone cancels the live job");
        assert_eq!(wait_terminal(&witness), JobState::Aborted);
    }

    /// Precheck: a 0-page document never reaches submission.
    #[test]
    fn precheck_blocks_empty_document() {
        let panel = PrintPanelState::new(0);
        assert_eq!(
            panel.submit_precheck(1),
            Err("no printable pages".to_string())
        );
    }

    /// An invalid custom spec blocks the submit at precheck time.
    #[test]
    fn invalid_custom_spec_blocks_submit() {
        let mut panel = panel_with_printers(1);
        panel.cycle_range("2");
        panel.edit_settings(|s| {
            s.range = PageRange::Custom {
                spec: "1-3,x".into(),
            }
        });
        assert!(!panel.dialog.errors.is_empty(), "edit revalidates");
        let blocked = panel.try_submit(1).unwrap_err();
        assert!(
            matches!(&blocked, SubmitBlock::Invalid(msg) if msg.contains("x")),
            "got {blocked:?}"
        );
        assert!(panel.pending.is_none());
    }

    /// Parity narrows the resolved list but the freeze still lands on the
    /// resolved page; a parity selection that filters out the current page
    /// resolves to an EMPTY list which the precheck passes through
    /// (resolve only errors on bad specs/counts — the empty-selection
    /// contract belongs to the print kernel, xilem parity).
    #[test]
    fn current_freezes_through_parity_filter() {
        let mut panel = panel_with_printers(1);
        panel.edit_settings(|s| {
            s.range = PageRange::Current;
            s.parity = office_rs_print::Parity::Even;
        });
        // Editor page 4 (even) survives the Even filter.
        assert_eq!(panel.try_submit(4), Ok(()));
        let (_, frozen, _) = panel.take_pending().unwrap();
        assert_eq!(frozen.range, PageRange::Custom { spec: "4".into() });

        // Odd editor page + Even parity → empty resolved list, still Ok.
        assert_eq!(panel.try_submit(3), Ok(()));
        let (_, frozen, _) = panel.take_pending().unwrap();
        assert_eq!(
            frozen.range,
            PageRange::Custom { spec: "3".into() },
            "freeze falls back to the editor page when parity empties the list"
        );
    }

    /// Discovery flow through the machine: Finding → Printers reaches
    /// Ready and enables submit; a backend failure lands in Failed.
    #[test]
    fn discovery_flow_drives_phase() {
        let panel = panel_with_printers(2);
        assert_eq!(panel.dialog.phase, DialogPhase::Ready);
        assert_eq!(panel.dialog.printers.len(), 2);
        assert!(panel.can_submit());

        let mut panel = PrintPanelState::new(3);
        panel.open();
        assert_eq!(panel.dialog.phase, DialogPhase::Finding);
        assert_eq!(panel.status_text(), panel.labels.finding);
        panel.apply_discovery(DiscoverState::Failed("spooler down".into()));
        assert_eq!(panel.dialog.phase, DialogPhase::Failed);
        assert!(panel.status_text().contains("spooler down"));
        assert!(!panel.can_submit());
    }

    /// The render-tick discovery drain consumes a straggler receiver and
    /// releases it at the terminal message.
    #[test]
    fn drain_discovery_consumes_stragglers() {
        let (tx, rx) = mpsc::channel();
        let mut panel = PrintPanelState::new(3);
        panel.open();
        panel.receiver = Some(rx);
        tx.send(DiscoverState::Finding).unwrap();
        tx.send(DiscoverState::Printers(vec!["p".into()])).unwrap();
        drop(tx);
        assert_eq!(panel.drain_discovery(), 2);
        assert_eq!(panel.dialog.phase, DialogPhase::Ready);
        assert!(
            panel.receiver.is_none(),
            "terminal message releases the channel"
        );
        assert_eq!(panel.drain_discovery(), 0);
    }

    /// Printer cycling: discovered list + system default, wrapping.
    #[test]
    fn printer_cycle_wraps_through_system_default() {
        let mut panel = panel_with_printers(2);
        assert_eq!(panel.settings.printer, None, "starts at system default");
        panel.cycle_printer();
        assert_eq!(panel.settings.printer.as_deref(), Some("printer-0"));
        panel.cycle_printer();
        assert_eq!(panel.settings.printer.as_deref(), Some("printer-1"));
        panel.cycle_printer();
        assert_eq!(panel.settings.printer, None, "wraps to system default");
        // An empty list is a no-op.
        let mut empty = PrintPanelState::new(3);
        empty.cycle_printer();
        assert_eq!(empty.settings.printer, None);
    }

    /// Range cycling pre-fills the Custom spec with the caller's page and
    /// REPORTS the landed spec so the caller can sync the display input
    /// (final-review M-2: displayed spec == backing value).
    #[test]
    fn range_cycle_prefills_custom_spec() {
        let mut panel = panel_with_printers(1);
        assert!(matches!(panel.settings.range, PageRange::All));
        assert_eq!(panel.cycle_range("7"), None, "All -> Current");
        assert_eq!(panel.settings.range, PageRange::Current);
        assert_eq!(
            panel.cycle_range("7"),
            Some("7".to_string()),
            "Current -> Custom carries the prefill for input sync"
        );
        assert_eq!(panel.settings.range, PageRange::Custom { spec: "7".into() });
        assert_eq!(panel.cycle_range("7"), None, "Custom -> All");
        assert_eq!(panel.settings.range, PageRange::All);
    }

    /// Paper defaults follow the document's first page (xilem drain ③).
    #[test]
    fn document_defaults_sync_once() {
        let mut panel = PrintPanelState::new(3);
        panel.settings.paper = Paper::Letter;
        panel.settings.orientation = Orientation::Landscape;
        panel.sync_document_defaults(Paper::A4, Orientation::Portrait);
        assert_eq!(panel.settings.paper, Paper::A4);
        assert_eq!(panel.settings.orientation, Orientation::Portrait);
        assert!(panel.defaults_synced);
    }

    /// The submit snapshot carries the full triple (printer override,
    /// frozen settings, content flags) — content flags travel with the
    /// job (xilem `submit_snapshot_carries_content_triple` port).
    #[test]
    fn submit_snapshot_carries_content_triple() {
        let mut panel = panel_with_printers(1);
        panel.cycle_printer();
        panel.edit_content(|c| c.watermark = false);
        assert_eq!(panel.try_submit(2), Ok(()));
        let (printer, settings, content) = panel.take_pending().unwrap();
        assert_eq!(printer.as_deref(), Some("printer-0"));
        assert_eq!(settings, panel.settings);
        assert!(!content.watermark, "the edited flag travels with the job");
        assert!(
            content.annotations,
            "untouched flags travel at the all-on default"
        );
        assert!(content.seal, "untouched flags travel at the all-on default");
    }

    /// Accepted submit: the handle becomes the live job, the phase moves
    /// to Submitted, and further submits are blocked while it is live.
    #[test]
    fn submit_accepted_owns_single_track() {
        let mut panel = panel_with_printers(1);
        assert_eq!(panel.try_submit(1), Ok(()));
        panel.take_pending().unwrap();
        panel.submit_accepted(submit_noop(2, None, 0));
        assert!(panel.job.is_some());
        assert_eq!(panel.dialog.phase, DialogPhase::Submitted);
        assert_eq!(panel.status, panel.labels.submitted);
        assert!(!panel.can_submit());
        assert_eq!(panel.try_submit(1), Err(SubmitBlock::Busy));
    }

    /// Terminal polling: Done → submitted label; Failed → failed label
    /// with the worker error (xilem `poll_job_terminal` port).
    #[test]
    fn terminal_poll_refreshes_status() {
        // Done path. submit_accepted pre-sets the submitted status, so the
        // terminal poll is string-idempotent (xilem parity); clearing the
        // status first exposes the Done mapping.
        let mut panel = panel_with_printers(1);
        panel.submit_accepted(submit_noop(1, None, 0));
        let job = panel.job.clone().expect("live job");
        assert_eq!(wait_terminal(&job), JobState::Done);
        assert!(!panel.poll_job_terminal(), "same text → no change");
        panel.status.clear();
        assert!(panel.poll_job_terminal());
        assert_eq!(panel.status, panel.labels.submitted);
        // Idempotent: a second poll reports no change.
        assert!(!panel.poll_job_terminal());

        // Failed path (worker error text rides along).
        let mut panel = panel_with_printers(1);
        panel.submit_accepted(submit_noop(3, Some(2), 0));
        let job = panel.job.clone().expect("live job");
        assert_eq!(wait_terminal(&job), JobState::Failed);
        assert!(panel.poll_job_terminal());
        assert!(panel.status.contains(&panel.labels.failed));
        assert!(panel.status.contains("page 2"));

        // No live job → no change.
        let mut panel = panel_with_printers(1);
        assert!(!panel.poll_job_terminal());
    }

    /// Validation errors render as zh status copy, never Debug output
    /// (xilem `errors_status_line` port).
    #[test]
    fn validation_errors_render_as_chinese_status() {
        let mut panel = panel_with_printers(1);
        panel.edit_settings(|s| {
            s.range = PageRange::Custom {
                spec: "1-3,x".into(),
            }
        });
        let line = panel.status_text();
        assert!(line.starts_with(&panel.labels.failed));
        assert!(line.contains("页码范围无效"), "got {line}");
        assert!(
            !line.contains("InvalidRangeSpec"),
            "no Debug output: {line}"
        );
    }

    /// Phase-driven status: the no-printers state surfaces the
    /// no-printers label even though the host status is empty.
    #[test]
    fn no_printers_phase_surfaces_label() {
        let mut panel = PrintPanelState::new(3);
        panel.open();
        panel.apply_discovery(DiscoverState::NoPrinters);
        assert_eq!(panel.status_text(), panel.labels.no_printers);
    }

    /// Editing settings revalidates through the machine.
    #[test]
    fn edit_settings_revalidates() {
        let mut panel = panel_with_printers(1);
        panel.edit_settings(|s| s.copies = 2);
        assert_eq!(panel.settings.copies, 2);
        assert!(panel.dialog.errors.is_empty());
    }
}
