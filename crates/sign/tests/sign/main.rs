//! Creating signatures (spec 2026-11-23 §10): a seal or a digital signature
//! appended to a package, with references that leave the annotation layer
//! free, and what becomes of a signed document that is saved afterwards.

use base64::Engine as _;
use rofd_dom::{
    Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, OfdDocument, PageId, Rect,
    ShapeKind, SignatureKind, TextLayoutHints,
};
use rofd_io::{max_unit_id_of, parse_ofd, save_ofd, with_annotations_loc, DocumentXmlChange};
use rofd_sign::{
    prepare_sign, sign, verify, Pages, PfxSigner, Placement, Provider, SealImage, SignError,
    SignOptions, SignStep, SignatureOverall, SignatureReport, SignatureValueStatus,
    SignatureVariant, Signer, TestCa, VerifyOptions, SIGNATURE_ID_RESERVE,
};

const NOW: &str = "2026-11-24 10:30:00";
const BOTTOM_RIGHT: Rect = Rect {
    x: 160.0,
    y: 247.0,
    w: 40.0,
    h: 40.0,
};

// ---- package builders ----

fn zip_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    rofd_io::zip_util::write_zip(files).expect("zip")
}

/// Every `(name, bytes)` entry of a package, in archive order.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    rofd_io::zip_util::read_all_entries(bytes).expect("entries")
}

fn option<'a>(all: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    all.iter()
        .find(|(entry, _)| entry == name)
        .map(|(_, bytes)| bytes.as_slice())
}

fn entry<'a>(all: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    option(all, name).unwrap_or_else(|| panic!("entry {name} is in the package"))
}

fn text(all: &[(String, Vec<u8>)], name: &str) -> String {
    String::from_utf8_lossy(entry(all, name)).into_owned()
}

fn ofd_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody>\
<ofd:DocInfo><ofd:Title>t</ofd:Title></ofd:DocInfo>\
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>\
</ofd:DocBody></ofd:OFD>"
        .to_owned()
}

/// Two pages by default. `max_unit_id` of `None` leaves the element out.
fn document_xml(boxes: &[&str], max_unit_id: Option<u64>, annotations: bool) -> String {
    let max_unit_id = max_unit_id
        .map(|value| format!("<ofd:MaxUnitID>{value}</ofd:MaxUnitID>"))
        .unwrap_or_default();
    let pages: String = boxes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            format!(
                "<ofd:Page ID=\"{}\" BaseLoc=\"Pages/Page_{index}/Content.xml\"/>",
                index + 1
            )
        })
        .collect();
    let annotations = if annotations {
        "<ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:CommonData>{max_unit_id}<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData>\
<ofd:Pages>{pages}</ofd:Pages>{annotations}\
<ofd:Extensions><ofd:Extension>App</ofd:Extension></ofd:Extensions>\
</ofd:Document>"
    )
}

/// A page with the physical box `x y w h`.
fn page(physical_box: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Area><ofd:PhysicalBox>{physical_box}</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>"
    )
}

/// An annotation file another writer produced: a comment and no line breaks
/// where rofd would put them.
const PAGE_ANNOT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:PageAnnot xmlns:ofd=\"http://www.ofdspec.org/2016\"><!-- kept as is --></ofd:PageAnnot>";

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Page PageID=\"1\"><ofd:FileLoc>P_7.xml</ofd:FileLoc></ofd:Page>\
</ofd:Annotations>";

const EMPTY_MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Annotations xmlns:ofd=\"http://www.ofdspec.org/2016\"></ofd:Annotations>";

/// An unsigned package with two A4 pages. `annotations` adds an annotation
/// entry declared under a non-canonical name, with one page file.
fn package(boxes: &[&str], annotations: bool) -> Vec<u8> {
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("OFD.xml".to_owned(), ofd_xml().into_bytes()),
        (
            "Doc_0/Document.xml".to_owned(),
            document_xml(boxes, Some(9), annotations).into_bytes(),
        ),
        ("Doc_0/Attachs/note.txt".to_owned(), b"kept".to_vec()),
    ];
    for (index, physical_box) in boxes.iter().enumerate() {
        files.push((
            format!("Doc_0/Pages/Page_{index}/Content.xml"),
            page(physical_box).into_bytes(),
        ));
    }
    if annotations {
        files.push((
            "Doc_0/Annots/Annotations.xml".to_owned(),
            MANIFEST.as_bytes().to_vec(),
        ));
        files.push((
            "Doc_0/Annots/P_7.xml".to_owned(),
            PAGE_ANNOT.as_bytes().to_vec(),
        ));
    }
    zip_of(&files)
}

fn two_pages() -> [&'static str; 2] {
    ["0 0 210 297", "0 0 210 297"]
}

fn annotated_package() -> Vec<u8> {
    package(&two_pages(), true)
}

/// The same package without any annotation layer.
fn bare_package() -> Vec<u8> {
    package(&two_pages(), false)
}

// ---- seals, signers, reports ----

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([190, 30, 30, 200]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    bytes.into_inner()
}

fn square_seal() -> SealImage {
    SealImage {
        png: png(40, 40),
        size_mm: 0.0,
    }
}

fn seal_options() -> SignOptions {
    SignOptions {
        seal: Some(square_seal()),
        ..SignOptions::new(NOW)
    }
}

fn options(placement: Placement) -> SignOptions {
    SignOptions {
        seal: Some(square_seal()),
        placement: Some(placement),
        ..SignOptions::new(NOW)
    }
}

fn seal_of(png: Vec<u8>, size_mm: f64) -> SignOptions {
    SignOptions {
        seal: Some(SealImage { png, size_mm }),
        ..SignOptions::new(NOW)
    }
}

fn ca(common_name: &str, seed: &[u8]) -> TestCa {
    TestCa::from_seed(common_name, seed)
}

fn first_ca() -> TestCa {
    ca("rofd test signer", b"seed-a")
}

fn sign_seal(bytes: &[u8]) -> Vec<u8> {
    sign(bytes, &mut first_ca().signer(), &seal_options()).expect("sign")
}

fn reports_of(bytes: &[u8]) -> Vec<SignatureReport> {
    verify(bytes, &VerifyOptions::default())
}

fn all_valid(bytes: &[u8]) -> bool {
    let reports = reports_of(bytes);
    !reports.is_empty()
        && reports
            .iter()
            .all(|report| report.overall == SignatureOverall::Valid)
}

/// `(page, boundary)` of every stamp of the first signature.
fn stamps(bytes: &[u8]) -> Vec<(PageId, Rect)> {
    let document = parse_ofd(bytes).expect("parse_ofd").document;
    let signature = &document.signatures.items[0];
    signature
        .stamps
        .iter()
        .map(|stamp| (stamp.page_id.clone(), stamp.boundary))
        .collect()
}

fn hints() -> TextLayoutHints {
    TextLayoutHints::new()
}

/// Add a rectangle annotation the way the editor does on a signed document:
/// the next ID above the highest one in use.
fn add_rect(doc: &mut OfdDocument, page: &str) {
    let next = if doc.signatures.declared {
        doc.highest_known_object_id() + 1
    } else {
        doc.max_unit_id + 1
    };
    doc.annotations.insert(Annotation {
        id: AnnotationId::from_int(next),
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

/// Every `ID="n"` of an XML text, in order.
fn numeric_ids(xml: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("ID=\"") {
        rest = &rest[at + 4..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(id) = digits.parse() {
            ids.push(id);
        }
    }
    ids
}

/// `(FileRef, CheckValue)` of a Signature.xml, in order.
fn file_refs(signature_xml: &str) -> Vec<(String, String)> {
    const OPEN: &str = "<ofd:Reference FileRef=\"";
    const CHECK_OPEN: &str = "\"><ofd:CheckValue>";
    const CHECK_CLOSE: &str = "</ofd:CheckValue>";
    signature_xml
        .split(OPEN)
        .skip(1)
        .map(|tail| {
            let end = tail.find(CHECK_OPEN).expect("FileRef");
            let file_ref = tail[..end].to_owned();
            let tail = &tail[end + CHECK_OPEN.len()..];
            let end = tail.find(CHECK_CLOSE).expect("CheckValue");
            (file_ref, tail[..end].to_owned())
        })
        .collect()
}

/// `bytes` with one entry replaced, or added at the end.
fn with_entry(bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    let mut files = entries(bytes);
    match files.iter_mut().find(|(entry, _)| entry == name) {
        Some((_, data)) => *data = content.to_vec(),
        None => files.push((name.to_owned(), content.to_vec())),
    }
    zip_of(&files)
}

/// The signature declaration `loc`, as OFD.xml's DocBody carries it.
fn declaring(loc: &str) -> String {
    ofd_xml().replace(
        "</ofd:DocBody>",
        &format!("<ofd:Signatures>{loc}</ofd:Signatures></ofd:DocBody>"),
    )
}

fn is_invalid<T: std::fmt::Debug>(result: Result<T, SignError>) -> bool {
    matches!(result, Err(SignError::InvalidInput { .. }))
}

// ---- S1..S7, S9 ----

mod appearance;
mod flow;
mod in_place;
mod package;

#[test]
fn signed_seal_document_verifies_valid() {
    let signed = sign_seal(&annotated_package());
    let reports = reports_of(&signed);
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.overall, SignatureOverall::Valid);
    assert_eq!(report.kind, SignatureKind::Seal);
    assert_eq!(report.id, "1");
    assert_eq!(report.variant, SignatureVariant::SesV4);
    assert!(report.integrity.ok);
    assert_eq!(report.seal_match, Some(true));
    assert_eq!(report.signature_value.status, SignatureValueStatus::Ok);
    let certificate = report
        .certificate
        .as_ref()
        .expect("the certificate travels with the signature");
    assert_eq!(certificate.subject, "rofd test signer");
    assert_eq!(report.provider.as_deref(), Some("rofd"));
    assert_eq!(report.date_time.as_deref(), Some(NOW));

    // The editor side sees the seal and where it was put.
    let document = parse_ofd(&signed)
        .expect("the signed package parses")
        .document;
    assert!(document.signatures.declared);
    assert_eq!(document.signatures.items.len(), 1);
    let signature = &document.signatures.items[0];
    assert_eq!(signature.id, "1");
    assert_eq!(
        signature.base_loc, "/Doc_0/Signs/Sign_0/Signature.xml",
        "the loc is written package-absolute"
    );
    assert_eq!(signature.stamps.len(), 1);
    assert_eq!(signature.stamps[0].page_id, PageId::new("2"));
    let picture = signature.seal_picture.as_ref().expect("the seal picture");
    assert_eq!((picture.width, picture.height), (40.0, 40.0));
    assert_eq!(picture.png.len(), png(40, 40).len());
}

#[test]
fn reopen_and_save_keeps_bytes_and_validity() {
    // Without annotation files, the empty list written at signing time is
    // already what a save would write: nothing changes.
    let signed = sign_seal(&bare_package());
    let report = parse_ofd(&signed).expect("parse_ofd");
    let preserved = save_ofd(&report.document, &hints()).expect("save");
    assert!(!preserved.blocked_by_signature);
    assert!(!preserved.full_write_fallback);
    assert!(!preserved.reserve_exhausted);
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    assert_eq!(
        entries(&preserved.bytes),
        entries(&signed),
        "every entry is byte-identical"
    );
    assert!(all_valid(&preserved.bytes));

    // With annotation files, a save rewrites them from the model. Everything
    // else stays as signed.
    let signed = sign_seal(&annotated_package());
    let report = parse_ofd(&signed).expect("parse_ofd");
    let preserved = save_ofd(&report.document, &hints()).expect("save");
    assert!(!preserved.blocked_by_signature);
    let (before, after) = (entries(&signed), entries(&preserved.bytes));
    assert_eq!(before.len(), after.len());
    for (name, bytes) in &before {
        if !name.starts_with("Doc_0/Annots/") {
            assert_eq!(option(&after, name), Some(bytes.as_slice()), "{name}");
        }
    }
    assert!(all_valid(&preserved.bytes));
}

#[test]
fn annotation_after_signing_keeps_signature_valid() {
    let signed = sign_seal(&bare_package());
    let report = parse_ofd(&signed).expect("parse_ofd");
    let mut document = report.document.clone();
    add_rect(&mut document, "1");
    let preserved = save_ofd(&document, &hints()).expect("save");
    assert!(!preserved.blocked_by_signature);
    assert_eq!(preserved.document_xml, DocumentXmlChange::Untouched);
    let (before, after) = (entries(&signed), entries(&preserved.bytes));
    assert_eq!(
        entry(&after, "Doc_0/Document.xml"),
        entry(&before, "Doc_0/Document.xml"),
        "Document.xml is byte-identical"
    );
    assert!(option(&after, "Doc_0/Annots/Page_0/Annotation.xml").is_some());
    let reopened = parse_ofd(&preserved.bytes).expect("parse_ofd").document;
    assert_eq!(reopened.annotations.for_page(&PageId::new("1")).len(), 1);
    assert_eq!(stamps(&preserved.bytes).len(), 1, "the seal is still there");
    assert!(all_valid(&preserved.bytes));
}

#[test]
fn second_signature_leaves_first_untouched_and_both_valid() {
    let first = sign_seal(&annotated_package());
    let second = sign(
        &first,
        &mut ca("second signer", b"seed-b").signer(),
        &seal_options(),
    )
    .expect("second signature");
    let (before, after) = (entries(&first), entries(&second));
    for name in [
        "OFD.xml",
        "Doc_0/Document.xml",
        "Doc_0/Annots/Annotations.xml",
        "Doc_0/Annots/P_7.xml",
        "Doc_0/Signs/Sign_0/Signature.xml",
        "Doc_0/Signs/Sign_0/SignedValue.dat",
        "Doc_0/Signs/Sign_0/Seal.esl",
    ] {
        assert_eq!(option(&after, name), option(&before, name), "{name}");
    }
    assert_eq!(after.len(), before.len() + 3, "three files for the new one");
    assert!(option(&after, "Doc_0/Signs/Sign_1/Signature.xml").is_some());

    let reports = reports_of(&second);
    assert_eq!(reports.len(), 2);
    assert!(reports
        .iter()
        .all(|report| report.overall == SignatureOverall::Valid));
    assert_eq!(
        reports[1].certificate.as_ref().map(|c| c.subject.as_str()),
        Some("second signer")
    );

    // IDs continue above the first signature's; its record is kept as written.
    let list = text(&after, "Doc_0/Signs/Signatures.xml");
    assert!(list.contains(
        r#"<ofd:Signature ID="1" Type="Seal" BaseLoc="/Doc_0/Signs/Sign_0/Signature.xml"/>"#
    ));
    assert!(list.contains(
        r#"<ofd:Signature ID="3" Type="Seal" BaseLoc="/Doc_0/Signs/Sign_1/Signature.xml"/>"#
    ));
    assert!(list.contains("<ofd:MaxSignId>4</ofd:MaxSignId>"));

    // The first signature is not behind the second one's references.
    let refs = file_refs(&text(&after, "Doc_0/Signs/Sign_1/Signature.xml"));
    assert!(!refs
        .iter()
        .any(|(path, _)| path.starts_with("/Doc_0/Signs/Sign_0/")));
    assert!(refs
        .iter()
        .any(|(path, _)| path == "/Doc_0/Signs/Sign_1/Seal.esl"));
}
