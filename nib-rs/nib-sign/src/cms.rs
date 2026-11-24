//! Detached CMS SignedData (RFC 5652) for PAdES baseline B-B
//! (`ETSI.CAdES.detached`) and `adbe.pkcs7.detached`, built from typed
//! `cms` structures (no `cms::builder`, so no RSA 0.9 dependency).

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use const_oid::db::{rfc5911, rfc5912};
use const_oid::ObjectIdentifier;
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Decode, Encode, Sequence};
use nib_core::model::SigLevel;
use nib_core::model::{DigestAlgorithm, SignatureAlgorithm, SubFilter};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::traits::PublicKeyParts;
use sha2::{Digest, Sha256, Sha384, Sha512};
use signature::hazmat::PrehashVerifier;
use spki::SubjectPublicKeyInfoOwned;
use x509_cert::attr::Attribute;
use x509_cert::Certificate;

use crate::algorithm::{
    self, digest_algorithm_id, digest_oid, params_absent_or_null, Scheme, ID_AA_TIME_STAMP_TOKEN,
};
use crate::SignError;

/// Smallest accepted RSA modulus.
const MIN_RSA_BITS: usize = 2048;
const MAX_CMS_CERTIFICATES: usize = 32;

/// `ESSCertIDv2` (RFC 5035). `hashAlgorithm` DEFAULT sha256 is omitted when
/// encoding.
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct EssCertIdV2 {
    #[asn1(optional = "true")]
    hash_algorithm: Option<spki::AlgorithmIdentifierOwned>,
    cert_hash: OctetString,
    #[asn1(optional = "true")]
    issuer_serial: Option<Any>,
}

/// `SigningCertificateV2` (RFC 5035).
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct SigningCertificateV2 {
    certs: Vec<EssCertIdV2>,
    #[asn1(optional = "true")]
    policies: Option<Any>,
}

fn attribute(oid: ObjectIdentifier, value: Any) -> Result<Attribute, SignError> {
    Ok(Attribute {
        oid,
        values: SetOfVec::try_from(vec![value])?,
    })
}

/// Canonical DER `SET OF Attribute` (tag `0x31`): content-type `id-data`,
/// message-digest of the PDF ByteRange and ESS signing-certificate-v2 with
/// the SHA-256 hash of the leaf certificate. No `signingTime`.
pub(crate) fn signed_attributes(
    pdf_digest: &[u8],
    leaf_sha256: &[u8; 32],
) -> Result<Vec<u8>, SignError> {
    let ess = SigningCertificateV2 {
        certs: vec![EssCertIdV2 {
            hash_algorithm: None,
            cert_hash: OctetString::new(leaf_sha256.to_vec())?,
            issuer_serial: None,
        }],
        policies: None,
    };
    let attrs = SetOfVec::try_from(vec![
        attribute(
            rfc5911::ID_CONTENT_TYPE,
            Any::encode_from(&rfc5911::ID_DATA)?,
        )?,
        attribute(
            rfc5911::ID_MESSAGE_DIGEST,
            Any::encode_from(&OctetString::new(pdf_digest.to_vec())?)?,
        )?,
        attribute(
            rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2,
            Any::encode_from(&ess)?,
        )?,
    ])?;
    Ok(attrs.to_der()?)
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(crate) fn parse_certificate(der: &[u8]) -> Result<Certificate, SignError> {
    let cert = Certificate::from_der(der)
        .map_err(|e| SignError::Certificate(format!("malformed certificate: {e}")))?;
    if cert.to_der()? != der {
        return Err(SignError::Certificate(
            "certificate is not canonical DER".into(),
        ));
    }
    Ok(cert)
}

/// Fail unless the certificate key can produce `algorithm` signatures.
pub(crate) fn check_key_type(
    algorithm: SignatureAlgorithm,
    spki: &SubjectPublicKeyInfoOwned,
) -> Result<(), SignError> {
    let ok = match algorithm {
        SignatureAlgorithm::RsassaPss | SignatureAlgorithm::RsaPkcs1v15 => {
            let oid = spki.algorithm.oid;
            if oid != rfc5912::RSA_ENCRYPTION && oid != rfc5912::ID_RSASSA_PSS {
                false
            } else {
                let key = rsa_key(spki)?;
                if key.size() * 8 < MIN_RSA_BITS {
                    return Err(SignError::Certificate(format!(
                        "RSA key smaller than {MIN_RSA_BITS} bits"
                    )));
                }
                true
            }
        }
        SignatureAlgorithm::EcdsaP256 => algorithm::ec_curve(spki) == Some(rfc5912::SECP_256_R_1),
        SignatureAlgorithm::EcdsaP384 => algorithm::ec_curve(spki) == Some(rfc5912::SECP_384_R_1),
    };
    if !ok {
        return Err(SignError::Certificate(format!(
            "certificate key ({}) cannot produce {algorithm:?} signatures",
            spki.algorithm.oid
        )));
    }
    Ok(())
}

fn rsa_key(spki: &SubjectPublicKeyInfoOwned) -> Result<rsa::PublicKey, SignError> {
    let bits = spki.subject_public_key.as_bytes().ok_or_else(|| {
        SignError::Certificate("public key bit string is not octet aligned".into())
    })?;
    rsa::PublicKey::from_pkcs1_der(bits)
        .map_err(|e| SignError::Certificate(format!("RSA public key: {e}")))
}

/// Verify a raw signature value over the DER signed attributes.
pub(crate) fn verify_signature(
    scheme: &Scheme,
    spki: &SubjectPublicKeyInfoOwned,
    signed_attrs_der: &[u8],
    sig: &[u8],
) -> Result<(), SignError> {
    check_key_type(scheme.algorithm, spki)?;
    let hash = algorithm::digest_parts(scheme.digest, &[signed_attrs_der]);
    let bad = |e: String| SignError::InvalidSignature(e);
    match scheme.algorithm {
        SignatureAlgorithm::RsassaPss => {
            let key = rsa_key(spki)?;
            match scheme.digest {
                DigestAlgorithm::Sha256 => key.pss_verify_prehash::<Sha256>(&hash, sig),
                DigestAlgorithm::Sha384 => key.pss_verify_prehash::<Sha384>(&hash, sig),
                DigestAlgorithm::Sha512 => key.pss_verify_prehash::<Sha512>(&hash, sig),
            }
            .map_err(|e| bad(format!("RSASSA-PSS: {e}")))
        }
        SignatureAlgorithm::RsaPkcs1v15 => {
            let key = rsa_key(spki)?;
            let scheme = match scheme.digest {
                DigestAlgorithm::Sha256 => rsa::Pkcs1v15Sign::new::<Sha256>(),
                DigestAlgorithm::Sha384 => rsa::Pkcs1v15Sign::new::<Sha384>(),
                DigestAlgorithm::Sha512 => rsa::Pkcs1v15Sign::new::<Sha512>(),
            };
            key.verify(scheme, &hash, sig)
                .map_err(|e| bad(format!("RSA-PKCS1v15: {e}")))
        }
        SignatureAlgorithm::EcdsaP256 => {
            let point = spki.subject_public_key.raw_bytes();
            let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(point)
                .map_err(|e| SignError::Certificate(format!("P-256 key: {e}")))?;
            let s = p256::ecdsa::Signature::from_der(sig)
                .map_err(|e| bad(format!("ECDSA signature must be DER: {e}")))?;
            vk.verify_prehash(&hash, &s)
                .map_err(|e| bad(format!("ECDSA-P256: {e}")))
        }
        SignatureAlgorithm::EcdsaP384 => {
            let point = spki.subject_public_key.raw_bytes();
            let vk = p384::ecdsa::VerifyingKey::from_sec1_bytes(point)
                .map_err(|e| SignError::Certificate(format!("P-384 key: {e}")))?;
            let s = p384::ecdsa::Signature::from_der(sig)
                .map_err(|e| bad(format!("ECDSA signature must be DER: {e}")))?;
            vk.verify_prehash(&hash, &s)
                .map_err(|e| bad(format!("ECDSA-P384: {e}")))
        }
    }
}

/// Assemble the detached `ContentInfo(SignedData)`; `chain[0]` is the leaf.
pub(crate) fn build(
    scheme: &Scheme,
    signed_attrs_der: &[u8],
    signature: &[u8],
    chain: &[Certificate],
) -> Result<Vec<u8>, SignError> {
    let leaf = chain
        .first()
        .ok_or_else(|| SignError::Certificate("empty certificate chain".into()))?;
    let digest_alg = digest_algorithm_id(scheme.digest);
    let signer_info = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: leaf.tbs_certificate().issuer().clone(),
            serial_number: leaf.tbs_certificate().serial_number().clone(),
        }),
        digest_alg: digest_alg.clone(),
        signed_attrs: Some(x509_cert::attr::Attributes::from_der(signed_attrs_der)?),
        signature_algorithm: scheme.signature_algorithm_id()?,
        signature: OctetString::new(signature.to_vec())?,
        unsigned_attrs: None,
    };
    let certs: Vec<CertificateChoices> = chain
        .iter()
        .cloned()
        .map(CertificateChoices::Certificate)
        .collect();
    let sd = SignedData {
        version: CmsVersion::V1,
        digest_algorithms: SetOfVec::try_from(vec![digest_alg])?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: rfc5911::ID_DATA,
            econtent: None,
        },
        certificates: Some(CertificateSet::try_from(certs)?),
        crls: None,
        signer_infos: SignerInfos::try_from(vec![signer_info])?,
    };
    let ci = ContentInfo {
        content_type: rfc5911::ID_SIGNED_DATA,
        content: Any::encode_from(&sd)?,
    };
    Ok(ci.to_der()?)
}

/// What a completed CMS must prove.
pub(crate) struct Expectation<'a> {
    pub scheme: Scheme,
    pub pdf_digest: &'a [u8],
    /// SHA-256 of the expected leaf certificate.
    pub leaf_sha256: [u8; 32],
}

/// Strictly check a complete detached CMS: canonical DER, one signer,
/// detached `id-data`, expected digest and signature algorithms, signed
/// attributes (unique single-valued content-type, message-digest equal to
/// the PDF digest, ESS signing-certificate-v2 naming the leaf, no
/// `signingTime` for CAdES), no unsigned attributes, the signer certificate
/// located through `sid` equal to the expected leaf, and a valid signature
/// over the DER signed attributes. Returns the leaf certificate.
pub(crate) fn verify(der: &[u8], exp: &Expectation<'_>) -> Result<Certificate, SignError> {
    let invalid = |m: &str| SignError::InvalidCms(m.to_string());
    let ci = ContentInfo::from_der(der).map_err(|e| SignError::InvalidCms(format!("DER: {e}")))?;
    if ci.to_der()? != der {
        return Err(invalid("CMS is not canonical DER"));
    }
    if ci.content_type != rfc5911::ID_SIGNED_DATA {
        return Err(invalid("content type is not signedData"));
    }
    let sd: SignedData = ci
        .content
        .decode_as()
        .map_err(|e| SignError::InvalidCms(format!("SignedData: {e}")))?;
    if sd.encap_content_info.econtent_type != rfc5911::ID_DATA
        || sd.encap_content_info.econtent.is_some()
    {
        return Err(invalid("encapsulated content must be detached id-data"));
    }
    let [si] = sd.signer_infos.0.as_slice() else {
        return Err(invalid("exactly one SignerInfo is required"));
    };
    if si.digest_alg.oid != digest_oid(exp.scheme.digest) || !params_absent_or_null(&si.digest_alg)
    {
        return Err(SignError::InvalidCms(format!(
            "digest algorithm {} does not match {:?}",
            si.digest_alg.oid, exp.scheme.digest
        )));
    }
    if !sd
        .digest_algorithms
        .iter()
        .any(|a| a.oid == si.digest_alg.oid)
    {
        return Err(invalid(
            "SignedData digestAlgorithms omits the signer digest",
        ));
    }
    exp.scheme
        .check_signature_algorithm_id(&si.signature_algorithm)?;
    if let Some(u) = &si.unsigned_attrs {
        if u.iter().any(|a| a.oid == ID_AA_TIME_STAMP_TOKEN) {
            return Err(SignError::UnsupportedLevel(nib_core::model::SigLevel::BT));
        }
        return Err(invalid("unsigned attributes are not supported"));
    }

    // Signer certificate via sid.
    let SignerIdentifier::IssuerAndSerialNumber(isn) = &si.sid else {
        return Err(invalid(
            "only issuerAndSerialNumber signer identifiers are supported",
        ));
    };
    let certs: Vec<&Certificate> = sd
        .certificates
        .as_ref()
        .map(|s| {
            s.0.iter()
                .filter_map(|c| match c {
                    CertificateChoices::Certificate(c) => Some(c),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let matching: Vec<&Certificate> = certs
        .into_iter()
        .filter(|c| {
            c.tbs_certificate().issuer() == &isn.issuer
                && c.tbs_certificate().serial_number() == &isn.serial_number
        })
        .collect();
    let [leaf] = matching.as_slice() else {
        return Err(SignError::Certificate(format!(
            "{} certificates match the signer identifier; exactly one is required",
            matching.len()
        )));
    };
    let leaf_der = leaf.to_der()?;
    if sha256(&leaf_der) != exp.leaf_sha256 {
        return Err(SignError::CertificateMismatch(
            "signer certificate differs from the expected certificate".into(),
        ));
    }

    // Signed attributes.
    let attrs = si
        .signed_attrs
        .as_ref()
        .ok_or_else(|| invalid("signed attributes are required"))?;
    let mut seen = std::collections::HashSet::new();
    let mut content_type = None;
    let mut message_digest = None;
    let mut ess = None;
    for a in attrs.iter() {
        if !seen.insert(a.oid) {
            return Err(SignError::InvalidCms(format!(
                "attribute {} repeated",
                a.oid
            )));
        }
        let [value] = a.values.as_slice() else {
            return Err(SignError::InvalidCms(format!(
                "attribute {} must have exactly one value",
                a.oid
            )));
        };
        match a.oid {
            o if o == rfc5911::ID_CONTENT_TYPE => content_type = Some(value),
            o if o == rfc5911::ID_MESSAGE_DIGEST => message_digest = Some(value),
            o if o == rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2 => ess = Some(value),
            o if o == rfc5911::ID_AA_SIGNING_CERTIFICATE => {
                return Err(invalid(
                    "ESS signing-certificate (v1, SHA-1) is not supported",
                ));
            }
            o if o == rfc5911::ID_SIGNING_TIME
                && exp.scheme.sub_filter == SubFilter::EtsiCadesDetached =>
            {
                return Err(invalid("CAdES signatures must not carry signingTime"));
            }
            _ => {}
        }
    }
    let ct: ObjectIdentifier = content_type
        .ok_or_else(|| invalid("content-type attribute missing"))?
        .decode_as()
        .map_err(|_| invalid("content-type attribute malformed"))?;
    if ct != rfc5911::ID_DATA {
        return Err(invalid("content-type attribute is not id-data"));
    }
    let md: OctetString = message_digest
        .ok_or_else(|| invalid("message-digest attribute missing"))?
        .decode_as()
        .map_err(|_| invalid("message-digest attribute malformed"))?;
    if md.as_bytes() != exp.pdf_digest {
        return Err(invalid(
            "message-digest does not match the PDF ByteRange digest",
        ));
    }
    // Required for both profiles: it binds the signer certificate into the
    // signed attributes so a substituted certificate is detectable.
    check_ess(
        ess.ok_or_else(|| invalid("ESS signing-certificate-v2 attribute missing"))?,
        &leaf_der,
    )?;

    let attrs_der = attrs.to_der()?;
    verify_signature(
        &exp.scheme,
        leaf.tbs_certificate().subject_public_key_info(),
        &attrs_der,
        si.signature.as_bytes(),
    )?;
    Ok((*leaf).clone())
}

/// Verify an existing detached PDF signature and return the signer plus the
/// certificates embedded in its CMS SignedData.
pub(crate) fn verify_existing(
    der: &[u8],
    covered_bytes: &[u8],
    sub_filter: SubFilter,
) -> Result<(Certificate, Vec<Certificate>, Scheme), SignError> {
    let ci = ContentInfo::from_der(der).map_err(|e| SignError::InvalidCms(format!("DER: {e}")))?;
    if ci.to_der()? != der || ci.content_type != rfc5911::ID_SIGNED_DATA {
        return Err(SignError::InvalidCms(
            "CMS is not canonical detached SignedData".into(),
        ));
    }
    let sd: SignedData = ci
        .content
        .decode_as()
        .map_err(|e| SignError::InvalidCms(format!("SignedData: {e}")))?;
    let [si] = sd.signer_infos.0.as_slice() else {
        return Err(SignError::InvalidCms(
            "exactly one SignerInfo is required".into(),
        ));
    };
    let digest = match si.digest_alg.oid {
        oid if oid == rfc5912::ID_SHA_256 => DigestAlgorithm::Sha256,
        oid if oid == rfc5912::ID_SHA_384 => DigestAlgorithm::Sha384,
        oid if oid == rfc5912::ID_SHA_512 => DigestAlgorithm::Sha512,
        oid => {
            return Err(SignError::UnsupportedAlgorithm(format!(
                "CMS digest algorithm {oid}"
            )))
        }
    };
    let algorithm = match si.signature_algorithm.oid {
        oid if oid == rfc5912::ID_RSASSA_PSS => SignatureAlgorithm::RsassaPss,
        oid if oid == rfc5912::RSA_ENCRYPTION
            || oid == rfc5912::SHA_256_WITH_RSA_ENCRYPTION
            || oid == rfc5912::SHA_384_WITH_RSA_ENCRYPTION
            || oid == rfc5912::SHA_512_WITH_RSA_ENCRYPTION =>
        {
            SignatureAlgorithm::RsaPkcs1v15
        }
        oid if oid == rfc5912::ECDSA_WITH_SHA_256 => SignatureAlgorithm::EcdsaP256,
        oid if oid == rfc5912::ECDSA_WITH_SHA_384 => SignatureAlgorithm::EcdsaP384,
        oid => {
            return Err(SignError::UnsupportedAlgorithm(format!(
                "CMS signature algorithm {oid}"
            )))
        }
    };
    let scheme = Scheme::new(sub_filter, digest, algorithm, SigLevel::BB)?;
    scheme.check_signature_algorithm_id(&si.signature_algorithm)?;

    let SignerIdentifier::IssuerAndSerialNumber(isn) = &si.sid else {
        return Err(SignError::InvalidCms(
            "only issuerAndSerialNumber signer identifiers are supported".into(),
        ));
    };
    let certificates: Vec<Certificate> = sd
        .certificates
        .as_ref()
        .map(|set| {
            set.0
                .iter()
                .filter_map(|choice| match choice {
                    CertificateChoices::Certificate(cert) => Some(cert.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if certificates.len() > MAX_CMS_CERTIFICATES {
        return Err(SignError::InvalidCms(format!(
            "CMS contains more than {MAX_CMS_CERTIFICATES} certificates"
        )));
    }
    let matches: Vec<_> = certificates
        .iter()
        .filter(|cert| {
            cert.tbs_certificate().issuer() == &isn.issuer
                && cert.tbs_certificate().serial_number() == &isn.serial_number
        })
        .collect();
    let [leaf] = matches.as_slice() else {
        return Err(SignError::InvalidCms(format!(
            "{} certificates match the signer identifier; exactly one is required",
            matches.len()
        )));
    };
    let leaf_der = leaf.to_der()?;
    let pdf_digest = algorithm::digest_parts(scheme.digest, &[covered_bytes]);
    let expectation = Expectation {
        scheme,
        pdf_digest: &pdf_digest,
        leaf_sha256: sha256(&leaf_der),
    };
    let verified_leaf = verify(der, &expectation)?;
    Ok((verified_leaf, certificates, scheme))
}

fn check_ess(value: &Any, leaf_der: &[u8]) -> Result<(), SignError> {
    let invalid = |m: &str| SignError::InvalidCms(m.to_string());
    let ess: SigningCertificateV2 = value
        .decode_as()
        .map_err(|_| invalid("ESS signing-certificate-v2 malformed"))?;
    let first = ess
        .certs
        .first()
        .ok_or_else(|| invalid("ESS signing-certificate-v2 lists no certificate"))?;
    let digest = match &first.hash_algorithm {
        None => DigestAlgorithm::Sha256,
        Some(h) if params_absent_or_null(h) && h.oid == rfc5912::ID_SHA_256 => {
            DigestAlgorithm::Sha256
        }
        Some(h) if params_absent_or_null(h) && h.oid == rfc5912::ID_SHA_384 => {
            DigestAlgorithm::Sha384
        }
        Some(h) if params_absent_or_null(h) && h.oid == rfc5912::ID_SHA_512 => {
            DigestAlgorithm::Sha512
        }
        Some(_) => return Err(invalid("ESS certHash algorithm unsupported")),
    };
    if first.cert_hash.as_bytes() != algorithm::digest_parts(digest, &[leaf_der]) {
        return Err(SignError::CertificateMismatch(
            "ESS signing-certificate-v2 does not name the signer certificate".into(),
        ));
    }
    Ok(())
}

/// Exact DER length of the CMS at the start of a zero-padded `/Contents`.
pub fn der_prefix_len(contents: &[u8]) -> Result<usize, SignError> {
    let mut reader = der::SliceReader::new(contents)?;
    let header = der::Header::decode(&mut reader)?;
    let header_len = usize::try_from(u32::from(der::Reader::position(&reader)))
        .map_err(|_| SignError::InvalidCms("length overflow".into()))?;
    let body = usize::try_from(u32::from(header.length()))
        .map_err(|_| SignError::InvalidCms("length overflow".into()))?;
    let total = header_len
        .checked_add(body)
        .filter(|&t| t <= contents.len())
        .ok_or_else(|| SignError::InvalidCms("CMS length exceeds /Contents".into()))?;
    Ok(total)
}
