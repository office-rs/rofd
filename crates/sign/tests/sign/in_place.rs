//! Signing in place (spec 2026-11-23 §17.11): the signature goes into the
//! document that is open. Nothing of it is made again - not the model, not
//! the package, not a signed copy of the file - and what comes out is what a
//! reload would give.

use std::collections::BTreeMap;
use std::sync::Arc;

use rofd_sign::{
    prepare_in_place, sign_in_place, sign_refusal, verify_document, InPlaceStep, SignatureToAppend,
};

use super::*;

/// A document open the way a host opens it.
fn opened(bytes: &[u8]) -> OfdDocument {
    parse_ofd(bytes).expect("parse_ofd").document
}

/// The bytes a save of `document` writes.
fn saved(document: &OfdDocument) -> Vec<u8> {
    save_ofd(document, &hints()).expect("save").bytes
}

fn entry_map(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    entries(bytes).into_iter().collect()
}

fn text_of(map: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(map.get(name).expect("entry").clone()).expect("UTF-8")
}

/// The entries that carry signatures: the list and everything below a
/// `Signs` directory.
fn signature_entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    entry_map(bytes)
        .into_iter()
        .filter(|(name, _)| name.ends_with("Signatures.xml") || name.contains("/Signs/"))
        .collect()
}

/// Built packages, and the workspace's own samples when they are checked
/// out (io's tests read them the same way).
fn samples() -> Vec<(String, Vec<u8>)> {
    let mut samples = vec![
        ("unsigned, no annotation layer".to_owned(), bare_package()),
        ("unsigned, with annotations".to_owned(), annotated_package()),
        (
            "signed already: the second signature".to_owned(),
            sign_seal(&annotated_package()),
        ),
    ];
    for name in [
        "ru-yuan-ji-lu.ofd",
        "text_annotation.ofd",
        "sample-content.ofd",
    ] {
        if let Ok(bytes) = std::fs::read(format!("../../test/{name}")) {
            samples.push((name.to_owned(), bytes));
        }
    }
    samples
}

#[test]
fn signing_in_place_touches_only_the_signature_and_saves_valid() {
    let bytes = annotated_package();
    let mut document = opened(&bytes);
    let page = Arc::clone(
        &document
            .package
            .as_ref()
            .unwrap()
            .find("Doc_0/Pages/Page_0/Content.xml")
            .unwrap()
            .bytes,
    );
    let before = entry_map(&bytes);
    let reports = sign_in_place(&mut document, &mut first_ca().signer(), &seal_options())
        .expect("sign")
        .reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    // The same entry buffer - the handle is a copy that shares it:
    // nothing was parsed or copied again.
    let package = document.package.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &page,
        &package
            .find("Doc_0/Pages/Page_0/Content.xml")
            .unwrap()
            .bytes
    ));
    assert!(package.signatures_declared);
    assert_eq!(document.signatures.items.len(), 1);
    let file = saved(&document);
    assert!(all_valid(&file));
    let after = entry_map(&file);
    for (name, bytes) in &before {
        let patched = name.ends_with("Document.xml") || name == "OFD.xml";
        let annotation = name.ends_with("Annotations.xml") || name.ends_with("P_7.xml");
        if !patched && !annotation {
            assert_eq!(after.get(name), Some(bytes), "{name}");
        }
    }
}

#[test]
fn in_place_and_bytes_signing_write_the_same_signature() {
    let bytes = annotated_package();
    let mut document = opened(&bytes);
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    let ours = entry_map(&saved(&document));
    let file = sign_seal(&bytes);
    let theirs = entry_map(&file);
    assert_eq!(
        ours.keys().collect::<Vec<_>>(),
        theirs.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        ours.get("Doc_0/Signs/Signatures.xml"),
        theirs.get("Doc_0/Signs/Signatures.xml")
    );
    let ours_xml = text_of(&ours, "Doc_0/Signs/Sign_0/Signature.xml");
    let theirs_xml = text_of(&theirs, "Doc_0/Signs/Sign_0/Signature.xml");
    let fixed_refs = |xml: &str| {
        file_refs(xml)
            .into_iter()
            .filter(|(reference, _)| !reference.ends_with("Seal.esl"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        fixed_refs(&ours_xml),
        fixed_refs(&theirs_xml),
        "the seal carries a random number; the rest is fixed"
    );
    assert_eq!(stamps(&saved(&document)), stamps(&file));
}

#[test]
fn the_signed_model_is_what_a_reload_gives() {
    for (name, bytes) in samples() {
        for (what, opts) in [("seal", seal_options()), ("no seal", SignOptions::new(NOW))] {
            let case = format!("{name}, {what}");
            let mut document = opened(&bytes);
            sign_in_place(&mut document, &mut first_ca().signer(), &opts).expect("sign");
            let reloaded = parse_ofd(&saved(&document)).expect("parse").document;
            assert_eq!(document.max_unit_id, reloaded.max_unit_id, "{case}");
            assert_eq!(document.meta, reloaded.meta, "{case}");
            assert_eq!(
                document.raw_doc_children, reloaded.raw_doc_children,
                "{case}"
            );
            assert_eq!(
                document.raw_common_children, reloaded.raw_common_children,
                "{case}"
            );
            assert_eq!(
                document.raw_ofd_body_children, reloaded.raw_ofd_body_children,
                "{case}"
            );
            assert_eq!(document.signatures, reloaded.signatures, "{case}");
            assert!(all_valid(&saved(&document)), "{case}");
        }
    }
}

#[test]
fn a_document_without_a_package_gets_one_and_saves_valid() {
    let mut document = opened(&bare_package());
    document.package = None;
    assert!(verify_document(&document, &VerifyOptions::default()).is_empty());
    add_rect(&mut document, "1");
    let reports = sign_in_place(&mut document, &mut first_ca().signer(), &seal_options())
        .expect("sign")
        .reports;
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    assert!(document.package.is_some());
    let file = saved(&document);
    assert!(all_valid(&file));
    assert_eq!(
        parse_ofd(&file).unwrap().document.annotations.by_page.len(),
        1,
        "the edit is in the file"
    );
}

#[test]
fn a_second_signature_in_place_sees_the_first() {
    let mut document = opened(&annotated_package());
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("first");
    let reports = sign_in_place(
        &mut document,
        &mut ca("second", b"seed-b").signer(),
        &SignOptions::new(NOW),
    )
    .expect("second")
    .reports;
    assert_eq!(reports.len(), 2);
    assert!(reports
        .iter()
        .all(|report| report.overall == SignatureOverall::Valid));
    assert_eq!(document.signatures.items.len(), 2);
    assert!(all_valid(&saved(&document)));
}

#[test]
fn an_unsigned_file_without_an_anchor_is_regenerated_first() {
    // Document.xml without MaxUnitID. A save of such a file falls back to
    // the full write; signing in place does the same, once.
    let files: Vec<(String, Vec<u8>)> = vec![
        ("OFD.xml".to_owned(), ofd_xml().into_bytes()),
        (
            "Doc_0/Document.xml".to_owned(),
            document_xml(&two_pages(), None, false).into_bytes(),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml".to_owned(),
            page(two_pages()[0]).into_bytes(),
        ),
        (
            "Doc_0/Pages/Page_1/Content.xml".to_owned(),
            page(two_pages()[1]).into_bytes(),
        ),
    ];
    let bytes = zip_of(&files);
    let mut document = opened(&bytes);
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    assert!(document.max_unit_id > 65_536);
    assert!(all_valid(&saved(&document)));
    // The bytes API keeps refusing it, as it always did.
    let error = sign(&bytes, &mut first_ca().signer(), &seal_options()).unwrap_err();
    assert!(matches!(error, SignError::Encode { .. }), "{error}");
}

#[test]
fn a_signed_file_without_annotation_entry_is_refused_unchanged() {
    let bytes = std::fs::read("tests/fixtures/SESV4SignDoc.ofd").unwrap();
    let mut document = opened(&bytes);
    // No annotation to write: nothing is in the way.
    assert!(sign_refusal(&document).is_ok());
    let page = document.pages[0].id.0.clone();
    add_rect(&mut document, &page);
    assert!(matches!(
        sign_refusal(&document),
        Err(SignError::AnnotationsUnwritable)
    ));
    let before = serde_json::to_value(&document.signatures).unwrap();
    let error =
        sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).unwrap_err();
    assert!(matches!(error, SignError::AnnotationsUnwritable), "{error}");
    assert_eq!(serde_json::to_value(&document.signatures).unwrap(), before);
    assert!(document
        .package
        .as_ref()
        .unwrap()
        .find("Doc_0/Signs/Sign_1/Signature.xml")
        .is_none());
}

#[test]
fn edits_and_saves_after_the_signature_keep_its_files() {
    let mut document = opened(&annotated_package());
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("sign");
    let first = signature_entries(&saved(&document));
    add_rect(&mut document, "1");
    let file = saved(&document);
    assert_eq!(
        signature_entries(&file),
        first,
        "the signature's own files are written as they were made"
    );
    assert!(all_valid(&file));
    assert_eq!(
        parse_ofd(&file).unwrap().document.annotations.by_page.len(),
        1
    );
    assert_eq!(
        verify_document(&document, &VerifyOptions::default())[0].overall,
        SignatureOverall::Valid
    );
}

#[test]
fn verify_document_reads_the_document() {
    let mut document = opened(&bare_package());
    assert!(verify_document(&document, &VerifyOptions::default()).is_empty());
    sign_in_place(
        &mut document,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    assert_eq!(
        verify_document(&document, &VerifyOptions::default()).len(),
        1
    );
}

#[test]
fn old_layout_annotations_survive_a_signature_in_place() {
    // The page file in its page directory, no entry file (the layout of
    // `signing_an_old_layout_package_keeps_its_annotations_reachable`):
    // the scaffold's list names it through `..` and nothing moves.
    let mut files = entries(&bare_package());
    files.push((
        "Doc_0/Pages/Page_0/Annotation.xml".to_owned(),
        PAGE_ANNOT.as_bytes().to_vec(),
    ));
    let mut document = opened(&zip_of(&files));
    sign_in_place(
        &mut document,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    let package = document.package.as_ref().unwrap();
    assert_eq!(
        package.annotations_entry.as_deref(),
        Some("Doc_0/Annots/Annotations.xml")
    );
    assert_eq!(package.annotation_files.len(), 1);
    let file = saved(&document);
    assert!(text_of(&entry_map(&file), "Doc_0/Annots/Annotations.xml")
        .contains("../Pages/Page_0/Annotation.xml"));
    assert_eq!(
        parse_ofd(&file)
            .unwrap()
            .document
            .package
            .as_ref()
            .unwrap()
            .annotation_files
            .len(),
        1
    );
    assert!(all_valid(&file));
}

// ---- the two-phase form ----

#[test]
fn preparing_changes_nothing() {
    let document = opened(&annotated_package());
    let before = serde_json::to_value(&document.signatures).unwrap();
    let prepared =
        prepare_in_place(&document, &first_ca().cert_der, &seal_options()).expect("prepare");
    assert!(!prepared.to_sign().is_empty());
    assert_eq!(serde_json::to_value(&document.signatures).unwrap(), before);
    assert!(document
        .package
        .as_ref()
        .unwrap()
        .find("Doc_0/Signs/Signatures.xml")
        .is_none());
    assert_eq!(document.max_unit_id, 9);
}

#[test]
fn two_phase_in_place_lands_the_signature() {
    let mut document = opened(&annotated_package());
    let ca = first_ca();
    let mut signer = ca.signer();
    let mut step = InPlaceStep::More(
        prepare_in_place(&document, &ca.cert_der, &seal_options()).expect("prepare"),
    );
    let reports = loop {
        step = match step {
            InPlaceStep::More(prepared) => {
                let signature = signer.sign(prepared.to_sign()).expect("sign");
                prepared.apply(&signature).expect("apply")
            }
            InPlaceStep::Done(appendix) => {
                break appendix.append_to(&mut document).expect("land").reports;
            }
        };
    };
    assert_eq!(reports.len(), 1);
    assert!(all_valid(&saved(&document)));
}

#[test]
fn a_signature_prepared_before_another_landed_is_refused() {
    let mut document = opened(&annotated_package());
    let later = ca("later", b"seed-l");
    let prepared =
        prepare_in_place(&document, &later.cert_der, &SignOptions::new(NOW)).expect("prepare");
    let signature = later.signer().sign(prepared.to_sign()).expect("sign");
    // Another signature lands in the meantime.
    sign_in_place(&mut document, &mut first_ca().signer(), &seal_options()).expect("first");
    let InPlaceStep::Done(appendix) = prepared
        .apply(&signature)
        .expect("one round without a seal")
    else {
        panic!("a digital signature takes one round");
    };
    let error = appendix.append_to(&mut document).unwrap_err();
    assert!(matches!(error, SignError::DocumentChanged), "{error}");
    // The one signature in the file is the first one, with its seal. Both
    // were planned for the same directory, so had the refused one landed it
    // would have replaced it: its signer must be nowhere in the file.
    let file = saved(&document);
    let reports = reports_of(&file);
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0]
            .certificate
            .as_ref()
            .map(|certificate| certificate.subject.clone()),
        Some("rofd test signer".to_owned())
    );
    assert_eq!(reports[0].variant, SignatureVariant::SesV4);
    assert!(
        entry_map(&file)
            .values()
            .all(|bytes| !bytes.windows(5).any(|window| window == b"later")),
        "nothing of the refused signature is in the file"
    );
}

#[test]
fn annotation_edits_between_preparing_and_landing_are_fine() {
    let mut document = opened(&annotated_package());
    let ca = first_ca();
    let prepared =
        prepare_in_place(&document, &ca.cert_der, &SignOptions::new(NOW)).expect("prepare");
    let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
    add_rect(&mut document, "1");
    let InPlaceStep::Done(appendix) = prepared.apply(&signature).expect("one round") else {
        panic!("a digital signature takes one round");
    };
    assert_eq!(
        appendix
            .append_to(&mut document)
            .expect("land")
            .reports
            .len(),
        1
    );
    assert!(all_valid(&saved(&document)));
}

/// Prepare a digital signature on `document` and bring it to the round
/// before landing.
fn prepared_signature(document: &OfdDocument) -> SignatureToAppend {
    let ca = first_ca();
    let prepared =
        prepare_in_place(document, &ca.cert_der, &SignOptions::new(NOW)).expect("prepare");
    let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
    let InPlaceStep::Done(appendix) = prepared.apply(&signature).expect("one round") else {
        panic!("a digital signature takes one round");
    };
    *appendix
}

#[test]
fn annotation_edits_between_preparing_and_landing_are_fine_without_a_package() {
    // A document that was never saved: one is made for the signature, and an
    // edit of the annotations in the meantime is still no change - made the
    // way the editor makes one, MaxUnitID raised to the new ID included.
    let mut document = opened(&bare_package());
    document.package = None;
    let appendix = prepared_signature(&document);
    add_rect(&mut document, "1");
    document.max_unit_id += 1;
    assert_eq!(
        appendix
            .append_to(&mut document)
            .expect("land")
            .reports
            .len(),
        1
    );
    let file = saved(&document);
    assert!(all_valid(&file));
    assert_eq!(
        parse_ofd(&file).unwrap().document.annotations.by_page.len(),
        1,
        "the edit is in the file"
    );
}

/// The same package with one more entry: an annotation file of a page the
/// document does not have, so nothing locates it.
fn with_orphan_annotation(bytes: &[u8]) -> Vec<u8> {
    let mut files = entries(bytes);
    files.push((
        "Doc_0/Pages/Page_7/Annotation.xml".to_owned(),
        PAGE_ANNOT.as_bytes().to_vec(),
    ));
    zip_of(&files)
}

#[test]
fn a_signature_prepared_for_one_file_lands_on_no_twin_in_a_file_no_page_uses() {
    // That file is not the annotation layer as the package locates it, so a
    // signature protects it: a twin that differs only there is another
    // package.
    let bytes = with_orphan_annotation(&bare_package());
    let mut signed = opened(&bytes);
    sign_in_place(
        &mut signed,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    let list = signature_entries(&saved(&signed));
    let description = list
        .iter()
        .find(|(name, _)| name.ends_with("Signature.xml"))
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .expect("the signature's description file");
    assert!(
        file_refs(&description)
            .iter()
            .any(|(path, _)| path.ends_with("Page_7/Annotation.xml")),
        "the signature protects the orphan file: {description}"
    );

    let mut files = entries(&bytes);
    let orphan = files
        .iter_mut()
        .find(|(name, _)| name.ends_with("Page_7/Annotation.xml"))
        .expect("the orphan file");
    orphan.1.extend_from_slice(b"<!-- another file -->");
    let mut document = opened(&bytes);
    let appendix = prepared_signature(&document);
    document = opened(&zip_of(&files));
    let error = appendix.append_to(&mut document).unwrap_err();
    assert!(matches!(error, SignError::DocumentChanged), "{error}");
    assert!(document.signatures.items.is_empty());
}

#[test]
fn a_signature_prepared_for_a_document_without_a_package_lands_on_no_other() {
    // Such a document has no package to be recognised by.
    let document = opened(&bare_package());
    let bare = OfdDocument {
        package: None,
        ..document
    };
    let appendix = prepared_signature(&bare);
    // Another document that was never saved is opened in the meantime.
    let mut other = bare.clone();
    other.pages.push(rofd_dom::Page {
        id: PageId::new("another"),
        ..other.pages[0].clone()
    });
    let error = appendix.append_to(&mut other).unwrap_err();
    assert!(matches!(error, SignError::DocumentChanged), "{error}");
    assert!(other.package.is_none(), "no package was handed to it");
    assert!(other.signatures.items.is_empty());
}

#[test]
fn a_signature_prepared_for_one_file_lands_on_no_twin() {
    // The same OFD.xml, Document.xml and (no) signature list - and another page.
    let mut document = opened(&bare_package());
    let mut twin = opened(&bare_package());
    let content = twin
        .package
        .as_mut()
        .expect("parsed")
        .entries
        .iter_mut()
        .find(|entry| entry.name.ends_with("Content.xml"))
        .expect("the page's content");
    let mut bytes = content.bytes.to_vec();
    bytes.extend_from_slice(b"<!-- another file -->");
    content.bytes = Arc::new(bytes);
    let appendix = prepared_signature(&document);
    document = twin;
    let error = appendix.append_to(&mut document).unwrap_err();
    assert!(matches!(error, SignError::DocumentChanged), "{error}");
    assert!(document.signatures.items.is_empty());
    assert_eq!(
        document.package.as_ref().expect("parsed").entries.len(),
        entries(&bare_package()).len(),
        "nothing was written into it"
    );
}

#[test]
fn a_signature_prepared_for_a_file_lands_on_the_same_file_read_again() {
    // What the signature was made over is still there, byte for byte.
    let bytes = annotated_package();
    let mut document = opened(&bytes);
    let appendix = prepared_signature(&document);
    document = opened(&bytes);
    assert_eq!(
        appendix
            .append_to(&mut document)
            .expect("land")
            .reports
            .len(),
        1
    );
    assert!(all_valid(&saved(&document)));
}

// ---- what the landing hands back ----

#[test]
fn the_handle_says_what_a_reload_says_for_a_list_ofd_xml_never_declares() {
    // A package another tool got wrong: a signature list with a record, and
    // an OFD.xml that declares no list. Nothing is patched for the new
    // signature, so the handle must not pretend that something was.
    let mut files = entries(&sign_seal(&annotated_package()));
    let ofd = files
        .iter_mut()
        .find(|(name, _)| name == "OFD.xml")
        .expect("OFD.xml");
    ofd.1 = ofd_xml().into_bytes();
    let mut document = opened(&zip_of(&files));
    sign_in_place(
        &mut document,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    let handle = document.package.as_ref().expect("parsed");
    let reloaded = parse_ofd(&saved(&document)).unwrap().document;
    let reloaded_handle = reloaded.package.as_ref().unwrap();
    assert_eq!(
        handle.signatures_declared,
        reloaded_handle.signatures_declared
    );
    assert_eq!(handle.signatures_entry, reloaded_handle.signatures_entry);
    assert!(!handle.signatures_declared);
    assert_eq!(document.signatures, reloaded.signatures);
}

#[test]
fn a_regenerated_package_brings_its_parse_warnings_back() {
    // Document.xml without MaxUnitID, so a package is made again before the
    // signature - and an element on the page that rofd does not model.
    let files: Vec<(String, Vec<u8>)> = vec![
        ("OFD.xml".to_owned(), ofd_xml().into_bytes()),
        (
            "Doc_0/Document.xml".to_owned(),
            document_xml(&["0 0 210 297"], None, false).into_bytes(),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml".to_owned(),
            page("0 0 210 297")
                .replace(
                    "<ofd:Content/>",
                    "<ofd:Content><ofd:Unknown/></ofd:Content>",
                )
                .into_bytes(),
        ),
    ];
    let bytes = zip_of(&files);
    assert!(
        parse_ofd(&bytes)
            .unwrap()
            .warnings
            .iter()
            .any(|warning| matches!(warning, rofd_dom::OfdWarning::PreservedRaw { .. })),
        "reading the file says what it keeps verbatim"
    );
    let mut document = opened(&bytes);
    let landed = sign_in_place(
        &mut document,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    assert!(
        landed
            .warnings
            .iter()
            .any(|warning| matches!(warning, rofd_dom::OfdWarning::PreservedRaw { .. })),
        "reading the package that was made says it again: {:?}",
        landed.warnings
    );
    assert_eq!(landed.reports.len(), 1);
    assert!(all_valid(&saved(&document)));
}

#[test]
fn the_warnings_of_the_signatures_that_were_there_are_not_repeated() {
    // A file signed with a seal that is no picture. Reading it warns about
    // that seal - once, when the file is opened.
    let mut files = entries(&sign_seal(&annotated_package()));
    let seal = files
        .iter_mut()
        .find(|(name, _)| name.ends_with("Seal.esl"))
        .expect("the seal");
    seal.1 = b"not a picture".to_vec();
    let bytes = zip_of(&files);
    let about_the_seal = |warning: &rofd_dom::OfdWarning| {
        matches!(
            warning,
            rofd_dom::OfdWarning::MissingFeature { feature, .. } if feature.contains("seal picture")
        )
    };
    assert!(
        parse_ofd(&bytes)
            .unwrap()
            .warnings
            .iter()
            .any(about_the_seal),
        "reading the file says so about that seal"
    );
    let mut document = opened(&bytes);
    let landed = sign_in_place(
        &mut document,
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    assert!(landed.warnings.is_empty(), "{:?}", landed.warnings);
    assert_eq!(landed.reports.len(), 2);
}
