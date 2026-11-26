//! What a signature protects and what signing does to a package it did
//! not sign before: the References list, the scaffolds, the ID reserve.

use super::*;

#[test]
fn references_follow_i4() {
    let signed = sign_seal(&annotated_package());
    let all = entries(&signed);
    let refs = file_refs(&text(&all, "Doc_0/Signs/Sign_0/Signature.xml"));
    let paths: Vec<&str> = refs.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/Doc_0/Attachs/note.txt",
            "/Doc_0/Document.xml",
            "/Doc_0/Pages/Page_0/Content.xml",
            "/Doc_0/Pages/Page_1/Content.xml",
            "/OFD.xml",
            "/Doc_0/Signs/Sign_0/Seal.esl",
        ],
        "everything but the annotation layer and the signatures, by path; the seal last"
    );
    assert!(text(&all, "Doc_0/Signs/Sign_0/Signature.xml")
        .contains(r#"<ofd:References CheckMethod="SM3">"#));
    for (path, check) in &refs {
        let bytes = entry(&all, &path[1..]);
        let expected: String =
            base64::engine::general_purpose::STANDARD.encode(rofd_sign::sm3(bytes));
        assert_eq!(check, &expected, "{path}");
    }
}

#[test]
fn unsigned_foreign_file_gets_scaffolds_only() {
    let original = bare_package();
    let signed = sign_seal(&original);
    let (before, after) = (entries(&original), entries(&signed));
    for name in [
        "Doc_0/Attachs/note.txt",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Pages/Page_1/Content.xml",
    ] {
        assert_eq!(option(&after, name), option(&before, name), "{name}");
    }
    assert_eq!(
        text(&after, "OFD.xml"),
        rofd_io::with_signatures_loc(&text(&before, "OFD.xml"), "Doc_0/Signs/Signatures.xml")
            .expect("patched"),
        "OFD.xml gains the signature entry and nothing else"
    );
    let reserve = 9 + SIGNATURE_ID_RESERVE;
    let expected = with_annotations_loc(
        &text(&before, "Doc_0/Document.xml"),
        "Annots/Annotations.xml",
    )
    .expect("patched")
    .replace(
        "<ofd:MaxUnitID>9</ofd:MaxUnitID>",
        &format!("<ofd:MaxUnitID>{reserve}</ofd:MaxUnitID>"),
    );
    assert_eq!(
        text(&after, "Doc_0/Document.xml"),
        expected,
        "Document.xml gains the annotation entry and the ID reserve"
    );
    assert_eq!(
        max_unit_id_of(&text(&after, "Doc_0/Document.xml")),
        Some(reserve)
    );
    assert_eq!(text(&after, "Doc_0/Annots/Annotations.xml"), EMPTY_MANIFEST);

    // Annotation entries are never touched.
    let original = annotated_package();
    let signed = sign_seal(&original);
    let (before, after) = (entries(&original), entries(&signed));
    for name in ["Doc_0/Annots/Annotations.xml", "Doc_0/Annots/P_7.xml"] {
        assert_eq!(option(&after, name), option(&before, name), "{name}");
    }
}

#[test]
fn reserve_written_at_signing_covers_post_signature_ids() {
    let signed = sign_seal(&bare_package());
    let report = parse_ofd(&signed).expect("parse_ofd");
    let mut document = report.document.clone();
    for _ in 0..3 {
        add_rect(&mut document, "1");
        add_rect(&mut document, "2");
    }
    let preserved = save_ofd(&document, &hints()).expect("save");
    assert!(!preserved.reserve_exhausted);
    assert!(!preserved.blocked_by_signature);
    let all = entries(&preserved.bytes);
    let ceiling = max_unit_id_of(&text(&all, "Doc_0/Document.xml")).expect("MaxUnitID");
    assert_eq!(ceiling, 9 + SIGNATURE_ID_RESERVE);
    let mut used = Vec::new();
    for (name, bytes) in &all {
        if name.starts_with("Doc_0/Annots/") {
            used.extend(numeric_ids(&String::from_utf8_lossy(bytes)));
        }
    }
    assert!(used.len() >= 6, "the six annotations carry IDs: {used:?}");
    assert!(
        used.iter().all(|id| *id <= ceiling),
        "every ID stays under the reserve: {used:?}"
    );
    assert!(all_valid(&preserved.bytes));
}

#[test]
fn ofdrw_sample_without_annotation_entry_is_blocked_not_damaged() {
    let original = std::fs::read("tests/fixtures/SESV4SignDoc.ofd").expect("fixture");
    let report = parse_ofd(&original).expect("parse_ofd");
    assert!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .annotations_entry
            .is_none(),
        "the sample declares no annotation entry"
    );
    let mut document = report.document.clone();
    let page = document.pages.last().expect("a page").id.0.clone();
    add_rect(&mut document, &page);
    let preserved = save_ofd(&document, &hints()).expect("save");
    assert!(preserved.blocked_by_signature);
    assert_eq!(
        entries(&preserved.bytes),
        entries(&original),
        "nothing is written"
    );
    assert!(all_valid(&preserved.bytes));

    // Another signature cannot change that: Document.xml is protected by
    // the one already there.
    let signed = sign_seal(&original);
    let report = parse_ofd(&signed).expect("parse_ofd");
    let mut document = report.document.clone();
    add_rect(&mut document, &page);
    let preserved = save_ofd(&document, &hints()).expect("save");
    assert!(preserved.blocked_by_signature);

    // The same file before anyone signed it: signing adds the annotation
    // layer, and annotations can then be added without touching
    // Document.xml again.
    let unsigned: Vec<(String, Vec<u8>)> = entries(&original)
        .into_iter()
        .filter(|(name, _)| !name.starts_with("Doc_0/Signs/"))
        .map(|(name, bytes)| {
            let bytes = if name == "OFD.xml" {
                String::from_utf8(bytes)
                    .expect("OFD.xml")
                    .replace(
                        "<ofd:Signatures>/Doc_0/Signs/Signatures.xml</ofd:Signatures>",
                        "",
                    )
                    .into_bytes()
            } else {
                bytes
            };
            (name, bytes)
        })
        .collect();
    let unsigned = zip_of(&unsigned);
    assert!(
        !parse_ofd(&unsigned)
            .expect("parse_ofd")
            .document
            .package
            .expect("package")
            .signatures_declared
    );
    let signed = sign_seal(&unsigned);
    let report = parse_ofd(&signed).expect("parse_ofd");
    let mut document = report.document.clone();
    add_rect(&mut document, &page);
    let preserved = save_ofd(&document, &hints()).expect("save");
    assert!(!preserved.blocked_by_signature);
    assert!(all_valid(&preserved.bytes));
}

#[test]
fn a_declared_but_empty_signature_list_still_gets_the_reserve() {
    // OFD.xml declares a signature list the package does not carry: nothing
    // is signed yet, so the document can still receive the scaffolds.
    let declared = declaring("Doc_0/Signs/Signatures.xml");
    let original = with_entry(&bare_package(), "OFD.xml", declared.as_bytes());
    assert!(
        parse_ofd(&original)
            .expect("parse_ofd")
            .document
            .package
            .expect("package")
            .signatures_declared
    );
    let signed = sign_seal(&original);
    let all = entries(&signed);
    assert_eq!(text(&all, "OFD.xml"), declared, "the declaration stays");
    assert_eq!(
        max_unit_id_of(&text(&all, "Doc_0/Document.xml")),
        Some(9 + SIGNATURE_ID_RESERVE)
    );
    assert_eq!(text(&all, "Doc_0/Annots/Annotations.xml"), EMPTY_MANIFEST);
    assert!(option(&all, "Doc_0/Signs/Signatures.xml").is_some());
    assert!(all_valid(&signed));
}

#[test]
fn a_shared_signature_directory_still_protects_the_document() {
    // The list sits next to the document's own files, not in a directory of
    // its own: only the signatures are left out of the references.
    let original = with_entry(
        &annotated_package(),
        "OFD.xml",
        declaring("Doc_0/Signatures.xml").as_bytes(),
    );
    let signed = sign_seal(&original);
    let all = entries(&signed);
    assert!(option(&all, "Doc_0/Signatures.xml").is_some());
    let paths = |all: &[(String, Vec<u8>)], signature: &str| -> Vec<String> {
        file_refs(&text(all, &format!("Doc_0/{signature}/Signature.xml")))
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    };
    assert_eq!(
        paths(&all, "Sign_0"),
        vec![
            "/Doc_0/Attachs/note.txt",
            "/Doc_0/Document.xml",
            "/Doc_0/Pages/Page_0/Content.xml",
            "/Doc_0/Pages/Page_1/Content.xml",
            "/OFD.xml",
            "/Doc_0/Sign_0/Seal.esl",
        ]
    );

    let twice = sign(
        &signed,
        &mut ca("second signer", b"seed-b").signer(),
        &SignOptions::new(NOW),
    )
    .expect("second");
    assert!(all_valid(&twice));
    let all = entries(&twice);
    assert_eq!(
        paths(&all, "Sign_1"),
        vec![
            "/Doc_0/Attachs/note.txt",
            "/Doc_0/Document.xml",
            "/Doc_0/Pages/Page_0/Content.xml",
            "/Doc_0/Pages/Page_1/Content.xml",
            "/OFD.xml",
        ]
    );
}

#[test]
fn directory_entries_are_not_references() {
    // Archives written by common tools carry entries for the directories
    // themselves. They hold no file, so they protect nothing.
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for name in [
        "OFD.xml",
        "Doc_0/",
        "Doc_0/Document.xml",
        "Doc_0/Pages/",
        "Doc_0/Pages/Page_0/Content.xml",
    ] {
        if let Some(directory) = name.strip_suffix('/') {
            writer
                .add_directory(directory.to_owned(), SimpleFileOptions::default())
                .expect("directory entry");
        } else {
            writer
                .start_file::<_, ()>(name, SimpleFileOptions::default())
                .expect("file entry");
            let content = match name {
                "OFD.xml" => ofd_xml(),
                "Doc_0/Document.xml" => document_xml(&["0 0 210 297"], Some(9), false),
                _ => page("0 0 210 297"),
            };
            writer.write_all(content.as_bytes()).expect("content");
        }
    }
    let package = writer.finish().expect("zip").into_inner();
    assert_eq!(entries(&package).len(), 5, "two of them are directories");

    let signed = sign_seal(&package);
    let all = entries(&signed);
    let paths: Vec<String> = file_refs(&text(&all, "Doc_0/Signs/Sign_0/Signature.xml"))
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(
        paths,
        vec![
            "/Doc_0/Document.xml",
            "/Doc_0/Pages/Page_0/Content.xml",
            "/OFD.xml",
            "/Doc_0/Signs/Sign_0/Seal.esl",
        ]
    );
    assert!(
        all.iter().any(|(name, _)| name == "Doc_0/Pages/"),
        "the directory entries are carried along"
    );
    assert!(all_valid(&signed));
}

// ---- found by the final review of the signing line ----

#[test]
fn signing_an_old_layout_package_keeps_its_annotations_reachable() {
    // No annotation entry: the page annotation file sits in the page
    // directory, a layout the parser reads through its fallback. The
    // scaffold adds the entry file, and that file has to name the page
    // file: an empty list would hide the annotation from every reader that
    // follows the entry file, rofd included.
    let mut files = entries(&bare_package());
    files.push((
        "Doc_0/Pages/Page_0/Annotation.xml".to_owned(),
        PAGE_ANNOT.as_bytes().to_vec(),
    ));
    let unsigned = zip_of(&files);
    let before = parse_ofd(&unsigned)
        .expect("parse_ofd")
        .document
        .package
        .expect("package");
    assert_eq!(
        before
            .annotation_files
            .iter()
            .map(|(_, path)| path.as_str())
            .collect::<Vec<_>>(),
        vec!["Doc_0/Pages/Page_0/Annotation.xml"],
        "the old layout is read"
    );

    let signed = sign_seal(&unsigned);
    assert!(all_valid(&signed));
    let all = entries(&signed);
    assert!(
        text(&all, "Doc_0/Annots/Annotations.xml")
            .contains("<ofd:Page PageID=\"1\"><ofd:FileLoc>../Pages/Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>"),
        "the entry file names the page file relative to its own directory"
    );
    assert_eq!(
        option(&all, "Doc_0/Pages/Page_0/Annotation.xml"),
        Some(PAGE_ANNOT.as_bytes()),
        "signing does not touch the page file"
    );
    let after = parse_ofd(&signed)
        .expect("parse_ofd")
        .document
        .package
        .expect("package");
    assert_eq!(after.annotation_files, before.annotation_files);
    // The annotation layer, the entry file included, stays unprotected.
    let paths: Vec<String> = file_refs(&text(&all, "Doc_0/Signs/Sign_0/Signature.xml"))
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert!(
        !paths.iter().any(|path| path.ends_with("Annotation.xml")),
        "{paths:?}"
    );

    // The editor's save afterwards keeps it reachable as well.
    let report = parse_ofd(&signed).expect("parse_ofd");
    let mut doc = report.document;
    add_rect(&mut doc, "1");
    let preserved = save_ofd(&doc, &hints()).expect("save");
    assert!(!preserved.blocked_by_signature);
    let reopened = parse_ofd(&preserved.bytes).expect("the saved package re-parses");
    assert_eq!(reopened.document.annotations, doc.annotations);
    assert!(all_valid(&preserved.bytes));
}
