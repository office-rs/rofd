//! Certificate files (PKCS#12, `.pfx` and `.p12`): the form a user's
//! certificate arrives in.
//!
//! Opening a file needs its password. The private key then lives in memory for
//! as long as the signer does: the key type erases itself on drop, and every
//! byte buffer this module owns is erased too. Nothing is written anywhere.
//!
//! `keystore-pkcs12` reads the file - the modern layout (AES, PBES2) as well
//! as the older one (3DES and RC2-40, PBES1) - and confirms the password
//! against the file's integrity check. Files written as BER with indefinite
//! lengths, which several Chinese CA tools still produce, are first rewritten
//! as DER; their integrity check covers the original bytes, so it is
//! evaluated separately. The key is never trusted because of what the file
//! says about its algorithm: it is used to derive a public key, which must
//! equal the one in the certificate. That one check rules out every other key
//! type and confirms that key and certificate belong together.
//!
//! The bags are read straight from the parsed file: nothing here goes
//! through a keystore, which would stamp its entries with the wall clock -
//! a clock wasm32-unknown-unknown does not have, and one this library never
//! reads.

use keystore_core::{KeyStoreFormat as _, Password};
use keystore_pkcs12::{CertType, Pkcs12Store, SafeBag};
use sm2::dsa::signature::Signer as _;
use zeroize::Zeroizing;

use crate::cert::{parse_certificate, CertificateInfo};
use crate::crypto::DEFAULT_SM2_USER_ID;
use crate::der::{self, TAG_OCTET_STRING, TAG_SEQUENCE};
use crate::error::SignError;
use crate::signer::Signer;

/// A certificate file opened with its password: the certificate and the
/// private key that goes with it.
pub struct PfxSigner {
    certificate_der: Vec<u8>,
    secret: sm2::SecretKey,
}

fn certificate_error(reason: impl Into<String>) -> SignError {
    SignError::Certificate {
        reason: reason.into(),
    }
}

/// Why a certificate file could not be opened.
enum OpenError {
    /// The password is wrong: the file itself said so, through its
    /// integrity check.
    WrongPassword(String),
    /// The file cannot be read as a certificate file.
    Unreadable(String),
}

/// A certificate file opened: the decrypted key (PKCS#8), every X.509
/// certificate it carries in file order, and whether the password was
/// confirmed by the file's integrity check.
struct Opened {
    pkcs8: Zeroizing<Vec<u8>>,
    certificates: Vec<Vec<u8>>,
    integrity_checked: bool,
}

/// Read a certificate file: the modern reader first; a file it cannot parse
/// goes to the older reader. A password the first reader rejected stays
/// rejected.
fn open_file(bytes: &[u8], password: &str) -> Result<Opened, OpenError> {
    read_modern(bytes, password).or_else(|error| match error {
        OpenError::WrongPassword(reason) => Err(OpenError::WrongPassword(reason)),
        OpenError::Unreadable(reason) => read_older(bytes, password, &reason),
    })
}

fn read_modern(bytes: &[u8], password: &str) -> Result<Opened, OpenError> {
    use keystore_core::Error as KeystoreError;
    let parsed =
        Pkcs12Store::parse(bytes, &Password::new(password)).map_err(|error| match error {
            KeystoreError::IntegrityFailure => OpenError::WrongPassword(
                "wrong password (the file's integrity check failed)".to_owned(),
            ),
            KeystoreError::UnsupportedAlgorithm(algorithm) => OpenError::Unreadable(format!(
                "the file uses a protection algorithm that is not supported ({algorithm})"
            )),
            other => OpenError::Unreadable(format!(
                "not a certificate file, or not the layout expected ({other})"
            )),
        })?;
    // The bags are taken straight from the parsed file. Going through a
    // keystore would stamp every entry with the wall clock, which
    // wasm32-unknown-unknown has none of - and which this library never reads.
    let mut keys = Vec::new();
    let mut certificates = Vec::new();
    for bag in parsed.bags() {
        match bag {
            SafeBag::Key(key) => keys.push(Zeroizing::new(key.key.0.clone())),
            // Only x509Certificate bags hold certificates.
            SafeBag::Cert(cert) if cert.cert_type == CertType::X509 => {
                certificates.push(cert.cert.0.clone());
            }
            _ => {}
        }
    }
    let pkcs8 = match keys.as_slice() {
        [pkcs8] => pkcs8.clone(),
        [] => {
            return Err(OpenError::Unreadable(
                "the file holds no private key".to_owned(),
            ))
        }
        _ => {
            return Err(OpenError::Unreadable(
                "the file holds more than one private key; import one certificate at a time"
                    .to_owned(),
            ))
        }
    };
    // The parsed file holds the decrypted key as plain bytes: it is no
    // longer needed.
    drop(parsed);
    drop(keys);
    Ok(Opened {
        pkcs8,
        certificates,
        integrity_checked: true,
    })
}

/// A file the DER reader could not parse may be written as BER: rewrite its
/// encoding and read it again. `first_reason` says why the first reading
/// failed.
fn read_older(bytes: &[u8], password: &str, first_reason: &str) -> Result<Opened, OpenError> {
    let unreadable = |reason: String| OpenError::Unreadable(format!("{first_reason}; {reason}"));
    let Some(rewritten) = crate::ber::pkcs12_to_der(bytes) else {
        return Err(unreadable(
            "not a certificate file in a known encoding".to_owned(),
        ));
    };
    // The integrity check covers the file's original encoding, so the reader
    // cannot redo it on the rewritten file: confirm the password against the
    // original bytes here.
    let mut integrity_checked = false;
    if let Some(mac) = rewritten.mac.as_deref() {
        match integrity_check_holds(mac, &rewritten.auth_safe_original, password) {
            Some(true) => integrity_checked = true,
            Some(false) => {
                return Err(OpenError::WrongPassword(
                    "wrong password (the file's integrity check failed)".to_owned(),
                ))
            }
            // An integrity check this line cannot evaluate is skipped.
            None => {}
        }
    }
    let mut opened = read_modern(&rewritten.der, password).map_err(|error| match error {
        // Unreachable: the integrity-check data was taken out of the file.
        OpenError::WrongPassword(reason) => unreadable(reason),
        OpenError::Unreadable(reason) => unreadable(reason),
    })?;
    opened.integrity_checked = integrity_checked;
    Ok(opened)
}

/// Whether a PKCS#12 integrity check holds for `data` and `password`.
/// `None` when the check uses an algorithm this line does not evaluate.
fn integrity_check_holds(mac: &[u8], data: &[u8], password: &str) -> Option<bool> {
    // MacData ::= SEQUENCE { mac DigestInfo, salt OCTET STRING, iterations INTEGER }.
    let mac_data = crate::der::parse_one(mac).ok()?;
    let kids = crate::der::parse_children(&mac_data).ok()?;
    let digest_info = crate::der::parse_children(kids.first()?).ok()?;
    let algorithm = crate::der::parse_children(digest_info.first()?).ok()?;
    // Only HMAC-SHA1 is evaluated; it is what these files use.
    if !algorithm
        .first()?
        .as_oid()
        .is_ok_and(|oid| oid == "1.3.14.3.2.26")
    {
        return None;
    }
    let check = yasna::parse_der(mac, p12::MacData::parse).ok()?;
    Some(check.verify_mac(data, &bmp_string(password)))
}

/// A PKCS#12 password: UTF-16BE with a trailing NUL unit.
fn bmp_string(password: &str) -> Vec<u8> {
    password
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_be_bytes)
        .collect()
}

impl PfxSigner {
    /// Open a certificate file with its password. The file must hold one
    /// private key with its certificate.
    pub fn open(pfx_bytes: &[u8], password: &str) -> Result<Self, SignError> {
        let Opened {
            pkcs8,
            certificates,
            integrity_checked,
        } = open_file(pfx_bytes, password).map_err(|error| match error {
            // Only the file's own verdict is a wrong password (spec section
            // 16); anything else is a file that cannot be used.
            OpenError::WrongPassword(reason) => SignError::Password { reason },
            OpenError::Unreadable(reason) => certificate_error(reason),
        })?;
        let scalar = sm2_scalar(&pkcs8)?;
        let secret = sm2::SecretKey::from_slice(&scalar)
            .map_err(|_| certificate_error("the key in the file is not an SM2 key"))?;
        let derived = public_key_sec1(&secret)?;
        drop(scalar);
        // The certificate that goes with the key is the one whose public key
        // the key derives. A file with no such certificate goes on with its
        // first one, and is refused below as not the key's certificate.
        let owns_key = |der: &[u8]| {
            parse_certificate(der).is_ok_and(|certificate| certificate.public_key_sec1 == derived)
        };
        let certificate_der = certificates
            .iter()
            .position(|der| owns_key(der))
            .or_else(|| (!certificates.is_empty()).then_some(0))
            .map(|index| certificates[index].clone())
            .ok_or_else(|| certificate_error("the file's key has no certificate"))?;
        let certificate = parse_certificate(&certificate_der).map_err(|error| {
            certificate_error(format!("the certificate cannot be read: {error}"))
        })?;
        if derived != certificate.public_key_sec1 {
            return Err(certificate_error(if integrity_checked {
                "the key in the file is not the SM2 private key of its certificate"
            } else {
                "wrong password, or the key in the file is not the SM2 private key of its certificate"
            }));
        }
        Ok(Self {
            certificate_der,
            secret,
        })
    }

    /// The signer's certificate.
    pub fn certificate(&self) -> Result<CertificateInfo, SignError> {
        parse_certificate(&self.certificate_der)
            .map_err(|error| certificate_error(format!("the certificate cannot be read: {error}")))
    }

    fn signing_key(&self) -> Result<sm2::dsa::SigningKey, SignError> {
        let user_id =
            std::str::from_utf8(DEFAULT_SM2_USER_ID).expect("the default user id is ASCII");
        sm2::dsa::SigningKey::new(user_id, &self.secret).map_err(|_| SignError::Crypto {
            reason: "the SM2 private key cannot be used".to_owned(),
        })
    }
}

impl std::fmt::Debug for PfxSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("PfxSigner")
            .field("certificate", &self.certificate())
            .finish_non_exhaustive()
    }
}

impl Signer for PfxSigner {
    fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }

    fn sign(&mut self, to_sign: &[u8]) -> Result<Vec<u8>, SignError> {
        self.signing_key()?
            .try_sign(to_sign)
            .map(|signature: sm2::dsa::Signature| signature.to_der().to_vec())
            .map_err(|_| SignError::Signer {
                reason: "SM2 signing failed".to_owned(),
            })
    }
}

/// The 32-byte private scalar of an EC private key given as PKCS#8
/// `PrivateKeyInfo` (the decrypted content of the file's key bag).
fn sm2_scalar(pkcs8: &[u8]) -> Result<Zeroizing<Vec<u8>>, SignError> {
    let malformed = || certificate_error("the key in the file is not an SM2 key");
    let info = der::parse_one(pkcs8).map_err(|_| malformed())?;
    let info_kids = der::parse_children(&info).map_err(|_| malformed())?;
    let Some(private_key) = info_kids.get(2) else {
        return Err(malformed());
    };
    if private_key.tag != TAG_OCTET_STRING {
        return Err(malformed());
    }
    // ECPrivateKey ::= SEQUENCE { version INTEGER 1, privateKey OCTET STRING, ... }.
    let ec = der::parse_one(private_key.content).map_err(|_| malformed())?;
    if ec.tag != TAG_SEQUENCE {
        return Err(malformed());
    }
    let ec_kids = der::parse_children(&ec).map_err(|_| malformed())?;
    let Some(scalar) = ec_kids.get(1) else {
        return Err(malformed());
    };
    if scalar.tag != TAG_OCTET_STRING || scalar.content.is_empty() {
        return Err(malformed());
    }
    Ok(Zeroizing::new(scalar.content.to_vec()))
}

/// The public key that goes with `secret`: the uncompressed point
/// `04 || X || Y`.
fn public_key_sec1(secret: &sm2::SecretKey) -> Result<Vec<u8>, SignError> {
    let user_id = std::str::from_utf8(DEFAULT_SM2_USER_ID).expect("the default user id is ASCII");
    let signing = sm2::dsa::SigningKey::new(user_id, secret).map_err(|_| SignError::Crypto {
        reason: "the SM2 private key cannot be used".to_owned(),
    })?;
    Ok(signing.verifying_key().to_sec1_bytes().to_vec())
}
