//! Signature-structure projection (spec 2026-11-23 §8.2): io lifts the seal
//! placements and seal pictures out of the `Signs/` entries into
//! `OfdDocument::signatures`, read-only, and never touches those entries.

use std::io::Write;

use rofd_dom::{PageId, Rect, SignatureKind, SignatureSet, TextLayoutHints};
use rofd_io::{parse_ofd, zip_util};

const SIGNATURE_FILES: [&str; 4] = [
    "Doc_0/Signs/Signatures.xml",
    "Doc_0/Signs/Sign_0/Signature.xml",
    "Doc_0/Signs/Sign_0/SignedValue.dat",
    "Doc_0/Signs/Sign_0/Seal.esl",
];

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/sign/{name}"))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let borrowed: Vec<(&str, &[u8])> = files.iter().map(|(n, c)| (*n, c.as_bytes())).collect();
    zip_of_bytes(&borrowed)
}

/// Build a zip from `(entry name, bytes)` pairs, in order.
fn zip_of_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        w.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        w.write_all(content).expect("write entry");
    }
    w.finish().expect("finish zip").into_inner()
}

fn entry_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    zip_util::read_all_entries(bytes)
        .expect("read entries")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("entry {name} present"))
        .1
}

/// Rebuild `bytes` with some entries replaced (or added) and some removed.
fn rebuilt(bytes: &[u8], replace: &[(&str, &[u8])], remove: &[&str]) -> Vec<u8> {
    let kept: Vec<(String, Vec<u8>)> = zip_util::read_all_entries(bytes)
        .expect("read entries")
        .into_iter()
        .filter(|(name, _)| {
            !remove.contains(&name.as_str()) && !replace.iter().any(|(n, _)| *n == name)
        })
        .collect();
    let mut entries = kept;
    for (name, data) in replace {
        entries.push(((*name).to_owned(), data.to_vec()));
    }
    zip_util::write_zip(&entries).expect("write zip")
}

const PAGE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>\
<ofd:Content/></ofd:Page>";

#[test]
fn sesv4_sample_projects_one_seal_with_one_stamp() {
    let report = parse_ofd(&fixture("SESV4SignDoc.ofd")).expect("parse_ofd");
    let signatures = &report.document.signatures;
    assert!(signatures.declared);
    assert_eq!(signatures.items.len(), 1);
    let signature = &signatures.items[0];
    assert_eq!(signature.id, "1");
    assert_eq!(signature.kind, SignatureKind::Seal);
    assert_eq!(
        signature.base_loc, "/Doc_0/Signs/Sign_0/Signature.xml",
        "BaseLoc is kept as written"
    );
    assert_eq!(signature.stamps.len(), 1);
    assert_eq!(signature.stamps[0].page_id, PageId::new("1"));
    assert_eq!(
        signature.stamps[0].boundary,
        Rect {
            x: 50.0,
            y: 50.0,
            w: 40.0,
            h: 40.0
        }
    );
    assert_eq!(signature.stamps[0].clip, None);
    let picture = signature.seal_picture.as_ref().expect("seal picture");
    assert!(
        picture.png.starts_with(&[0x89, b'P', b'N', b'G']),
        "PNG magic bytes"
    );
    assert_eq!(picture.width, 40.0);
    assert_eq!(picture.height, 40.0);
    assert_eq!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .signatures_entry
            .as_deref(),
        Some("Doc_0/Signs/Signatures.xml")
    );
    assert!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .signatures_declared
    );
}

#[test]
fn sesv1_sample_projects_its_seal_and_keeps_non_numeric_ids() {
    let report = parse_ofd(&fixture("SESV1SignDoc.ofd")).expect("parse_ofd");
    let signatures = &report.document.signatures;
    assert_eq!(signatures.items.len(), 1);
    let signature = &signatures.items[0];
    assert_eq!(signature.id, "s001");
    assert_eq!(signature.stamps.len(), 1);
    assert!(signature.seal_picture.is_some(), "SES v1 seal picture");
}

#[test]
fn unsigned_package_has_default_signature_set() {
    let bytes = zip_of(&[
        (
            "OFD.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>",
        ),
        (
            "Doc_0/Document.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:CommonData><ofd:MaxUnitID>3</ofd:MaxUnitID></ofd:CommonData><ofd:Pages><ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>",
        ),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE),
    ]);
    let report = parse_ofd(&bytes).expect("parse_ofd");
    assert_eq!(report.document.signatures, SignatureSet::default());
    assert_eq!(
        report
            .document
            .package
            .as_ref()
            .expect("package")
            .signatures_entry,
        None
    );
}

#[test]
fn corrupt_seal_still_opens_with_warning() {
    // Both carriers of the picture are garbage: no picture can be shown,
    // but the document opens and the placements survive.
    let bytes = rebuilt(
        &fixture("SESV4SignDoc.ofd"),
        &[
            ("Doc_0/Signs/Sign_0/Seal.esl", b"not der at all"),
            ("Doc_0/Signs/Sign_0/SignedValue.dat", b"not der either"),
        ],
        &[],
    );
    let report = parse_ofd(&bytes).expect("the document still opens");
    assert_eq!(report.document.signatures.items.len(), 1);
    assert_eq!(report.document.signatures.items[0].seal_picture, None);
    assert_eq!(
        report.document.signatures.items[0].stamps.len(),
        1,
        "placements survive"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.to_string().contains("Seal.esl")),
        "a warning names the seal entry: {:?}",
        report.warnings
    );
}

#[test]
fn signature_entries_are_never_consumed() {
    let bytes = fixture("SESV4SignDoc.ofd");
    let report = parse_ofd(&bytes).expect("parse_ofd");
    for name in SIGNATURE_FILES {
        assert!(
            !report
                .document
                .package
                .as_ref()
                .expect("package")
                .consumed
                .contains(name),
            "{name} is not written from the model"
        );
    }
    let preserved = rofd_io::save_ofd(&report.document, &TextLayoutHints::new()).expect("save_ofd");
    for name in SIGNATURE_FILES {
        assert_eq!(
            entry_bytes(&preserved.bytes, name),
            entry_bytes(&bytes, name),
            "{name} stays byte-identical across a save"
        );
    }
}

const VENDOR_MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:MaxSignId>s1</ofd:MaxSignId>\
<ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"Sign_0/Signature.xml\" />\
</ofd:Signatures>";

const VENDOR_SIGNATURE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\
<ofd:Signature xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:SignedValue>SignedValue.dat</ofd:SignedValue>\
<ofd:SignedInfo>\
<ofd:Provider ProviderName=\"BJCA-OES\"/>\
<ofd:SignatureDateTime>20260116004922Z</ofd:SignatureDateTime>\
<ofd:Seal><ofd:BaseLoc>Seal.esl</ofd:BaseLoc></ofd:Seal>\
<ofd:StampAnnot PageRef=\"1\" ID=\"1\" Boundary=\"136 277.5 68 15\" />\
<ofd:StampAnnot PageRef=\"790\" ID=\"1\" Boundary=\"136 277.5 68 15\" />\
<ofd:StampAnnot PageRef=\"1775\" ID=\"1\" Boundary=\"136 277.5 68 15\" />\
<ofd:References CheckMethod=\"SM3\"/>\
</ofd:SignedInfo></ofd:Signature>";

#[test]
fn vendor_style_signature_parses() {
    // Vendor quirks: a relative BaseLoc, a non-numeric MaxSignId, three
    // stamps sharing one ID, and a relative seal loc.
    let document = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:CommonData><ofd:MaxUnitID>20</ofd:MaxUnitID>\
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>\
</ofd:CommonData>\
<ofd:Pages>\
<ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/>\
<ofd:Page ID=\"790\" BaseLoc=\"Pages/Page_1/Content.xml\"/>\
<ofd:Page ID=\"1775\" BaseLoc=\"Pages/Page_2/Content.xml\"/>\
</ofd:Pages></ofd:Document>";
    let ofd = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody>\
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\
<ofd:Signatures>/Doc_0/Signs/Signatures.xml</ofd:Signatures>\
</ofd:DocBody></ofd:OFD>";
    let seal = entry_bytes(&fixture("SESV4SignDoc.ofd"), "Doc_0/Signs/Sign_0/Seal.esl");
    let bytes = zip_of_bytes(&[
        ("OFD.xml", ofd.as_bytes()),
        ("Doc_0/Document.xml", document.as_bytes()),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE.as_bytes()),
        ("Doc_0/Pages/Page_1/Content.xml", PAGE.as_bytes()),
        ("Doc_0/Pages/Page_2/Content.xml", PAGE.as_bytes()),
        ("Doc_0/Signs/Signatures.xml", VENDOR_MANIFEST.as_bytes()),
        (
            "Doc_0/Signs/Sign_0/Signature.xml",
            VENDOR_SIGNATURE.as_bytes(),
        ),
        ("Doc_0/Signs/Sign_0/Seal.esl", &seal),
    ]);

    let report = parse_ofd(&bytes).expect("parse_ofd");
    let signatures = &report.document.signatures;
    assert_eq!(signatures.items.len(), 1);
    let signature = &signatures.items[0];
    assert_eq!(
        signature.base_loc, "Sign_0/Signature.xml",
        "kept as written"
    );
    let pages: Vec<&str> = signature
        .stamps
        .iter()
        .map(|s| s.page_id.0.as_str())
        .collect();
    assert_eq!(pages, vec!["1", "790", "1775"]);
    for stamp in &signature.stamps {
        assert_eq!(
            stamp.boundary,
            Rect {
                x: 136.0,
                y: 277.5,
                w: 68.0,
                h: 15.0
            }
        );
    }
    let page = PageId::new("1");
    let mut placed = signatures.stamps_on(&page);
    let first = placed.next().expect("page 1 carries the picture");
    assert_eq!(placed.count(), 0);
    assert_eq!(signatures.stamps_on(&PageId::new("790")).count(), 1);
    assert_eq!(signatures.stamps_on(&PageId::new("1775")).count(), 1);
    assert_eq!(first.1.width, 40.0);
}

#[test]
fn picture_falls_back_to_the_embedded_seal_without_seal_esl() {
    let bytes = rebuilt(
        &fixture("SESV4SignDoc.ofd"),
        &[],
        &["Doc_0/Signs/Sign_0/Seal.esl"],
    );
    let report = parse_ofd(&bytes).expect("parse_ofd");
    let picture = report.document.signatures.items[0]
        .seal_picture
        .as_ref()
        .expect("the picture comes from the signature value");
    assert!(picture.png.starts_with(&[0x89, b'P', b'N', b'G']));
    assert_eq!((picture.width, picture.height), (40.0, 40.0));
}

#[test]
fn non_png_seal_picture_is_skipped_with_a_warning() {
    // Patch the picture's type tag from PNG to JPG inside Seal.esl.
    let mut seal = entry_bytes(&fixture("SESV4SignDoc.ofd"), "Doc_0/Signs/Sign_0/Seal.esl");
    let at = seal
        .windows(5)
        .position(|w| w == [0x16, 0x03, b'P', b'N', b'G'])
        .expect("IA5String PNG in the seal");
    seal[at + 2..at + 5].copy_from_slice(b"JPG");
    let bytes = rebuilt(
        &fixture("SESV4SignDoc.ofd"),
        &[
            ("Doc_0/Signs/Sign_0/Seal.esl", &seal),
            ("Doc_0/Signs/Sign_0/SignedValue.dat", b"no embedded copy"),
        ],
        &[],
    );
    let report = parse_ofd(&bytes).expect("parse_ofd");
    assert_eq!(report.document.signatures.items[0].seal_picture, None);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.to_string().contains("PNG")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn missing_signature_description_is_reported_not_fatal() {
    let bytes = rebuilt(
        &fixture("SESV4SignDoc.ofd"),
        &[],
        &["Doc_0/Signs/Sign_0/Signature.xml"],
    );
    let report = parse_ofd(&bytes).expect("parse_ofd");
    assert_eq!(report.document.signatures.items.len(), 1);
    assert!(report.document.signatures.items[0].stamps.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.to_string().contains("Signature.xml")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn truncated_seal_input_never_panics() {
    let seal = entry_bytes(&fixture("SESV4SignDoc.ofd"), "Doc_0/Signs/Sign_0/Seal.esl");
    let value = entry_bytes(
        &fixture("SESV4SignDoc.ofd"),
        "Doc_0/Signs/Sign_0/SignedValue.dat",
    );
    for end in 0..seal.len() {
        let _ = rofd_io::seal_picture_from_der(&seal[..end]);
    }
    for end in 0..value.len() {
        let _ = rofd_io::seal_picture_from_der(&value[..end]);
    }
    assert!(rofd_io::seal_picture_from_der(&seal).is_some());
    assert!(
        rofd_io::seal_picture_from_der(&value).is_none(),
        "the signature value is not an ESS_Seal itself"
    );
}

#[test]
fn digital_signature_sample_has_no_seal_and_no_warning() {
    let report = parse_ofd(&fixture("DigitalSign.ofd")).expect("parse_ofd");
    assert_eq!(report.document.signatures.items.len(), 1);
    assert_eq!(
        report.document.signatures.items[0].kind,
        SignatureKind::Sign
    );
    assert_eq!(report.document.signatures.items[0].seal_picture, None);
    assert!(
        report.warnings.is_empty(),
        "nothing is wrong with this file: {:?}",
        report.warnings
    );
}
