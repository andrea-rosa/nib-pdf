//! Cryptographic digital-signature entities (JSON_SCHEMA §6.2).

use crate::geom::Rect;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `target: "signature"` payload (§6.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignatureEntity {
    /// Fully-qualified `/T`; creates the field if absent.
    pub field_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<SigWidget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_filter: Option<SubFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest_algorithm: Option<DigestAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_algorithm: Option<SignatureAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<SigLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<SigSigner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_mdp: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_fields_after_signing: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<SigAppearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<Timestamp>,
    #[serde(default)]
    pub status: SigStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_range: Option<[u64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SigWidget {
    pub page: u32,
    /// May be `[0,0,0,0]` for an invisible signature.
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SigSigner {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain_sha256: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SigAppearance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SigAppearanceMode>,
    /// `Ink`/`Stamp` annotation id whose content is copied into `/n2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphic_ref: Option<String>,
    /// Text displayed in the signature appearance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_name: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_date: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_reason: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Timestamp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsa_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_sha256: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SubFilter {
    #[serde(rename = "ETSI.CAdES.detached")]
    EtsiCadesDetached,
    #[serde(rename = "adbe.pkcs7.detached")]
    AdbePkcs7Detached,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum DigestAlgorithm {
    #[serde(rename = "SHA-256")]
    Sha256,
    #[serde(rename = "SHA-384")]
    Sha384,
    #[serde(rename = "SHA-512")]
    Sha512,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SignatureAlgorithm {
    #[serde(rename = "RSASSA-PSS")]
    RsassaPss,
    #[serde(rename = "RSA-PKCS1v15")]
    RsaPkcs1v15,
    #[serde(rename = "ECDSA-P256")]
    EcdsaP256,
    #[serde(rename = "ECDSA-P384")]
    EcdsaP384,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SigLevel {
    #[serde(rename = "B-B")]
    BB,
    #[serde(rename = "B-T")]
    BT,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SigAppearanceMode {
    Invisible,
    Description,
    Graphic,
    GraphicAndDescription,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SigStatus {
    #[default]
    Prepared,
    Signed,
    Failed,
}
