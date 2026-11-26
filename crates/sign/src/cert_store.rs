//! The certificate store (spec 2026-11-23 §10.5): the certificates a user
//! imported, kept in memory for the host to choose from.
//!
//! The store holds each certificate file exactly as imported - still
//! encrypted - together with what is needed to show it in a list. It never
//! holds a password or key material, so the host can persist it anywhere.
//! Signing asks for the password again and opens the file afresh.

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::crypto::sm3;
use crate::error::SignError;
use crate::pfx::PfxSigner;
use crate::signer::Signer;

/// A certificate's identity: the SM3 of its DER, as lower-case hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CertificateId(pub String);

/// What is shown for an imported certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateEntry {
    pub id: CertificateId,
    /// A name for lists. The subject's `CN` by default.
    pub label: String,
    pub subject_cn: String,
    pub issuer_cn: String,
    /// The serial number as upper-case hex.
    pub serial: String,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
}

/// One certificate as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    entry: CertificateEntry,
    /// The certificate file, byte for byte as imported (still encrypted).
    #[serde(with = "base64_bytes")]
    file: Vec<u8>,
}

/// Serialize a byte field as base64.
mod base64_bytes {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

/// The certificates a user imported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CertificateStore {
    entries: Vec<Stored>,
}

impl CertificateStore {
    /// Import a certificate file, opening it once with `password` to read
    /// what to show for it. Importing the same certificate again replaces
    /// its entry.
    pub fn import(&mut self, file: Vec<u8>, password: &str) -> Result<CertificateEntry, SignError> {
        let signer = PfxSigner::open(&file, password)?;
        let certificate = signer.certificate()?;
        let id: String = sm3(signer.certificate_der())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let label = if certificate.subject_cn.trim().is_empty() {
            "certificate".to_owned()
        } else {
            certificate.subject_cn.trim().to_owned()
        };
        let entry = CertificateEntry {
            id: CertificateId(id),
            label,
            subject_cn: certificate.subject_cn,
            issuer_cn: certificate.issuer_cn,
            serial: certificate.serial_hex,
            not_before: certificate.not_before,
            not_after: certificate.not_after,
        };
        self.entries.retain(|stored| stored.entry.id != entry.id);
        self.entries.push(Stored {
            entry: entry.clone(),
            file,
        });
        self.entries.sort_by(|a, b| {
            a.entry
                .label
                .to_lowercase()
                .cmp(&b.entry.label.to_lowercase())
        });
        Ok(entry)
    }

    /// The imported certificates, by label.
    pub fn list(&self) -> Vec<CertificateEntry> {
        self.entries
            .iter()
            .map(|stored| stored.entry.clone())
            .collect()
    }

    /// Forget a certificate.
    pub fn remove(&mut self, id: &CertificateId) {
        self.entries.retain(|stored| &stored.entry.id != id);
    }

    /// Open an imported certificate's key with its password, for signing.
    pub fn signer(&self, id: &CertificateId, password: &str) -> Result<PfxSigner, SignError> {
        let stored = self
            .entries
            .iter()
            .find(|stored| &stored.entry.id == id)
            .ok_or_else(|| SignError::Certificate {
                reason: "no imported certificate has this id".to_owned(),
            })?;
        PfxSigner::open(&stored.file, password)
    }

    /// An imported certificate's file, as the store keeps it: byte for byte
    /// as imported, still encrypted. `None` for an unknown id.
    pub fn certificate_file(&self, id: &CertificateId) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|stored| &stored.entry.id == id)
            .map(|stored| stored.file.as_slice())
    }

    /// The store as JSON: display data and the (still encrypted) files.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SignError> {
        serde_json::to_vec(&self.entries).map_err(|error| SignError::Encode {
            reason: format!("the certificate list cannot be saved: {error}"),
        })
    }

    /// Read a store written by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SignError> {
        let unreadable = |error: serde_json::Error| SignError::Certificate {
            reason: format!("the saved certificate list cannot be read: {error}"),
        };
        let entries: Vec<Stored> = serde_json::from_slice(bytes).map_err(unreadable)?;
        Ok(Self { entries })
    }
}
