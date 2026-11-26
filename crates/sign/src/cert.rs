//! X.509 certificate subset (spec 2026-11-23 §9): just enough of a
//! certificate to name its holder and to verify an SM2 signature with its
//! public key.

use crate::der::{self, DerNode, TAG_SEQUENCE, TAG_SET};
use crate::error::SignError;

/// What the signing line needs to know about an X.509 certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateInfo {
    /// The subject's `CN`, or an empty string when the subject has none.
    pub subject_cn: String,
    /// The issuer's `CN`, or an empty string.
    pub issuer_cn: String,
    /// The serial number as upper-case hex, without leading zero bytes.
    pub serial_hex: String,
    /// Start of validity, written as in the certificate (`UTCTime` or
    /// `GeneralizedTime`, for example `260101000000Z`).
    pub not_before: Option<String>,
    /// End of validity, in the same form.
    pub not_after: Option<String>,
    /// The public key as an uncompressed point, `04 || X || Y` (65 bytes
    /// for SM2).
    pub public_key_sec1: Vec<u8>,
}

/// `id-at-commonName`.
const OID_COMMON_NAME: &str = "2.5.4.3";

fn not_a_certificate(reason: &str) -> SignError {
    SignError::Certificate {
        reason: format!("not a certificate: {reason}"),
    }
}

/// Read the subset rofd-sign needs from a certificate given as DER.
pub fn parse_certificate(der: &[u8]) -> Result<CertificateInfo, SignError> {
    let error = |reason| not_a_certificate(reason);
    let root = der::parse_one(der).map_err(|_| error("malformed DER"))?;
    if root.tag != TAG_SEQUENCE {
        return Err(error("the root is not a SEQUENCE"));
    }
    let kids = der::parse_children(&root).map_err(|_| error("malformed DER"))?;
    let [tbs, ..] = kids.as_slice() else {
        return Err(error("the certificate is empty"));
    };
    let mut tbs_kids = der::parse_children(tbs).map_err(|_| error("malformed TBSCertificate"))?;
    // version is `[0] EXPLICIT INTEGER` and optional (v1 certificates omit it).
    if tbs_kids.first().is_some_and(|node| node.tag == 0xA0) {
        tbs_kids.remove(0);
    }
    let [serial, _signature, issuer, validity, subject, public_key_info, ..] = tbs_kids.as_slice()
    else {
        return Err(error("TBSCertificate is too short"));
    };
    let validity = der::parse_children(validity).map_err(|_| error("malformed Validity"))?;
    let (not_before, not_after) = match validity.as_slice() {
        [before, after, ..] => (time_text(before), time_text(after)),
        _ => return Err(error("Validity needs notBefore and notAfter")),
    };
    let public_key_sec1 =
        public_key_bytes(public_key_info).map_err(|_| error("malformed SubjectPublicKeyInfo"))?;
    Ok(CertificateInfo {
        subject_cn: common_name(subject).map_err(|_| error("malformed subject Name"))?,
        issuer_cn: common_name(issuer).map_err(|_| error("malformed issuer Name"))?,
        serial_hex: hex_without_leading_zeros(serial.content),
        not_before,
        not_after,
        public_key_sec1,
    })
}

/// The text of a validity time, when it has the shape of one. Anything can
/// sit where a time belongs in a certificate, and the reports hand these
/// strings to the hosts. Reported: `UTCTime` (`YYMMDDHHMM[SS]`) and
/// `GeneralizedTime` (`YYYYMMDDHHMM[SS]`), each with an optional fraction
/// and `Z`.
fn time_text(node: &DerNode<'_>) -> Option<String> {
    if !matches!(node.tag, der::TAG_UTC_TIME | der::TAG_GENERALIZED_TIME) {
        return None;
    }
    let text = node.as_text().ok()?;
    let body = text.strip_suffix('Z').unwrap_or(&text);
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let shaped = matches!(whole.len(), 10 | 12 | 13 | 14)
        && digits(whole)
        && (fraction.is_empty() || digits(fraction));
    shaped.then_some(text)
}

/// The key bytes of SubjectPublicKeyInfo ::= SEQUENCE { algorithm, BIT STRING }.
fn public_key_bytes(info: &DerNode<'_>) -> Result<Vec<u8>, SignError> {
    let kids = der::parse_children(info)?;
    let [_algorithm, key, ..] = kids.as_slice() else {
        return Err(SignError::InvalidInput {
            reason: "malformed DER: SubjectPublicKeyInfo is too short".to_owned(),
        });
    };
    key.as_bit_string()
}

/// The first `CN` of a Name (SEQUENCE OF SET OF AttributeTypeAndValue).
fn common_name(name: &DerNode<'_>) -> Result<String, SignError> {
    for rdn in der::parse_children(name)? {
        if rdn.tag != TAG_SET {
            continue;
        }
        for attribute in der::parse_children(&rdn)? {
            let Ok(kids) = der::parse_children(&attribute) else {
                continue;
            };
            let [kind, value, ..] = kids.as_slice() else {
                continue;
            };
            if kind.as_oid().is_ok_and(|oid| oid == OID_COMMON_NAME) {
                if let Ok(text) = value.as_text() {
                    return Ok(text);
                }
            }
        }
    }
    Ok(String::new())
}

fn hex_without_leading_zeros(bytes: &[u8]) -> String {
    let significant = bytes
        .iter()
        .skip_while(|&&byte| byte == 0)
        .copied()
        .collect::<Vec<u8>>();
    let digits = significant
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    if digits.is_empty() {
        "0".to_owned()
    } else {
        digits
    }
}

/// The `issuerAndSerialNumber` a certificate is identified by in a GB/T
/// 35275 signed message: the issuer Name TLV and the serialNumber INTEGER
/// TLV, as the bytes sit in `der`.
pub(crate) fn issuer_and_serial(der: &[u8]) -> Result<(&[u8], &[u8]), SignError> {
    let tbs = tbs_certificate(der)?;
    // After the optional `[0]` version: serialNumber, signature, issuer.
    let kids = der::parse_children(&tbs)?;
    let kids = if kids.first().is_some_and(|node| node.tag == 0xA0) {
        &kids[1..]
    } else {
        &kids[..]
    };
    let serial = kids
        .first()
        .ok_or_else(|| not_a_certificate("no serial number"))?;
    let issuer = kids.get(2).ok_or_else(|| not_a_certificate("no issuer"))?;
    if serial.tag != der::TAG_INTEGER || issuer.tag != der::TAG_SEQUENCE {
        return Err(not_a_certificate("malformed serial number or issuer"));
    }
    Ok((issuer.raw, serial.raw))
}

/// The to-be-signed part of a certificate.
fn tbs_certificate(der: &[u8]) -> Result<der::DerNode<'_>, SignError> {
    let root = der::parse_one(der).map_err(|_| not_a_certificate("malformed DER"))?;
    if root.tag != der::TAG_SEQUENCE {
        return Err(not_a_certificate("the root is not a SEQUENCE"));
    }
    let kids = der::parse_children(&root).map_err(|_| not_a_certificate("malformed DER"))?;
    let tbs = kids
        .first()
        .ok_or_else(|| not_a_certificate("the certificate is empty"))?;
    if tbs.tag != der::TAG_SEQUENCE {
        return Err(not_a_certificate("no TBSCertificate"));
    }
    Ok(*tbs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A certificate whose validity holds what `before` and `after` write.
    /// Nothing in it is true except the structure.
    fn certificate_with_validity(
        before: impl Fn(&mut der::DerWriter) -> &mut der::DerWriter,
        after: impl Fn(&mut der::DerWriter) -> &mut der::DerWriter,
    ) -> Vec<u8> {
        let name = |w: &mut der::DerWriter, common_name: &str| {
            w.sequence(|w| {
                w.set(|w| {
                    w.sequence(|w| {
                        w.oid(OID_COMMON_NAME);
                        w.utf8_string(common_name);
                    });
                });
            });
        };
        let mut writer = der::DerWriter::new();
        writer.sequence(|w| {
            w.sequence(|w| {
                w.integer(7);
                w.sequence(|w| {
                    w.oid("1.2.840.113549.1.1.2");
                    w.null();
                });
                name(w, "Issuer");
                w.sequence(|w| {
                    before(w);
                    after(w);
                });
                name(w, "Subject");
                w.sequence(|w| {
                    w.sequence(|w| {
                        w.oid("1.2.840.10045.2.1");
                    });
                    w.bit_string(&[0x04, 1, 2, 3]);
                });
            });
            w.sequence(|w| {
                w.oid("1.2.840.113549.1.1.2");
                w.null();
            });
            w.bit_string(&[0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01]);
        });
        writer.into_bytes()
    }

    #[test]
    fn a_validity_that_is_not_a_time_is_not_reported_as_one() {
        // A certificate can carry anything where a time belongs, and the
        // reports hand these strings to the hosts (final review I3).
        // A GeneralizedTime of eight bytes that are not UTF-8.
        let not_utf8 = [0x18, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let info = parse_certificate(&certificate_with_validity(
            |w| w.raw(&not_utf8),
            |w| w.integer(1),
        ))
        .expect("the structure is read");
        assert_eq!(info.subject_cn, "Subject");
        assert_eq!(info.not_before, None, "not a time");
        assert_eq!(info.not_after, None, "an INTEGER is not a time");
        let info = parse_certificate(&certificate_with_validity(
            |w| w.utf8_string("yesterday"),
            |w| w.generalized_time("not digits"),
        ))
        .expect("the structure is read");
        assert_eq!(info.not_before, None);
        assert_eq!(info.not_after, None);
    }

    #[test]
    fn the_times_are_reported_as_they_are_written() {
        let info = parse_certificate(&certificate_with_validity(
            |w| w.utc_time("2601010000Z"),
            |w| w.generalized_time("20291231235959Z"),
        ))
        .expect("the structure is read");
        assert_eq!(info.not_before.as_deref(), Some("2601010000Z"));
        assert_eq!(info.not_after.as_deref(), Some("20291231235959Z"));
        // A fraction, as some writers add one, is kept.
        let info = parse_certificate(&certificate_with_validity(
            |w| w.utc_time("260101000000Z"),
            |w| w.generalized_time("20291231235959.123Z"),
        ))
        .expect("the structure is read");
        assert_eq!(info.not_after.as_deref(), Some("20291231235959.123Z"));
    }
}
