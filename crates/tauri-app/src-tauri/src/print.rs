//! Print IPC bridge (print spec 2026-10-31 §5.2): the WebView panel submits
//! through Rust — discovery + spooling happen in the shell process via
//! office-rs-print (an external library; the shell still depends on NO rofd
//! crate, AGENTS §4.9). Progress flows back as `print-progress` events.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use office_rs_print::{JobHandle, PrintSettings};

/// Live jobs by id (cancel + progress polling).
static JOBS: std::sync::OnceLock<Mutex<HashMap<u32, JobHandle>>> = std::sync::OnceLock::new();
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn jobs() -> &'static Mutex<HashMap<u32, JobHandle>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterEntry {
    pub name: String,
    pub is_default: bool,
    pub port: Option<String>,
}

#[tauri::command]
pub fn print_discover() -> Result<Vec<PrinterEntry>, String> {
    Ok(office_rs_print::list_printers()
        .into_iter()
        .map(|p| PrinterEntry {
            name: p.name,
            is_default: p.is_default,
            port: p.port,
        })
        .collect())
}

/// Print a PDF payload (base64; a few MB where a serialized scene payload
/// would embed tens of MB of font bytes — the cross-process payload
/// discipline, print spec §1).
#[tauri::command]
pub async fn print_pdf(
    app: tauri::AppHandle,
    pdf_b64: String,
    settings: PrintSettings,
    title: String,
) -> Result<u32, String> {
    let pdf = base64::engine::general_purpose::STANDARD
        .decode(pdf_b64.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = office_rs_print::print_pdf(&pdf, &settings, &title);
    jobs().lock().unwrap().insert(id, handle);
    // Progress pump: poll snapshots onto the `print-progress` event until a
    // terminal state, then drop the job from the table.
    let pump_id = id;
    tauri::async_runtime::spawn(async move {
        loop {
            let snapshot = {
                let table = jobs().lock().unwrap();
                match table.get(&pump_id) {
                    Some(handle) => handle.snapshot(),
                    None => break,
                }
            };
            let _ = tauri::Emitter::emit(
                &app,
                "print-progress",
                serde_json::json!({
                    "jobId": pump_id,
                    "state": format!("{:?}", snapshot.state),
                    "pagesDone": snapshot.pages_done,
                    "pagesTotal": snapshot.pages_total,
                    "error": snapshot.error,
                }),
            );
            let terminal = matches!(
                snapshot.state,
                office_rs_print::JobState::Done
                    | office_rs_print::JobState::Aborted
                    | office_rs_print::JobState::Failed
            );
            if terminal {
                jobs().lock().unwrap().remove(&pump_id);
                break;
            }
            tauri::async_runtime::spawn(async move {
                std::thread::sleep(Duration::from_millis(200));
            })
            .await
            .ok();
        }
    });
    Ok(id)
}

#[tauri::command]
pub fn print_cancel(job_id: u32) -> Result<bool, String> {
    let table = jobs().lock().unwrap();
    match table.get(&job_id) {
        Some(handle) => {
            handle.cancel();
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Join in-flight spool jobs (window-close veto path).
#[tauri::command]
pub fn print_join_all() {
    office_rs_print::join_all();
}

#[cfg(test)]
mod tests {
    use office_rs_print::{
        ColorMode, Duplex, Orientation, PageRange, Paper, Parity, PrintSettings,
    };

    /// Pin the dialog (Task 8) JSON wire shape field by field: unit enums
    /// cross the wire as plain PascalCase strings, `PageRange` as an
    /// internally tagged (`kind`) object. If office-rs-print ever drifts,
    /// this fails before the dialog ships against it.
    #[test]
    fn print_settings_deserializes_dialog_wire_shape() {
        let json = r#"{"printer":null,"copies":2,"collate":true,"duplex":"Off","color":"Color",
                       "paper":"A4","orientation":"Portrait",
                       "range":{"kind":"Custom","spec":"1-3,5"},"parity":"Odd"}"#;
        let s: PrintSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.printer, None);
        assert_eq!(s.copies, 2);
        assert!(s.collate);
        assert_eq!(s.duplex, Duplex::Off);
        assert_eq!(s.color, ColorMode::Color);
        assert_eq!(s.paper, Paper::A4);
        assert_eq!(s.orientation, Orientation::Portrait);
        assert_eq!(
            s.range,
            PageRange::Custom {
                spec: "1-3,5".to_string()
            }
        );
        assert_eq!(s.parity, Parity::Odd);
    }
}
