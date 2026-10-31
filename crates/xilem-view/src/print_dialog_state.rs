//! Print dialog state machine — the brain of rofd's native xilem print
//! dialog (ported from rword's print_dialog_state: a UI-consumed state
//! model, not spooling). Discovery wiring stays behind the
//! [`PrintServices`] trait; submission bypasses this seam (it flows
//! through `OfdComponent::print_direct`); the dialog state advances
//! through pure [`advance`] transitions; [`Labels`] carries the i18n
//! strings.

use office_rs_print::{validate_settings, PrintSettings};
use std::sync::mpsc;

/// Discovery progress as emitted by a [`PrintServices::discover`] receiver
/// (one terminal-ish message per state).
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoverState {
    Finding,
    Printers(Vec<String>),
    NoPrinters,
    Failed(String),
}

/// Dialog phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogPhase {
    Idle,
    Finding,
    Ready,
    Submitting,
    Submitted,
    Failed,
}

/// Panel state: phase + the discovery result + the last validation
/// snapshot. The settings object itself stays host-owned (the host applies
/// edits and feeds them back through [`DialogEvent::SettingsChanged`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DialogState {
    pub phase: DialogPhase,
    pub printers: Vec<String>,
    pub errors: Vec<office_rs_print::SettingsError>,
    pub last_error: Option<String>,
}

impl DialogState {
    pub fn initial() -> Self {
        Self {
            phase: DialogPhase::Idle,
            printers: Vec::new(),
            errors: Vec::new(),
            last_error: None,
        }
    }

    /// Whether the submit control should be enabled (host rendering hint).
    pub fn can_submit(&self) -> bool {
        matches!(self.phase, DialogPhase::Ready | DialogPhase::Failed)
            && self.errors.is_empty()
            && !self.printers.is_empty()
    }
}

/// Service/host events driving the state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum DialogEvent {
    /// The panel opened / discovery was kicked off.
    DiscoveryStarted,
    /// A discovery update arrived (Finding → Printers/NoPrinters/Failed).
    DiscoveryUpdate(DiscoverState),
    /// The host edited the settings (revalidates).
    SettingsChanged(PrintSettings),
    /// The submit button was pressed.
    SubmitPressed,
    /// The service accepted the job.
    SubmitAccepted,
    /// The service rejected the job.
    SubmitFailed(String),
    /// Close/reopen: back to idle.
    Reset,
}

/// Pure state transition. No I/O — hosts wire real services around it.
pub fn advance(state: &DialogState, settings: &PrintSettings, event: DialogEvent) -> DialogState {
    let mut next = state.clone();
    match event {
        DialogEvent::DiscoveryStarted => {
            next.phase = DialogPhase::Finding;
            next.printers.clear();
            next.last_error = None;
        }
        DialogEvent::DiscoveryUpdate(update) => match update {
            DiscoverState::Finding => next.phase = DialogPhase::Finding,
            DiscoverState::Printers(printers) => {
                next.printers = printers;
                next.phase = DialogPhase::Ready;
            }
            DiscoverState::NoPrinters => {
                next.printers.clear();
                next.phase = DialogPhase::Ready;
            }
            DiscoverState::Failed(msg) => {
                next.printers.clear();
                next.phase = DialogPhase::Failed;
                next.last_error = Some(msg);
            }
        },
        DialogEvent::SettingsChanged(s) => {
            next.errors = validate_settings(&s);
        }
        DialogEvent::SubmitPressed => {
            // Validate one more time against the event's settings view.
            next.errors = validate_settings(settings);
            if !next.errors.is_empty() {
                next.phase = DialogPhase::Failed;
                next.last_error = Some(format!("{:?}", next.errors[0]));
            } else if state.printers.is_empty() {
                // Nothing to submit to (native semantics).
                next.phase = DialogPhase::Failed;
                next.last_error = Some("no printers".to_string());
            } else {
                next.phase = DialogPhase::Submitting;
                next.last_error = None;
            }
        }
        DialogEvent::SubmitAccepted => {
            next.phase = DialogPhase::Submitted;
        }
        DialogEvent::SubmitFailed(msg) => {
            next.phase = DialogPhase::Failed;
            next.last_error = Some(msg);
        }
        DialogEvent::Reset => {
            next.phase = DialogPhase::Idle;
            next.printers.clear();
            next.errors.clear();
            next.last_error = None;
        }
    }
    next
}

/// i18n label set for the panel face (both shipped languages cover every
/// control; hosts may build their own).
#[derive(Debug, Clone, PartialEq)]
pub struct Labels {
    pub title: String,
    pub printer: String,
    pub copies: String,
    pub collate: String,
    pub duplex: String,
    pub color: String,
    pub paper: String,
    pub orientation: String,
    pub range: String,
    pub parity: String,
    pub submit: String,
    pub cancel: String,
    pub finding: String,
    pub no_printers: String,
    pub submitted: String,
    pub failed: String,
}

impl Labels {
    pub fn zh_cn() -> Self {
        Self {
            title: "打印".into(),
            printer: "打印机".into(),
            copies: "份数".into(),
            collate: "逐份打印".into(),
            duplex: "双面打印".into(),
            color: "颜色".into(),
            paper: "纸张".into(),
            orientation: "方向".into(),
            range: "页码范围".into(),
            parity: "奇偶页".into(),
            submit: "打印".into(),
            cancel: "取消".into(),
            finding: "正在查找打印机…".into(),
            no_printers: "未找到打印机".into(),
            submitted: "已发送到打印机".into(),
            failed: "打印失败".into(),
        }
    }

    pub fn en() -> Self {
        Self {
            title: "Print".into(),
            printer: "Printer".into(),
            copies: "Copies".into(),
            collate: "Collate".into(),
            duplex: "Two-sided".into(),
            color: "Color".into(),
            paper: "Paper".into(),
            orientation: "Orientation".into(),
            range: "Pages".into(),
            parity: "Odd/Even".into(),
            submit: "Print".into(),
            cancel: "Cancel".into(),
            finding: "Finding printers…".into(),
            no_printers: "No printers found".into(),
            submitted: "Sent to printer".into(),
            failed: "Print failed".into(),
        }
    }
}

/// UI-agnostic service seam: the native host injects the crate's printer
/// discovery. Submission is NOT part of this seam — rofd submits through
/// `OfdComponent::print_direct`, so only discovery lives here.
pub trait PrintServices {
    /// Emits `Finding` first, then a terminal message. The receiver is
    /// consumed by the host loop (one `try_recv` per frame / on wake).
    fn discover(&self) -> mpsc::Receiver<DiscoverState>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_rs_print::{PageRange, SettingsError};

    fn state() -> DialogState {
        DialogState::initial()
    }

    fn settings() -> PrintSettings {
        PrintSettings::default()
    }

    #[test]
    fn discovery_flow_reaches_ready() {
        let mut s = state();
        s = advance(&s, &settings(), DialogEvent::DiscoveryStarted);
        assert_eq!(s.phase, DialogPhase::Finding);
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Printers(vec![
                "Microsoft Print to PDF".into(),
                "HP LaserJet".into(),
            ])),
        );
        assert_eq!(s.phase, DialogPhase::Ready);
        assert_eq!(s.printers.len(), 2);
        assert!(s.can_submit(), "ready + valid + printers → submittable");
    }

    #[test]
    fn no_printers_blocks_submit() {
        let mut s = state();
        s = advance(&s, &settings(), DialogEvent::DiscoveryStarted);
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::NoPrinters),
        );
        assert_eq!(s.phase, DialogPhase::Ready);
        assert!(s.printers.is_empty());
        s = advance(&s, &settings(), DialogEvent::SubmitPressed);
        assert_eq!(s.phase, DialogPhase::Failed);
        assert_eq!(s.last_error.as_deref(), Some("no printers"));
    }

    #[test]
    fn discovery_failure_lands_in_failed() {
        let mut s = state();
        s = advance(&s, &settings(), DialogEvent::DiscoveryStarted);
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Failed("spooler down".into())),
        );
        assert_eq!(s.phase, DialogPhase::Failed);
        assert_eq!(s.last_error.as_deref(), Some("spooler down"));
    }

    #[test]
    fn settings_change_updates_validation() {
        let mut s = state();
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Printers(vec!["p".into()])),
        );
        let bad = PrintSettings {
            range: PageRange::Custom {
                spec: "1-3,x".into(),
            },
            ..PrintSettings::default()
        };
        s = advance(&s, &bad, DialogEvent::SettingsChanged(bad.clone()));
        assert_eq!(
            s.errors,
            vec![SettingsError::InvalidRangeSpec {
                reason: "invalid page number \"x\"".to_string()
            }]
        );
        // Submit while invalid → Failed, not Submitting.
        s = advance(&s, &bad, DialogEvent::SubmitPressed);
        assert_eq!(s.phase, DialogPhase::Failed);
        assert!(!s.can_submit());
    }

    #[test]
    fn happy_submit_flow() {
        let mut s = state();
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Printers(vec!["p".into()])),
        );
        s = advance(&s, &settings(), DialogEvent::SubmitPressed);
        assert_eq!(s.phase, DialogPhase::Submitting);
        assert!(!s.can_submit(), "no double submit while submitting");
        s = advance(&s, &settings(), DialogEvent::SubmitAccepted);
        assert_eq!(s.phase, DialogPhase::Submitted);
    }

    #[test]
    fn submit_failure_keeps_dialog_usable() {
        let mut s = state();
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Printers(vec!["p".into()])),
        );
        s = advance(&s, &settings(), DialogEvent::SubmitPressed);
        s = advance(&s, &settings(), DialogEvent::SubmitFailed("offline".into()));
        assert_eq!(s.phase, DialogPhase::Failed);
        assert_eq!(s.last_error.as_deref(), Some("offline"));
        // Retry is possible (phase Failed + no validation errors).
        assert!(s.can_submit());
        s = advance(&s, &settings(), DialogEvent::SubmitPressed);
        assert_eq!(s.phase, DialogPhase::Submitting);
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut s = state();
        s = advance(
            &s,
            &settings(),
            DialogEvent::DiscoveryUpdate(DiscoverState::Printers(vec!["p".into()])),
        );
        s = advance(&s, &settings(), DialogEvent::Reset);
        assert_eq!(s.phase, DialogPhase::Idle);
        assert!(s.printers.is_empty());
    }

    #[test]
    fn labels_cover_both_languages() {
        let zh = Labels::zh_cn();
        let en = Labels::en();
        assert_eq!(zh.title, "打印");
        assert_eq!(en.title, "Print");
        assert_ne!(zh.submit, en.submit);
    }

    /// The service seam is implementable with a plain mpsc pair.
    #[test]
    fn print_services_trait_object_works() {
        struct FakeServices;
        impl PrintServices for FakeServices {
            fn discover(&self) -> mpsc::Receiver<DiscoverState> {
                let (tx, rx) = mpsc::channel();
                tx.send(DiscoverState::Finding).unwrap();
                tx.send(DiscoverState::Printers(vec!["fake".into()]))
                    .unwrap();
                rx
            }
        }
        let services: Box<dyn PrintServices> = Box::new(FakeServices);
        let rx = services.discover();
        assert_eq!(rx.recv().unwrap(), DiscoverState::Finding);
        assert_eq!(
            rx.recv().unwrap(),
            DiscoverState::Printers(vec!["fake".into()])
        );
    }
}
