//! The signing flow of the xilem host (spec 2026-11-23 §10, §11, §13,
//! M13, M16, M17) over the command queue: the component is only reachable
//! at command-drain time, so commands snapshot what is needed into slots
//! and the next `app_logic` pass acts on them. The decisions are in
//! `super`; this module wires them to the app's state, the editor and the
//! files.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rfd::FileDialog;
use rofd_component::{SealPlacement, SealPreview};
use rofd_io::{write_ofd_with_text_layout, Preserved};
use rofd_sign::{SignatureReport, VerifyOptions};

use super::{
    host_now, pre_save_check, prepare_sign, read_seal_image, save_notes, seal_preview_size,
    sign_refusal_text, signing_material, store_file, SaveCheck, SaveConfirm, SignDialog,
    SigningState,
};
use crate::AppState;

/// A queued command's fill of the sign slot: the reports of every signature
/// once the signature is in the document, or why it is not.
pub(crate) type SignOutcome = Arc<Mutex<Option<Result<Vec<SignatureReport>, String>>>>;

/// The document's seal pictures, in signature-list order (`None` for a
/// signature without one), filled by a command after every document swap.
pub(crate) type SealPictures = Arc<Mutex<Vec<Option<Arc<Vec<u8>>>>>>;

/// A save whose bytes must not be written without the user's word (M16,
/// M17): a save command fills it and [`drain_save_candidate`] asks.
pub(crate) type SaveCandidateSlot = Arc<Mutex<Option<SaveCandidate>>>;

#[derive(Debug)]
pub(crate) struct SaveCandidate {
    pub(crate) path: PathBuf,
    /// The preserving save's own output.
    pub(crate) bytes: Vec<u8>,
    /// Why the bytes wait: `Blocked`, or the signatures they invalidate.
    pub(crate) check: SaveCheck,
}

/// The state and slots a new `AppState` starts the signing line with.
pub(crate) fn initial() -> (SigningState, SignOutcome, SealPictures, SaveCandidateSlot) {
    // The certificate store as it was left; one that cannot be read is an
    // empty one for this run, and the log says so.
    let (store, error) = store_file::load_store();
    if let Some(error) = error {
        eprintln!("[warning] {error}");
    }
    (
        SigningState {
            store,
            ..SigningState::default()
        },
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(None)),
    )
}

/// Push a host command onto the host→widget channel.
fn push(app: &mut AppState, f: impl Fn(&mut rofd_component::OfdComponent) + Send + Sync + 'static) {
    app.commands.lock().unwrap().push(Arc::new(f));
}

/// A document swap (new, open): the placement, the dialog and the open
/// question belonged to the document that was there. The placement state
/// itself lives in the component, which drops it when the document is
/// replaced. A signature is not a swap: the document, and what belonged to
/// it, stays.
pub(crate) fn document_replaced(app: &mut AppState) {
    app.signing.pending = None;
    app.signing.pending_placed = None;
    app.signing.dialog = None;
    app.signing.importing = None;
    app.signing.detail_open = false;
    app.signing.save_confirm = None;
    refresh_seal_pictures(app);
}

/// The seal pictures of the document in the component, for the detail
/// panel's thumbnails: read at the next drain, when the component is
/// reachable.
pub(crate) fn refresh_seal_pictures(app: &mut AppState) {
    let pictures = app.seal_pictures.clone();
    push(app, move |c| *pictures.lock().unwrap() = c.seal_pictures());
}

// ---- the dialog ----

/// 签章 button: the dialog, with the store's certificates.
pub(crate) fn open_sign_dialog(app: &mut AppState) {
    let first = app
        .signing
        .store
        .list()
        .first()
        .map(|entry| entry.id.clone());
    app.signing.dialog = Some(SignDialog {
        certificate: first,
        ..SignDialog::new()
    });
    app.signing.importing = None;
}

/// 取消: the dialog closes; a certificate picked for import is forgotten.
pub(crate) fn cancel_sign_dialog(app: &mut AppState) {
    app.signing.dialog = None;
    app.signing.importing = None;
}

/// 确定: the password is checked with the certificate file (M7). Then the
/// document is signed right away - no picture, or auto-stamped - or the
/// seal placement state is entered at the next command drain and the sign
/// waits for the click. A refused dialog stays open with the reason.
pub(crate) fn confirm_sign_dialog(app: &mut AppState) {
    let Some(dialog) = app.signing.dialog.take() else {
        return;
    };
    match prepare_sign(&dialog, &app.signing.store, &host_now()) {
        Ok(pending) => {
            app.signing.importing = None;
            app.signing.pending_placed = None;
            let direct = pending.request.signs_directly();
            app.signing.pending = Some(pending);
            if direct {
                queue_sign(app);
            } else {
                queue_seal_placement(app, &dialog);
            }
        }
        Err(error) => {
            app.signing.dialog = Some(SignDialog {
                error: Some(error),
                ..dialog
            });
        }
    }
}

/// The sign slot gets the reason when the signature was not even started.
fn refuse(slot: &SignOutcome, reason: &str) {
    *slot.lock().unwrap() = Some(Err(reason.to_owned()));
}

/// Queue the seal placement state on the component: the preview follows
/// the pointer until the click. A picture the component cannot read comes
/// back through the sign slot.
fn queue_seal_placement(app: &mut AppState, dialog: &SignDialog) {
    let slot = app.sign_outcome.clone();
    let Some(pending) = app.signing.pending.as_ref() else {
        return;
    };
    let size = seal_preview_size(&dialog.seal);
    let picture = pending.seal_png.clone();
    let (Some((width_mm, height_mm)), Some(png)) = (size, picture) else {
        refuse(&slot, "没有章图，或章图无法读取");
        return;
    };
    app.status = "请在页面上点击左键放置签章（Esc 或右键取消）".to_string();
    let png = Arc::new(png);
    push(app, move |c| {
        let preview = SealPreview {
            png: png.clone(),
            width_mm,
            height_mm,
        };
        if !c.begin_seal_placement(preview) {
            refuse(&slot, "章图无法读取");
        }
    });
}

/// Queue the signature (spec 2026-11-23 §17.11): the command signs on the
/// component at the next drain, one call into the document that is open,
/// and the outcome lands in the sign slot. Said first, in the host's
/// words: a document whose file cannot take its annotations.
fn queue_sign(app: &mut AppState) {
    let Some(pending) = app.signing.pending.take() else {
        return;
    };
    let placed = app.signing.pending_placed.take();
    let slot = app.sign_outcome.clone();
    push(app, move |c| {
        let done =
            signing_material(&pending, &host_now(), placed).and_then(|(mut signer, options)| {
                c.sign_refusal()
                    .map_err(|error| sign_refusal_text(&error))?;
                c.sign(&mut signer, &options)
                    .map_err(|error| format!("签章失败：{error}"))
            });
        *slot.lock().unwrap() = Some(done);
    });
}

/// The click that placed the seal (`.on_seal_placed`): the signature is made
/// at the next command drain.
pub(crate) fn on_seal_placed(app: &mut AppState, placement: SealPlacement) {
    if app.signing.pending.is_none() {
        return;
    }
    app.signing.pending_placed = Some((placement.page_index as u32, placement.boundary));
    queue_sign(app);
}

/// A seal placement the user cancelled (Esc or the right mouse button).
pub(crate) fn on_seal_placement_cancelled(app: &mut AppState) {
    if app.signing.pending.take().is_some() {
        app.signing.pending_placed = None;
        app.status = "已取消签章".to_string();
    }
}

/// The signing pass at the top of every `app_logic`: read the outcome of the
/// queued signature. The signature is in the document when this reads `Ok`
/// (the command made it); nothing is written, and the document is unsaved
/// until the user saves (M13).
pub(crate) fn drain_signing(app: &mut AppState) {
    let Some(outcome) = app.sign_outcome.lock().unwrap().take() else {
        return;
    };
    app.signing.pending_placed = None;
    match outcome {
        Ok(reports) => {
            // What the document carries now (M12).
            app.signatures = reports;
            app.modified = true;
            app.status = "签章完成，尚未保存：保存（Ctrl+S）后写入文件".to_string();
            refresh_seal_pictures(app);
        }
        Err(error) => {
            app.status = error.clone();
            eprintln!("[ERROR] {error}");
        }
    }
}

// ---- the certificate store and the seal picture ----

/// 导入证书…: pick a `.pfx` / `.p12` file; its password is asked for in
/// the dialog, then the import is confirmed.
pub(crate) fn import_certificate(app: &mut AppState) {
    if app.signing.dialog.is_none() {
        return;
    }
    let Some(path) = FileDialog::new()
        .add_filter("证书文件", &["pfx", "p12"])
        .set_title("选择证书文件（.pfx / .p12）")
        .pick_file()
    else {
        return;
    };
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "证书文件".to_owned());
    let imported = std::fs::read(&path)
        .map(|bytes| super::ImportingCertificate { file_name, bytes })
        .map_err(|e| format!("读取证书文件失败（{}）：{e}", path.display()));
    let dialog = app.signing.dialog.as_mut().expect("checked above");
    match imported {
        Ok(import) => {
            dialog.error = None;
            app.signing.importing = Some(import);
        }
        Err(e) => dialog.error = Some(e),
    }
}

/// The import's confirm: the password field holds the file's password.
pub(crate) fn confirm_import_certificate(app: &mut AppState) {
    let Some(import) = app.signing.importing.take() else {
        return;
    };
    let password = app
        .signing
        .dialog
        .as_ref()
        .map_or_else(String::new, |dialog| dialog.password.clone());
    let result = match app.signing.store.import(import.bytes.clone(), &password) {
        Ok(entry) => {
            if let Err(e) = store_file::save_store(&app.signing.store) {
                eprintln!("[ERROR] {e}");
            }
            if let Some(dialog) = app.signing.dialog.as_mut() {
                dialog.certificate = Some(entry.id);
                dialog.error = None;
            }
            app.status = format!("已导入证书 {}", entry.label);
            Ok(())
        }
        Err(e) => Err(format!("导入失败：{e}")),
    };
    if let Err(error) = result {
        app.signing.importing = Some(import);
        if let Some(dialog) = app.signing.dialog.as_mut() {
            dialog.error = Some(error);
        }
    }
}

/// 选择图片…: the user's own seal picture, any image the `image` crate
/// reads.
pub(crate) fn pick_seal_image(app: &mut AppState) {
    if app.signing.dialog.is_none() {
        return;
    }
    let Some(path) = FileDialog::new()
        .add_filter("图片", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
        .set_title("选择章图图片")
        .pick_file()
    else {
        return;
    };
    let picked = read_seal_image(&path);
    let dialog = app.signing.dialog.as_mut().expect("checked above");
    match picked {
        Ok(choice) => {
            dialog.seal = choice;
            dialog.error = None;
        }
        Err(e) => dialog.error = Some(e),
    }
}

// ---- the pre-save check (M16, M17) ----

/// In a save command: run the pre-save check on the bytes to be written.
/// When nothing speaks against them they are written and the outcome
/// slot reports; when the user has to decide first, the bytes wait in the
/// candidate slot for [`drain_save_candidate`].
pub(crate) fn inspect_save(
    candidate: &SaveCandidateSlot,
    preserved: Result<Preserved, String>,
    path: PathBuf,
    before: &[SignatureReport],
    outcome: &Arc<Mutex<Option<crate::HostOutcome>>>,
) {
    let preserved = match preserved {
        Ok(preserved) => preserved,
        Err(e) => {
            *outcome.lock().unwrap() = Some(crate::HostOutcome::Save(Err(e)));
            return;
        }
    };
    match pre_save_check(before, &preserved) {
        Ok(SaveCheck::Clear(reports)) => {
            let notes = save_notes(&preserved);
            let written =
                crate::host::document_io::write_atomic(&path, &preserved.bytes).map(|()| {
                    crate::SaveDone {
                        signatures: reports,
                        notes,
                    }
                });
            *outcome.lock().unwrap() = Some(crate::HostOutcome::Save(written));
        }
        Ok(check) => {
            *candidate.lock().unwrap() = Some(SaveCandidate {
                path,
                bytes: preserved.bytes,
                check,
            });
        }
        Err(e) => *outcome.lock().unwrap() = Some(crate::HostOutcome::Save(Err(e))),
    }
}

/// A save command found something the user has to decide: show the
/// question (`SigningState::save_confirm`; the panel view renders it).
pub(crate) fn drain_save_candidate(app: &mut AppState) {
    let Some(candidate) = app.save_candidate.lock().unwrap().take() else {
        return;
    };
    app.signing.save_confirm = match candidate.check {
        // Written by the command itself.
        SaveCheck::Clear(_) => None,
        SaveCheck::Blocked => Some(SaveConfirm::Blocked {
            path: candidate.path,
        }),
        SaveCheck::Invalidated(ids) => Some(SaveConfirm::Invalidated {
            path: candidate.path,
            bytes: candidate.bytes,
            ids,
        }),
    };
}

/// The user's word on a save that would invalidate signatures (M17), or
/// that cannot write the annotations of a package without an annotation
/// entry (M16). A no writes nothing and keeps the document unsaved.
pub(crate) fn save_confirm_answer(app: &mut AppState, yes: bool) {
    let Some(confirm) = app.signing.save_confirm.take() else {
        return;
    };
    if !yes {
        // A Save-As that did not happen must not promote its path.
        app.pending_save_as_path = None;
        return;
    }
    match confirm {
        SaveConfirm::Invalidated { path, bytes, .. } => {
            let reports = rofd_sign::verify(&bytes, &VerifyOptions::default());
            let written =
                crate::host::document_io::write_atomic(&path, &bytes).map(|()| crate::SaveDone {
                    signatures: reports,
                    notes: Vec::new(),
                });
            *app.save_outcome.lock().unwrap() = Some(crate::HostOutcome::Save(written));
        }
        SaveConfirm::Blocked { path } => {
            // The whole package, written again.
            let outcome = app.save_outcome.clone();
            push(app, move |c| {
                let snapshot = c.save_snapshot();
                let written = write_ofd_with_text_layout(
                    &snapshot.document,
                    snapshot.document.package.as_ref(),
                    &snapshot.text_layout,
                )
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    crate::host::document_io::write_atomic(&path, &bytes).map(|()| bytes)
                })
                .map(|bytes| crate::SaveDone {
                    signatures: rofd_sign::verify(&bytes, &VerifyOptions::default()),
                    notes: Vec::new(),
                });
                *outcome.lock().unwrap() = Some(crate::HostOutcome::Save(written));
            });
        }
    }
}
