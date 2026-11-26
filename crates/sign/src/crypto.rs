//! SM2 and SM3 behind rofd-sign's own names (spec 2026-11-23 §10).
//!
//! The primitives come from RustCrypto (`sm2`, `sm3`); this module fixes how
//! they are used so that every caller agrees: SM3 over the bytes as they
//! are, and SM2DSA over a message with an explicit distinguishing identifier
//! (`ZA || M`), signatures in DER.

use ::sm3::{Digest, Sm3};

use sm2::dsa::signature::{Signer, Verifier};

use crate::error::SignError;

/// The SM2 distinguishing identifier every seal in this line uses
/// (`1234567812345678`, the default of GM/T 0009).
pub const DEFAULT_SM2_USER_ID: &[u8] = b"1234567812345678";

/// The SM3 digest of `bytes`.
pub fn sm3(bytes: &[u8]) -> [u8; 32] {
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&Sm3::digest(bytes));
    digest
}

fn user_id(user_id: &[u8]) -> Result<&str, SignError> {
    std::str::from_utf8(user_id).map_err(|_| SignError::InvalidInput {
        reason: "the SM2 user id must be UTF-8".to_owned(),
    })
}

/// Whether `signature_der` is the SM2DSA signature of `message` made by the
/// owner of `public_key_sec1` (an uncompressed point, `04 || X || Y`) under
/// `user_id`. Anything that cannot be parsed counts as `false`.
pub fn sm2_verify(
    public_key_sec1: &[u8],
    user_id: &[u8],
    message: &[u8],
    signature_der: &[u8],
) -> bool {
    let Ok(distid) = std::str::from_utf8(user_id) else {
        return false;
    };
    let Ok(verifying_key) = sm2::dsa::VerifyingKey::from_sec1_bytes(distid, public_key_sec1) else {
        return false;
    };
    let Ok(signature) = sm2::dsa::Signature::from_der(signature_der) else {
        return false;
    };
    verifying_key.verify(message, &signature).is_ok()
}

/// Sign `message` with SM2DSA under `user_id`. The signature is DER and the
/// nonces are deterministic (RFC 6979): the same key and message always give
/// the same bytes.
pub fn sm2_sign(
    secret: &sm2::SecretKey,
    user_id: &[u8],
    message: &[u8],
) -> Result<Vec<u8>, SignError> {
    let distid = self::user_id(user_id)?;
    let signing_key = sm2::dsa::SigningKey::new(distid, secret).map_err(|_| SignError::Crypto {
        reason: "the SM2 private key cannot be used".to_owned(),
    })?;
    let signature = signing_key
        .try_sign(message)
        .map_err(|_| SignError::Crypto {
            reason: "SM2 signing failed".to_owned(),
        })?;
    Ok(signature.to_der().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm2::elliptic_curve::Generate;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn sm3_standard_vectors() {
        // GB/T 32905-2016, appendix A (cross-checked with OpenSSL).
        assert_eq!(
            hex(&sm3(b"abc")),
            "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"
        );
        let sixteen = b"abcd".repeat(16);
        assert_eq!(
            hex(&sm3(&sixteen)),
            "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732"
        );
        let sixty_four = b"abcd".repeat(64);
        assert_eq!(
            hex(&sm3(&sixty_four)),
            "b965764c8bebb091c7602b74afd34eefb531dccb4e0076d9b7cd813199b45971"
        );
    }

    fn key_pair() -> (sm2::SecretKey, Vec<u8>) {
        let secret = sm2::SecretKey::generate();
        let signing = sm2::dsa::SigningKey::new("1234567812345678", &secret).unwrap();
        (secret, signing.verifying_key().to_sec1_bytes().to_vec())
    }

    #[test]
    fn sm2_sign_verify_round_trip() {
        let (secret, public) = key_pair();
        let message = b"message to sign";
        let signature = sm2_sign(&secret, DEFAULT_SM2_USER_ID, message).expect("sign");
        assert_eq!(signature[0], 0x30, "the signature is a DER SEQUENCE");
        assert!(
            sm2_verify(&public, DEFAULT_SM2_USER_ID, message, &signature),
            "own signature verifies"
        );
        let mut tampered = message.to_vec();
        tampered[0] ^= 1;
        assert!(!sm2_verify(
            &public,
            DEFAULT_SM2_USER_ID,
            &tampered,
            &signature
        ));
        assert!(
            !sm2_verify(&public, b"another user id!!", message, &signature),
            "a different user id changes ZA"
        );
        assert!(!sm2_verify(
            &public,
            DEFAULT_SM2_USER_ID,
            message,
            &signature[..signature.len() - 1]
        ));
        assert!(!sm2_verify(&public, DEFAULT_SM2_USER_ID, message, &[]));
        assert!(!sm2_verify(
            &[0u8; 65],
            DEFAULT_SM2_USER_ID,
            message,
            &signature
        ));
        assert!(
            !sm2_verify(&public, &[0xff], message, &signature),
            "user id must be UTF-8"
        );
        assert!(
            sm2_sign(&secret, &[0xff], message).is_err(),
            "user id must be UTF-8"
        );
    }

    #[test]
    fn sm2_signing_is_deterministic() {
        let (secret, _) = key_pair();
        let first = sm2_sign(&secret, DEFAULT_SM2_USER_ID, b"same message").unwrap();
        let second = sm2_sign(&secret, DEFAULT_SM2_USER_ID, b"same message").unwrap();
        assert_eq!(first, second, "RFC 6979 nonces: same input, same signature");
    }
}
