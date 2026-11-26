//! The GB/T 35275 signed message: what a plain digital signature
//! (`Type="Sign"`) stores in SignedValue.dat.
//!
//! ```text
//! ContentInfo ::= SEQUENCE { signedData OID, [0] EXPLICIT SignedData }
//! SignedData  ::= SEQUENCE { version INTEGER 1, digestAlgorithms SET { SM3 OID },
//!                            contentInfo SEQUENCE { data OID, [0] EXPLICIT OCTET STRING plaintext },
//!                            [0] IMPLICIT SET { Certificate }, signerInfos SET { SignerInfo } }
//! SignerInfo  ::= SEQUENCE { version INTEGER 1, issuerAndSerialNumber, digestAlgorithm,
//!                            digestEncryptionAlgorithm, encryptedDigest OCTET STRING }
//! ```
//!
//! The `plaintext` is the SM3 digest of the signature description file and
//! the signature is SM2DSA over it. The message carries the signer's
//! certificate, so verifying it needs nothing from outside the package.
//! Layout follows `org.ofdrw.gm.sm2strut` (`SignedDataBuilder`).

use crate::der::{self, DerNode, DerWriter, TAG_INTEGER, TAG_OCTET_STRING, TAG_SEQUENCE, TAG_SET};
use crate::error::SignError;

const OID_CONTENT_SIGNED_DATA: &str = "1.2.156.10197.6.1.4.2.2";
const OID_CONTENT_DATA: &str = "1.2.156.10197.6.1.4.2.1";
const OID_SM3: &str = "1.2.156.10197.1.401";
const OID_SM2_SIGN: &str = "1.2.156.10197.1.301.1";

/// A constructed `[0]` node, and the same byte as a SET tagged `[0]`
/// implicitly: the SET's tag is replaced by the context tag.
const TAG_CONTEXT_0: u8 = 0xA0;

/// One GB/T 35275 signed message, read without re-encoding anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedData<'a> {
    /// The signed digest: SM3 of the signature description file.
    pub plaintext: &'a [u8],
    /// The SM2DSA signature over `plaintext`, DER.
    pub signature: &'a [u8],
    /// The signer's certificate.
    pub cert_der: &'a [u8],
}

fn malformed(reason: &str) -> SignError {
    SignError::InvalidInput {
        reason: format!("not a GB/T 35275 signed message: {reason}"),
    }
}

fn first_child<'a>(node: &DerNode<'a>, what: &str) -> Result<DerNode<'a>, SignError> {
    der::parse_children(node)
        .map_err(|_| malformed("malformed DER"))?
        .into_iter()
        .next()
        .ok_or_else(|| malformed(what))
}

fn algorithm(oid: &str, w: &mut DerWriter) {
    w.sequence(|w| {
        w.oid(oid);
    });
}

/// Build a signed message: `plaintext` signed as `signature` by the owner
/// of `cert_der`.
pub fn build_signed_data(
    plaintext: &[u8],
    signature: &[u8],
    cert_der: &[u8],
) -> Result<Vec<u8>, SignError> {
    if plaintext.is_empty() {
        return Err(SignError::InvalidInput {
            reason: "the signed digest is empty".to_owned(),
        });
    }
    if signature.is_empty() {
        return Err(SignError::Signer {
            reason: "the signature is empty".to_owned(),
        });
    }
    let (issuer, serial) = crate::cert::issuer_and_serial(cert_der)?;

    let mut signer_info = DerWriter::new();
    signer_info.sequence(|w| {
        w.integer(1);
        w.sequence(|w| {
            w.raw(issuer);
            w.raw(serial);
        });
        algorithm(OID_SM3, w);
        algorithm(OID_SM2_SIGN, w);
        w.octet_string(signature);
    });
    let signer_info = signer_info.into_bytes();

    let mut signed_data = DerWriter::new();
    signed_data.sequence(|w| {
        w.integer(1);
        w.set(|w| algorithm(OID_SM3, w));
        w.sequence(|w| {
            w.oid(OID_CONTENT_DATA);
            w.explicit(0, |w| {
                w.octet_string(plaintext);
            });
        });
        w.implicit_set(0, |w| {
            w.raw(cert_der);
        });
        w.set(|w| {
            w.raw(&signer_info);
        });
    });
    let signed_data = signed_data.into_bytes();

    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.oid(OID_CONTENT_SIGNED_DATA);
        w.explicit(0, |w| {
            w.raw(&signed_data);
        });
    });
    Ok(writer.into_bytes())
}

/// Read a signed message.
///
/// The single-signer form rofd-sign writes is what reads back. A message
/// with signed attributes, which would need a different verification path,
/// is rejected.
pub fn parse_signed_data(der: &[u8]) -> Result<SignedData<'_>, SignError> {
    let root = der::parse_one(der).map_err(|_| malformed("malformed DER"))?;
    if root.tag != TAG_SEQUENCE {
        return Err(malformed("the root is not a SEQUENCE"));
    }
    let kids = der::parse_children(&root).map_err(|_| malformed("malformed DER"))?;
    let [content_type, content, ..] = kids.as_slice() else {
        return Err(malformed("no content"));
    };
    if content_type
        .as_oid()
        .map_err(|_| malformed("no content type"))?
        != OID_CONTENT_SIGNED_DATA
    {
        return Err(malformed("the content type is not signedData"));
    }
    if content.tag != TAG_CONTEXT_0 {
        return Err(malformed("the content is not [0]-tagged"));
    }
    let signed_data =
        der::parse_one(content.content).map_err(|_| malformed("malformed SignedData"))?;
    let sd_kids =
        der::parse_children(&signed_data).map_err(|_| malformed("malformed SignedData"))?;
    let [version, _digest_algorithms, content_info, rest @ ..] = sd_kids.as_slice() else {
        return Err(malformed("SignedData is too short"));
    };
    if version.tag != TAG_INTEGER || version.as_i64().map_err(|_| malformed("bad version"))? != 1 {
        return Err(malformed("the version is not 1"));
    }
    let ci_kids =
        der::parse_children(content_info).map_err(|_| malformed("malformed contentInfo"))?;
    let [data_type, data, ..] = ci_kids.as_slice() else {
        return Err(malformed("contentInfo is too short"));
    };
    if data_type
        .as_oid()
        .map_err(|_| malformed("no content type"))?
        != OID_CONTENT_DATA
    {
        return Err(malformed("the inner content type is not data"));
    }
    if data.tag != TAG_CONTEXT_0 {
        return Err(malformed("the inner content is not [0]-tagged"));
    }
    // An EXPLICIT [0] holds the one node itself.
    let plaintext = der::parse_one(data.content).map_err(|_| malformed("no plaintext"))?;
    if plaintext.tag != TAG_OCTET_STRING {
        return Err(malformed("the plaintext is not an OCTET STRING"));
    }

    // After contentInfo: an optional [0] certificates set, then the SET of
    // signer infos.
    let mut cert_der: Option<&[u8]> = None;
    let mut signer_info: Option<DerNode<'_>> = None;
    for node in rest {
        if node.tag == TAG_CONTEXT_0 {
            // An IMPLICIT [0] replaces the SET's tag: the content is the
            // certificate itself (for the single-certificate form).
            let certificate =
                der::parse_one(node.content).map_err(|_| malformed("no certificate"))?;
            cert_der = Some(certificate.raw);
        } else if node.tag == TAG_SET {
            signer_info = Some(first_child(node, "no signer info")?);
        }
    }
    let cert_der = cert_der.ok_or_else(|| malformed("no certificate in the message"))?;
    let signer_info = signer_info.ok_or_else(|| malformed("no signer info"))?;
    let si_kids =
        der::parse_children(&signer_info).map_err(|_| malformed("malformed SignerInfo"))?;
    let [_version, _issuer_and_serial, _digest_algorithm, encryption @ ..] = si_kids.as_slice()
    else {
        return Err(malformed("SignerInfo is too short"));
    };
    if encryption
        .first()
        .is_some_and(|node| node.tag == TAG_CONTEXT_0)
    {
        return Err(malformed("signed attributes are not supported"));
    }
    let [_encryption_algorithm, digest, ..] = encryption else {
        return Err(malformed("SignerInfo has no signature"));
    };
    if digest.tag != TAG_OCTET_STRING {
        return Err(malformed("the signature is not an OCTET STRING"));
    }
    Ok(SignedData {
        plaintext: plaintext.content,
        signature: digest.content,
        cert_der,
    })
}
