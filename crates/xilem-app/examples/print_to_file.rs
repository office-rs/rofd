//! Manual print-to-file test channel (print phase 2, task 6).
//!
//! Loads an `.ofd` through the host's `load_ofd`, snapshots the document
//! with `begin_print`, and routes the all-pages vector payload to a file
//! instead of the spooler port via office-rs-print's `print_pages_to_file`
//! (Windows GDI sets `DOCINFOW.lpszOutput`). With a PDF driver as the
//! target printer (e.g. "Microsoft Print to PDF") the output is a real
//! PDF; other drivers materialize their spool format.
//!
//! Channel choice: the Pages payload is submitted as-is — the backend
//! accepts it natively, so the PDF detour (`rofd-export` +
//! `print_pdf_to_file`) is not needed here.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p xilem-app --example print_to_file -- test/ru-yuan-ji-lu.ofd target/print-probe.out
//! ```

use std::ffi::OsString;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use office_rs_print::{print_pages_to_file, JobState, PrintSettings};
use rofd_component::{OfdComponent, OfdConfig, PrintContent};

use xilem_app::host::document_io::load_ofd;

const USAGE: &str =
    "usage: cargo run -p xilem-app --example print_to_file -- <file.ofd> <out_file>";

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("{USAGE}");
        process::exit(1);
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);

    let loaded = match load_ofd(&input) {
        Ok(loaded) => loaded,
        Err(message) => {
            eprintln!("error: {message}");
            process::exit(1);
        }
    };
    for warning in &loaded.warnings {
        eprintln!("warning: {warning:?}");
    }

    // Host pattern (crates/xilem-app/src/main.rs): empty font config,
    // glyph rendering falls back to system fonts.
    let mut component = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    component.load_document(loaded.document);

    let mut session = component.begin_print(PrintContent::default());
    let total = session.page_count();
    if total == 0 {
        eprintln!("error: no printable pages in {}", input.display());
        process::exit(1);
    }
    let all_pages: Vec<usize> = (1..=total).collect();
    let payload = session.payload_for(&all_pages);

    let title = format!("rofd print_to_file {}", input.display());
    let handle = print_pages_to_file(
        payload,
        &PrintSettings::default(),
        &title,
        Some(output.clone()),
    );
    // The example owns the only joining handle, so `wait` (not `join_all`)
    // is the correct shutdown point.
    let snapshot = handle.wait();

    println!("print_to_file: {total} page(s) -> {}", output.display());
    println!(
        "job state: {:?} (pages {}/{})",
        snapshot.state, snapshot.pages_done, snapshot.pages_total
    );
    if let Some(error) = &snapshot.error {
        eprintln!("error: {error}");
    }

    let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    if snapshot.state == JobState::Done && bytes > 0 {
        println!("output file: {} ({bytes} bytes)", output.display());
    } else {
        if snapshot.state == JobState::Done {
            eprintln!(
                "error: job reported Done but the output file is missing or empty: {}",
                output.display()
            );
        }
        process::exit(1);
    }
}
