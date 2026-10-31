//! Print dialog face (spec 2026-10-31 print phase 2 §2): the xilem view
//! the rofd host renders as a Backstage-style print screen. The BRAIN
//! stays in office-rs-print (settings + dialog state machine + validation)
//! and in rofd-component (print sessions); this module renders [`PrintUi`]
//! state with stock masonry widgets and a canvas preview pager (the
//! session's page-local scenes replayed through the imaging record canvas
//! — gray desk / white page LO-style paper preview).

use crate::print_dialog_state::{advance, DialogEvent, DialogState, Labels};
use office_rs_print::{
    ColorMode, Duplex, Orientation, PageRange, Paper, Parity, PrintSettings, SettingsError,
};
use rofd_component::PrintContent;
use std::sync::{Arc, Mutex};
use xilem::masonry::imaging::kurbo::{Affine, Rect, Size};

use record::Scene as RecordScene;
use xilem::masonry::imaging::record::{Draw, Geometry};
use xilem::masonry::imaging::Composite;
use xilem::masonry::imaging::{peniko, record};
use xilem::masonry::layout::Length;
use xilem::masonry::properties::Padding;
use xilem::style::Style as _;
use xilem::view::{
    canvas, checkbox, flex_col, flex_row, label, sized_box, text_button, text_input,
};
use xilem::WidgetView;

/// Preview material produced from a `PrintSession` at open time (the
/// component is only reachable at command time; scenes are materialized
/// once and shared with the canvas draws).
#[derive(Default)]
pub struct PrintMaterial {
    /// Page-local scenes (index 0-based), one per document page.
    pub scenes: Vec<record::Scene>,
    /// Per-page (width_px, height_px).
    pub dims: Vec<(f32, f32)>,
    /// Per-page (paper, resolved orientation) — the paper authority.
    pub papers: Vec<(Paper, Orientation)>,
}

impl PrintMaterial {
    pub fn page_count(&self) -> usize {
        self.scenes.len()
    }
}

/// Dialog UI state (host-owned; `advance` transitions stay pure).
pub struct PrintUi {
    pub settings: PrintSettings,
    pub state: DialogState,
    pub labels: Labels,
    pub printers: Vec<String>,
    /// 0-based preview page.
    pub page: usize,
    pub material: Arc<Mutex<PrintMaterial>>,
    /// Submitted job (cancel handle) + status line.
    pub job: Option<office_rs_print::JobHandle>,
    pub status: String,
    /// Content flags (watermark/annotations/seal, phase 1 §2.1) — NOT
    /// part of the dialog state machine: the host re-materializes the
    /// preview material when `content_dirty` flips.
    pub content: PrintContent,
    pub content_dirty: bool,
    /// Host actions (state lives in the host's AppState; the closures
    /// mutate the print UI through them).
    pub on_cancel: Option<PrintAction>,
    pub on_submit: Option<PrintAction>,
    /// Set by the default cancel action: app_logic clears the host's
    /// print state on it (the closures only reach the PrintUi, not the
    /// host AppState).
    pub exit: bool,
    /// Confirmation snapshot: (printer override, settings, content
    /// flags). The host drains it on rebuild and runs
    /// [`rofd_component::OfdComponent::print_direct`] in a command (the
    /// dialog's closures cannot reach the component).
    pub pending: Option<(Option<String>, PrintSettings, PrintContent)>,
    /// Whether [`PrintUi::sync_document_defaults`] already ran against a
    /// non-empty material for this dialog. The open-time materialize
    /// command drains one rebuild LATER, so the host re-syncs in its
    /// dialog drain instead of at open time (papers are content-independent,
    /// so once per dialog is enough — a `content_dirty` re-materialize
    /// does not reset this).
    pub defaults_synced: bool,
}

/// Host action: mutates the print UI (cancel / submit are host-owned).
pub type PrintAction = Arc<dyn Fn(&mut PrintUi) + Send + Sync>;

impl PrintUi {
    pub fn new(material: Arc<Mutex<PrintMaterial>>, lang_zh: bool) -> Self {
        let labels = if lang_zh {
            Labels::zh_cn()
        } else {
            Labels::en()
        };
        Self {
            settings: PrintSettings::default(),
            state: DialogState::initial(),
            labels,
            printers: Vec::new(),
            page: 0,
            material,
            job: None,
            status: String::new(),
            content: PrintContent::default(),
            content_dirty: false,
            on_cancel: None,
            on_submit: None,
            exit: false,
            pending: None,
            defaults_synced: false,
        }
    }

    fn advance(&mut self, event: DialogEvent) {
        self.state = advance(&self.state, &self.settings, event);
    }

    pub fn page_count(&self) -> usize {
        self.material.lock().map(|m| m.page_count()).unwrap_or(0)
    }

    /// Content-flag edit shared by the three checkbox callbacks and the
    /// tests: mutate the flags, then mark the material stale. No
    /// `advance` event — content flags sit outside the state machine;
    /// the host re-`begin_print`s when it sees `content_dirty`.
    pub(crate) fn edit_content(&mut self, edit: impl FnOnce(&mut PrintContent)) {
        edit(&mut self.content);
        self.content_dirty = true;
    }

    /// Paper default follows the document (first page, spec §7).
    pub fn sync_document_defaults(&mut self) {
        if let Ok(m) = self.material.lock() {
            if let Some((paper, orientation)) = m.papers.first().copied() {
                self.settings.paper = paper;
                self.settings.orientation = orientation;
            }
        }
    }
}

/// Preview pager clamp shared by the view and the tests: the 0-based
/// preview page bounded by the last material page (empty material → 0,
/// so the pager never renders an out-of-range label).
pub(crate) fn page_clamped(ui: &PrintUi) -> usize {
    let total = ui.page_count();
    ui.page.min(total.saturating_sub(1))
}

/// Validation-error status line: each [`SettingsError`] mapped to a short
/// zh string (this is the dialog's primary validation feedback surface —
/// Rust `Debug` output has no place in the zh UI). The match is exhaustive
/// so a new office-rs-print variant fails the build here, forcing a
/// mapping instead of silently falling back.
pub(crate) fn errors_status_line(errors: &[SettingsError], failed_label: &str) -> String {
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

/// The print screen view. `on_cancel`/`on_submit` are host actions (state
/// mutations on `AppState`); submission itself goes through the host so
/// the payload is built next to the session material.
/// Host accessor: how the view reaches the dialog UI from the host state
/// (the xilem app wires `AppState.print`).
pub trait HasPrintUi {
    fn print_ui(&mut self) -> Option<&mut PrintUi>;
}

pub fn print_dialog_view<State: HasPrintUi + 'static>(
    ui: &mut PrintUi,
) -> impl WidgetView<State> + use<State> {
    let l = &ui.labels;
    let zh = l.title == "打印";

    // ---- settings rows (built as locals, assembled in one tuple) ----

    let printer_label = ui.settings.printer.clone().unwrap_or_else(|| {
        if zh {
            "（系统默认）".into()
        } else {
            "(default)".into()
        }
    });
    let printer_row = flex_row((
        label(format!("{}:", l.printer)),
        text_button(format!("{printer_label} ◂▸"), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            if !ui.printers.is_empty() {
                let idx = ui
                    .printers
                    .iter()
                    .position(|p| Some(p) == ui.settings.printer.as_ref())
                    .map(|i| (i + 1) % (ui.printers.len() + 1))
                    .unwrap_or(0);
                ui.settings.printer = ui.printers.get(idx).cloned();
                ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
            }
        }),
    ));

    let copies_row = flex_row((
        label(format!("{}: {}", l.copies, ui.settings.copies)),
        text_button("−", |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.copies = ui.settings.copies.saturating_sub(1).max(1);
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
        text_button("+", |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.copies = (ui.settings.copies + 1).min(PrintSettings::MAX_COPIES);
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
        checkbox(
            l.collate.clone(),
            ui.settings.collate,
            |state: &mut State, checked| {
                let Some(ui) = state.print_ui() else { return };
                ui.settings.collate = checked;
                ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
            },
        ),
    ));

    let duplex_options = if zh {
        ["单面", "双面(长边)", "双面(短边)"]
    } else {
        ["Off", "Long edge", "Short edge"]
    };
    let duplex_current = match ui.settings.duplex {
        Duplex::Off => duplex_options[0],
        Duplex::LongEdge => duplex_options[1],
        Duplex::ShortEdge => duplex_options[2],
    };
    let color_options = if zh {
        ["彩色", "黑白"]
    } else {
        ["Color", "Gray"]
    };
    let color_current = match ui.settings.color {
        ColorMode::Color => color_options[0],
        ColorMode::Grayscale => color_options[1],
    };
    let duplex_color_row = flex_row((
        label(format!("{}:", l.duplex)),
        text_button(format!("{duplex_current} ▸"), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.duplex = match ui.settings.duplex {
                Duplex::Off => Duplex::LongEdge,
                Duplex::LongEdge => Duplex::ShortEdge,
                Duplex::ShortEdge => Duplex::Off,
            };
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
        label(format!("{}:", l.color)),
        text_button(format!("{color_current} ▸"), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.color = match ui.settings.color {
                ColorMode::Color => ColorMode::Grayscale,
                ColorMode::Grayscale => ColorMode::Color,
            };
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
    ));

    let parity_options = if zh {
        ["全部", "奇数页", "偶数页"]
    } else {
        ["All", "Odd", "Even"]
    };
    let parity_current = match ui.settings.parity {
        Parity::All => parity_options[0],
        Parity::Odd => parity_options[1],
        Parity::Even => parity_options[2],
    };
    let range_options = if zh {
        ["全部", "当前页", "自定义"]
    } else {
        ["All", "Current", "Custom"]
    };
    let range_current = match &ui.settings.range {
        PageRange::All => range_options[0],
        PageRange::Current => range_options[1],
        PageRange::Custom { .. } => range_options[2],
    };
    let spec = match &ui.settings.range {
        PageRange::Custom { spec } => spec.clone(),
        _ => String::new(),
    };
    let range_row = flex_row((
        label(format!("{}:", l.range)),
        text_button(format!("{range_current} ▸"), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.range = match &ui.settings.range {
                PageRange::All => PageRange::Current,
                PageRange::Current => PageRange::Custom {
                    spec: String::new(),
                },
                PageRange::Custom { .. } => PageRange::All,
            };
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
        text_input(spec, |state: &mut State, input: String| {
            let Some(ui) = state.print_ui() else { return };
            if let PageRange::Custom { spec } = &mut ui.settings.range {
                *spec = input;
            }
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
        text_button(format!("{} ▸", parity_current), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            ui.settings.parity = match ui.settings.parity {
                Parity::All => Parity::Odd,
                Parity::Odd => Parity::Even,
                Parity::Even => Parity::All,
            };
            ui.advance(DialogEvent::SettingsChanged(ui.settings.clone()));
        }),
    ));

    // Content flags (phase 1 §2.1): three checkboxes, no state-machine
    // event — any toggle just marks the material stale.
    let content_labels = if zh {
        ["水印", "批注", "图章"]
    } else {
        ["Watermark", "Annotations", "Seal"]
    };
    let content_row = flex_row((
        checkbox(
            content_labels[0].to_string(),
            ui.content.watermark,
            |state: &mut State, checked| {
                let Some(ui) = state.print_ui() else { return };
                ui.edit_content(|c| c.watermark = checked);
            },
        ),
        checkbox(
            content_labels[1].to_string(),
            ui.content.annotations,
            |state: &mut State, checked| {
                let Some(ui) = state.print_ui() else { return };
                ui.edit_content(|c| c.annotations = checked);
            },
        ),
        checkbox(
            content_labels[2].to_string(),
            ui.content.seal,
            |state: &mut State, checked| {
                let Some(ui) = state.print_ui() else { return };
                ui.edit_content(|c| c.seal = checked);
            },
        ),
    ));

    let status_text = if !ui.state.errors.is_empty() {
        errors_status_line(&ui.state.errors, &l.failed)
    } else if !ui.status.is_empty() {
        ui.status.clone()
    } else {
        String::new()
    };
    // Submit gating: the state machine's can_submit() (ready phase, no
    // validation errors, printers discovered) AND no in-flight submission
    // (pending not yet drained / a live job) — mirrors the host-side
    // on_submit guard so a double click cannot spool a duplicate job.
    let can_submit = ui.state.can_submit() && ui.pending.is_none() && ui.job.is_none();
    let actions_row = flex_row((
        text_button(l.cancel.clone(), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            if let Some(f) = ui.on_cancel.clone() {
                f(ui);
            }
        }),
        text_button(format!("{} 🖨", l.submit), |state: &mut State| {
            let Some(ui) = state.print_ui() else { return };
            if let Some(f) = ui.on_submit.clone() {
                f(ui);
            }
        })
        .disabled(!can_submit),
    ));

    let settings_col = flex_col((
        label(format!("🖨  {}", l.title)).text_size(20.0),
        printer_row,
        copies_row,
        duplex_color_row,
        range_row,
        content_row,
        label(status_text),
        actions_row,
    ));

    // ---- preview pager ----
    let total = ui.page_count();
    let page = page_clamped(ui);
    let material = ui.material.clone();
    let preview = flex_col((
        canvas(
            move |_state: &mut State, _ctx, scene: &mut RecordScene, size: Size| {
                draw_preview(&material, page, scene, size);
            },
        )
        .alt_text("print preview"),
        flex_row((
            text_button("◀", |state: &mut State| {
                let Some(ui) = state.print_ui() else { return };
                if ui.page > 0 {
                    ui.page -= 1;
                }
            }),
            label(format!("{} / {}", page + 1, total.max(1))),
            text_button("▶", |state: &mut State| {
                let Some(ui) = state.print_ui() else { return };
                let total = ui.page_count();
                if ui.page + 1 < total {
                    ui.page += 1;
                }
            }),
        )),
    ));

    flex_row((
        sized_box(settings_col)
            .fixed_width(Length::const_px(360.0))
            .padding(Padding::from_vh(
                Length::const_px(24.0),
                Length::const_px(24.0),
            )),
        preview,
    ))
}

/// Gray desk + white page + the recorded page scene, fit-contained. The
/// canvas in this xilem rev IS an `imaging::record::Scene`, so the page
/// material replays via `append_transformed` (no vello hop needed).
fn draw_preview(
    material: &Arc<Mutex<PrintMaterial>>,
    page: usize,
    scene: &mut RecordScene,
    size: Size,
) {
    let solid = |rgba: [u8; 4]| {
        peniko::Brush::Solid(peniko::Color::from_rgba8(
            rgba[0], rgba[1], rgba[2], rgba[3],
        ))
    };
    let fill = |scene: &mut RecordScene, brush, transform, rect: (f64, f64, f64, f64)| {
        scene.draw(Draw::Fill {
            transform,
            fill_rule: peniko::Fill::NonZero,
            brush,
            brush_transform: None,
            shape: Geometry::Rect(Rect::new(rect.0, rect.1, rect.2, rect.3)),
            composite: Composite::default(),
        });
    };
    // Desk.
    fill(
        scene,
        solid([0xE0, 0xE0, 0xE0, 0xFF]),
        Affine::IDENTITY,
        (0.0, 0.0, size.width, size.height),
    );
    let Ok(m) = material.lock() else { return };
    let Some(record) = m.scenes.get(page) else {
        return;
    };
    let (w, h) = m.dims.get(page).copied().unwrap_or((595.0, 842.0));
    let scale =
        ((size.width - 24.0).max(1.0) / w as f64).min((size.height - 24.0).max(1.0) / h as f64);
    let ox = (size.width - w as f64 * scale) / 2.0;
    let oy = (size.height - h as f64 * scale) / 2.0;
    // White page under the content.
    fill(
        scene,
        solid([0xFF, 0xFF, 0xFF, 0xFF]),
        Affine::translate((ox, oy)) * Affine::scale(scale),
        (0.0, 0.0, w as f64, h as f64),
    );
    scene.append_transformed(record, Affine::translate((ox, oy)) * Affine::scale(scale));
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_rs_print::{Orientation, Paper};
    use std::sync::{Arc, Mutex};

    fn ui_with_material(material: PrintMaterial) -> PrintUi {
        PrintUi::new(Arc::new(Mutex::new(material)), true)
    }

    /// Flipping any content flag through the checkbox-callback path marks
    /// the dialog content-dirty (the host re-materializes on it).
    #[test]
    fn content_toggle_marks_dirty() {
        let mut ui = ui_with_material(PrintMaterial::default());
        assert!(!ui.content_dirty, "fresh dialog is not content-dirty");

        ui.edit_content(|c| c.watermark = true);
        assert!(ui.content_dirty);
        assert!(ui.content.watermark);

        ui.content_dirty = false;
        ui.edit_content(|c| c.annotations = true);
        assert!(ui.content_dirty);
        assert!(ui.content.annotations);

        ui.content_dirty = false;
        ui.edit_content(|c| c.seal = false);
        assert!(ui.content_dirty);
        assert!(!ui.content.seal);
    }

    /// The submit snapshot carries the full triple: printer override,
    /// settings, and the content flags (materialized copy, not a borrow).
    #[test]
    fn submit_snapshot_carries_content_triple() {
        let mut ui = ui_with_material(PrintMaterial::default());
        ui.settings.printer = Some("Microsoft Print to PDF".into());
        ui.edit_content(|c| c.annotations = true);
        // Host submit-action shape: snapshot all three parts into pending.
        ui.pending = Some((ui.settings.printer.clone(), ui.settings.clone(), ui.content));
        let (printer, settings, content) = ui.pending.take().unwrap();
        assert_eq!(printer.as_deref(), Some("Microsoft Print to PDF"));
        assert_eq!(settings, ui.settings);
        assert_eq!(content, ui.content);
        assert!(content.annotations, "content flags travel with the job");
    }

    /// Paper authority is the document: papers[0] drives the default
    /// paper/orientation (spec §7, inherited from phase 1).
    #[test]
    fn sync_document_defaults_reads_first_paper() {
        let mut material = PrintMaterial::default();
        material.papers.push((Paper::A4, Orientation::Portrait));
        let mut ui = ui_with_material(material);
        // Diverge first so the sync is proven to read the material
        // (PrintSettings::default() is already A4/Portrait).
        ui.settings.paper = Paper::Letter;
        ui.settings.orientation = Orientation::Landscape;
        ui.sync_document_defaults();
        assert_eq!(ui.settings.paper, Paper::A4);
        assert_eq!(ui.settings.orientation, Orientation::Portrait);
    }

    /// Empty material: page_count 0 and the shared pager clamp yields 0
    /// (no underflow, no out-of-range preview label).
    #[test]
    fn initial_page_zero_clamped_against_empty_material() {
        let ui = ui_with_material(PrintMaterial::default());
        assert_eq!(ui.page_count(), 0);
        assert_eq!(page_clamped(&ui), 0);
    }

    /// A fresh dialog has not yet synced its paper defaults (the host's
    /// drain re-syncs once the materialize command lands).
    #[test]
    fn fresh_dialog_is_defaults_unsynced() {
        let ui = ui_with_material(PrintMaterial::default());
        assert!(!ui.defaults_synced);
    }

    /// Validation errors render as zh strings behind the failed label,
    /// never as Rust Debug output (Carry B).
    #[test]
    fn validation_errors_render_as_chinese_status() {
        let errors = [
            SettingsError::InvalidRangeSpec {
                reason: "invalid page number \"x\"".to_string(),
            },
            SettingsError::RangeSpecTooLong { len: 201 },
        ];
        let line = errors_status_line(&errors, "打印失败");
        assert!(
            line.contains("页码范围无效：invalid page number \"x\""),
            "invalid spec mapped to zh: {line}"
        );
        assert!(
            line.contains("页码范围过长（201 字符）"),
            "too-long spec mapped to zh: {line}"
        );
        assert!(
            line.starts_with("打印失败: "),
            "failed label prefixes: {line}"
        );
        assert!(
            !line.contains("SettingsError"),
            "no Rust Debug leaks: {line}"
        );
    }
}
