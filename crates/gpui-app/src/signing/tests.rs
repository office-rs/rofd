//! Unit tests of the signing flow, the dialog rules and the pre-save check;
//! the `gpui::test` cases run the shell in a `TestAppContext`.
use super::*;
use gpui::{px, size, TestAppContext, VisualTestContext};
use rofd_component::OfdConfig;
use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, PageId, Rect, ShapeKind,
    TextLayoutHints,
};
use rofd_io::DocumentXmlChange;
use rofd_sign::{
    Pages, Placement, SignatureOverall, SignatureReport, SignatureValueStatus, VerifyOptions,
};
use std::sync::Arc;

const NOW: &str = "2026-11-24 10:30:00";

fn fixture_user_p12() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sign/tests/fixtures/USER.p12"
    ))
    .expect("fixture")
}

/// An unsigned package another tool wrote: one A4 page, an attachment.
fn unsigned_package() -> Vec<u8> {
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let document = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData><ofd:Pages><ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>";
    let page = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>";
    rofd_io::zip_util::write_zip(&[
        ("OFD.xml".to_owned(), ofd_xml.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml".to_owned(),
            document.as_bytes().to_vec(),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml".to_owned(),
            page.as_bytes().to_vec(),
        ),
        ("Doc_0/Attachs/note.txt".to_owned(), b"kept".to_vec()),
    ])
    .expect("zip")
}

fn to_temp(bytes: &[u8], name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("rofd_gpui_sign_{}_{}", std::process::id(), name));
    std::fs::write(&path, bytes).expect("write the temp file");
    path
}

fn verdicts(bytes: &[u8]) -> Vec<String> {
    rofd_sign::verify(bytes, &VerifyOptions::default())
        .into_iter()
        .map(|report| format!("{:?}/{:?}", report.overall, report.variant))
        .collect()
}

fn add_rect(doc: &mut rofd_dom::OfdDocument, page: &str) {
    doc.annotations.insert(Annotation {
        id: AnnotationId::from_int(doc.highest_known_object_id() + 1),
        kind: AnnotationKind::Shape(ShapeKind::Rect),
        page: PageId::new(page),
        creator: "tester".into(),
        created: 1_700_000_000_000,
        modified: 1_700_000_000_000,
        reply_to: None,
        payload: AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 30.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        },
    });
}

/// `bytes` with one entry replaced by other bytes.
fn with_entry(bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = rofd_io::zip_util::read_all_entries(bytes)
        .expect("entries")
        .into_iter()
        .map(|(entry, data)| {
            if entry == name {
                (entry, content.to_vec())
            } else {
                (entry, data)
            }
        })
        .collect();
    if !entries.iter().any(|(entry, _)| entry == name) {
        entries.push((name.to_owned(), content.to_vec()));
    }
    rofd_io::zip_util::write_zip(&entries).expect("zip")
}

#[test]
fn sign_request_from_dialog_maps_fields() {
    // The defaults: the first built-in seal, placed by hand.
    let dialog = SignDialog::new();
    assert_eq!(dialog.seal, SealChoice::Builtin(0));
    assert!(!dialog.request().signs_directly());
    assert_eq!(dialog.request().placement(), None);

    // No picture: an electronic signature is made right away.
    let bare = SignDialog {
        seal: SealChoice::None,
        ..SignDialog::new()
    };
    assert!(bare.request().signs_directly());
    assert_eq!(bare.request().placement(), None);

    // Auto-stamping: the margins and the pages come from the dialog.
    let auto = SignDialog {
        auto_place: true,
        right_mm: 15.5,
        bottom_mm: 8.0,
        last_page_only: false,
        ..SignDialog::new()
    };
    assert!(auto.request().signs_directly());
    assert_eq!(
        auto.request().placement(),
        Some(Placement::BottomRight {
            pages: Pages::All,
            right_mm: 15.5,
            bottom_mm: 8.0
        })
    );
    let last = SignDialog {
        last_page_only: true,
        ..auto.clone()
    };
    assert_eq!(
        last.request().placement(),
        Some(Placement::BottomRight {
            pages: Pages::Last,
            right_mm: 15.5,
            bottom_mm: 8.0
        })
    );

    // Validation: the messages keep the dialog open.
    let empty = CertificateStore::default();
    let no_certificate = SignDialog::new();
    assert_eq!(
        validate_dialog(&no_certificate, &empty).unwrap_err(),
        "请选择证书"
    );
    let mut store = CertificateStore::default();
    let entry = store.import(fixture_user_p12(), "777777").expect("import");
    let no_password = SignDialog {
        certificate: Some(entry.id.clone()),
        ..SignDialog::new()
    };
    assert_eq!(
        validate_dialog(&no_password, &store).unwrap_err(),
        "请输入证书口令"
    );
    let ready = SignDialog {
        password: "777777".into(),
        ..no_password.clone()
    };
    assert!(validate_dialog(&ready, &store).is_ok());
    for bad in [0.0, -1.0, f64::NAN] {
        let bad_margin = SignDialog {
            auto_place: true,
            right_mm: bad,
            ..ready.clone()
        };
        assert_eq!(
            validate_dialog(&bad_margin, &store).unwrap_err(),
            "距右边和距底部必须是大于 0 的数"
        );
    }
    // The margins are only read for auto-stamping.
    let unused = SignDialog {
        right_mm: f64::NAN,
        ..ready.clone()
    };
    assert!(validate_dialog(&unused, &store).is_ok());

    // Preparing checks the password with the certificate file (M7).
    let wrong = SignDialog {
        password: "wrong".into(),
        ..ready.clone()
    };
    assert!(prepare_sign(&wrong, &store, NOW).is_err());
    let pending = prepare_sign(&ready, &store, NOW).expect("prepare");
    assert_eq!(pending.password, "777777");
    assert_eq!(pending.seal_png.as_deref(), Some(BUILTIN_SEALS[0].png));
    assert!(!pending.request.signs_directly());
}

#[test]
fn the_seal_preview_keeps_the_pixel_ratio_with_a_40_mm_long_side() {
    assert_eq!(
        seal_preview_size(&SealChoice::Builtin(0)),
        Some((40.0, 40.0))
    );
    let bar = seal_preview_size(&SealChoice::Builtin(2)).expect("bar");
    assert!((bar.0 - 40.0).abs() < 1e-9 && (bar.1 - 40.0 / 3.0).abs() < 1e-9);
    assert_eq!(seal_preview_size(&SealChoice::None), None);
}

#[test]
fn pre_save_check_finds_nothing_for_annotation_only_changes() {
    // A file rofd signed itself.
    let pending = PendingSign {
        pfx: fixture_user_p12(),
        password: "777777".into(),
        request: SignRequest {
            auto_place: true,
            ..SignRequest::new()
        },
        seal_png: Some(BUILTIN_SEALS[0].png.to_vec()),
    };
    let signed = signed_package(&pending, None);
    let before = rofd_sign::verify(&signed, &VerifyOptions::default());
    assert_eq!(before[0].overall, SignatureOverall::Valid);

    // An annotation added and saved: nothing is invalidated.
    let report = rofd_io::parse_ofd(&signed).expect("parse");
    let mut document = report.document.clone();
    add_rect(&mut document, "1");
    let preserved = rofd_io::save_ofd(&document, &TextLayoutHints::new()).expect("save");
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    assert_eq!(
        pre_save_check(&before, &preserved).expect("check"),
        SaveCheck::Clear(rofd_sign::verify(
            &preserved.bytes,
            &VerifyOptions::default()
        ))
    );

    // A protected entry changed: the signature would not survive it.
    let tampered = with_entry(&signed, "Doc_0/Attachs/note.txt", b"changed after signing");
    let report = rofd_io::parse_ofd(&tampered).expect("parse");
    let preserved = rofd_io::save_ofd(&report.document, &TextLayoutHints::new()).expect("save");
    assert_eq!(
        pre_save_check(&before, &preserved).expect("check"),
        SaveCheck::Invalidated(vec!["1".to_owned()])
    );

    // A package without an annotation entry cannot take the save at all.
    let blocked = rofd_io::Preserved {
        bytes: signed.clone(),
        document_xml: DocumentXmlChange::Untouched,
        full_write_fallback: false,
        blocked_by_signature: true,
        reserve_exhausted: false,
    };
    assert_eq!(
        pre_save_check(&before, &blocked).expect("check"),
        SaveCheck::Blocked
    );
}

#[test]
fn texts_name_each_summary_and_detail_line() {
    let report = |overall: SignatureOverall| SignatureReport {
        id: "1".into(),
        kind: rofd_dom::SignatureKind::Seal,
        variant: rofd_sign::SignatureVariant::SesV4,
        overall,
        integrity: rofd_sign::IntegrityResult {
            ok: true,
            failures: vec![],
            protected_files: 1,
        },
        seal_match: Some(true),
        signature_value: rofd_sign::SignatureValue {
            status: SignatureValueStatus::Ok,
            reason: None,
        },
        certificate: None,
        provider: Some("rofd".into()),
        date_time: Some(NOW.into()),
    };
    assert_eq!(summary_text(&[]), None);
    assert_eq!(
        summary_text(&[report(SignatureOverall::Valid)]).as_deref(),
        Some("签章：全部有效（1）")
    );
    assert_eq!(
        summary_text(&[
            report(SignatureOverall::Valid),
            report(SignatureOverall::Invalid)
        ])
        .as_deref(),
        Some("签章：有失效（1/2）")
    );
    assert_eq!(
        summary_text(&[report(SignatureOverall::Unverified)]).as_deref(),
        Some("签章：仅未验证（1）")
    );
    let valid = report(SignatureOverall::Valid);
    assert_eq!(verdict_text(valid.overall), "有效");
    assert_eq!(seal_match_text(valid.seal_match), "一致");
    assert_eq!(signature_value_text(&valid.signature_value), "有效");
    assert_eq!(
        validity_text(Some("20200101000000Z"), Some("20991231235959Z")),
        "2020-01-01 至 2099-12-31"
    );
}

/// A shell with one window and no file.
fn shell_blank(cx: &mut TestAppContext) -> gpui::WindowHandle<crate::shell::AppShell> {
    cx.update(gpui_kit::init);
    cx.open_window(size(px(1000.), px(760.)), |window, cx| {
        crate::shell::AppShell::new(OfdConfig::new(Arc::new(vec![])), None, window, cx)
    })
}

/// A shell with one window and one session opened from `path`.
fn shell_with_file(
    cx: &mut TestAppContext,
    path: std::path::PathBuf,
) -> gpui::WindowHandle<crate::shell::AppShell> {
    let window = shell_blank(cx);
    window
        .update(cx, |shell, _window, cx| {
            let loaded = crate::host::document_io::load_ofd(&path).expect("load");
            shell.install_loaded_session(loaded, path, cx);
        })
        .unwrap();
    window
}

fn rect_payload() -> AnnotationPayload {
    AnnotationPayload::Shape {
        kind: ShapeKind::Rect,
        rect: Rect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 30.0,
        },
        stroke: Color::Rgb(255, 0, 0),
        fill: None,
        width: 1.0,
        points: vec![],
    }
}

/// The unsigned package signed with what `pending` prepares: a file made by
/// the bytes API, for tests that start from a signed document.
fn signed_package(pending: &PendingSign, placed: Option<(u32, Rect)>) -> Vec<u8> {
    let (mut signer, options) =
        signing_material(pending, NOW, placed).expect("the signing material");
    rofd_sign::sign(&unsigned_package(), &mut signer, &options).expect("sign")
}

/// A file another tool signed without an annotation entry (ofdrw's v4
/// sample). A file rofd signed always has one: the first signature of a
/// package declares it.
fn signed_without_entry_package() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../io/tests/fixtures/sign/SESV4SignDoc.ofd"
    ))
    .expect("the io fixture")
}

/// Confirm the dialog with `certificate`, auto-stamping.
fn confirm_auto_stamp(
    shell: &mut crate::shell::AppShell,
    certificate: rofd_sign::CertificateId,
    cx: &mut gpui::Context<crate::shell::AppShell>,
) {
    shell.signing.dialog = Some(SignDialog {
        certificate: Some(certificate),
        password: "777777".into(),
        auto_place: true,
        ..SignDialog::new()
    });
    shell.confirm_sign_dialog(cx);
}

#[gpui::test]
fn seal_placed_signs_and_the_save_writes_it(cx: &mut TestAppContext) {
    let path = to_temp(&unsigned_package(), "unsigned.ofd");
    let window = shell_with_file(cx, path.clone());
    let placement = rofd_component::SealPlacement {
        page_index: 0,
        page_id: PageId::new("1"),
        boundary: Rect {
            x: 80.0,
            y: 80.0,
            w: 40.0,
            h: 40.0,
        },
    };
    window
        .update(cx, |shell, _window, cx| {
            let idx = shell.sessions.active;
            assert!(
                shell.sessions.sessions[idx].signatures.is_empty(),
                "unsigned file"
            );
            // The user has an edit in hand. It is not the signature's to
            // undo.
            let edit = shell.sessions.sessions[idx].editor.update(cx, |view, cx| {
                let id = view.component.create_annotation(
                    AnnotationKind::Shape(ShapeKind::Rect),
                    PageId::new("1"),
                    rect_payload(),
                );
                cx.notify();
                id
            });

            // The user confirmed the dialog: the password was checked, the
            // placement state entered, the sign is pending.
            let entry = shell
                .signing
                .store
                .import(fixture_user_p12(), "777777")
                .expect("import");
            shell.signing.dialog = Some(SignDialog {
                certificate: Some(entry.id),
                password: "777777".into(),
                ..SignDialog::new()
            });
            shell.confirm_sign_dialog(cx);
            let pending = shell.signing.pending.as_ref().expect("pending");
            assert_eq!(pending.password, "777777");
            assert_eq!(shell.signing.pending_session, idx);

            // The click that places the seal.
            shell.on_seal_placed(idx, placement.clone(), cx);

            // The signature is in the document that was open: one valid
            // signature in the session, the seal on the editor's page, the
            // signature in the package the document carries. The document
            // was not loaded again, so the edit is still selected and can
            // still be undone. The signature is in the document, not in the
            // file: the session is unsaved until the user saves.
            let session = &shell.sessions.sessions[idx];
            assert_eq!(session.signatures.len(), 1);
            assert_eq!(session.signatures[0].overall, SignatureOverall::Valid);
            assert!(session.modified);
            assert!(shell.signing.pending.is_none());
            let component = &session.editor.read(cx).component;
            let document = component.document();
            let stamp = &document.signatures.items[0].stamps[0];
            assert_eq!(stamp.page_id, PageId::new("1"));
            assert_eq!(stamp.boundary, placement.boundary);
            assert!(
                document
                    .package
                    .as_ref()
                    .expect("the document carries its package")
                    .signatures_declared
            );
            assert!(component.can_undo(), "the edit can still be undone");
            assert!(
                matches!(
                    component.selection(),
                    rofd_dom::AnnotationSelection::Single(selected) if *selected == edit
                ),
                "the selection stays: {:?}",
                component.selection()
            );
            assert!(
                document.annotations.find(&edit).is_some(),
                "the edit is in the signed document"
            );
        })
        .unwrap();

    // Signing itself writes nothing: the file is as it was opened.
    assert!(
        verdicts(&std::fs::read(&path).expect("file on disk")).is_empty(),
        "the file is as it was opened until the user saves"
    );

    // The user saves: an ordinary save, and the file carries the signature.
    let idx = window
        .update(cx, |shell, _window, _cx| shell.sessions.active)
        .unwrap();
    window
        .update(cx, |shell, _window, cx| {
            shell.save_checked(idx, None, false, cx)
        })
        .unwrap();
    let on_disk = std::fs::read(&path).expect("file on disk");
    assert_eq!(verdicts(&on_disk), vec!["Valid/SesV4"]);
    window
        .update(cx, |shell, _window, _cx| {
            let session = &shell.sessions.sessions[idx];
            assert!(!session.modified);
        })
        .unwrap();
    let _ = std::fs::remove_file(&path);
}

#[gpui::test]
fn a_never_saved_document_signed_then_saved_twice_keeps_its_signature(cx: &mut TestAppContext) {
    // A session without a file, whose document has no package: nothing to
    // append to yet.
    let path = to_temp(&unsigned_package(), "never_saved.ofd");
    let target = to_temp(b"old", "never_saved_target.ofd");
    let window = shell_blank(cx);
    window
        .update(cx, |shell, _window, cx| {
            let idx = shell.sessions.active;
            let mut document = crate::host::document_io::load_ofd(&path).unwrap().document;
            document.package = None;
            shell.sessions.sessions[idx].editor.update(cx, |view, cx| {
                view.component.load_document(document);
                cx.notify();
            });
            let entry = shell
                .signing
                .store
                .import(fixture_user_p12(), "777777")
                .unwrap();
            confirm_auto_stamp(shell, entry.id, cx);
            assert!(shell.signing.dialog.is_none(), "{:?}", shell.signing.dialog);
            assert_eq!(shell.sessions.sessions[idx].signatures.len(), 1);
            // The document has a package now: the one made for the signature.
            assert!(shell.sessions.sessions[idx]
                .editor
                .read(cx)
                .component
                .document()
                .package
                .is_some());
            shell.save_checked(idx, Some(target.clone()), false, cx);
        })
        .unwrap();
    let first = std::fs::read(&target).unwrap();
    assert_eq!(verdicts(&first), vec!["Valid/SesV4"]);

    // A second save of the untouched document writes the same file: the
    // signature's own files are never made again.
    let idx = window
        .update(cx, |shell, _window, _cx| shell.sessions.active)
        .unwrap();
    window
        .update(cx, |shell, _window, cx| {
            shell.save_checked(idx, None, false, cx)
        })
        .unwrap();
    assert_eq!(
        std::fs::read(&target).unwrap(),
        first,
        "a second save writes the same file"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&target);
}

#[gpui::test]
fn a_signed_file_that_cannot_take_annotations_is_not_signed(cx: &mut TestAppContext) {
    let path = to_temp(&signed_without_entry_package(), "no_entry.ofd");
    let window = shell_with_file(cx, path.clone());
    window
        .update(cx, |shell, _window, cx| {
            let idx = shell.sessions.active;
            shell.sessions.sessions[idx].editor.update(cx, |view, cx| {
                view.component.create_annotation(
                    AnnotationKind::Shape(ShapeKind::Rect),
                    PageId::new("1"),
                    rect_payload(),
                );
                cx.notify();
            });
            let entry = shell
                .signing
                .store
                .import(fixture_user_p12(), "777777")
                .unwrap();
            confirm_auto_stamp(shell, entry.id, cx);
            let dialog = shell.signing.dialog.as_ref().expect("still open");
            assert!(
                dialog
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("没有批注入口")),
                "{:?}",
                dialog.error
            );
            assert_eq!(
                shell.sessions.sessions[idx].signatures.len(),
                1,
                "the file's own signature, nothing more"
            );
            assert!(shell.signing.pending.is_none());
        })
        .unwrap();
    // The file is as it was.
    assert_eq!(
        std::fs::read(&path).unwrap(),
        signed_without_entry_package(),
    );
    let _ = std::fs::remove_file(&path);
}

#[gpui::test]
fn an_auto_stamped_sign_needs_no_click_and_a_cancelled_one_is_forgotten(cx: &mut TestAppContext) {
    let path = to_temp(&unsigned_package(), "unsigned_auto.ofd");
    let window = shell_with_file(cx, path.clone());
    window
        .update(cx, |shell, _window, cx| {
            let idx = shell.sessions.active;
            let entry = shell
                .signing
                .store
                .import(fixture_user_p12(), "777777")
                .expect("import");
            // Auto-stamp: the dialog signs right away.
            shell.signing.dialog = Some(SignDialog {
                certificate: Some(entry.id.clone()),
                password: "777777".into(),
                auto_place: true,
                ..SignDialog::new()
            });
            shell.confirm_sign_dialog(cx);
            assert!(shell.signing.pending.is_none(), "nothing waits for a click");
            assert!(shell.signing.dialog.is_none(), "the dialog closed");
            assert_eq!(shell.sessions.sessions[idx].signatures.len(), 1);
            // In the document, not in the file: unsaved until the user saves.
            assert!(shell.sessions.sessions[idx].modified);
            assert!(!shell.sessions.sessions[idx]
                .editor
                .read(cx)
                .component
                .seal_placement_active());

            // A seal placed by hand that the user cancels: nothing is signed.
            shell.signing.dialog = Some(SignDialog {
                certificate: Some(entry.id),
                password: "777777".into(),
                ..SignDialog::new()
            });
            shell.confirm_sign_dialog(cx);
            assert!(shell.signing.pending.is_some());
            assert!(shell.sessions.sessions[idx]
                .editor
                .read(cx)
                .component
                .seal_placement_active());
            shell.on_seal_placement_cancelled(cx);
            assert!(shell.signing.pending.is_none());
            assert_eq!(
                shell.sessions.sessions[idx].signatures.len(),
                1,
                "still the one from before"
            );
        })
        .unwrap();
    assert!(
        verdicts(&std::fs::read(&path).expect("file on disk")).is_empty(),
        "signing writes nothing; the file changes when the user saves"
    );
    let _ = std::fs::remove_file(&path);
}

#[gpui::test]
fn a_wrong_password_keeps_the_dialog_open(cx: &mut TestAppContext) {
    let path = to_temp(&unsigned_package(), "unsigned_pw.ofd");
    let window = shell_with_file(cx, path.clone());
    window
        .update(cx, |shell, _window, cx| {
            let entry = shell
                .signing
                .store
                .import(fixture_user_p12(), "777777")
                .expect("import");
            shell.signing.dialog = Some(SignDialog {
                certificate: Some(entry.id),
                password: "wrong".into(),
                auto_place: true,
                ..SignDialog::new()
            });
            shell.confirm_sign_dialog(cx);
            let dialog = shell.signing.dialog.as_ref().expect("still open");
            assert!(
                dialog.error.as_deref().is_some_and(|e| e.contains("口令")),
                "{:?}",
                dialog.error
            );
            assert!(shell.signing.pending.is_none());
            assert!(shell.sessions.sessions[shell.sessions.active]
                .signatures
                .is_empty());
        })
        .unwrap();
    let _ = std::fs::remove_file(&path);
}

#[gpui::test]
fn the_shell_draws_its_signing_panels_for_a_signed_document(cx: &mut TestAppContext) {
    // What only drawing shows: the panels as elements, with the seal's
    // thumbnail in the detail view. A frame is drawn after each change
    // of state (the shell cannot be drawn from inside its own update).
    let pending = PendingSign {
        pfx: fixture_user_p12(),
        password: "777777".into(),
        request: SignRequest {
            auto_place: true,
            ..SignRequest::new()
        },
        seal_png: Some(BUILTIN_SEALS[0].png.to_vec()),
    };
    let signed = signed_package(&pending, None);
    let path = to_temp(&signed, "signed_render.ofd");
    let window = shell_with_file(cx, path.clone());
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let draw = |vcx: &mut VisualTestContext| {
        vcx.run_until_parked();
    };
    let show = |vcx: &mut VisualTestContext,
                show: fn(
        &mut crate::shell::AppShell,
        &mut gpui::Context<crate::shell::AppShell>,
    )| {
        let _ = window.update(vcx, |shell, _window, cx| {
            show(shell, cx);
            cx.notify();
        });
    };

    show(&mut vcx, |shell, _cx| shell.signing.detail_open = true);
    draw(&mut vcx);
    // The signing dialog, in every state it has.
    show(&mut vcx, |shell, _cx| {
        let mut dialog = SignDialog::new();
        dialog.auto_place = true;
        dialog.seal = SealChoice::File {
            name: "mine.png".into(),
            png: BUILTIN_SEALS[1].png.to_vec(),
            width_px: 256,
            height_px: 256,
        };
        dialog.error = Some("证书口令错误".into());
        shell.signing.dialog = Some(dialog);
        shell.signing.importing = Some(ImportingCertificate {
            file_name: "mine.p12".into(),
            bytes: Vec::new(),
        });
    });
    draw(&mut vcx);
    // Both things a save may have to ask.
    show(&mut vcx, |shell, _cx| {
        let editor = shell.sessions.active_editor().expect("a session");
        shell.signing.dialog = None;
        shell.signing.importing = None;
        shell.save_confirm = Some(SaveConfirm::Blocked {
            editor,
            path: None,
            close_after: false,
        });
    });
    draw(&mut vcx);
    show(&mut vcx, |shell, _cx| {
        let editor = shell.sessions.active_editor().expect("a session");
        shell.save_confirm = Some(SaveConfirm::Invalidated {
            editor,
            path: None,
            close_after: false,
            bytes: Vec::new(),
            ids: vec!["1".to_owned()],
        });
    });
    draw(&mut vcx);
    // The status bar has its entry for a signed document.
    window
        .update(&mut vcx, |shell, _window, _cx| {
            shell.save_confirm = None;
            summary_text(&shell.sessions.active().expect("a session").signatures)
        })
        .unwrap();
}

#[test]
fn the_integrity_line_names_a_signature_that_protects_nothing() {
    let nothing = rofd_sign::IntegrityResult {
        ok: false,
        failures: vec![],
        protected_files: 0,
    };
    assert_eq!(integrity_text(&nothing), "失效（该签名未保护任何文件）");
    let changed = rofd_sign::IntegrityResult {
        ok: false,
        failures: vec![rofd_sign::IntegrityFailure {
            file_ref: "/Doc_0/Pages/Page_0/Content.xml".into(),
            expected: String::new(),
            actual: String::new(),
        }],
        protected_files: 2,
    };
    assert_eq!(
        integrity_text(&changed),
        "失效（/Doc_0/Pages/Page_0/Content.xml）"
    );
    let intact = rofd_sign::IntegrityResult {
        ok: true,
        failures: vec![],
        protected_files: 2,
    };
    assert_eq!(integrity_text(&intact), "有效");
}

#[test]
fn a_validity_time_that_is_not_a_day_is_shown_as_it_is() {
    // A report carries whatever the certificate says there. The detail
    // panel must not panic on it (final review I3).
    for odd in [
        "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}",
        "\u{FFFD}\u{FFFD}abc",
    ] {
        assert_eq!(validity_text(Some(odd), None), format!("{odd} 起"));
        assert_eq!(
            validity_text(Some(odd), Some(odd)),
            format!("{odd} 至 {odd}")
        );
    }
    assert_eq!(validity_text(None, Some("x")), "至 x");
    assert_eq!(
        validity_text(Some("20200101000000Z"), Some("20991231235959Z")),
        "2020-01-01 至 2099-12-31"
    );
}

#[test]
fn a_wrong_password_and_an_unreadable_file_get_their_own_messages() {
    // Spec section 16: a file that cannot be read gets a clear error and is
    // never reported as a wrong password.
    let pfx = fixture_user_p12();
    let wrong = open_certificate(&pfx, "wrong").unwrap_err();
    assert!(wrong.starts_with("证书口令错误"), "{wrong}");
    let unreadable = open_certificate(b"not a certificate file", "x").unwrap_err();
    assert!(unreadable.starts_with("证书文件无法使用"), "{unreadable}");
    assert!(!unreadable.contains("口令"), "{unreadable}");
    let rsa = open_certificate(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sign/tests/fixtures/rsa_test.p12"
        ))
        .unwrap(),
        "changeit",
    )
    .unwrap_err();
    assert!(rsa.starts_with("证书文件无法使用"), "{rsa}");
    assert!(!rsa.contains("口令"), "{rsa}");
}

#[test]
fn save_notes_name_an_exhausted_reserve_and_a_full_write_fallback() {
    // Spec M14 and M20: both are written all the same and reported. The
    // library reports them; the status bar is where the user hears of them
    // (final review I5).
    let preserved = |reserve_exhausted, full_write_fallback| Preserved {
        bytes: Vec::new(),
        document_xml: rofd_io::DocumentXmlChange::Untouched,
        full_write_fallback,
        blocked_by_signature: false,
        reserve_exhausted,
    };
    assert!(
        save_notes(&preserved(false, false)).is_empty(),
        "the normal save is silent"
    );
    let notes = save_notes(&preserved(true, false));
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("预留"), "{}", notes[0]);
    let notes = save_notes(&preserved(false, true));
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("整份"), "{}", notes[0]);
    assert_eq!(save_notes(&preserved(true, true)).len(), 2);
}

#[gpui::test]
fn the_status_bar_names_what_a_save_had_to_do(cx: &mut TestAppContext) {
    let path = to_temp(&unsigned_package(), "notes.ofd");
    let window = shell_with_file(cx, path.clone());
    window
        .update(cx, |shell, _window, cx| {
            let idx = shell.sessions.active;
            shell.write_saved(
                idx,
                Some(path.clone()),
                unsigned_package(),
                flow::Written {
                    reports: Vec::new(),
                    notes: Vec::new(),
                },
                false,
                cx,
            );
            assert!(shell.status.contains("已保存"), "{}", shell.status);
            shell.write_saved(
                idx,
                Some(path.clone()),
                unsigned_package(),
                flow::Written {
                    reports: Vec::new(),
                    notes: vec!["一项说明".to_owned(), "另一项说明".to_owned()],
                },
                false,
                cx,
            );
            assert!(
                shell.status.contains("已保存")
                    && shell.status.contains("一项说明")
                    && shell.status.contains("另一项说明"),
                "{}",
                shell.status
            );
        })
        .unwrap();
    let _ = std::fs::remove_file(&path);
}

// ---- the keyboard and the modal panels (found by hand after the line shipped) ----

/// A shell with a file and its signing dialog open, drawn once. The file's
/// name is unique per call: the tests run in parallel and each removes its
/// file when it is done with it.
fn shell_with_sign_dialog(
    cx: &mut TestAppContext,
) -> (
    gpui::WindowHandle<crate::shell::AppShell>,
    VisualTestContext,
) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let file = format!(
        "sign_dialog_{}.ofd",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let path = to_temp(&unsigned_package(), &file);
    let window = shell_with_file(cx, path.clone());
    let _ = std::fs::remove_file(&path);
    window
        .update(cx, |shell, window, cx| shell.open_sign_dialog(window, cx))
        .unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    (window, vcx)
}

/// The focus handles of the dialog's password field and of the active
/// session's editor.
fn password_and_editor(
    window: &gpui::WindowHandle<crate::shell::AppShell>,
    vcx: &VisualTestContext,
) -> (gpui::FocusHandle, gpui::FocusHandle) {
    window
        .read_with(vcx, |shell, cx| {
            let password =
                gpui_kit::component::input::AnyInputState::from(shell.sign_password_input.clone())
                    .focus_handle(cx);
            let editor = shell
                .sessions
                .active_editor()
                .expect("a session")
                .read(cx)
                .focus_handle
                .clone();
            (password, editor)
        })
        .unwrap()
}

fn password_value(
    window: &gpui::WindowHandle<crate::shell::AppShell>,
    vcx: &VisualTestContext,
) -> String {
    window
        .read_with(vcx, |shell, cx| {
            shell.sign_password_input.read(cx).value().to_string()
        })
        .unwrap()
}

#[gpui::test]
fn opening_the_signing_dialog_puts_the_cursor_in_the_password_field(cx: &mut TestAppContext) {
    // The dialog is modal: from its first moment the keys belong to it, not
    // to the document behind it. A password typed straight away used to go
    // into the editor.
    let (window, mut vcx) = shell_with_sign_dialog(cx);
    let (password, editor) = password_and_editor(&window, &vcx);
    assert!(vcx.update(|window, _| password.is_focused(window)));
    assert!(!vcx.update(|window, _| editor.is_focused(window)));
    vcx.simulate_input("777");
    vcx.simulate_keystrokes("7");
    vcx.run_until_parked();
    assert_eq!(password_value(&window, &vcx), "7777");
    let mirrored = window
        .read_with(&vcx, |shell, _| {
            shell
                .signing
                .dialog
                .as_ref()
                .map(|dialog| dialog.password.clone())
        })
        .unwrap();
    assert_eq!(mirrored.as_deref(), Some("7777"));

    // Closing it hands the keyboard back to the document.
    window
        .update(&mut vcx, |shell, _window, cx| shell.cancel_sign_dialog(cx))
        .unwrap();
    vcx.run_until_parked();
    assert!(vcx.update(|window, _| editor.is_focused(window)));
}

#[gpui::test]
fn a_click_in_the_password_field_focuses_it(cx: &mut TestAppContext) {
    // The dialog's backdrop did not block mouse events: the press also
    // reached the editor under the dialog, whose press handler takes the
    // focus back. The field never kept it, and every key went into the
    // document.
    let (window, mut vcx) = shell_with_sign_dialog(cx);
    let (password, editor) = password_and_editor(&window, &vcx);
    // Start from the document, as someone who clicked the page does.
    vcx.update(|window, cx| editor.focus(window, cx));
    vcx.run_until_parked();
    let bounds = vcx
        .debug_bounds("sg-password")
        .expect("the field is on screen");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, _| password.is_focused(window)),
        "the click focuses the field"
    );
    assert!(
        !vcx.update(|window, _| editor.is_focused(window)),
        "the document under the dialog does not take the focus back"
    );
    vcx.simulate_input("ab");
    vcx.simulate_keystrokes("c");
    vcx.run_until_parked();
    assert_eq!(password_value(&window, &vcx), "abc");
}

#[gpui::test]
fn a_click_in_the_print_panels_page_range_input_focuses_it_too(cx: &mut TestAppContext) {
    // The same backdrop, older than the signing line: the press went
    // through it as well. The test lives here because the shell fixture
    // and the cause are the same.
    let path = to_temp(&unsigned_package(), "print_input.ofd");
    let window = shell_with_file(cx, path.clone());
    let _ = std::fs::remove_file(&path);
    window
        .update(cx, |shell, _window, cx| {
            let mut panel = crate::print_dialog::PrintPanelState::new(3);
            panel.open();
            // All -> Current -> Custom: the page-range input is mounted
            // in Custom mode only.
            for _ in 0..2 {
                panel.cycle_range("1-3");
            }
            shell.print_panel = Some(panel);
            cx.notify();
        })
        .unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let editor = window
        .read_with(&vcx, |shell, cx| {
            shell
                .sessions
                .active_editor()
                .expect("a session")
                .read(cx)
                .focus_handle
                .clone()
        })
        .unwrap();
    assert!(vcx.update(|window, _| editor.is_focused(window)));
    let bounds = vcx.debug_bounds("pp-spec").expect("the input is on screen");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(!vcx.update(|window, _| editor.is_focused(window)));
    vcx.simulate_input("7");
    vcx.run_until_parked();
    let value = window
        .read_with(&vcx, |shell, cx| {
            shell.print_spec_input.read(cx).value().to_string()
        })
        .unwrap();
    assert!(value.contains('7'), "{value}");
}
