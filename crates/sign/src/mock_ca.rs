//! A test certificate authority: an SM2 key pair and a self-signed X.509
//! certificate, both derived from a seed.
//!
//! For tests, examples and manual trials only. Never a production signing
//! path: the key is derived from a seed anyone can read.

use sm2::dsa::signature::Signer as _;

use crate::crypto::{sm2_sign, sm3, DEFAULT_SM2_USER_ID};
use crate::der::DerWriter;
use crate::error::SignError;
use crate::signer::Signer;

/// `sm2sign-with-sm3` (GM/T 0006).
const OID_SM2_WITH_SM3: &str = "1.2.156.10197.1.501";
/// `id-ecPublicKey`.
const OID_EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
/// The SM2 curve, `sm2p256v1`.
const OID_SM2_CURVE: &str = "1.2.156.10197.1.301";
/// `id-at-commonName`.
const OID_COMMON_NAME: &str = "2.5.4.3";
/// The validity every test certificate gets. Written as GeneralizedTime
/// (`YYYYMMDDHHMMSSZ`); fixed, because the library never reads the clock.
const VALID_FROM: &str = "20200101000000Z";
const VALID_UNTIL: &str = "20991231235959Z";

/// A test certificate authority: one key pair and its self-signed
/// certificate.
#[derive(Clone)]
pub struct TestCa {
    /// The self-signed X.509 certificate, DER.
    pub cert_der: Vec<u8>,
    signing: sm2::dsa::SigningKey,
}

impl TestCa {
    /// Derive a key pair and a self-signed certificate from `seed`. The
    /// same seed and name always give the same bytes.
    pub fn from_seed(common_name: &str, seed: &[u8]) -> Self {
        let secret = secret_from_seed(seed);
        let user_id =
            std::str::from_utf8(DEFAULT_SM2_USER_ID).expect("the default user id is ASCII");
        let signing = sm2::dsa::SigningKey::new(user_id, &secret).expect("a valid SM2 key");
        let public_key = signing.verifying_key().to_sec1_bytes();
        let tbs = tbs_certificate(common_name, &public_key);
        let signature =
            sm2_sign(&secret, DEFAULT_SM2_USER_ID, &tbs).expect("signing the certificate");
        let mut writer = DerWriter::new();
        // Certificate ::= SEQUENCE { tbsCertificate, algorithm, signature }.
        writer.sequence(|w| {
            w.raw(&tbs);
            algorithm(w);
            w.bit_string(&signature);
        });
        Self {
            cert_der: writer.into_bytes(),
            signing,
        }
    }

    /// A signer holding this authority's key and certificate.
    pub fn signer(&self) -> TestSigner {
        TestSigner {
            signing: self.signing.clone(),
            cert_der: self.cert_der.clone(),
        }
    }
}

/// The private key for a seed: SM3 of the seed, re-hashed on the rare
/// occasion the result is not a valid scalar.
fn secret_from_seed(seed: &[u8]) -> sm2::SecretKey {
    let mut material = sm3(seed);
    loop {
        if let Ok(secret) = sm2::SecretKey::from_slice(&material) {
            return secret;
        }
        material = sm3(&material);
    }
}

/// A positive serial number derived from the public key.
fn serial_number(public_key: &[u8]) -> i64 {
    let digest = sm3(public_key);
    i64::from_be_bytes(digest[..8].try_into().expect("eight bytes")) & 0x7FFF_FFFF_FFFF_FFFF
}

/// AlgorithmIdentifier for SM2withSM3.
fn algorithm(w: &mut DerWriter) {
    w.sequence(|w| {
        w.oid(OID_SM2_WITH_SM3);
    });
}

/// A Name with a single attribute: the common name.
fn name(common_name: &str, w: &mut DerWriter) {
    w.sequence(|w| {
        w.set(|w| {
            w.sequence(|w| {
                w.oid(OID_COMMON_NAME);
                w.utf8_string(common_name);
            });
        });
    });
}

/// TBSCertificate: version v3, serial, algorithm, issuer, validity, subject,
/// and the SM2 public key in an EC SubjectPublicKeyInfo.
fn tbs_certificate(common_name: &str, public_key: &[u8]) -> Vec<u8> {
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.explicit(0, |w| {
            w.integer(2);
        });
        w.integer(serial_number(public_key));
        algorithm(w);
        name(common_name, w);
        w.sequence(|w| {
            w.generalized_time(VALID_FROM);
            w.generalized_time(VALID_UNTIL);
        });
        name(common_name, w);
        w.sequence(|w| {
            w.sequence(|w| {
                w.oid(OID_EC_PUBLIC_KEY);
                w.oid(OID_SM2_CURVE);
            });
            w.bit_string(public_key);
        });
    });
    writer.into_bytes()
}

/// A [`Signer`] backed by a [`TestCa`].
pub struct TestSigner {
    signing: sm2::dsa::SigningKey,
    cert_der: Vec<u8>,
}

impl Signer for TestSigner {
    fn certificate_der(&self) -> &[u8] {
        &self.cert_der
    }

    fn sign(&mut self, to_sign: &[u8]) -> Result<Vec<u8>, SignError> {
        self.signing
            .try_sign(to_sign)
            .map(|signature: sm2::dsa::Signature| signature.to_der().to_vec())
            .map_err(|_| SignError::Signer {
                reason: "the test signer could not sign".to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cert::parse_certificate;
    use crate::crypto::{sm2_verify, DEFAULT_SM2_USER_ID};

    #[test]
    fn test_ca_certificate_parses_back() {
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let certificate = parse_certificate(&ca.cert_der).expect("the certificate parses");
        assert_eq!(certificate.subject_cn, "rofd test signer");
        assert_eq!(certificate.issuer_cn, "rofd test signer", "self-signed");
        assert_eq!(certificate.public_key_sec1.len(), 65);
        assert_eq!(certificate.public_key_sec1[0], 4);
        assert_eq!(certificate.not_before.as_deref(), Some(VALID_FROM));
        assert_eq!(certificate.not_after.as_deref(), Some(VALID_UNTIL));
        assert!(!certificate.serial_hex.is_empty());
        assert_ne!(certificate.serial_hex, "0", "a positive serial number");
    }

    #[test]
    fn test_ca_certificate_signature_verifies() {
        // Certificate ::= SEQUENCE { tbsCertificate, algorithm, signature }.
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let root = crate::der::parse_one(&ca.cert_der).unwrap();
        let kids = crate::der::parse_children(&root).unwrap();
        let certificate = parse_certificate(&ca.cert_der).unwrap();
        let signature = kids[2].as_bit_string().unwrap();
        assert!(
            sm2_verify(
                &certificate.public_key_sec1,
                DEFAULT_SM2_USER_ID,
                kids[0].raw,
                &signature
            ),
            "the certificate's own signature verifies over its to-be-signed bytes"
        );
    }

    #[test]
    fn test_ca_signer_round_trips() {
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let certificate = parse_certificate(&ca.cert_der).unwrap();
        let mut signer = ca.signer();
        assert_eq!(signer.certificate_der(), ca.cert_der.as_slice());
        let signature = signer.sign(b"to be signed").expect("sign");
        assert_eq!(signature[0], 0x30, "DER");
        assert!(sm2_verify(
            &certificate.public_key_sec1,
            DEFAULT_SM2_USER_ID,
            b"to be signed",
            &signature
        ));
        assert!(!sm2_verify(
            &certificate.public_key_sec1,
            DEFAULT_SM2_USER_ID,
            b"something else",
            &signature
        ));
    }

    #[test]
    fn test_ca_is_deterministic_for_a_seed() {
        let first = TestCa::from_seed("rofd test signer", b"seed-a");
        let second = TestCa::from_seed("rofd test signer", b"seed-a");
        assert_eq!(first.cert_der, second.cert_der);
        assert_eq!(
            first.signer().sign(b"m").unwrap(),
            second.signer().sign(b"m").unwrap(),
            "same key, same message, same signature"
        );
    }

    #[test]
    fn different_seeds_give_different_certificates() {
        let a = TestCa::from_seed("rofd test signer", b"seed-a");
        let b = TestCa::from_seed("rofd test signer", b"seed-b");
        assert_ne!(a.cert_der, b.cert_der);
        let b_certificate = parse_certificate(&b.cert_der).unwrap();
        let signature = a.signer().sign(b"m").unwrap();
        assert!(!sm2_verify(
            &b_certificate.public_key_sec1,
            DEFAULT_SM2_USER_ID,
            b"m",
            &signature
        ));
    }
}
