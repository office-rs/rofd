//! rofd-sign: electronic-seal and signature verification and creation for
//! OFD packages (GB/T 33190 chapter 18; spec 2026-11-23).
//!
//! Two doors. A signature goes into the package a document carries, in
//! place ([`in_place::sign_in_place`]; [`in_place::verify_document`]
//! verifies that package without a save) - this is what the editor
//! component does for a host. And bytes in, bytes out for a package that is
//! not in an editor ([`sign::sign`], [`verify::verify`]). The crate never
//! holds keys (the host injects a signer), never reads the clock (the host
//! passes the time) and never touches the network.
//!
//! Hosts meet this line through the library's exits, `rofd-component` and
//! the web SDK (spec §17.13); they re-export what a host needs.

pub mod ber;
pub mod cert;
pub mod cert_store;
pub mod crypto;
pub mod der;
pub mod error;
pub mod flow;
pub mod gbt35275;
pub mod in_place;
#[cfg(any(test, feature = "test-ca"))]
pub mod mock_ca;
mod package_util;
pub mod pfx;
pub mod report;
pub mod ses;
pub mod ses_encoder;
mod sign;
pub mod signature_xml;
pub mod signer;
pub mod verify;

pub use cert::{parse_certificate, CertificateInfo};
pub use cert_store::{CertificateEntry, CertificateId, CertificateStore};
pub use crypto::{sm2_sign, sm2_verify, sm3, DEFAULT_SM2_USER_ID};
pub use der::{
    parse_children, parse_one, DerNode, DerWriter, TAG_BOOLEAN, TAG_GENERALIZED_TIME,
    TAG_IA5_STRING, TAG_INTEGER, TAG_NULL, TAG_OCTET_STRING, TAG_PRINTABLE_STRING, TAG_SEQUENCE,
    TAG_SET, TAG_UTF8_STRING,
};
pub use error::SignError;
pub use flow::{newly_invalidated, summarize, SignRequest, SignatureSummary};
pub use gbt35275::{build_signed_data, parse_signed_data, SignedData};
pub use in_place::{
    prepare_in_place, sign_in_place, sign_refusal, verify_document, InPlaceStep, Landed,
    PreparedInPlace, SignatureToAppend,
};
#[cfg(any(test, feature = "test-ca"))]
pub use mock_ca::{TestCa, TestSigner};
pub use pfx::PfxSigner;
pub use report::{
    CertificateReport, IntegrityFailure, IntegrityResult, SignatureOverall, SignatureReport,
    SignatureValue, SignatureValueStatus, SignatureVariant, VerifyOptions,
};
pub use ses::{parse_ses_signature, SesSignature};
pub use ses_encoder::{
    build_e_seal_info, build_tbs, check_value, finish_seal, finish_signature, SealInput,
};
pub use sign::{
    prepare_sign, sign, Pages, Placement, Prepared, Provider, SealImage, SignOptions, SignStep,
    DEFAULT_MARGIN_MM, DEFAULT_SEAL_SIZE_MM, SIGNATURE_ID_RESERVE,
};
pub use signature_xml::{
    generalized_time, signature_xml, Reference, SignatureXmlInput, StampRef,
    SIGNATURE_METHOD_SM2_SM3,
};
pub use signer::Signer;
pub use verify::verify;
