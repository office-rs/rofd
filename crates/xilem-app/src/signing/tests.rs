//! Unit tests of the signing flow, the dialog rules and the pre-save check.
//! They use the real component over the command queue (`seal_placed_action_*`).
use super::*;
use crate::signing::flow::{confirm_sign_dialog, drain_signing, on_seal_placed};
use crate::AppState;
use rofd_component::{OfdComponent, OfdConfig, SealPlacement};
use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, AnnotationSelection, Color,
    PageId, Rect, ShapeKind, TextLayoutHints,
};
use rofd_io::DocumentXmlChange;
use rofd_sign::{
    Pages, Placement, SignatureOverall, SignatureReport, SignatureValueStatus, VerifyOptions,
};
use std::sync::Arc;

const NOW: &str = "2026-11-24 10:30:00";
/// A seal position that is on no page of the test documents: page index 9.
const NOWHERE: Rect = Rect {
    x: 10.0,
    y: 10.0,
    w: 10.0,
    h: 10.0,
};

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
    let path =
        std::env::temp_dir().join(format!("rofd_xilem_sign_{}_{}", std::process::id(), name));
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

/// An `AppState` with `bytes` as its file, the way `do_open` leaves
/// it, and the component the queue's commands run against.
fn app_with_file(bytes: &[u8], name: &str) -> (AppState, OfdComponent, std::path::PathBuf) {
    let path = to_temp(bytes, name);
    let loaded = crate::host::document_io::load_ofd(&path).expect("load");
    let mut app = AppState::new();
    let mut component = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
    component.load_document(loaded.document.clone());
    app.doc_pages = loaded.document.pages.len();
    app.file = Some(path.clone());
    app.signatures = loaded.signatures;
    // The clock the start-up command would seed.
    crate::drain_commands(&mut app, &mut component);
    (app, component, path)
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

    // Validation keeps the dialog open.
    let mut store = CertificateStore::default();
    assert_eq!(
        validate_dialog(&SignDialog::new(), &store).unwrap_err(),
        "请选择证书"
    );
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
    let error = prepare_sign(&wrong, &store, NOW).unwrap_err();
    assert!(error.contains("口令"), "{error}");
    let pending = prepare_sign(&ready, &store, NOW).expect("prepare");
    assert_eq!(pending.password, "777777");
    assert_eq!(pending.seal_png.as_deref(), Some(BUILTIN_SEALS[0].png));
    assert!(!pending.request.signs_directly());
}

#[test]
fn a_password_typed_into_the_bulleted_field_is_rebuilt() {
    // The field shows one bullet per character; what it shows after an
    // edit says what was typed.
    assert_eq!(merge_masked("", "abc"), "abc", "typed into an empty field");
    assert_eq!(merge_masked("abc", "•••d"), "abcd", "appended");
    assert_eq!(
        merge_masked("abc", "•x••"),
        "axbc",
        "inserted in the middle"
    );
    assert_eq!(
        merge_masked("abc", "secret"),
        "secret",
        "replaced everything"
    );
    assert_eq!(merge_masked("abc", "••"), "ab", "one deleted (at the end)");
    // A bullet that is not the field's own is a typed character.
    assert_eq!(merge_masked("", "•"), "•");
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

#[test]
fn seal_placed_action_signs_and_the_save_writes_it() {
    let (mut app, mut component, path) = app_with_file(&unsigned_package(), "unsigned.ofd");
    assert!(app.signatures.is_empty(), "unsigned file");
    // The user has an edit in hand. It is not the signature's to undo.
    let edit = component.create_annotation(
        AnnotationKind::Shape(ShapeKind::Rect),
        PageId::new("1"),
        rect_payload(),
    );

    // The user confirmed the dialog: the password was checked, the
    // placement state is entered at the next drain, the sign is pending.
    let entry = app
        .signing
        .store
        .import(fixture_user_p12(), "777777")
        .expect("import");
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id),
        password: "777777".into(),
        ..SignDialog::new()
    });
    confirm_sign_dialog(&mut app);
    assert!(app.signing.dialog.is_none(), "the dialog closed");
    assert!(app.signing.pending.is_some(), "waiting for the click");
    crate::drain_commands(&mut app, &mut component);
    assert!(component.seal_placement_active());

    // The click that places the seal, then the command it queues: the
    // signature is made on the component, and the outcome is read back.
    let placement = SealPlacement {
        page_index: 0,
        page_id: PageId::new("1"),
        boundary: Rect {
            x: 80.0,
            y: 80.0,
            w: 40.0,
            h: 40.0,
        },
    };
    on_seal_placed(&mut app, placement.clone());
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    crate::drain_commands(&mut app, &mut component);

    // The signature is in the document that was open: it carries one valid
    // signature, in the package the document carries; the editor shows the
    // seal and nothing is pending. The document was not loaded again, so
    // the edit is still selected and can still be undone. The signature is
    // in the document, not in the file: the document is unsaved until the
    // user saves.
    assert_eq!(app.signatures.len(), 1);
    assert_eq!(app.signatures[0].overall, SignatureOverall::Valid);
    assert!(app.signing.pending.is_none());
    assert!(app.modified);
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
    assert!(
        document.annotations.find(&edit).is_some(),
        "the edit is in the signed document"
    );
    assert!(component.can_undo(), "the edit can still be undone");
    assert!(
        matches!(
            component.selection(),
            AnnotationSelection::Single(selected) if *selected == edit
        ),
        "the selection stays: {:?}",
        component.selection()
    );
    // The detail panel's thumbnails know the new seal.
    assert_eq!(
        app.seal_pictures
            .lock()
            .unwrap()
            .iter()
            .filter(|picture| picture.is_some())
            .count(),
        1
    );
    // Signing itself writes nothing.
    assert!(
        verdicts(&std::fs::read(&path).expect("file on disk")).is_empty(),
        "the file is as it was opened until the user saves"
    );

    // The user saves: an ordinary save, and the file carries the signature.
    crate::save_to(&mut app, path.clone());
    crate::drain_commands(&mut app, &mut component);
    crate::consume_save_outcome(&mut app);
    let on_disk = std::fs::read(&path).expect("file on disk");
    assert_eq!(verdicts(&on_disk), vec!["Valid/SesV4"]);
    assert!(!app.modified);
    let _ = std::fs::remove_file(&path);
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

/// Import the test certificate and confirm the dialog, auto-stamping.
fn sign_auto(app: &mut AppState) {
    let entry = app
        .signing
        .store
        .import(fixture_user_p12(), "777777")
        .expect("import");
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id),
        password: "777777".into(),
        auto_place: true,
        ..SignDialog::new()
    });
    confirm_sign_dialog(app);
}

/// Import the test certificate and confirm the dialog for a seal the user
/// places by hand.
fn sign_by_hand(app: &mut AppState) {
    let entry = app
        .signing
        .store
        .import(fixture_user_p12(), "777777")
        .expect("import");
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id),
        password: "777777".into(),
        ..SignDialog::new()
    });
    confirm_sign_dialog(app);
}

#[test]
fn a_refused_sign_reports_and_leaves_nothing_pending() {
    // A file another tool signed without an annotation entry, and an
    // annotation to write.
    let (mut app, mut component, path) =
        app_with_file(&signed_without_entry_package(), "refused.ofd");
    component.create_annotation(
        AnnotationKind::Shape(ShapeKind::Rect),
        PageId::new("1"),
        rect_payload(),
    );
    sign_auto(&mut app);
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert!(app.status.contains("没有批注入口"), "{}", app.status);
    assert!(app.signing.pending.is_none());
    assert_eq!(
        component.document().signatures.items.len(),
        1,
        "the file's own signature, nothing more"
    );
    assert_eq!(app.signatures.len(), 1);
    let _ = std::fs::remove_file(&path);

    // A seal placed off every page is refused the same way; the next sign
    // goes on.
    let (mut app, mut component, path) = app_with_file(&unsigned_package(), "after_refusal.ofd");
    sign_by_hand(&mut app);
    crate::drain_commands(&mut app, &mut component);
    on_seal_placed(
        &mut app,
        SealPlacement {
            page_index: 9,
            page_id: PageId::new("1"),
            boundary: NOWHERE,
        },
    );
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert!(app.status.contains("签章失败"), "{}", app.status);
    assert!(app.signing.pending.is_none());
    assert!(component.document().signatures.items.is_empty());
    sign_auto(&mut app);
    crate::drain_commands(&mut app, &mut component);
    drain_signing(&mut app);
    assert_eq!(app.signatures.len(), 1, "the refusal left nothing behind");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_auto_stamped_sign_needs_no_click_and_a_cancelled_one_is_forgotten() {
    let (mut app, mut component, path) = app_with_file(&unsigned_package(), "unsigned_auto.ofd");
    let entry = app
        .signing
        .store
        .import(fixture_user_p12(), "777777")
        .expect("import");
    // Auto-stamp: the material is queued at once.
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id.clone()),
        password: "777777".into(),
        auto_place: true,
        ..SignDialog::new()
    });
    confirm_sign_dialog(&mut app);
    crate::drain_commands(&mut app, &mut component);
    assert!(
        app.signatures.is_empty(),
        "nothing is signed before the pass"
    );
    drain_signing(&mut app);
    crate::drain_commands(&mut app, &mut component);
    assert_eq!(app.signatures.len(), 1);
    assert!(!component.seal_placement_active());

    // A seal placed by hand that the user cancels: nothing more is signed.
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id),
        password: "777777".into(),
        ..SignDialog::new()
    });
    confirm_sign_dialog(&mut app);
    crate::drain_commands(&mut app, &mut component);
    assert!(app.signing.pending.is_some());
    crate::sign_flow::on_seal_placement_cancelled(&mut app);
    drain_signing(&mut app);
    crate::drain_commands(&mut app, &mut component);
    assert!(app.signing.pending.is_none());
    assert_eq!(app.signatures.len(), 1, "still the one from before");
    assert!(app.modified, "unsaved until the user saves");
    assert!(
        verdicts(&std::fs::read(&path).expect("file on disk")).is_empty(),
        "signing writes nothing; the file changes when the user saves"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_wrong_password_keeps_the_dialog_open() {
    let (mut app, _component, path) = app_with_file(&unsigned_package(), "unsigned_pw.ofd");
    let entry = app
        .signing
        .store
        .import(fixture_user_p12(), "777777")
        .expect("import");
    app.signing.dialog = Some(SignDialog {
        certificate: Some(entry.id),
        password: "wrong".into(),
        auto_place: true,
        ..SignDialog::new()
    });
    confirm_sign_dialog(&mut app);
    let dialog = app.signing.dialog.as_ref().expect("still open");
    assert!(
        dialog.error.as_deref().is_some_and(|e| e.contains("口令")),
        "{:?}",
        dialog.error
    );
    assert!(app.signing.pending.is_none());
    assert!(app.signatures.is_empty());
    let _ = std::fs::remove_file(&path);
}

/// An unsigned package WITH an annotation layer: the entry file and
/// one page file, so that there is an annotation entry to protect.
fn annotated_package() -> Vec<u8> {
    let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
    let document = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData><ofd:Pages><ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages><ofd:Annotations>Annots/Annotations.xml</ofd:Annotations></ofd:Document>";
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
        (
            "Doc_0/Annots/Annotations.xml".to_owned(),
            MANIFEST.as_bytes().to_vec(),
        ),
        (
            "Doc_0/Annots/Page_0/Annotation.xml".to_owned(),
            EMPTY_PAGE_ANNOT.as_bytes().to_vec(),
        ),
    ])
    .expect("zip")
}

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:Page PageID=\"1\"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page></ofd:Annotations>";
const EMPTY_PAGE_ANNOT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\"></ofd:PageAnnot>";

/// Sign `bytes` the way another tool does, one that lists the
/// annotation entries among the protected files (the spec records that
/// such signatures exist): everything in the package is protected.
fn signed_by_a_tool_that_protects_annotations(bytes: &[u8], ca: &rofd_sign::TestCa) -> Vec<u8> {
    use rofd_sign::{
        build_e_seal_info, build_tbs, check_value, finish_seal, finish_signature, signature_xml,
        sm3, Reference, SealInput, SignatureXmlInput, Signer as _, TestSigner,
    };
    let mut signer: TestSigner = ca.signer();
    let png = BUILTIN_SEALS[0].png;
    let e_seal_info = build_e_seal_info(
        &SealInput {
            png,
            width: 40.0,
            height: 40.0,
            certificate_der: signer.certificate_der(),
        },
        NOW,
    )
    .expect("seal info");
    let maker = signer.sign(&e_seal_info).expect("maker");
    let seal = finish_seal(&e_seal_info, &maker, signer.certificate_der()).expect("seal");
    let mut entries: Vec<(String, Vec<u8>)> = rofd_io::zip_util::read_all_entries(bytes)
        .expect("entries")
        .into_iter()
        .map(|(name, data)| {
            if name == "OFD.xml" {
                let patched = rofd_io::with_signatures_loc(
                    std::str::from_utf8(&data).expect("OFD.xml"),
                    "Doc_0/Signs/Signatures.xml",
                )
                .expect("declare the list");
                (name, patched.into_bytes())
            } else {
                (name, data)
            }
        })
        .collect();
    let mut references: Vec<Reference> = entries
        .iter()
        .map(|(name, data)| Reference {
            file_ref: format!("/{name}"),
            check_value_base64: check_value(data),
        })
        .collect();
    references.push(Reference {
        file_ref: "/Doc_0/Signs/Sign_0/Seal.esl".to_owned(),
        check_value_base64: check_value(&seal),
    });
    let description = signature_xml(&SignatureXmlInput {
        signed_value_loc: "/Doc_0/Signs/Sign_0/SignedValue.dat",
        provider_name: "another tool",
        provider_company: None,
        provider_version: None,
        date_time: NOW,
        seal_base_loc: Some("/Doc_0/Signs/Sign_0/Seal.esl"),
        stamps: &[],
        references: &references,
    });
    let tbs = build_tbs(
        &seal,
        &sm3(description.as_bytes()),
        "/Doc_0/Signs/Sign_0/Signature.xml",
        NOW,
    )
    .expect("tbs");
    let signature = signer.sign(&tbs).expect("sign");
    let value = finish_signature(&tbs, &signature, signer.certificate_der()).expect("value");
    let list = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:MaxSignId>1</ofd:MaxSignId><ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_0/Signature.xml\"/></ofd:Signatures>";
    entries.push((
        "Doc_0/Signs/Signatures.xml".to_owned(),
        list.as_bytes().to_vec(),
    ));
    entries.push(("Doc_0/Signs/Sign_0/Seal.esl".to_owned(), seal));
    entries.push(("Doc_0/Signs/Sign_0/SignedValue.dat".to_owned(), value));
    entries.push((
        "Doc_0/Signs/Sign_0/Signature.xml".to_owned(),
        description.into_bytes(),
    ));
    rofd_io::zip_util::write_zip(&entries).expect("zip")
}

#[test]
fn a_save_that_would_invalidate_a_signature_waits_for_a_word() {
    // A file signed by a tool that protects the annotation entries: an
    // annotation added and saved would break that signature (M17).
    let ca = rofd_sign::TestCa::from_seed("another tool", b"seed-tool");
    let signed = signed_by_a_tool_that_protects_annotations(&annotated_package(), &ca);
    assert_eq!(verdicts(&signed), vec!["Valid/SesV4"]);
    let (mut app, mut component, path) = app_with_file(&signed, "protecting.ofd");
    assert_eq!(app.signatures.len(), 1);
    assert_eq!(app.signatures[0].overall, SignatureOverall::Valid);

    // An annotation is added in the editor.
    component.create_annotation(
        AnnotationKind::Shape(ShapeKind::Rect),
        PageId::new("1"),
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
        },
    );
    app.modified = true;

    // Save: the candidate bytes are checked, and the save waits.
    crate::do_save(&mut app);
    crate::drain_commands(&mut app, &mut component);
    crate::sign_flow::drain_save_candidate(&mut app);
    let confirm = app
        .signing
        .save_confirm
        .as_ref()
        .expect("the save asks first");
    assert!(
        matches!(confirm, SaveConfirm::Invalidated { ids, .. } if ids.len() == 1),
        "{confirm:?}"
    );

    // A no: nothing is written, the document stays unsaved.
    let on_disk = std::fs::read(&path).expect("file on disk");
    crate::sign_flow::save_confirm_answer(&mut app, false);
    crate::drain_commands(&mut app, &mut component);
    crate::consume_save_outcome(&mut app);
    assert!(app.signing.save_confirm.is_none());
    assert_eq!(
        std::fs::read(&path).expect("file on disk"),
        on_disk,
        "not written"
    );
    assert!(app.modified);
    assert_eq!(app.signatures[0].overall, SignatureOverall::Valid);

    // A yes: the bytes go out as they are, and the file's verdict
    // follows.
    crate::do_save(&mut app);
    crate::drain_commands(&mut app, &mut component);
    crate::sign_flow::drain_save_candidate(&mut app);
    assert!(app.signing.save_confirm.is_some());
    crate::sign_flow::save_confirm_answer(&mut app, true);
    crate::drain_commands(&mut app, &mut component);
    crate::consume_save_outcome(&mut app);
    assert!(app.signing.save_confirm.is_none());
    assert!(!app.modified);
    assert_eq!(
        app.signatures[0].overall,
        SignatureOverall::Invalid,
        "what the file now carries"
    );
    assert_eq!(
        verdicts(&std::fs::read(&path).expect("file on disk")),
        vec!["Invalid/SesV4"]
    );
    let _ = std::fs::remove_file(&path);
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
