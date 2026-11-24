/// Errors surfaced across the FFI boundary.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum NibError {
    #[error("pdfium: {0}")]
    Pdfium(String),
    #[error("internal: {0}")]
    Internal(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("document is password-protected; encrypted PDFs are unsupported")]
    EncryptedUnsupported,
    #[error("cancelled")]
    Cancelled,
    #[error("document has signatures; full rewrite refused")]
    SignedDocument,
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("io: {0}")]
    Io(String),
    /// The payload is a JSON array of `DeltaValidationError`.
    #[error("invalid delta: {0}")]
    InvalidDelta(String),
    #[error("revision mismatch: expected {expected}, got {got}")]
    RevisionMismatch { expected: u32, got: u32 },
    #[error("signature operation failed: {0}")]
    Signature(String),
    #[error("stale signature preparation: {0}")]
    StaleSignature(String),
}

impl From<nib_render::types::RenderError> for NibError {
    fn from(e: nib_render::types::RenderError) -> Self {
        use nib_render::types::RenderError::*;
        match e {
            Pdfium(m) => NibError::Pdfium(m),
            Parse(m) => NibError::Parse(m),
            EncryptedUnsupported => NibError::EncryptedUnsupported,
            Cancelled => NibError::Cancelled,
            Internal(m) => NibError::Internal(m),
        }
    }
}

impl From<nib_core::writer::WriterError> for NibError {
    fn from(e: nib_core::writer::WriterError) -> Self {
        use nib_core::writer::WriterError::*;
        match e {
            Encrypted => NibError::EncryptedUnsupported,
            SignedDocument => NibError::SignedDocument,
            Io(m) => NibError::Io(m),
            other => NibError::WriteFailed(other.to_string()),
        }
    }
}
