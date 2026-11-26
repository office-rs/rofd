//! Where the private-key operation comes from (spec 2026-11-23 §10.4).
//!
//! The library never holds a key. The host hands in a signer: something
//! that owns a certificate and can sign with the matching private key. A
//! certificate file the user imported, a vendor library, a hardware token
//! or a remote service all look the same from here.

use crate::error::SignError;

/// A signature capability: a certificate plus the private key that goes
/// with it.
pub trait Signer {
    /// The signer's X.509 certificate, DER. It is embedded in what gets
    /// signed.
    fn certificate_der(&self) -> &[u8];
    /// Sign `to_sign` with SM2DSA (SM3, the default user id) and return the
    /// signature in DER.
    fn sign(&mut self, to_sign: &[u8]) -> Result<Vec<u8>, SignError>;
}
