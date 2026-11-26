//! The verification report (spec 2026-11-23 §9.1).
//!
//! Verification never fails, it reports: every signature of a package gets
//! one [`SignatureReport`], whatever state its files are in. The types
//! serialize to JSON for the wasm SDK, with snake_case field names.

use serde::{Deserialize, Serialize};

/// What kind of signature value file a signature has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureVariant {
    /// GM/T 0031-2014 seal signature.
    SesV1,
    /// GB/T 38540-2020 seal signature, the mainstream format.
    SesV4,
    /// SES v5: recognised, not supported.
    SesV5,
    /// A GB/T 35275 signed message.
    Gbt35275,
    /// A bare SM2 r/s pair (`Type="Sign"`); its certificate travels out of
    /// band.
    Digital,
    /// None of the above.
    Unknown,
}

/// The verdict on one signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureOverall {
    /// Every check that could run passed.
    Valid,
    /// A check that ran failed: a protected file changed, the seal does not
    /// match, or the signature does not verify.
    Invalid,
    /// Nothing failed, but a check could not run for lack of material (for
    /// example a digital signature without its certificate).
    Unverified,
    /// The signature's own files are missing or cannot be read.
    Unparseable,
    /// The format or algorithm is recognised but not supported.
    Unsupported,
}

/// One protected file whose digest does not match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityFailure {
    /// The `FileRef` as written in the signature.
    pub file_ref: String,
    /// The digest recorded in the signature, base64.
    pub expected: String,
    /// The digest computed from the package, base64; empty when the file
    /// is missing.
    pub actual: String,
}

/// Layer 1: do the protected files still hash to the recorded values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityResult {
    /// False when a protected file changed or is missing, and when the
    /// signature names no file at all.
    pub ok: bool,
    pub failures: Vec<IntegrityFailure>,
    /// How many files the reference list names. A signature that names none
    /// covers no document content, whatever else verifies (GB/T 33190 takes
    /// the list as one or more).
    pub protected_files: usize,
}

/// Layer 3, the state of the signature value check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureValueStatus {
    Ok,
    Fail,
    Unverifiable,
}

/// Layer 3: does the signature verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureValue {
    pub status: SignatureValueStatus,
    /// Why the status is what it is, when it is not `Ok`.
    pub reason: Option<String>,
}

/// What the signature says about its certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateReport {
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    /// Whether the certificate was valid at the supplied time. `None`
    /// when no time was supplied or one of them cannot be read. This never
    /// changes the verdict.
    pub valid_at: Option<bool>,
    /// Always `false`: the certificate chain is never validated here.
    pub chain_validated: bool,
}

/// The verdict on one signature of a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureReport {
    pub id: String,
    pub kind: rofd_dom::SignatureKind,
    pub variant: SignatureVariant,
    pub overall: SignatureOverall,
    pub integrity: IntegrityResult,
    /// Layer 2: whether the standalone seal file equals the seal embedded in
    /// the signature value. `None` when there is no standalone seal to
    /// compare, or the signature value could not be read.
    pub seal_match: Option<bool>,
    pub signature_value: SignatureValue,
    /// `None` when no certificate is available: a digital signature whose
    /// certificate was not supplied.
    pub certificate: Option<CertificateReport>,
    /// `ProviderName` of the signature.
    pub provider: Option<String>,
    /// `SignatureDateTime` as written.
    pub date_time: Option<String>,
}

/// What the caller supplies to verification.
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    /// The time to judge certificate validity against, `YYYY-MM-DD HH:mm:ss`.
    /// The library never reads the clock.
    pub now: Option<String>,
    /// Certificates that are not in the package, as `(signature id, DER)`.
    /// They are used only for signatures that carry no certificate of their
    /// own; an embedded certificate is part of the signing fact and is
    /// never overridden.
    pub certs: Vec<(String, Vec<u8>)>,
}
