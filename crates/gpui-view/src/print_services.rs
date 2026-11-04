// Ported from crates/xilem-view/src/print_services.rs (print phase 2,
// spec 2026-10-31).

//! Native print services (spec 2026-10-31 print phase 2 §2): the
//! office-rs-print backend behind the print dialog face — discovery on a
//! worker thread (mpsc receiver). Submission is NOT part of this seam:
//! it flows through `rofd_component::OfdComponent::print_direct`, so the
//! [`PrintServices`] trait stays discover-only.

use crate::print_dialog_state::{DiscoverState, PrintServices};
use std::sync::mpsc;

/// Backend services for the native dialog.
pub struct NativePrintServices;

impl PrintServices for NativePrintServices {
    fn discover(&self) -> mpsc::Receiver<DiscoverState> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(DiscoverState::Finding);
            let printers = office_rs_print::list_printers();
            if printers.is_empty() {
                let _ = tx.send(DiscoverState::NoPrinters);
            } else {
                let _ = tx.send(DiscoverState::Printers(
                    printers.into_iter().map(|p| p.name).collect(),
                ));
            }
        });
        rx
    }
}
