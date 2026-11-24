//! Supported profile / digest / signature-algorithm combinations and their
//! ASN.1 identifiers.

use const_oid::db::{rfc5753, rfc5912};
use const_oid::ObjectIdentifier;
use der::asn1::Null;
use der::{Any, Encode, Sequence};
use nib_core::model::{DigestAlgorithm, SigLevel, SignatureAlgorithm, SubFilter};
use sha2::{Digest, Sha256, Sha384, Sha512};
use spki::AlgorithmIdentifierOwned;

use crate::SignError;

/// `id-aa-signatureTimeStampToken` (RFC 3161 unsigned attribute).
pub(crate) const ID_AA_TIME_STAMP_TOKEN: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.14");
/// `ecdsa-with-SHA512`.
const ECDSA_WITH_SHA_512: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.4");

/// A validated combination: CMS profile + digest + signature algorithm at
/// baseline level B-B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    pub sub_filter: SubFilter,
    pub digest: DigestAlgorithm,
    pub algorithm: SignatureAlgorithm,
}

impl Scheme {
    /// Supported combinations:
    ///
    /// | algorithm      | digests                   |
    /// |----------------|---------------------------|
    /// | `RSASSA-PSS`   | SHA-256, SHA-384, SHA-512 |
    /// | `RSA-PKCS1v15` | SHA-256, SHA-384, SHA-512 |
    /// | `ECDSA-P256`   | SHA-256                   |
    /// | `ECDSA-P384`   | SHA-384                   |
    ///
    /// for both `ETSI.CAdES.detached` and `adbe.pkcs7.detached`. Level B-T
    /// (RFC 3161 timestamp) is not implemented and is rejected explicitly.
    pub fn new(
        sub_filter: SubFilter,
        digest: DigestAlgorithm,
        algorithm: SignatureAlgorithm,
        level: SigLevel,
    ) -> Result<Scheme, SignError> {
        if level != SigLevel::BB {
            return Err(SignError::UnsupportedLevel(level));
        }
        use DigestAlgorithm::*;
        use SignatureAlgorithm::*;
        let ok = match algorithm {
            RsassaPss | RsaPkcs1v15 => true,
            EcdsaP256 => digest == Sha256,
            EcdsaP384 => digest == Sha384,
        };
        if !ok {
            return Err(SignError::UnsupportedAlgorithm(format!(
                "{algorithm:?} with {digest:?} is not a supported combination"
            )));
        }
        Ok(Scheme {
            sub_filter,
            digest,
            algorithm,
        })
    }

    /// `SignatureAlgorithmIdentifier` written into the SignerInfo.
    pub(crate) fn signature_algorithm_id(&self) -> Result<AlgorithmIdentifierOwned, SignError> {
        Ok(match self.algorithm {
            SignatureAlgorithm::RsaPkcs1v15 => AlgorithmIdentifierOwned {
                oid: rfc5912::RSA_ENCRYPTION,
                parameters: Some(Any::encode_from(&Null)?),
            },
            SignatureAlgorithm::RsassaPss => AlgorithmIdentifierOwned {
                oid: rfc5912::ID_RSASSA_PSS,
                parameters: Some(Any::encode_from(&pss_params(self.digest)?)?),
            },
            SignatureAlgorithm::EcdsaP256 | SignatureAlgorithm::EcdsaP384 => {
                AlgorithmIdentifierOwned {
                    oid: ecdsa_oid(self.digest),
                    parameters: None,
                }
            }
        })
    }

    /// Accept a SignerInfo `signatureAlgorithm` from a supplied CMS when it
    /// denotes exactly this scheme.
    pub(crate) fn check_signature_algorithm_id(
        &self,
        id: &AlgorithmIdentifierOwned,
    ) -> Result<(), SignError> {
        let mismatch = || {
            SignError::InvalidCms(format!(
                "signatureAlgorithm {} does not match {:?}/{:?}",
                id.oid, self.algorithm, self.digest
            ))
        };
        match self.algorithm {
            SignatureAlgorithm::RsaPkcs1v15 => {
                let with_digest = match self.digest {
                    DigestAlgorithm::Sha256 => rfc5912::SHA_256_WITH_RSA_ENCRYPTION,
                    DigestAlgorithm::Sha384 => rfc5912::SHA_384_WITH_RSA_ENCRYPTION,
                    DigestAlgorithm::Sha512 => rfc5912::SHA_512_WITH_RSA_ENCRYPTION,
                };
                if (id.oid != rfc5912::RSA_ENCRYPTION && id.oid != with_digest)
                    || !params_absent_or_null(id)
                {
                    return Err(mismatch());
                }
            }
            SignatureAlgorithm::RsassaPss => {
                if id.oid != rfc5912::ID_RSASSA_PSS {
                    return Err(mismatch());
                }
                let params: PssParams = id
                    .parameters
                    .as_ref()
                    .ok_or_else(mismatch)?
                    .decode_as()
                    .map_err(|_| mismatch())?;
                let hash_ok = params
                    .hash
                    .as_ref()
                    .is_some_and(|h| h.oid == digest_oid(self.digest) && params_absent_or_null(h));
                let mgf_ok = params.mgf.as_ref().is_some_and(|m| {
                    m.oid == rfc5912::ID_MGF_1
                        && m.parameters
                            .as_ref()
                            .and_then(|p| p.decode_as::<AlgorithmIdentifierOwned>().ok())
                            .is_some_and(|h| {
                                h.oid == digest_oid(self.digest) && params_absent_or_null(&h)
                            })
                });
                let salt_ok = params.salt == Some(digest_len(self.digest) as u32);
                let trailer_ok = params.trailer.is_none_or(|t| t == 1);
                if !(hash_ok && mgf_ok && salt_ok && trailer_ok) {
                    return Err(SignError::UnsupportedAlgorithm(
                        "RSASSA-PSS parameters must use the signature digest for hash and MGF1 \
                         and a salt length equal to the digest length"
                            .into(),
                    ));
                }
            }
            SignatureAlgorithm::EcdsaP256 | SignatureAlgorithm::EcdsaP384 => {
                if id.oid != ecdsa_oid(self.digest) || id.parameters.is_some() {
                    return Err(mismatch());
                }
            }
        }
        Ok(())
    }
}

/// `RSASSA-PSS-params` (RFC 4055); all fields are `[n] EXPLICIT ... DEFAULT`.
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
pub(crate) struct PssParams {
    #[asn1(context_specific = "0", tag_mode = "EXPLICIT", optional = "true")]
    pub hash: Option<AlgorithmIdentifierOwned>,
    #[asn1(context_specific = "1", tag_mode = "EXPLICIT", optional = "true")]
    pub mgf: Option<AlgorithmIdentifierOwned>,
    #[asn1(context_specific = "2", tag_mode = "EXPLICIT", optional = "true")]
    pub salt: Option<u32>,
    #[asn1(context_specific = "3", tag_mode = "EXPLICIT", optional = "true")]
    pub trailer: Option<u32>,
}

fn pss_params(digest: DigestAlgorithm) -> Result<PssParams, SignError> {
    let hash = digest_algorithm_id(digest);
    Ok(PssParams {
        mgf: Some(AlgorithmIdentifierOwned {
            oid: rfc5912::ID_MGF_1,
            parameters: Some(Any::encode_from(&hash)?),
        }),
        hash: Some(hash),
        salt: Some(digest_len(digest) as u32),
        trailer: None,
    })
}

pub(crate) fn params_absent_or_null(id: &AlgorithmIdentifierOwned) -> bool {
    match &id.parameters {
        None => true,
        Some(p) => p.to_der().ok().as_deref() == Some(&[0x05, 0x00][..]),
    }
}

fn ecdsa_oid(digest: DigestAlgorithm) -> ObjectIdentifier {
    match digest {
        DigestAlgorithm::Sha256 => rfc5912::ECDSA_WITH_SHA_256,
        DigestAlgorithm::Sha384 => rfc5912::ECDSA_WITH_SHA_384,
        DigestAlgorithm::Sha512 => ECDSA_WITH_SHA_512,
    }
}

pub(crate) fn digest_oid(digest: DigestAlgorithm) -> ObjectIdentifier {
    match digest {
        DigestAlgorithm::Sha256 => rfc5912::ID_SHA_256,
        DigestAlgorithm::Sha384 => rfc5912::ID_SHA_384,
        DigestAlgorithm::Sha512 => rfc5912::ID_SHA_512,
    }
}

/// Digest `AlgorithmIdentifier` with absent parameters (RFC 5754).
pub(crate) fn digest_algorithm_id(digest: DigestAlgorithm) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid: digest_oid(digest),
        parameters: None,
    }
}

pub(crate) fn digest_len(digest: DigestAlgorithm) -> usize {
    match digest {
        DigestAlgorithm::Sha256 => 32,
        DigestAlgorithm::Sha384 => 48,
        DigestAlgorithm::Sha512 => 64,
    }
}

/// Digest of the concatenated parts.
pub(crate) fn digest_parts(digest: DigestAlgorithm, parts: &[&[u8]]) -> Vec<u8> {
    fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
        let mut d = D::new();
        for p in parts {
            d.update(p);
        }
        d.finalize().to_vec()
    }
    match digest {
        DigestAlgorithm::Sha256 => run::<Sha256>(parts),
        DigestAlgorithm::Sha384 => run::<Sha384>(parts),
        DigestAlgorithm::Sha512 => run::<Sha512>(parts),
    }
}

/// Named curve of an `id-ecPublicKey` SPKI.
pub(crate) fn ec_curve(spki: &spki::SubjectPublicKeyInfoOwned) -> Option<ObjectIdentifier> {
    if spki.algorithm.oid != rfc5753::ID_EC_PUBLIC_KEY {
        return None;
    }
    spki.algorithm
        .parameters
        .as_ref()
        .and_then(|p| p.decode_as::<ObjectIdentifier>().ok())
}
