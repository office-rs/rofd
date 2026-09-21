#[path = "fixtures/fixtures.rs"]
mod fixtures;

#[test]
fn surgical_save_preserves_body_byte_identical() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();

    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();

    // Body Content.xml + OFD.xml + resources must be byte-identical
    // (surgical save invariant §4.3). Document.xml is NOT in this set -
    // its <MaxUnitID> is byte-patched to cover the IDs minted while
    // re-serializing the annotation files.
    for name in [
        "OFD.xml",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Res/Font.xml",
    ] {
        assert_eq!(
            by_name(&orig_entries, name),
            by_name(&saved_entries, name),
            "{name} changed"
        );
    }
    // Document.xml: the ONLY textual difference is the <MaxUnitID> value.
    // Strip that element and the rest must be byte-identical.
    let orig_doc = std::str::from_utf8(by_name(&orig_entries, "Doc_0/Document.xml")).unwrap();
    let saved_doc = std::str::from_utf8(by_name(&saved_entries, "Doc_0/Document.xml")).unwrap();
    assert_eq!(strip_max_unit_id(orig_doc), strip_max_unit_id(saved_doc));
    let new_max: u64 = saved_doc
        .split("MaxUnitID>")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        new_max > report.document.max_unit_id,
        "MaxUnitID must cover minted appearance object IDs: {new_max} vs {}",
        report.document.max_unit_id
    );
}

/// Remove the `<...MaxUnitID>N</...MaxUnitID>` element so two Document.xml
/// strings can be compared with the patched value factored out.
fn strip_max_unit_id(xml: &str) -> String {
    const KEY: &str = "MaxUnitID>";
    let Some(start) = xml.find(KEY) else {
        return xml.to_string();
    };
    let open_start = xml[..start].rfind('<').unwrap_or(0);
    let text_start = start + KEY.len();
    let Some(end_rel) = xml[text_start..].find('<') else {
        return xml.to_string();
    };
    // End of the closing tag.
    let close_end = xml[text_start + end_rel..]
        .find('>')
        .map(|i| text_start + end_rel + i + 1)
        .unwrap_or(xml.len());
    format!("{}{}", &xml[..open_start], &xml[close_end..])
}

/// Strict-reader interop: adding the FIRST annotation to a previously-bare
/// document must wire the full GB/T 33190 discovery chain - readers locate
/// annotations via `Document.xml` `<Annotations>` loc -> `Annots/Annotations.xml`
/// entry -> per-page `FileLoc`. `ensure_annotation_entries` adds the files; the
/// `<Annotations>` loc must be inserted into the byte-copied Document.xml too,
/// or a strict reader (discovery-only, no directory scan) shows no annotations
/// at all while rofd's fallback scan still finds them.
#[test]
fn surgical_save_inserts_annotations_ref_for_previously_bare_document() {
    use rofd_dom::*;
    let original = fixtures::build_bare_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    assert!(
        report.document.annotations.by_page.is_empty(),
        "fixture starts bare"
    );
    let mut doc = report.document.clone();
    doc.annotations
        .by_page
        .entry(PageId::new("1"))
        .or_default()
        .push(Annotation {
            id: AnnotationId::from_int(102),
            kind: AnnotationKind::Note,
            page: PageId::new("1"),
            creator: "t".into(),
            created: 1_783_656_237_000,
            modified: 1_783_656_237_000,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 5.0,
                    h: 5.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "first".into(),
                icon: NoteIcon::Note,
            },
        });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();

    // Document.xml gains the <Annotations> loc.
    let doc_xml = std::str::from_utf8(by_name(&saved_entries, "Doc_0/Document.xml")).unwrap();
    assert!(
        doc_xml.contains("<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>"),
        "Document.xml must reference the annotation entry file: {doc_xml}"
    );
    // Body Content.xml stays byte-identical (invariant §4.3).
    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    assert_eq!(
        by_name(&orig_entries, "Doc_0/Pages/Page_0/Content.xml"),
        by_name(&saved_entries, "Doc_0/Pages/Page_0/Content.xml"),
        "body Content.xml byte-identical"
    );
    // The saved package is discoverable through the STANDARD chain: re-parse
    // finds the annotation and does not fall back to the /Annotation.xml scan
    // (which would emit a MissingFeature warning).
    let re = rofd_io::parse_ofd(&saved).unwrap();
    let count: usize = re
        .document
        .annotations
        .by_page
        .values()
        .map(|v| v.len())
        .sum();
    assert_eq!(count, 1, "annotation survives reload");
    assert!(
        !re.warnings.iter().any(|w| matches!(
            w,
            rofd_io::OfdWarning::MissingFeature { feature, .. } if feature.contains("annotation entry")
        )),
        "standard discovery path used, no fallback warning: {:?}",
        re.warnings
    );
}

/// The inverse guard: a bare document saved WITHOUT annotations must stay
/// byte-identical - the `<Annotations>` loc is only inserted when the model
/// actually has annotations to point at.
#[test]
fn surgical_save_bare_document_without_annotations_stays_identical() {
    let original = fixtures::build_bare_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    for name in [
        "OFD.xml",
        "Doc_0/Document.xml",
        "Doc_0/Pages/Page_0/Content.xml",
        "Doc_0/Res/Font.xml",
    ] {
        assert_eq!(
            by_name(&orig_entries, name),
            by_name(&saved_entries, name),
            "{name} changed"
        );
    }
}

/// Real-file regression: sample-content.ofd ships without annotations;
/// after annotating + surgical save, a strict reader must be able to
/// discover them.
#[test]
#[ignore = "requires the real OFD at ../../test/sample-content.ofd"]
fn surgical_save_real_bare_file_gains_annotations_ref() {
    use rofd_dom::*;
    let raw = std::fs::read("../../test/sample-content.ofd").expect("test sample present");
    let report = rofd_io::parse_ofd(&raw).unwrap();
    let mut doc = report.document.clone();
    let pid = doc.pages[0].id.clone();
    doc.annotations
        .by_page
        .entry(pid.clone())
        .or_default()
        .push(Annotation {
            id: AnnotationId::from_int(doc.max_unit_id + 1),
            kind: AnnotationKind::Highlight,
            page: pid,
            creator: "t".into(),
            created: 1_783_656_237_000,
            modified: 1_783_656_237_000,
            reply_to: None,
            payload: AnnotationPayload::Markup {
                quad_points: vec![
                    rofd_dom::Point { x: 10.0, y: 10.0 },
                    rofd_dom::Point { x: 60.0, y: 20.0 },
                ],
                color: Color::Rgb(255, 221, 0),
            },
        });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let doc_xml = std::str::from_utf8(by_name(&saved_entries, "Doc_0/Document.xml")).unwrap();
    assert!(
        doc_xml.contains("<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>"),
        "real-file Document.xml gains the ref: {doc_xml}"
    );
}

fn by_name<'a>(entries: &'a [(String, Vec<u8>)], name: &'a str) -> &'a [u8] {
    entries
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| b.as_slice())
        .unwrap()
}

#[test]
fn surgical_save_rewrites_annotation_entry() {
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let saved = rofd_io::save_ofd(&report.document, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let ann = saved_entries
        .iter()
        .find(|(n, _)| n == "Doc_0/Annots/Page_0/Annotation.xml")
        .map(|(_, b)| b.as_slice())
        .unwrap();
    // Re-serialized in GB/T 33190 §15.2 <PageAnnot><Annot> format.
    assert!(std::str::from_utf8(ann).unwrap().contains("<ofd:PageAnnot"));
    assert!(std::str::from_utf8(ann).unwrap().contains("<ofd:Annot"));
}

/// Task 9: surgical save expands the dirty set -
/// (a) entry file `Annotations.xml` is re-serialized,
/// (b) per-page `Page_N/Annotation.xml` is re-serialized,
/// (c) `Document.xml` `<MaxUnitID>` is byte-patched past the annotation ID
///     to also cover the appearance object IDs minted during (b),
/// (d) body `Content.xml` entries are byte-identical (invariant §4.3).
#[test]
fn surgical_save_rewrites_annotation_entry_and_per_page_and_max_unit_id() {
    use rofd_dom::*;
    let original = fixtures::build_minimal_ofd();
    let report = rofd_io::parse_ofd(&original).unwrap();
    let mut doc = report.document.clone();
    // Add a new annotation (triggers max_unit_id increment).
    let new_id = doc.max_unit_id + 1;
    doc.max_unit_id = new_id;
    doc.annotations
        .by_page
        .entry(PageId::new("1"))
        .or_default()
        .push(Annotation {
            id: AnnotationId::from_int(new_id),
            kind: AnnotationKind::Note,
            page: PageId::new("1"),
            creator: "t".into(),
            created: 1_783_656_237_000,
            modified: 1_783_656_237_000,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 5.0,
                    h: 5.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: "new".into(),
                icon: NoteIcon::Note,
            },
        });
    let saved = rofd_io::save_ofd(&doc, &report.package).unwrap();
    let saved_entries = rofd_io::zip_util::read_all_entries(&saved).unwrap();
    let orig_entries = rofd_io::zip_util::read_all_entries(&original).unwrap();
    // (d) body Content.xml byte-identical (invariant §4.3).
    for name in orig_entries
        .iter()
        .filter(|(n, _)| n.ends_with("Content.xml"))
        .map(|(n, _)| n.as_str())
    {
        let o = orig_entries.iter().find(|(n, _)| n == name).unwrap();
        let s = saved_entries.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(o.1, s.1, "body {name} byte-identical");
    }
    // (c) Document.xml MaxUnitID covers every object ID in the annotation
    // files - both annotations' own IDs and the appearance object IDs
    // minted while re-serializing them (2 annots -> 2 minted PathObjects).
    let doc_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Document.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    let new_max: u64 = doc_xml
        .split("MaxUnitID>")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        new_max > new_id,
        "MaxUnitID {new_max} must cover minted object IDs past {new_id}"
    );
    let ann_xml_probe = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Annots/Page_0/Annotation.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    for id in ann_xml_probe
        .split("ID=\"")
        .skip(1)
        .map(|r| r.split('"').next().unwrap())
    {
        let n: u64 = id
            .parse()
            .unwrap_or_else(|_| panic!("non-integer ID {id:?}"));
        assert!(n <= new_max, "object ID {n} exceeds MaxUnitID {new_max}");
    }
    // (b) per-page annotation file contains the new annotation.
    let ann_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Annots/Page_0/Annotation.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(
        ann_xml.contains("Note") && ann_xml.contains("new"),
        "new annot in per-page file: {ann_xml}"
    );
    // (a) entry file `Annotations.xml` re-serialized (still references Page_0).
    let entry_xml = std::str::from_utf8(
        &saved_entries
            .iter()
            .find(|(n, _)| n.ends_with("Annots/Annotations.xml"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(
        entry_xml.contains("Page_0/Annotation.xml"),
        "entry file references per-page file: {entry_xml}"
    );
}
