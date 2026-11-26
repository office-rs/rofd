//! The SES v4 encoders (GB/T 38540-2020): an electronic seal and the
//! signature value around it.
//!
//! ```text
//! ESS_Seal      ::= SEQUENCE { eSealInfo, certificate OCTET STRING, algorithm OID, maker signature BIT STRING }
//! eSealInfo     ::= SEQUENCE { header SEQUENCE { "ES" IA5, head INTEGER 4, maker IA5 }, id IA5,
//!                            property SEQUENCE { type, name, certListType,
//!                                                certList SEQUENCE OF OCTET STRING (certificates),
//!                                                created, validFrom, validUntil },
//!                            picture SEQUENCE { "PNG" IA5, data OCTET STRING, width INTEGER, height INTEGER } }
//! TBS_Sign      ::= SEQUENCE { version INTEGER 4, ESS_Seal, time GeneralizedTime, dataHash BIT STRING, propertyInfo IA5 }
//! SES_Signature ::= SEQUENCE { TBS_Sign, certificate OCTET STRING, algorithm OID, signature BIT STRING }
//! ```
//!
//! Field order and layout follow `org.ofdrw.gm.ses.v4`; the tag tree of a
//! seal written here equals that of the ofdrw samples, which is also what
//! its validator parses strictly (a certificate list that is not a list of
//! OCTET STRINGs stops it). Creating a seal takes one signature (over
//! `eSealInfo`); signing a document takes another (over the TBS).

use base64::Engine as _;

use crate::cert::parse_certificate;
use crate::crypto::sm3;
use crate::der::DerWriter;
use crate::error::SignError;
use crate::signature_xml::{as_generalized_time, generalized_time, SIGNATURE_METHOD_SM2_SM3};

/// The GB/T 38540 seal head.
const SEAL_HEAD: i64 = 4;
/// Who made the seal.
const SEAL_MAKER: &str = "rofd";
/// The seal's display name.
const SEAL_NAME: &str = "rofd seal";

/// What a seal is made of.
pub struct SealInput<'a> {
    /// The seal picture, PNG.
    pub png: &'a [u8],
    /// Millimetres. Rounded to the integers the structure holds.
    pub width: f64,
    pub height: f64,
    /// The certificate the seal is bound to. It is embedded in the seal,
    /// and the seal's validity is this certificate's validity.
    pub certificate_der: &'a [u8],
}

fn invalid(reason: &str) -> SignError {
    SignError::InvalidInput {
        reason: reason.to_owned(),
    }
}

/// Build `eSealInfo`, the part of a seal its maker signs. `now` only has to
/// parse: the times written are the certificate's.
pub fn build_e_seal_info(seal: &SealInput<'_>, now: &str) -> Result<Vec<u8>, SignError> {
    generalized_time(now)?;
    if !seal.png.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err(invalid("the seal picture must be a PNG image"));
    }
    image::load_from_memory(seal.png).map_err(|_| invalid("the seal picture cannot be decoded"))?;
    if !seal.width.is_finite()
        || !seal.height.is_finite()
        || seal.width <= 0.0
        || seal.height <= 0.0
    {
        return Err(invalid(
            "the seal size must be a positive number of millimetres",
        ));
    }
    let certificate = parse_certificate(seal.certificate_der)
        .map_err(|_| invalid("the seal certificate is not an X.509 certificate"))?;
    let valid_from = certificate
        .not_before
        .as_deref()
        .and_then(as_generalized_time)
        .ok_or_else(|| invalid("the seal certificate has no readable validity"))?;
    let valid_until = certificate
        .not_after
        .as_deref()
        .and_then(as_generalized_time)
        .ok_or_else(|| invalid("the seal certificate has no readable validity"))?;

    // A 32-hex-digit seal id derived from the picture: deterministic, with
    // no clock and no counter.
    let id: String = sm3(seal.png)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(32)
        .collect();

    let mut property = DerWriter::new();
    property.sequence(|w| {
        w.integer(1);
        w.utf8_string(SEAL_NAME);
        // The certificate list: every certificate in an OCTET STRING.
        w.integer(1);
        w.sequence(|w| {
            w.octet_string(seal.certificate_der);
        });
        w.generalized_time(&valid_from);
        w.generalized_time(&valid_from);
        w.generalized_time(&valid_until);
    });
    let mut picture = DerWriter::new();
    picture.sequence(|w| {
        w.ia5_string("PNG");
        w.octet_string(seal.png);
        w.integer(seal.width.round() as i64);
        w.integer(seal.height.round() as i64);
    });
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.sequence(|w| {
            w.ia5_string("ES");
            w.integer(SEAL_HEAD);
            w.ia5_string(SEAL_MAKER);
        });
        w.ia5_string(&id);
        w.raw(&property.into_bytes());
        w.raw(&picture.into_bytes());
    });
    Ok(writer.into_bytes())
}

/// Complete the seal: `ESS_Seal` (the content of Seal.esl) from the
/// `e_seal_info` built by [`build_e_seal_info`], the maker's signature over
/// it, and the maker's certificate.
pub fn finish_seal(
    e_seal_info: &[u8],
    maker_signature: &[u8],
    maker_certificate_der: &[u8],
) -> Result<Vec<u8>, SignError> {
    if maker_signature.is_empty() {
        return Err(SignError::Signer {
            reason: "the maker signature is empty".to_owned(),
        });
    }
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.raw(e_seal_info);
        w.octet_string(maker_certificate_der);
        w.oid(SIGNATURE_METHOD_SM2_SM3);
        w.bit_string(maker_signature);
    });
    Ok(writer.into_bytes())
}

/// Build `TBS_Sign`, the to-be-signed part of a signature value. Its exact
/// bytes are what the signer signs and what verification checks.
///
/// `time` is `YYYY-MM-DD HH:mm:ss`; `property_info` is the package path of
/// the signature's own Signature.xml; `data_hash` is SM3 of that file.
pub fn build_tbs(
    seal_der: &[u8],
    data_hash: &[u8],
    property_info: &str,
    time: &str,
) -> Result<Vec<u8>, SignError> {
    if data_hash.len() != 32 {
        return Err(invalid("the data hash must be an SM3 value"));
    }
    let time = generalized_time(time)?;
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.integer(SEAL_HEAD);
        w.raw(seal_der);
        w.generalized_time(&time);
        w.bit_string(data_hash);
        w.ia5_string(property_info);
    });
    Ok(writer.into_bytes())
}

/// Complete the signature value: `SES_Signature` (the content of
/// SignedValue.dat) from the TBS built by [`build_tbs`], the signer's
/// signature over it, and the signer's certificate.
pub fn finish_signature(
    tbs_raw: &[u8],
    signature: &[u8],
    signer_certificate_der: &[u8],
) -> Result<Vec<u8>, SignError> {
    if signature.is_empty() {
        return Err(SignError::Signer {
            reason: "the signature is empty".to_owned(),
        });
    }
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.raw(tbs_raw);
        w.octet_string(signer_certificate_der);
        w.oid(SIGNATURE_METHOD_SM2_SM3);
        w.bit_string(signature);
    });
    Ok(writer.into_bytes())
}

/// The SM3 digest of `bytes`, base64: the form a `CheckValue` takes.
pub fn check_value(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(sm3(bytes))
}
