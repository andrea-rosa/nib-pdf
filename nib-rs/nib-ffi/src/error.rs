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
