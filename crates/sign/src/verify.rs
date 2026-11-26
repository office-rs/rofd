//! The verification pipeline (spec 2026-11-23 §9.2).
//!
//! Each signature of a package is checked on its own, straight from the
//! package bytes, in three layers:
//!
//! 1. **Integrity**: every file the reference list names still hashes to the
//!    recorded value (SM3).
//! 2. **Seal consistency**: the standalone seal file equals the seal embedded
//!    in the signature value.
//! 3. **Signature value**: the recorded data hash is SM3 of the signature
//!    description file, and the SM2 signature verifies against the raw bytes
//!    of the to-be-signed part.
//!
//! The certificate is reported, never judged beyond an optional validity
//! period: the chain and revocation are out of scope. Nothing here panics on
//! damaged input and nothing here re-encodes a signature structure; the SM2
//! check runs on the bytes exactly as they sit in the file.

use std::collections::BTreeMap;

use base64::Engine as _;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use rofd_dom::SignatureKind;
use rofd_io::parse::ofd_xml::parse_signatures_loc;
use rofd_io::{zip_util::read_all_entries, PackageHandle};

use crate::cert::{parse_certificate, CertificateInfo};
use crate::crypto::{sm2_verify, sm3, DEFAULT_SM2_USER_ID};
use crate::der::{self, TAG_INTEGER, TAG_OID, TAG_SEQUENCE};
use crate::error::SignError;
use crate::gbt35275::parse_signed_data;
use crate::report::{
    CertificateReport, IntegrityFailure, IntegrityResult, SignatureOverall, SignatureReport,
    SignatureValue, SignatureValueStatus, SignatureVariant, VerifyOptions,
};
use crate::ses::parse_ses_signature;

/// `sm2sign-with-sm3` (GM/T 0006): the only signature method this line
/// produces and verifies.
const OID_SM2_WITH_SM3: &str = "1.2.156.10197.1.501";
/// The GB/T 35275 `signedData` content type.
const OID_GBT35275_SIGNED_DATA: &str = "1.2.156.10197.6.1.4.2.2";

/// Where verification reads an entry: a package read from bytes, or the
/// package a document carries (spec 2026-11-23 §17.11). Either way the
/// digest is taken where the bytes lie.
trait Entries {
    fn entry(&self, path: &str) -> Option<&[u8]>;
}

impl<A: AsRef<str>, B: AsRef<[u8]>> Entries for [(A, B)] {
    fn entry(&self, path: &str) -> Option<&[u8]> {
        self.iter()
            .find(|(name, _)| name.as_ref() == path)
            .map(|(_, bytes)| bytes.as_ref())
    }
}

impl Entries for PackageHandle {
    fn entry(&self, path: &str) -> Option<&[u8]> {
        self.find(path).map(|entry| entry.bytes.as_slice())
    }
}

/// Verify every signature of an OFD package.
///
/// Returns one report per signature in list order. A package that cannot be
/// opened, or that declares no signature list, yields no reports.
pub fn verify(ofd_bytes: &[u8], opts: &VerifyOptions) -> Vec<SignatureReport> {
    let Ok(entries) = read_all_entries(ofd_bytes) else {
        return Vec::new();
    };
    verify_entries(&entries[..], opts)
}

/// [`verify`] over the package a document carries, without making a file of
/// it first.
pub(crate) fn verify_package(
    package: &PackageHandle,
    opts: &VerifyOptions,
) -> Vec<SignatureReport> {
    verify_entries(package, opts)
}

fn verify_entries<E: Entries + ?Sized>(entries: &E, opts: &VerifyOptions) -> Vec<SignatureReport> {
    let ofd_xml = text_of(entries.entry("OFD.xml").unwrap_or(&[]));
    // OFD.xml sits at the package root, so its locs resolve against "".
    let Some(signatures_loc) = parse_signatures_loc(&ofd_xml) else {
        return Vec::new();
    };
    let list_path = resolve_loc(&signatures_loc, "");
    let Some(list) = entries.entry(&list_path) else {
        return Vec::new();
    };
    let Some(records) = parse_signature_list(&text_of(list)) else {
        return Vec::new();
    };
    let supplied: BTreeMap<&str, &[u8]> = opts
        .certs
        .iter()
        .map(|(id, der)| (id.as_str(), der.as_slice()))
        .collect();
    let list_dir = parent_dir(&list_path).to_owned();
    records
        .into_iter()
        .map(|record| verify_one(entries, &list_dir, record, opts, &supplied))
        .collect()
}

// ---- package access ----

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A loc as a package path: a leading `/` is package-absolute, anything
/// else is relative to `base_dir`, the directory of the file that holds the
/// reference.
fn resolve_loc(loc: &str, base_dir: &str) -> String {
    let loc = loc.trim();
    if let Some(absolute) = loc.strip_prefix('/') {
        return absolute.to_owned();
    }
    if base_dir.is_empty() {
        loc.to_owned()
    } else {
        format!("{base_dir}/{loc}")
    }
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

// ---- signature list and description ----

struct ListEntry {
    id: String,
    kind: SignatureKind,
    base_loc: String,
}

/// The `Signature` records of the signature list. `None` when the XML is
/// unreadable.
fn parse_signature_list(xml: &str) -> Option<Vec<ListEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().local_name().as_ref() == b"Signature" =>
            {
                let kind = match attr(&e, "Type").as_deref() {
                    Some("Sign") => SignatureKind::Sign,
                    _ => SignatureKind::Seal,
                };
                out.push(ListEntry {
                    id: attr(&e, "ID").unwrap_or_default(),
                    kind,
                    base_loc: attr(&e, "BaseLoc").unwrap_or_default(),
                });
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(out)
}

pub(crate) fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// What the signature description file (Signature.xml) declares.
#[derive(Debug, Default)]
struct Description {
    signed_value_loc: Option<String>,
    provider: Option<String>,
    signature_method: Option<String>,
    date_time: Option<String>,
    seal_loc: Option<String>,
    check_method: Option<String>,
    /// `(FileRef, CheckValue)` in list order.
    references: Vec<(String, String)>,
}

fn parse_description(xml: &str) -> Option<Description> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut description = Description::default();
    let mut in_seal = false;
    let mut in_reference = false;
    let mut file_ref: Option<String> = None;
    // Which element's text the next Text event belongs to.
    let mut target: Option<&'static str> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().local_name().as_ref() {
                b"SignedValue" => target = Some("SignedValue"),
                b"SignatureMethod" => target = Some("SignatureMethod"),
                b"SignatureDateTime" => target = Some("SignatureDateTime"),
                b"Seal" => in_seal = true,
                b"BaseLoc" => target = Some("Seal"),
                b"Provider" => description.provider = attr(&e, "ProviderName"),
                b"References" => description.check_method = attr(&e, "CheckMethod"),
                b"Reference" => {
                    file_ref = attr(&e, "FileRef");
                    in_reference = true;
                    target = Some("CheckValue");
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                let text = text.trim().to_owned();
                if !text.is_empty() {
                    match target {
                        Some("SignedValue") => description.signed_value_loc = Some(text),
                        Some("SignatureMethod") => description.signature_method = Some(text),
                        Some("SignatureDateTime") => description.date_time = Some(text),
                        Some("Seal") if in_seal => description.seal_loc = Some(text),
                        Some("CheckValue") if in_reference => {
                            if let Some(file_ref) = file_ref.take() {
                                description.references.push((file_ref, text));
                            }
                        }
                        _ => {}
                    }
                }
                target = None;
            }
            Ok(Event::End(e)) => match e.name().local_name().as_ref() {
                b"Seal" => in_seal = false,
                b"Reference" => {
                    in_reference = false;
                    file_ref = None;
                }
                b"SignedValue" | b"SignatureMethod" | b"SignatureDateTime" | b"BaseLoc"
                | b"CheckValue" => target = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(description)
}

// ---- the three layers ----

fn check_integrity<E: Entries + ?Sized>(
    entries: &E,
    references: &[(String, String)],
) -> IntegrityResult {
    let mut failures = Vec::new();
    for (file_ref, check_value) in references {
        // A FileRef is package-absolute (leading `/`) or relative to the
        // package root.
        let path = resolve_loc(file_ref, "");
        let expected = check_value.trim();
        let matches = entries.entry(&path).is_some_and(|bytes| {
            base64::engine::general_purpose::STANDARD.encode(sm3(bytes)) == expected
        });
        if !matches {
            let actual = entries
                .entry(&path)
                .map(|bytes| base64::engine::general_purpose::STANDARD.encode(sm3(bytes)))
                .unwrap_or_default();
            failures.push(IntegrityFailure {
                file_ref: file_ref.clone(),
                expected: expected.to_owned(),
                actual,
            });
        }
    }
    IntegrityResult {
        // A signature that names no file protects nothing. That is a failed
        // layer, not an empty list of failures: with nothing to check, the
        // verdict would rest on the signature value alone and a changed page
        // would still read as Valid.
        ok: !references.is_empty() && failures.is_empty(),
        failures,
        protected_files: references.len(),
    }
}

enum ValueKind<'a> {
    Ses(crate::ses::SesSignature<'a>),
    SesV5,
    Gbt35275,
    /// A bare r/s pair: the DER bytes are the signature itself.
    Digital(&'a [u8]),
}

fn unrecognized(reason: &str) -> SignError {
    SignError::InvalidInput {
        reason: format!("unrecognized signature value: {reason}"),
    }
}

/// What kind of signature value file `value` is.
fn classify(value: &[u8]) -> Result<ValueKind<'_>, SignError> {
    match parse_ses_signature(value) {
        Ok(ses) => return Ok(ValueKind::Ses(ses)),
        Err(error) => {
            let unsupported = matches!(
                &error,
                SignError::InvalidInput { reason } if reason.starts_with("unsupported SES version")
            );
            if unsupported {
                return if ses_version(value) == Some(5) {
                    Ok(ValueKind::SesV5)
                } else {
                    Err(error)
                };
            }
        }
    }
    let root = der::parse_one(value).map_err(|_| unrecognized("malformed DER"))?;
    if root.tag != TAG_SEQUENCE {
        return Err(unrecognized("the root is not a SEQUENCE"));
    }
    let kids = der::parse_children(&root).map_err(|_| unrecognized("malformed DER"))?;
    // ContentInfo ::= SEQUENCE { contentType OID, content [0] EXPLICIT }.
    if let Some(content_type) = kids.first().filter(|node| node.tag == TAG_OID) {
        return if content_type
            .as_oid()
            .is_ok_and(|oid| oid == OID_GBT35275_SIGNED_DATA)
        {
            Ok(ValueKind::Gbt35275)
        } else {
            Err(unrecognized("an unknown content type"))
        };
    }
    if kids.len() == 2 && kids.iter().all(|node| node.tag == TAG_INTEGER) {
        return Ok(ValueKind::Digital(value));
    }
    Err(unrecognized("neither an SES signature nor an r/s pair"))
}

/// The version INTEGER an SES signature value opens with.
fn ses_version(value: &[u8]) -> Option<i64> {
    let outer = der::parse_one(value).ok()?;
    let tbs_node = der::parse_children(&outer).ok()?.into_iter().next()?;
    der::parse_children(&tbs_node).ok()?.first()?.as_i64().ok()
}

fn unverifiable(reason: &str) -> SignatureValue {
    SignatureValue {
        status: SignatureValueStatus::Unverifiable,
        reason: Some(reason.to_owned()),
    }
}

fn failed(reason: &str) -> SignatureValue {
    SignatureValue {
        status: SignatureValueStatus::Fail,
        reason: Some(reason.to_owned()),
    }
}

fn certificate_report(info: &CertificateInfo, now: Option<&str>) -> CertificateReport {
    let valid_at = now
        .and_then(instant)
        .zip(info.not_before.as_deref().and_then(instant))
        .zip(info.not_after.as_deref().and_then(instant));
    CertificateReport {
        subject: info.subject_cn.clone(),
        issuer: info.issuer_cn.clone(),
        serial: info.serial_hex.clone(),
        not_before: info.not_before.clone(),
        not_after: info.not_after.clone(),
        valid_at: valid_at.map(|((now, before), after)| before <= now && now <= after),
        chain_validated: false,
    }
}

/// Whether the digest inside a GB/T 35275 message is SM3 of the description
/// file. Some writers store the digest base64-encoded.
fn covers_description(plaintext: &[u8], description: &[u8]) -> bool {
    let expected = sm3(description);
    plaintext == expected
        || base64::engine::general_purpose::STANDARD
            .decode(plaintext)
            .is_ok_and(|decoded| decoded == expected)
}

/// A moment as `(days since 1970-01-01, seconds of day)`, read from
/// `GeneralizedTime` (`YYYYMMDDHHMMSSZ`), `UTCTime` (`YYMMDDHHMMSSZ`) or
/// `YYYY-MM-DD HH:MM:SS`. Seconds may be omitted.
fn instant(text: &str) -> Option<(i64, u32)> {
    let digits: String = text
        .trim_end_matches('Z')
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let number =
        |from: usize, len: usize| -> Option<i64> { digits.get(from..from + len)?.parse().ok() };
    // GeneralizedTime starts with a four-digit year, UTCTime with two.
    let (year, rest) = match digits.len() {
        13 | 14 => (number(0, 4)?, 4),
        11 | 12 => {
            let two = number(0, 2)?;
            // UTCTime: 00..49 are 2000..2049, 50..99 are 1950..1999.
            (if two < 50 { 2000 + two } else { 1900 + two }, 2)
        }
        _ => return None,
    };
    let field = |offset: usize| number(rest + offset, 2);
    let (month, day, hour, minute) = (field(2)?, field(4)?, field(6)?, field(8)?);
    let second = digits
        .get(rest + 10..rest + 12)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    // Days since 1970-01-01 by Howard Hinnant's civil calendar algorithm.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let march_based_month = (month + 9) % 12;
    let day_of_year = (153 * march_based_month + 2) / 5 + day - 1;
    let day_of_era =
        365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100) + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    Some((days, (hour * 3600 + minute * 60 + second) as u32))
}

// ---- one signature ----

fn verify_one<E: Entries + ?Sized>(
    entries: &E,
    list_dir: &str,
    record: ListEntry,
    opts: &VerifyOptions,
    supplied: &BTreeMap<&str, &[u8]>,
) -> SignatureReport {
    let description_path = resolve_loc(&record.base_loc, list_dir);
    let description_bytes = entries.entry(&description_path);
    let description = description_bytes
        .map(text_of)
        .and_then(|xml| parse_description(&xml));

    let mut variant = SignatureVariant::Unknown;
    let mut integrity = IntegrityResult {
        ok: false,
        failures: Vec::new(),
        protected_files: 0,
    };
    let mut seal_match = None;
    let mut value = unverifiable("the signature value was not examined");
    let mut certificate = None;
    let mut provider = None;
    let mut date_time = None;
    let mut unsupported: Option<String> = None;
    let mut broken: Option<String> = None;
    // Whether layer 1 ran for this signature. Until it does, `integrity`
    // claims nothing: its `ok` is false because nothing was checked.
    let mut integrity_checked = false;

    if let Some(description) = description {
        provider = description.provider.clone();
        date_time = description.date_time.clone();

        // Algorithms first: anything but SM3 and SM2withSM3 is reported,
        // never run through the wrong check.
        if let Some(method) = description.signature_method.as_deref() {
            if method.trim() != OID_SM2_WITH_SM3 {
                unsupported = Some(format!("signature method {method} is not SM2withSM3"));
            }
        }
        if unsupported.is_none() {
            if let Some(method) = description.check_method.as_deref() {
                if method.trim() != "SM3" {
                    unsupported = Some(format!("digest method {method} is not SM3"));
                }
            }
        }
        if let Some(reason) = &unsupported {
            value = unverifiable(reason);
        } else {
            integrity = check_integrity(entries, &description.references);
            integrity_checked = true;
            let description_dir = parent_dir(&description_path).to_owned();
            let value_path = description
                .signed_value_loc
                .as_deref()
                .map(|loc| resolve_loc(loc, &description_dir));
            let value_bytes = value_path.as_deref().and_then(|path| entries.entry(path));
            match value_bytes.map(classify).transpose() {
                Err(error) => broken = Some(error.to_string()),
                Ok(None) => broken = Some("the signature value file is missing".to_owned()),
                Ok(Some(ValueKind::SesV5)) => {
                    variant = SignatureVariant::SesV5;
                    unsupported = Some("SES version 5 is not supported".to_owned());
                    // Recognised, not supported: nothing is claimed.
                    integrity_checked = false;
                    integrity = IntegrityResult {
                        ok: false,
                        failures: Vec::new(),
                        protected_files: 0,
                    };
                    value = unverifiable("SES version 5 is not supported");
                }
                Ok(Some(ValueKind::Gbt35275)) => {
                    variant = SignatureVariant::Gbt35275;
                    // The message carries its own certificate and the digest
                    // of the description file.
                    match parse_signed_data(value_bytes.unwrap_or(&[])) {
                        Err(error) => broken = Some(error.to_string()),
                        Ok(message) => match parse_certificate(message.cert_der) {
                            Err(error) => {
                                broken = Some(format!(
                                    "the embedded certificate cannot be read: {error}"
                                ))
                            }
                            Ok(info) => {
                                certificate = Some(certificate_report(&info, opts.now.as_deref()));
                                let description = description_bytes.unwrap_or(&[]);
                                if !covers_description(message.plaintext, description) {
                                    value = failed(
                                        "the data hash does not match the signature description file",
                                    );
                                } else if sm2_verify(
                                    &info.public_key_sec1,
                                    DEFAULT_SM2_USER_ID,
                                    message.plaintext,
                                    message.signature,
                                ) {
                                    value = SignatureValue {
                                        status: SignatureValueStatus::Ok,
                                        reason: None,
                                    };
                                } else {
                                    value = failed("the SM2 signature does not verify");
                                }
                            }
                        },
                    }
                }
                Ok(Some(ValueKind::Digital(signature))) => {
                    variant = SignatureVariant::Digital;
                    // The certificate travels out of band: the signature
                    // is over the description file itself.
                    let description = description_bytes.unwrap_or(&[]);
                    match supplied.get(record.id.as_str()) {
                        None => {
                            value = unverifiable(
                                "a digital signature carries no certificate, and none was supplied",
                            )
                        }
                        Some(der) => match parse_certificate(der) {
                            Err(_) => {
                                value = unverifiable("the supplied certificate cannot be read")
                            }
                            Ok(info) => {
                                certificate = Some(certificate_report(&info, opts.now.as_deref()));
                                value = if sm2_verify(
                                    &info.public_key_sec1,
                                    DEFAULT_SM2_USER_ID,
                                    description,
                                    signature,
                                ) {
                                    SignatureValue {
                                        status: SignatureValueStatus::Ok,
                                        reason: None,
                                    }
                                } else {
                                    failed("the SM2 signature does not verify with the supplied certificate")
                                };
                            }
                        },
                    }
                }
                Ok(Some(ValueKind::Ses(ses))) => {
                    variant = if ses.version == 4 {
                        SignatureVariant::SesV4
                    } else {
                        SignatureVariant::SesV1
                    };
                    // Layer 2: the standalone seal, when the package has one.
                    let seal_path = description
                        .seal_loc
                        .as_deref()
                        .map(|loc| resolve_loc(loc, &description_dir));
                    seal_match = seal_path
                        .as_deref()
                        .and_then(|path| entries.entry(path))
                        .map(|seal| seal == ses.embedded_seal_raw);
                    // Layer 3: data hash, then the SM2 signature over the raw TBS bytes.
                    match parse_certificate(ses.cert_der) {
                        Err(error) => {
                            broken =
                                Some(format!("the embedded certificate cannot be read: {error}"))
                        }
                        Ok(info) => {
                            certificate = Some(certificate_report(&info, opts.now.as_deref()));
                            let description = description_bytes.unwrap_or(&[]);
                            if ses.data_hash != sm3(description) {
                                value = failed(
                                    "the data hash does not match the signature description file",
                                );
                            } else if sm2_verify(
                                &info.public_key_sec1,
                                DEFAULT_SM2_USER_ID,
                                ses.tbs_raw,
                                ses.signature_der,
                            ) {
                                value = SignatureValue {
                                    status: SignatureValueStatus::Ok,
                                    reason: None,
                                };
                            } else {
                                value = failed("the SM2 signature does not verify");
                            }
                        }
                    }
                }
            }
        }
    } else {
        broken = Some("the signature description file is missing or unreadable".to_owned());
    }

    let overall = if unsupported.is_some() {
        SignatureOverall::Unsupported
    } else if (integrity_checked && !integrity.ok)
        || seal_match == Some(false)
        || value.status == SignatureValueStatus::Fail
    {
        SignatureOverall::Invalid
    } else if broken.is_some() || variant == SignatureVariant::Unknown {
        SignatureOverall::Unparseable
    } else if value.status == SignatureValueStatus::Unverifiable {
        SignatureOverall::Unverified
    } else {
        SignatureOverall::Valid
    };

    if let Some(reason) = broken {
        value = unverifiable(&reason);
        value.status = SignatureValueStatus::Unverifiable;
    }
    SignatureReport {
        id: record.id,
        kind: record.kind,
        variant,
        overall,
        integrity,
        seal_match,
        signature_value: value,
        certificate,
        provider,
        date_time,
    }
}
