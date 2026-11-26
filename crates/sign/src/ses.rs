//! SES_Signature, the content of a signature value file (SignedValue.dat):
//! GM/T 0031-2014 (SES v1) and GB/T 38540-2020 (SES v4).
//!
//! Byte-level layout, established on the ofdrw samples and matching
//! `org.ofdrw.gm.ses.v1` / `v4`:
//!
//! ```text
//! v4  outer = [ TBS, signer certificate OCTET STRING, algorithm OID, signature BIT STRING ]
//! v1  outer = [ TBS, signature BIT STRING ]        (certificate and OID end the TBS)
//! both TBS  = [ version, seal, time, dataHash BIT STRING, propertyInfo, ... ]
//! ```
//!
//! Nothing here re-encodes: the to-be-signed part is handed out as the
//! exact bytes it occupies in the file, because that is what the signature
//! was computed over.

use crate::der::{self, DerNode, TAG_BIT_STRING, TAG_OCTET_STRING, TAG_SEQUENCE};
use crate::error::SignError;

/// One signature value file, read without re-encoding anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SesSignature<'a> {
    /// 1 (GM/T 0031-2014) or 4 (GB/T 38540-2020).
    pub version: u8,
    /// The to-be-signed part exactly as it sits in the file. The SM2
    /// signature is verified against these bytes.
    pub tbs_raw: &'a [u8],
    /// The signer's X.509 certificate.
    pub cert_der: &'a [u8],
    /// The SM2DSA signature over the to-be-signed part, in DER.
    pub signature_der: &'a [u8],
    /// SM3 of the signature description file, as recorded in the TBS.
    pub data_hash: &'a [u8],
    /// The electronic seal embedded in the TBS. It equals the standalone
    /// Seal.esl when the package has one. Empty when there is none.
    pub embedded_seal_raw: &'a [u8],
    /// The signing time as written in the TBS, when that node is a time.
    pub time: Option<String>,
}

fn not_ses(reason: &str) -> SignError {
    SignError::InvalidInput {
        reason: format!("not an SES signature: {reason}"),
    }
}

/// The bytes of a BIT STRING node, without its unused-bit count.
fn bit_string_bytes<'a>(node: Option<&DerNode<'a>>, what: &str) -> Result<&'a [u8], SignError> {
    let node = node.ok_or_else(|| not_ses(what))?;
    if node.tag != TAG_BIT_STRING || node.content.is_empty() {
        return Err(not_ses(what));
    }
    Ok(&node.content[1..])
}

/// The signing time. SES v4 writes a GeneralizedTime; SES v1 wraps an ASCII
/// time such as `2026-11-01 19:05:33` in a BIT STRING.
fn time_text(node: &DerNode<'_>) -> Option<String> {
    let text = if node.tag == TAG_BIT_STRING {
        String::from_utf8(node.content.get(1..)?.to_vec()).ok()
    } else {
        node.as_text().ok()
    }?;
    (!text.is_empty()).then_some(text)
}

/// Read a signature value file. SES v5 and the bare r/s pair of a plain
/// digital signature are not SES v1/v4 and are rejected.
pub fn parse_ses_signature(der: &[u8]) -> Result<SesSignature<'_>, SignError> {
    let outer = der::parse_one(der).map_err(|_| not_ses("malformed DER"))?;
    if outer.tag != TAG_SEQUENCE {
        return Err(not_ses("the root is not a SEQUENCE"));
    }
    let outer_kids = der::parse_children(&outer).map_err(|_| not_ses("malformed DER"))?;
    let tbs = outer_kids
        .first()
        .ok_or_else(|| not_ses("no to-be-signed part"))?;
    let tbs_kids = der::parse_children(tbs).map_err(|_| not_ses("malformed to-be-signed part"))?;
    let version = tbs_kids
        .first()
        .ok_or_else(|| not_ses("no version"))?
        .as_i64()
        .map_err(|_| not_ses("the version is not an integer"))?;
    if version != 1 && version != 4 {
        return Err(SignError::InvalidInput {
            reason: format!("unsupported SES version {version}"),
        });
    }

    // Both versions open the TBS with the version, the seal and the time
    // (see `time_text` for the one difference between them).
    let seal = tbs_kids.get(1).filter(|node| node.tag == TAG_SEQUENCE);
    let (cert, signature) = if version == 4 {
        (outer_kids.get(1), outer_kids.get(3))
    } else {
        (tbs_kids.get(5), outer_kids.get(1))
    };
    let cert_der = cert
        .filter(|node| node.tag == TAG_OCTET_STRING)
        .map(|node| node.content)
        .ok_or_else(|| not_ses("no signer certificate"))?;

    Ok(SesSignature {
        version: version as u8,
        tbs_raw: tbs.raw,
        cert_der,
        signature_der: bit_string_bytes(signature, "no signature value")?,
        data_hash: bit_string_bytes(tbs_kids.get(3), "no data hash")?,
        embedded_seal_raw: seal.map_or(&[], |node| node.raw),
        time: tbs_kids.get(2).and_then(time_text),
    })
}
