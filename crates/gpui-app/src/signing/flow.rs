//! The signing flow of the gpui shell (spec 2026-11-23 §10, §11, §13,
//! M13, M16, M17): what the 签章 and 验签 buttons, the seal placement
//! events and the save commands do to the sessions. The decisions are in
//! `super`; this module wires them to the shell's sessions, editors and
//! files.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Context, PathPromptOptions};
use rofd_component::SealPlacement;
use rofd_dom::Rect;
use rofd_io::{write_ofd_with_text_layout, Preserved};
use rofd_sign::{SignatureReport, VerifyOptions};

use super::{
    host_now, pre_save_check, prepare_sign, read_seal_image, save_notes, seal_preview_size,
    sign_refusal_text, signing_material, store_file, PendingSign, SaveCheck, SaveConfirm,
    SignDialog,
};
use crate::shell::AppShell;

/// What a written file carries and what else its save has to say.
pub(crate) struct Written {
    pub reports: Vec<SignatureReport>,
    /// [`save_notes`] of the preserving save; nothing after a full write.
    pub notes: Vec<String>,
}

impl AppShell {
    /// 签章 button: the dialog, with the store's certificates.
    pub(crate) fn open_sign_dialog(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let first = self
            .signing
            .store
            .list()
            .first()
            .map(|entry| entry.id.clone());
        self.signing.dialog = Some(SignDialog {
            certificate: first,
            ..SignDialog::new()
        });
        self.signing.importing = None;
        // The password is never kept from one signing to the next.
        self.sign_password_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        // The dialog is modal: the keys belong to it from its first moment,
        // so the cursor starts in the password field. Closing it hands them
        // back to the document (`pending_focus`).
        self.sign_password_input
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// 确定: the password is checked with the certificate file (M7). Then
    /// the document is signed right away - no picture, or auto-stamped -
    /// or the seal placement state is entered on the session's editor and
    /// the sign waits for the click. A refused dialog stays open with the
    /// reason.
    pub(crate) fn confirm_sign_dialog(&mut self, cx: &mut Context<Self>) {
        let idx = self.sessions.active;
        let Some(dialog) = self.signing.dialog.take() else {
            return;
        };
        match prepare_sign(&dialog, &self.signing.store, &host_now()) {
            Ok(pending) => {
                self.signing.importing = None;
                if pending.request.signs_directly() {
                    if let Err(error) = self.sign_now(idx, &pending, None, cx) {
                        self.signing.dialog = Some(SignDialog {
                            error: Some(error),
                            ..dialog
                        });
                    }
                } else {
                    self.enter_seal_placement(idx, &dialog, pending, cx);
                }
                // The dialog is gone: the keyboard goes back to the document
                // (a refused dialog stays open and keeps the cursor).
                if self.signing.dialog.is_none() {
                    self.pending_focus = self.sessions.active_editor();
                }
            }
            Err(error) => {
                self.signing.dialog = Some(SignDialog {
                    error: Some(error),
                    ..dialog
                });
            }
        }
        cx.notify();
    }

    /// 取消: the dialog closes; a certificate picked for import is forgotten.
    pub(crate) fn cancel_sign_dialog(&mut self, cx: &mut Context<Self>) {
        self.signing.dialog = None;
        self.signing.importing = None;
        // The keyboard goes back to the document.
        self.pending_focus = self.sessions.active_editor();
        cx.notify();
    }

    /// Enter the seal placement state on the session's editor; the
    /// preview follows the pointer until the click.
    fn enter_seal_placement(
        &mut self,
        idx: usize,
        dialog: &SignDialog,
        pending: PendingSign,
        cx: &mut Context<Self>,
    ) {
        let refused = |error: &str| SignDialog {
            error: Some(error.to_owned()),
            ..dialog.clone()
        };
        let Some((width_mm, height_mm)) = seal_preview_size(&dialog.seal) else {
            self.signing.dialog = Some(refused("没有章图"));
            return;
        };
        let Some(png) = pending.seal_png.clone() else {
            self.signing.dialog = Some(refused("没有章图"));
            return;
        };
        let Some(editor) = self.sessions.sessions.get(idx).map(|s| s.editor.clone()) else {
            return;
        };
        let preview = rofd_component::SealPreview {
            png: Arc::new(png),
            width_mm,
            height_mm,
        };
        if rofd_gpui_view::GpuiOfdHandle::new(editor).begin_seal_placement(preview, cx) {
            self.signing.pending = Some(pending);
            self.signing.pending_session = idx;
            self.status = "请在页面上点击左键放置签章（Esc 或右键取消）".to_string();
        } else {
            self.signing.dialog = Some(refused("章图无法读取"));
        }
    }

    /// The click that placed the seal (`OfdGpuiEvent::SealPlaced`): sign the
    /// document that is open (M13, spec §17.11).
    pub(crate) fn on_seal_placed(
        &mut self,
        idx: usize,
        placement: SealPlacement,
        cx: &mut Context<Self>,
    ) {
        if !(self.signing.pending.is_some() && self.signing.pending_session == idx) {
            return;
        }
        let Some(pending) = self.signing.pending.take() else {
            return;
        };
        let placed = (placement.page_index as u32, placement.boundary);
        if let Err(error) = self.sign_now(idx, &pending, Some(placed), cx) {
            self.status = error.clone();
            eprintln!("[ERROR] {error}");
        }
        cx.notify();
    }

    /// A seal placement the user cancelled (Esc or the right mouse button).
    pub(crate) fn on_seal_placement_cancelled(&mut self, cx: &mut Context<Self>) {
        if self.signing.pending.take().is_some() {
            self.status = "已取消签章".to_string();
        }
        cx.notify();
    }

    /// End a seal placement because its session is going away or losing
    /// the active role (the state lives in that session's editor).
    pub(crate) fn cancel_pending_placement(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.signing.pending.take() {
            if let Some(session) = self.sessions.sessions.get(self.signing.pending_session) {
                let editor = session.editor.clone();
                rofd_gpui_view::GpuiOfdHandle::new(editor).cancel_seal_placement(cx);
            }
            drop(pending);
        }
    }

    /// The document of session `idx` as a save would write it: the
    /// preserving save's own bytes, over the package the document carries.
    fn session_save_bytes(&self, idx: usize, cx: &App) -> Result<Preserved, String> {
        let session = self
            .sessions
            .sessions
            .get(idx)
            .ok_or_else(|| "文档已关闭".to_owned())?;
        let snapshot = session.editor.read(cx).component.save_snapshot();
        crate::host::document_io::save_bytes(&snapshot.document, &snapshot.text_layout)
    }

    /// Sign the document of session `idx` with what the dialog prepared: one
    /// call on its component (spec 2026-11-23 §17.11). The signature goes
    /// into the document that is open - nothing is loaded again, so the view,
    /// the selection and the undo history stay - and nothing is written: the
    /// session is unsaved, and the user's save writes the file.
    fn sign_now(
        &mut self,
        idx: usize,
        pending: &PendingSign,
        placed: Option<(u32, Rect)>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (mut signer, options) = signing_material(pending, &host_now(), placed)?;
        let session = self
            .sessions
            .sessions
            .get_mut(idx)
            .ok_or_else(|| "文档已关闭".to_owned())?;
        let reports = session.editor.update(cx, |view, cx| {
            // Said first, in the dialog's words: the file has no annotation
            // entry, and the annotations in the editor could not be written
            // into it, signed or not.
            view.component
                .sign_refusal()
                .map_err(|error| sign_refusal_text(&error))?;
            let reports = view
                .component
                .sign(&mut signer, &options)
                .map_err(|error| format!("签章失败：{error}"))?;
            cx.notify();
            Ok::<Vec<SignatureReport>, String>(reports)
        })?;
        let session = &mut self.sessions.sessions[idx];
        // What the document carries now (M12).
        session.signatures = reports;
        session.modified = true;
        session.pristine = false;
        self.status = "签章完成，尚未保存：保存（Ctrl+S）后写入文件".to_string();
        Ok(())
    }

    // -- the certificate store --

    /// 导入证书…: pick a `.pfx` / `.p12` file; its password is asked for
    /// in the dialog, then the import is confirmed.
    pub(crate) fn import_certificate(&mut self, cx: &mut Context<Self>) {
        if self.signing.dialog.is_none() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择证书文件（.pfx / .p12）".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "证书文件".to_owned());
            let imported = std::fs::read(&path)
                .map(|bytes| super::ImportingCertificate { file_name, bytes })
                .map_err(|e| format!("读取证书文件失败（{}）：{e}", path.display()));
            let _ = this.update(cx, |shell, cx| {
                match imported {
                    Ok(import) => {
                        if let Some(dialog) = shell.signing.dialog.as_mut() {
                            dialog.error = None;
                        }
                        shell.signing.importing = Some(import);
                    }
                    Err(e) => {
                        if let Some(dialog) = shell.signing.dialog.as_mut() {
                            dialog.error = Some(e);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The import's confirm: the password field holds the file's password.
    pub(crate) fn confirm_import_certificate(&mut self, cx: &mut Context<Self>) {
        let Some(import) = self.signing.importing.take() else {
            return;
        };
        let password = self.sign_password_input.read(cx).value().to_string();
        let outcome = self.signing.store.import(import.bytes.clone(), &password);
        let result = match outcome {
            Ok(entry) => {
                if let Err(e) = store_file::save_store(&self.signing.store) {
                    eprintln!("[ERROR] {e}");
                }
                if let Some(dialog) = self.signing.dialog.as_mut() {
                    dialog.certificate = Some(entry.id);
                    dialog.error = None;
                }
                self.status = format!("已导入证书 {}", entry.label);
                Ok(())
            }
            Err(e) => Err(format!("导入失败：{e}")),
        };
        if let Err(error) = result {
            self.signing.importing = Some(import);
            if let Some(dialog) = self.signing.dialog.as_mut() {
                dialog.error = Some(error);
            }
        }
        cx.notify();
    }

    /// 选择图片…: the user's own seal picture, any image the `image`
    /// crate reads.
    pub(crate) fn pick_seal_image(&mut self, cx: &mut Context<Self>) {
        if self.signing.dialog.is_none() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择章图图片".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let picked = read_seal_image(&path);
            let _ = this.update(cx, |shell, cx| {
                if let Some(dialog) = shell.signing.dialog.as_mut() {
                    match picked {
                        Ok(choice) => {
                            dialog.seal = choice;
                            dialog.error = None;
                        }
                        Err(e) => dialog.error = Some(e),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -- the pre-save check (M16, M17) --

    /// Run the pre-save check on the bytes a save of session `idx` would
    /// write, and go on: write them, or ask first. `target` is the path a
    /// Save-As prompt gave (`None`: the session's own path).
    pub(crate) fn save_checked(
        &mut self,
        idx: usize,
        target: Option<PathBuf>,
        close_after: bool,
        cx: &mut Context<Self>,
    ) {
        let before = match self.sessions.sessions.get(idx) {
            Some(session) => session.signatures.clone(),
            None => return,
        };
        let editor = self.sessions.sessions[idx].editor.clone();
        let preserved = match self.session_save_bytes(idx, cx) {
            Ok(preserved) => preserved,
            Err(e) => {
                self.status = format!("保存失败：{e}");
                eprintln!("[ERROR] {e}");
                cx.notify();
                return;
            }
        };
        match pre_save_check(&before, &preserved) {
            Ok(SaveCheck::Clear(reports)) => {
                let written = Written {
                    notes: save_notes(&preserved),
                    reports,
                };
                self.write_saved(idx, target, preserved.bytes, written, close_after, cx);
            }
            Ok(SaveCheck::Blocked) => {
                self.save_confirm = Some(SaveConfirm::Blocked {
                    editor,
                    path: target,
                    close_after,
                });
            }
            Ok(SaveCheck::Invalidated(ids)) => {
                self.save_confirm = Some(SaveConfirm::Invalidated {
                    editor,
                    path: target,
                    close_after,
                    bytes: preserved.bytes,
                    ids,
                });
            }
            Err(e) => {
                self.status = format!("保存失败：{e}");
                eprintln!("[ERROR] {e}");
            }
        }
        cx.notify();
    }

    /// Write `bytes` for session `idx` and record what they carry;
    /// `written.notes` join the status line.
    pub(crate) fn write_saved(
        &mut self,
        idx: usize,
        target: Option<PathBuf>,
        bytes: Vec<u8>,
        written: Written,
        close_after: bool,
        cx: &mut Context<Self>,
    ) {
        let Written { reports, notes } = written;
        let path = target.or_else(|| {
            self.sessions
                .sessions
                .get(idx)
                .and_then(|session| session.path.clone())
        });
        let Some(path) = path else {
            self.status = "保存失败：没有可写的路径".to_string();
            return;
        };
        match crate::host::document_io::write_atomic(&path, &bytes) {
            Ok(()) => {
                let session = &mut self.sessions.sessions[idx];
                session.modified = false;
                session.signatures = reports;
                if session.path.as_ref() != Some(&path) {
                    session.path = Some(path.clone());
                    if let Some(stem) = path.file_stem() {
                        session.file_name = stem.to_string_lossy().to_string();
                    }
                }
                self.status = if notes.is_empty() {
                    format!("已保存 {}", path.display())
                } else {
                    format!("已保存 {}（{}）", path.display(), notes.join("；"))
                };
                if close_after {
                    self.close_session(idx, cx);
                }
            }
            Err(e) => {
                self.status = format!("保存失败：{e}");
                eprintln!("[ERROR] {e}");
            }
        }
    }

    /// A save that would invalidate signatures, confirmed: the bytes are
    /// written as they are.
    pub(crate) fn save_invalidated_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(SaveConfirm::Invalidated {
            editor,
            path,
            close_after,
            bytes,
            ..
        }) = self.save_confirm.take()
        else {
            return;
        };
        let Some(idx) = self.sessions.index_of(&editor) else {
            return;
        };
        let reports = rofd_sign::verify(&bytes, &VerifyOptions::default());
        self.write_saved(
            idx,
            path,
            bytes,
            Written {
                reports,
                notes: Vec::new(),
            },
            close_after,
            cx,
        );
        cx.notify();
    }

    /// A package without an annotation entry, confirmed: the whole package
    /// is written again (which is what invalidates its signatures).
    pub(crate) fn save_blocked_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(SaveConfirm::Blocked {
            editor,
            path,
            close_after,
        }) = self.save_confirm.take()
        else {
            return;
        };
        let Some(idx) = self.sessions.index_of(&editor) else {
            return;
        };
        let session = &self.sessions.sessions[idx];
        let snapshot = session.editor.read(cx).component.save_snapshot();
        let written = write_ofd_with_text_layout(
            &snapshot.document,
            snapshot.document.package.as_ref(),
            &snapshot.text_layout,
        );
        match written {
            Ok(bytes) => {
                let reports = rofd_sign::verify(&bytes, &VerifyOptions::default());
                self.write_saved(
                    idx,
                    path,
                    bytes,
                    Written {
                        reports,
                        notes: Vec::new(),
                    },
                    close_after,
                    cx,
                );
            }
            Err(e) => {
                self.status = format!("保存失败：{e}");
                eprintln!("[ERROR] {e}");
            }
        }
        cx.notify();
    }

    /// A save that was about to invalidate signatures, cancelled: nothing
    /// is written and the document stays modified.
    pub(crate) fn save_confirm_cancelled(&mut self, cx: &mut Context<Self>) {
        self.save_confirm = None;
        cx.notify();
    }
}
