//! Errors of the signing line. Verification never fails: it reports. These
//! errors are for inputs the caller can fix and for operations that cannot
//! complete (creating a signature, opening a certificate file).

/// Why a signing operation could not complete.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    /// A caller argument is unusable: an image that does not decode, a page
    /// number out of range, margins the seal cannot fit into.
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    /// The password given for a certificate file is wrong. The file itself
    /// said so, through its integrity check; a host may tell the user so,
    /// where a file that cannot be read may not be called a wrong password.
    #[error("wrong password: {reason}")]
    Password { reason: String },
    /// A certificate file cannot be opened, or does not hold what is needed.
    #[error("certificate: {reason}")]
    Certificate { reason: String },
    /// The injected signer failed, or returned something unusable.
    #[error("signer: {reason}")]
    Signer { reason: String },
    /// A structure could not be encoded.
    #[error("encode: {reason}")]
    Encode { reason: String },
    /// A cryptographic primitive failed.
    #[error("crypto: {reason}")]
    Crypto { reason: String },
    /// The document has annotations, and its file declares signatures and
    /// declares no annotation entry: they cannot be written into it without
    /// changing a file a signature protects. Said before anything is made.
    #[error("the file declares signatures and has no annotation entry; the annotations cannot be written into it")]
    AnnotationsUnwritable,
    /// A signature prepared earlier is put into a document whose package
    /// changed since: another signature landed, or a save patched it. It has
    /// to be prepared again; nothing was written.
    #[error("the document changed since the signature was prepared; prepare it again")]
    DocumentChanged,
    /// An OFD package could not be read or written.
    #[error(transparent)]
    Io(#[from] rofd_io::OfdError),
}
