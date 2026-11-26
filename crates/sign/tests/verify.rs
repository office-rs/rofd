//! The verification pipeline against real signed packages (the ofdrw
//! samples in `tests/fixtures/`) and against packages derived from them by
//! tampering with one thing at a time.

use base64::Engine;
use rofd_dom::SignatureKind;
use rofd_io::zip_util::{read_all_entries, write_zip};
use rofd_sign::{
    verify, SignatureOverall as Overall, SignatureReport, SignatureValueStatus as Value,
    SignatureVariant, VerifyOptions,
};

const SIGNATURE_FILES: &str = "Doc_0/Signs/Sign_0";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    read_all_entries(bytes)
        .expect("read entries")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("entry {name} present"))
        .1
}

/// Rebuild `bytes` with some entries replaced (or added) and some removed.
fn rebuilt(bytes: &[u8], replace: &[(&str, &[u8])], remove: &[&str]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = read_all_entries(bytes)
        .expect("read entries")
        .into_iter()
        .filter(|(name, _)| {
            !remove.contains(&name.as_str()) && !replace.iter().any(|(n, _)| *n == name)
        })
        .collect();
    for (name, data) in replace {
        entries.push(((*name).to_owned(), data.to_vec()));
    }
    write_zip(&entries).expect("write zip")
}

fn single(bytes: &[u8], opts: &VerifyOptions) -> SignatureReport {
    let reports = verify(bytes, opts);
    assert_eq!(reports.len(), 1, "one signature: {reports:?}");
    reports.into_iter().next().unwrap()
}

fn default() -> VerifyOptions {
    VerifyOptions::default()
}

#[test]
fn sesv4_sample_is_valid() {
    let report = single(&fixture("SESV4SignDoc.ofd"), &default());
    assert_eq!(report.overall, Overall::Valid, "{report:?}");
    assert_eq!(report.id, "1");
    assert_eq!(report.kind, SignatureKind::Seal);
    assert_eq!(report.variant, SignatureVariant::SesV4);
    assert!(report.integrity.ok);
    assert!(report.integrity.failures.is_empty());
    assert!(
        report.integrity.protected_files > 1,
        "pages, resources, the document"
    );
    assert_eq!(report.seal_match, Some(true));
    assert_eq!(report.signature_value.status, Value::Ok);
    assert_eq!(report.signature_value.reason, None);
    let certificate = report
        .certificate
        .as_ref()
        .expect("the embedded certificate is reported");
    assert!(!certificate.subject.is_empty(), "{}", certificate.subject);
    assert!(!certificate.issuer.is_empty());
    assert!(!certificate.serial.is_empty());
    assert!(certificate.not_before.is_some());
    assert!(certificate.not_after.is_some());
    assert_eq!(certificate.valid_at, None, "no clock was supplied");
    assert!(
        !certificate.chain_validated,
        "the chain is never claimed as validated"
    );
    assert_eq!(report.provider.as_deref(), Some("ofdrw-sign"));
    assert_eq!(report.date_time.as_deref(), Some("2026-11-01 19:05:34"));
}

#[test]
fn sesv1_sample_is_valid() {
    let report = single(&fixture("SESV1SignDoc.ofd"), &default());
    assert_eq!(report.overall, Overall::Valid, "{report:?}");
    assert_eq!(report.variant, SignatureVariant::SesV1);
    assert_eq!(report.seal_match, Some(true));
    assert_eq!(report.signature_value.status, Value::Ok);
}

#[test]
fn tampered_protected_entry_is_invalid_and_names_the_file() {
    let package = fixture("SESV4SignDoc.ofd");
    let mut content = entry(&package, "Doc_0/Pages/Page_0/Content.xml");
    content.push(b' ');
    let bytes = rebuilt(
        &package,
        &[("Doc_0/Pages/Page_0/Content.xml", &content)],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.overall, Overall::Invalid);
    assert!(!report.integrity.ok);
    assert_eq!(
        report.integrity.failures.len(),
        1,
        "{:?}",
        report.integrity.failures
    );
    let failure = &report.integrity.failures[0];
    assert_eq!(failure.file_ref, "/Doc_0/Pages/Page_0/Content.xml");
    assert_ne!(failure.expected, failure.actual);
    assert_eq!(
        report.signature_value.status,
        Value::Ok,
        "the signature itself still verifies"
    );
}

#[test]
fn a_missing_protected_entry_is_a_failure() {
    let package = fixture("SESV4SignDoc.ofd");
    let bytes = rebuilt(&package, &[], &["Doc_0/PublicRes.xml"]);
    let report = single(&bytes, &default());
    assert_eq!(report.overall, Overall::Invalid);
    assert_eq!(report.integrity.failures.len(), 1);
    assert_eq!(
        report.integrity.failures[0].file_ref,
        "/Doc_0/PublicRes.xml"
    );
}

#[test]
fn replaced_seal_esl_fails_seal_match() {
    let package = fixture("SESV4SignDoc.ofd");
    let mut seal = entry(&package, &format!("{SIGNATURE_FILES}/Seal.esl"));
    let last = seal.len() - 1;
    seal[last] ^= 0x01;
    let bytes = rebuilt(
        &package,
        &[(&format!("{SIGNATURE_FILES}/Seal.esl"), &seal)],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.seal_match, Some(false), "{report:?}");
    assert_eq!(report.overall, Overall::Invalid);
}

#[test]
fn tampered_signature_xml_fails_data_hash() {
    let package = fixture("SESV4SignDoc.ofd");
    let description =
        String::from_utf8(entry(&package, &format!("{SIGNATURE_FILES}/Signature.xml")))
            .expect("UTF-8");
    let tampered = description.replace("2026-11-01 19:05:34", "2026-11-02 19:05:34");
    assert_ne!(tampered, description);
    let bytes = rebuilt(
        &package,
        &[(
            &format!("{SIGNATURE_FILES}/Signature.xml"),
            tampered.as_bytes(),
        )],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.signature_value.status, Value::Fail);
    assert!(
        report
            .signature_value
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("data hash")),
        "{report:?}"
    );
    assert_eq!(report.overall, Overall::Invalid);
    assert_eq!(
        report.date_time.as_deref(),
        Some("2026-11-02 19:05:34"),
        "reported as written"
    );
}

#[test]
fn flipped_signature_bytes_fail_sm2() {
    let package = fixture("SESV4SignDoc.ofd");
    let mut value = entry(&package, &format!("{SIGNATURE_FILES}/SignedValue.dat"));
    // The signature is the last node of the file: flip a byte well inside it.
    let at = value.len() - 10;
    value[at] ^= 0x01;
    let bytes = rebuilt(
        &package,
        &[(&format!("{SIGNATURE_FILES}/SignedValue.dat"), &value)],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.signature_value.status, Value::Fail);
    assert!(
        report
            .signature_value
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("SM2")),
        "{report:?}"
    );
    assert_eq!(report.overall, Overall::Invalid);
    // A byte inside the to-be-signed part changes the embedded seal: the
    // standalone Seal.esl no longer matches it, while the files the
    // reference list protects are untouched.
    let mut value = entry(&package, &format!("{SIGNATURE_FILES}/SignedValue.dat"));
    value[2000] ^= 0x01;
    let bytes = rebuilt(
        &package,
        &[(&format!("{SIGNATURE_FILES}/SignedValue.dat"), &value)],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.seal_match, Some(false), "{report:?}");
    assert!(report.integrity.ok, "{:?}", report.integrity.failures);
    assert_eq!(report.signature_value.status, Value::Fail);
}

#[test]
fn sesv5_sample_is_unsupported() {
    let report = single(&fixture("SESV5SignDoc.ofd"), &default());
    assert_eq!(report.variant, SignatureVariant::SesV5);
    assert_eq!(report.overall, Overall::Unsupported, "{report:?}");
    assert!(!report.integrity.ok, "nothing is claimed about the files");
    assert!(report.integrity.failures.is_empty());
}

fn pem_certificate_der(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("certificate file");
    let body: String = text
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .expect("PEM body is base64")
}

#[test]
fn digital_sign_sample_is_unverified_then_verifiable_with_injected_cert() {
    let package = fixture("DigitalSign.ofd");
    let report = single(&package, &default());
    assert_eq!(report.kind, SignatureKind::Sign);
    assert_eq!(report.variant, SignatureVariant::Digital);
    assert!(report.integrity.ok, "{:?}", report.integrity.failures);
    assert_eq!(
        report.signature_value.status,
        Value::Unverifiable,
        "{report:?}"
    );
    assert_eq!(
        report.certificate, None,
        "a digital signature carries no certificate"
    );
    assert_eq!(report.seal_match, None);
    assert_eq!(report.overall, Overall::Unverified);

    // The certificate supplied out of band makes the signature checkable.
    let der =
        std::fs::read("tests/fixtures/digital_signer_cert.der").expect("the signer certificate");
    let opts = VerifyOptions {
        now: None,
        certs: vec![("s001".to_owned(), der.clone())],
    };
    let report = single(&package, &opts);
    assert_eq!(
        report.signature_value.status,
        Value::Ok,
        "the sample signs its signature description file: {report:?}"
    );
    assert_eq!(report.overall, Overall::Valid);
    assert!(report.certificate.is_some());

    // A certificate that did not make the signature fails it. The
    // sign_cert.pem shipped next to the sample is such a certificate.
    let wrong = VerifyOptions {
        now: None,
        certs: vec![("s001".to_owned(), pem_certificate_der("sign_cert.pem"))],
    };
    let report = single(&package, &wrong);
    assert_eq!(report.signature_value.status, Value::Fail);
    assert_eq!(report.overall, Overall::Invalid);

    // An injection keyed for another signature is ignored.
    let other = VerifyOptions {
        now: None,
        certs: vec![("someone-else".to_owned(), der)],
    };
    assert_eq!(single(&package, &other).overall, Overall::Unverified);
}

#[test]
fn an_injected_certificate_does_not_override_an_embedded_one() {
    // The seal sample verifies with its own certificate; injecting a
    // different one under the same id changes nothing.
    let package = fixture("SESV4SignDoc.ofd");
    let opts = VerifyOptions {
        now: None,
        certs: vec![("1".to_owned(), pem_certificate_der("sign_cert.pem"))],
    };
    let report = single(&package, &opts);
    assert_eq!(report.overall, Overall::Valid);
    assert_eq!(report.signature_value.status, Value::Ok);
}

// ---- clock ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Moment {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn digits(text: &str, widths: &[usize]) -> Vec<i64> {
    let mut rest = text;
    let mut out = Vec::new();
    for width in widths {
        let (head, tail) = rest.split_at(*width);
        out.push(head.parse().expect("digits"));
        rest = tail;
    }
    out
}

/// Parse `YYYYMMDDHHMMSSZ`, `YYMMDDHHMMSSZ` or `YYYY-MM-DD HH:MM:SS`.
fn moment(text: &str) -> Option<Moment> {
    let compact: String = text
        .trim_end_matches('Z')
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let (year, month, day, hour, minute, second) = match compact.len() {
        14 => {
            let d = digits(&compact, &[4, 2, 2, 2, 2, 2]);
            (d[0], d[1], d[2], d[3], d[4], d[5])
        }
        12 => {
            let d = digits(&compact, &[2, 2, 2, 2, 2, 2]);
            let year = if d[0] < 50 { 2000 + d[0] } else { 1900 + d[0] };
            (year, d[1], d[2], d[3], d[4], d[5])
        }
        _ => return None,
    };
    Some(Moment {
        year,
        month: u32::try_from(month).ok()?,
        day: u32::try_from(day).ok()?,
        hour: u32::try_from(hour).ok()?,
        minute: u32::try_from(minute).ok()?,
        second: u32::try_from(second).ok()?,
    })
}

/// Days since 1970-01-01 (Howard Hinnant's civil calendar algorithm).
fn days(m: &Moment) -> i64 {
    let y = if m.month <= 2 { m.year - 1 } else { m.year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m.month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + m.day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn from_days(z: i64, clock: &Moment) -> Moment {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    Moment {
        year: if month <= 2 { y + 1 } else { y },
        month,
        day,
        hour: clock.hour,
        minute: clock.minute,
        second: clock.second,
    }
}

fn shift_day(m: &Moment, delta: i64) -> Moment {
    from_days(days(m) + delta, m)
}

fn text(m: &Moment) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        m.year, m.month, m.day, m.hour, m.minute, m.second
    )
}

#[test]
fn validity_boundaries_follow_injected_now() {
    let bytes = fixture("SESV4SignDoc.ofd");
    let certificate = single(&bytes, &default()).certificate.expect("certificate");
    let not_before = moment(certificate.not_before.as_deref().unwrap()).unwrap();
    let not_after = moment(certificate.not_after.as_deref().unwrap()).unwrap();
    let valid_at = |now: &Moment| {
        let opts = VerifyOptions {
            now: Some(text(now)),
            certs: Vec::new(),
        };
        let report = single(&bytes, &opts);
        assert_eq!(
            report.overall,
            Overall::Valid,
            "the clock never flips the verdict"
        );
        report.certificate.unwrap().valid_at
    };
    assert_eq!(valid_at(&shift_day(&not_before, -1)), Some(false));
    assert_eq!(valid_at(&not_before), Some(true));
    assert_eq!(valid_at(&not_after), Some(true));
    assert_eq!(valid_at(&shift_day(&not_after, 1)), Some(false));

    // An unreadable clock leaves the question open.
    let opts = VerifyOptions {
        now: Some("not a time".to_owned()),
        certs: Vec::new(),
    };
    assert_eq!(single(&bytes, &opts).certificate.unwrap().valid_at, None);
}

// ---- what the reference list covers ----

#[test]
fn vendor_style_references_verify_integrity() {
    // Drop the OFD.xml reference from the list, then alter OFD.xml: an
    // entry the list does not name is not checked.
    let package = fixture("SESV4SignDoc.ofd");
    let description =
        String::from_utf8(entry(&package, &format!("{SIGNATURE_FILES}/Signature.xml")))
            .expect("UTF-8");
    let start = description
        .find("<ofd:Reference FileRef=\"/OFD.xml\">")
        .expect("the OFD.xml reference");
    let end = start
        + description[start..]
            .find("</ofd:Reference>")
            .expect("its end")
        + "</ofd:Reference>".len();
    let shortened = format!("{}{}", &description[..start], &description[end..]);
    let ofd = String::from_utf8(entry(&package, "OFD.xml")).expect("UTF-8");
    let tampered = ofd.replacen("</ofd:OFD>", " </ofd:OFD>", 1);
    assert_ne!(tampered, ofd);
    let bytes = rebuilt(
        &package,
        &[
            (
                &format!("{SIGNATURE_FILES}/Signature.xml"),
                shortened.as_bytes(),
            ),
            ("OFD.xml", tampered.as_bytes()),
        ],
        &[],
    );
    let report = single(&bytes, &default());
    assert!(
        report.integrity.ok,
        "only listed entries are checked: {:?}",
        report.integrity.failures
    );
    assert!(report.integrity.failures.is_empty());
    // The description file itself changed, so the signature no longer holds.
    assert_eq!(report.signature_value.status, Value::Fail);
    assert_eq!(report.overall, Overall::Invalid);
}

#[test]
fn a_reference_with_a_relative_path_resolves() {
    // ofdrw writes absolute FileRefs; a relative one names the same entry.
    let package = fixture("SESV4SignDoc.ofd");
    let description =
        String::from_utf8(entry(&package, &format!("{SIGNATURE_FILES}/Signature.xml")))
            .expect("UTF-8");
    let relative = description.replace(
        "FileRef=\"/Doc_0/Pages/Page_0/Content.xml\"",
        "FileRef=\"Doc_0/Pages/Page_0/Content.xml\"",
    );
    assert_ne!(relative, description);
    let bytes = rebuilt(
        &package,
        &[(
            &format!("{SIGNATURE_FILES}/Signature.xml"),
            relative.as_bytes(),
        )],
        &[],
    );
    let report = single(&bytes, &default());
    assert!(report.integrity.ok, "{:?}", report.integrity.failures);
}

#[test]
fn an_unsupported_digest_method_is_reported_not_run() {
    let package = fixture("SESV4SignDoc.ofd");
    let description =
        String::from_utf8(entry(&package, &format!("{SIGNATURE_FILES}/Signature.xml")))
            .expect("UTF-8");
    let other = description.replace("CheckMethod=\"SM3\"", "CheckMethod=\"SHA256\"");
    let bytes = rebuilt(
        &package,
        &[(
            &format!("{SIGNATURE_FILES}/Signature.xml"),
            other.as_bytes(),
        )],
        &[],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.overall, Overall::Unsupported, "{report:?}");
    assert!(!report.integrity.ok);
    assert!(report.integrity.failures.is_empty());
    assert!(report
        .signature_value
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("SHA256")));
}

// ---- broken structure ----

#[test]
fn truncated_signed_value_is_unparseable() {
    let package = fixture("SESV4SignDoc.ofd");
    let mut value = entry(&package, &format!("{SIGNATURE_FILES}/SignedValue.dat"));
    value.truncate(value.len() / 2);
    let description =
        String::from_utf8(entry(&package, &format!("{SIGNATURE_FILES}/Signature.xml")))
            .expect("UTF-8")
            .replace("Sign_0/SignedValue.dat", "Sign_1/SignedValue.dat");
    let list = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Signatures xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Signature ID=\"1\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_0/Signature.xml\"/>\
<ofd:Signature ID=\"2\" Type=\"Seal\" BaseLoc=\"/Doc_0/Signs/Sign_1/Signature.xml\"/>\
<ofd:MaxSignId>3</ofd:MaxSignId></ofd:Signatures>";
    let bytes = rebuilt(
        &package,
        &[
            ("Doc_0/Signs/Signatures.xml", list.as_bytes()),
            ("Doc_0/Signs/Sign_1/Signature.xml", description.as_bytes()),
            ("Doc_0/Signs/Sign_1/SignedValue.dat", &value),
        ],
        &[],
    );
    let reports = verify(&bytes, &default());
    assert_eq!(reports.len(), 2);
    assert_eq!(
        (reports[0].id.as_str(), reports[0].overall),
        ("1", Overall::Valid)
    );
    assert_eq!(reports[1].id, "2");
    assert_eq!(reports[1].overall, Overall::Unparseable, "{:?}", reports[1]);
    assert!(reports[1].integrity.ok, "integrity is judged on its own");
    assert_eq!(reports[1].signature_value.status, Value::Unverifiable);
}

#[test]
fn a_missing_description_or_value_is_unparseable() {
    let package = fixture("SESV4SignDoc.ofd");
    let bytes = rebuilt(
        &package,
        &[],
        &[&format!("{SIGNATURE_FILES}/Signature.xml")],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.overall, Overall::Unparseable, "{report:?}");
    assert!(!report.integrity.ok);

    let bytes = rebuilt(
        &package,
        &[],
        &[&format!("{SIGNATURE_FILES}/SignedValue.dat")],
    );
    let report = single(&bytes, &default());
    assert_eq!(report.overall, Overall::Unparseable);
    assert!(report.integrity.ok);
}

#[test]
fn unopenable_package_yields_no_reports() {
    assert!(verify(b"not a zip at all", &default()).is_empty());
    assert!(verify(&[], &default()).is_empty());
    // A well-formed package without signatures.
    let blank = rofd_io::write_ofd(&rofd_dom::OfdDocument::default()).expect("blank package");
    assert!(verify(&blank, &default()).is_empty());
    // A declaration that points at a missing list.
    let package = fixture("SESV4SignDoc.ofd");
    let bytes = rebuilt(&package, &[], &["Doc_0/Signs/Signatures.xml"]);
    assert!(verify(&bytes, &default()).is_empty());
}

#[test]
fn a_damaged_gbt35275_message_is_unparseable() {
    let mut writer = rofd_sign::DerWriter::new();
    writer.sequence(|w| {
        w.oid("1.2.156.10197.6.1.4.2.2");
        w.explicit(0, |w| {
            w.sequence(|w| {
                w.integer(1);
            });
        });
    });
    let package = fixture("DigitalSign.ofd");
    let bytes = rebuilt(
        &package,
        &[(
            &format!("{SIGNATURE_FILES}/SignedValue.dat"),
            &writer.into_bytes(),
        )],
        &[],
    );
    let report = single(&bytes, &default());
    // Recognized by its content type, but the stub carries no signer: the
    // structure cannot be read. (A complete message verifies; see
    // tests/encode.rs.)
    assert_eq!(report.variant, SignatureVariant::Gbt35275);
    assert_eq!(
        report.signature_value.status,
        Value::Unverifiable,
        "{report:?}"
    );
    assert_eq!(report.overall, Overall::Unparseable);
}

// ---- a signature that protects nothing (final review I1) ----

fn unsigned_package() -> Vec<u8> {
    let files: Vec<(String, Vec<u8>)> = [
        (
            "OFD.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>",
        ),
        (
            "Doc_0/Document.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID>\
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData>\
<ofd:Pages><ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages></ofd:Document>",
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>",
        ),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.as_bytes().to_vec()))
    .collect();
    write_zip(&files).expect("zip")
}

fn seal() -> rofd_sign::SealImage {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(40, 40, image::Rgba([190, 30, 30, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("PNG");
    rofd_sign::SealImage {
        png: png.into_inner(),
        size_mm: 0.0,
    }
}

const SIGNATURE_XML: &str = "Doc_0/Signs/Sign_0/Signature.xml";
const SIGNATURE_VALUE: &str = "Doc_0/Signs/Sign_0/SignedValue.dat";

/// `package` signed by the test CA, with `references` in place of the
/// signature description's `References` element and a signature value that
/// really verifies over that description. The seal is the one of a first,
/// ordinary signature of the same package.
fn signed_with_references(references: &str) -> Vec<u8> {
    use rofd_sign::{sign, SignOptions, Signer, TestCa};
    const NOW: &str = "2026-11-24 10:30:00";
    let ordinary = sign(
        &unsigned_package(),
        &mut TestCa::from_seed("rofd test signer", b"seed-a").signer(),
        &SignOptions {
            seal: Some(seal()),
            ..SignOptions::new(NOW)
        },
    )
    .expect("an ordinary signature");
    assert_eq!(
        single(&ordinary, &default()).overall,
        Overall::Valid,
        "the starting point verifies"
    );
    let seal_esl = entry(&ordinary, "Doc_0/Signs/Sign_0/Seal.esl");
    let description = entry(&ordinary, SIGNATURE_XML);
    let description = String::from_utf8(description).expect("UTF-8");
    let start = description.find("<ofd:References").expect("References");
    let end = description[start..]
        .find("</ofd:References>")
        .expect("References close")
        + start
        + "</ofd:References>".len();
    let description = format!(
        "{}{}{}",
        &description[..start],
        references,
        &description[end..]
    );

    let mut signer = TestCa::from_seed("rofd test signer", b"seed-a").signer();
    let certificate = signer.certificate_der().to_vec();
    let tbs = rofd_sign::build_tbs(
        &seal_esl,
        &rofd_sign::sm3(description.as_bytes()),
        &format!("/{SIGNATURE_XML}"),
        NOW,
    )
    .expect("to-be-signed data");
    let signature = signer.sign(&tbs).expect("sign");
    let value =
        rofd_sign::finish_signature(&tbs, &signature, &certificate).expect("signature value");

    // The page is changed under the signature: a signature that protected
    // anything would have to fail here.
    let mut content = entry(&ordinary, "Doc_0/Pages/Page_0/Content.xml");
    content.push(b' ');
    rebuilt(
        &ordinary,
        &[
            (SIGNATURE_XML, description.as_bytes()),
            (SIGNATURE_VALUE, &value),
            ("Doc_0/Pages/Page_0/Content.xml", &content),
        ],
        &[],
    )
}

#[test]
fn a_signature_that_protects_no_file_is_not_valid() {
    // The integrity layer had nothing to fail on when the list was empty,
    // so the verdict rested on the signature value alone - and a changed
    // page still read as Valid.
    for references in ["<ofd:References CheckMethod=\"SM3\"></ofd:References>", ""] {
        let bytes = signed_with_references(references);
        let report = single(&bytes, &default());
        assert_eq!(
            report.signature_value.status,
            Value::Ok,
            "the value verifies over the description: {report:?}"
        );
        assert_eq!(report.integrity.protected_files, 0);
        assert!(!report.integrity.ok, "nothing is protected: {report:?}");
        assert_eq!(report.overall, Overall::Invalid, "{report:?}");
    }
}
