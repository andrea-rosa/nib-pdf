//! Internal PDF/CMS signing foundations (M5).
//!
//! Flow: [`prepare`] stages an incremental signature revision through the
//! `nib-core` writer (fixed-width `/ByteRange` + `/Contents` placeholders),
//! patches the ByteRange, digests the covered bytes and builds the canonical
//! DER signed attributes. The host signs [`SignRequest::data`] (the signed
//! attributes, or their digest when the prehashed input was requested
//! explicitly). [`PreparedSignature::complete`] verifies the signature or a
//! supplied complete CMS against the expected certificate, embeds the DER
//! and atomically adopts the revision in the writer.
//!
//! Private keys never enter this crate. Verification of existing
//! signatures, trust, DocMDP, timestamps (B-T), appearances and UniFFI
//! exposure are later M5 cycles.
#![forbid(unsafe_code)]

mod algorithm;
mod cms;
pub mod placeholder;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use nib_core::writer::sign::{SigDictSpec, StagedSignature};
use nib_core::writer::{FlushOutput, Writer, WriterError};

pub use algorithm::Scheme;
pub use cms::der_prefix_len;
pub use nib_core::model::{DigestAlgorithm, SigLevel, SignatureAlgorithm, SubFilter};
pub use nib_core::writer::sign::{SigTarget, DEFAULT_CONTENTS_HEX_LEN, MAX_CONTENTS_HEX_LEN};
pub use placeholder::Placeholder;

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
    #[error("unsupported signature level {0:?}")]
    UnsupportedLevel(SigLevel),
    #[error("certificate: {0}")]
    Certificate(String),
    #[error("certificate mismatch: {0}")]
    CertificateMismatch(String),
    #[error("placeholder: {0}")]
    Placeholder(String),
    /// The CMS does not fit. `required` is in hex characters, excluding the
    /// `<` `>` delimiters (2 × DER length).
    #[error("/Contents placeholder too small: {required} hex characters required")]
    PlaceholderTooSmall { required: usize },
    #[error("ByteRange offset {0} does not fit its 10-digit slot")]
    OffsetOverflow(u64),
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("invalid CMS: {0}")]
    InvalidCms(String),
    #[error("DER: {0}")]
    Der(String),
    #[error("writer: {0}")]
    Writer(#[from] WriterError),
}

impl From<der::Error> for SignError {
    fn from(e: der::Error) -> Self {
        SignError::Der(e.to_string())
    }
}

/// Signer identity fixed before the signed attributes are built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerIdentity {
    /// DER leaf certificate.
    Certificate(Vec<u8>),
    /// SHA-256 of the DER leaf certificate (the certificate itself arrives
    /// with the signature).
    Sha256Fingerprint([u8; 32]),
}

/// What the host signer receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignerInput {
    /// The DER signed-attributes bytes; the signer hashes them itself.
    #[default]
    SignedAttributes,
    /// The digest of the DER signed-attributes bytes, for signers that take
    /// a precomputed hash (`NONEwithECDSA`, raw PSS/PKCS#1 prehash APIs).
    Prehashed,
}

#[derive(Debug, Clone)]
pub struct SignOptions {
    pub target: SigTarget,
    pub signer: SignerIdentity,
    pub sub_filter: SubFilter,
    pub digest: DigestAlgorithm,
    pub algorithm: SignatureAlgorithm,
    pub level: SigLevel,
    pub input: SignerInput,
    /// `/Contents` placeholder in hex characters, excluding delimiters.
    pub contents_hex_len: usize,
    /// Untrusted claimed time for `/M`; never a CAdES `signingTime`.
    pub claimed_time: Option<DateTime<Utc>>,
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
}

impl SignOptions {
    /// `ETSI.CAdES.detached`, SHA-256 (SHA-384 for P-384), B-B,
    /// signed-attributes input and the default 16,384-hex-character
    /// placeholder.
    pub fn new(target: SigTarget, signer: SignerIdentity, algorithm: SignatureAlgorithm) -> Self {
        let digest = match algorithm {
            SignatureAlgorithm::EcdsaP384 => DigestAlgorithm::Sha384,
            _ => DigestAlgorithm::Sha256,
        };
        SignOptions {
            target,
            signer,
            sub_filter: SubFilter::EtsiCadesDetached,
            digest,
            algorithm,
            level: SigLevel::BB,
            input: SignerInput::SignedAttributes,
            contents_hex_len: DEFAULT_CONTENTS_HEX_LEN,
            claimed_time: None,
            name: None,
            reason: None,
            location: None,
            contact_info: None,
        }
    }
}

/// Data for the external signer.
#[derive(Debug, Clone, Copy)]
pub struct SignRequest<'a> {
    pub data: &'a [u8],
    pub input: SignerInput,
    pub digest: DigestAlgorithm,
    pub algorithm: SignatureAlgorithm,
}

/// How the host finishes the signature.
#[derive(Debug, Clone)]
pub enum Completion {
    /// Raw signature value over [`SignRequest::data`] (ECDSA as DER
    /// `Ecdsa-Sig-Value`) plus the DER chain, leaf first.
    Signature {
        signature: Vec<u8>,
        chain: Vec<Vec<u8>>,
    },
    /// A complete detached `ContentInfo(SignedData)` in DER.
    Cms(Vec<u8>),
}

/// A staged signature awaiting the external signer. Its bytes stay private;
/// nothing is published until [`PreparedSignature::complete`] succeeds.
pub struct PreparedSignature {
    staged: StagedSignature,
    placeholder: Placeholder,
    byte_range: [u64; 4],
    bytes: Vec<u8>,
    pdf_digest: Vec<u8>,
    signed_attrs: Vec<u8>,
    request_data: Vec<u8>,
    scheme: Scheme,
    input: SignerInput,
    leaf_sha256: [u8; 32],
}

impl std::fmt::Debug for PreparedSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSignature")
            .field("field", &self.staged.field_name())
            .field("byte_range", &self.byte_range)
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

/// Validate the scheme and signer, stage the signature revision and build
/// the signed attributes. On error the writer keeps no staged signature.
pub fn prepare(writer: &mut Writer, options: SignOptions) -> Result<PreparedSignature, SignError> {
    let scheme = Scheme::new(
        options.sub_filter,
        options.digest,
        options.algorithm,
        options.level,
    )?;
    let hex = options.contents_hex_len;
    if !hex.is_multiple_of(2) {
        return Err(SignError::Placeholder(format!(
            "/Contents placeholder length {hex} is odd"
        )));
    }
    if !(2..=MAX_CONTENTS_HEX_LEN).contains(&hex) {
        return Err(SignError::Placeholder(format!(
            "/Contents placeholder length {hex} is outside 2..={MAX_CONTENTS_HEX_LEN}"
        )));
    }
    let leaf_sha256 = match &options.signer {
        SignerIdentity::Certificate(der) => {
            let cert = cms::parse_certificate(der)?;
            cms::check_key_type(
                scheme.algorithm,
                cert.tbs_certificate().subject_public_key_info(),
            )?;
            cms::sha256(der)
        }
        SignerIdentity::Sha256Fingerprint(fp) => *fp,
    };

    let spec = SigDictSpec {
        target: options.target,
        sub_filter: scheme.sub_filter,
        contents_hex_len: hex,
        claimed_time: options.claimed_time,
        name: options.name,
        reason: options.reason,
        location: options.location,
        contact_info: options.contact_info,
    };
    let staged = writer.stage_signature(&spec)?;
    match finish_prepare(staged.clone(), scheme, options.input, hex, leaf_sha256) {
        Ok(p) => Ok(p),
        Err(e) => {
            writer.abort_signature(&staged);
            Err(e)
        }
    }
}

fn finish_prepare(
    staged: StagedSignature,
    scheme: Scheme,
    input: SignerInput,
    hex: usize,
    leaf_sha256: [u8; 32],
) -> Result<PreparedSignature, SignError> {
    let placeholder =
        placeholder::locate(staged.bytes(), staged.base_len(), staged.sig_object(), hex)?;
    let mut bytes = staged.bytes().to_vec();
    let byte_range = placeholder.byte_range(bytes.len())?;
    placeholder::patch_byte_range(&mut bytes, &placeholder, byte_range)?;
    placeholder::check_byte_range(byte_range, &placeholder, bytes.len())?;
    let pdf_digest = algorithm::digest_parts(
        scheme.digest,
        &[
            &bytes[..placeholder.contents_open],
            &bytes[placeholder.contents_end..],
        ],
    );
    let signed_attrs = cms::signed_attributes(&pdf_digest, &leaf_sha256)?;
    let request_data = match input {
        SignerInput::SignedAttributes => signed_attrs.clone(),
        SignerInput::Prehashed => algorithm::digest_parts(scheme.digest, &[&signed_attrs]),
    };
    Ok(PreparedSignature {
        staged,
        placeholder,
        byte_range,
        bytes,
        pdf_digest,
        signed_attrs,
        request_data,
        scheme,
        input,
        leaf_sha256,
    })
}

impl PreparedSignature {
    pub fn request(&self) -> SignRequest<'_> {
        SignRequest {
            data: &self.request_data,
            input: self.input,
            digest: self.scheme.digest,
            algorithm: self.scheme.algorithm,
        }
    }

    pub fn byte_range(&self) -> [u64; 4] {
        self.byte_range
    }

    pub fn placeholder(&self) -> Placeholder {
        self.placeholder
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn field_name(&self) -> &str {
        self.staged.field_name()
    }

    /// Verify, embed and adopt. On error nothing is published and the
    /// writer is unchanged; the prepared signature may be retried (for
    /// example with a corrected signature) or aborted.
    pub fn complete(
        &self,
        writer: &mut Writer,
        completion: Completion,
    ) -> Result<FlushOutput, SignError> {
        let exp = cms::Expectation {
            scheme: self.scheme,
            pdf_digest: &self.pdf_digest,
            leaf_sha256: self.leaf_sha256,
        };
        let der = match completion {
            Completion::Signature { signature, chain } => {
                let leaf_der = chain
                    .first()
                    .ok_or_else(|| SignError::Certificate("empty certificate chain".into()))?;
                if cms::sha256(leaf_der) != self.leaf_sha256 {
                    return Err(SignError::CertificateMismatch(
                        "leaf certificate differs from the identity fixed at prepare".into(),
                    ));
                }
                let chain = chain
                    .iter()
                    .map(|c| cms::parse_certificate(c))
                    .collect::<Result<Vec<_>, _>>()?;
                cms::verify_signature(
                    &self.scheme,
                    chain[0].tbs_certificate().subject_public_key_info(),
                    &self.signed_attrs,
                    &signature,
                )?;
                let der = cms::build(&self.scheme, &self.signed_attrs, &signature, &chain)?;
                cms::verify(&der, &exp)?;
                der
            }
            Completion::Cms(der) => {
                cms::verify(&der, &exp)?;
                der
            }
        };
        let mut bytes = self.bytes.clone();
        placeholder::embed_contents(&mut bytes, &self.placeholder, &der)?;
        Ok(writer.adopt_signed(&self.staged, bytes)?)
    }

    /// Drop the staged revision.
    pub fn abort(self, writer: &mut Writer) {
        writer.abort_signature(&self.staged);
    }
}
