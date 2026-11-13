//! Serde model for the delta JSON format (`docs/JSON_SCHEMA.md` §2–§7).
//! All numbers are `f64`; unknown keys are preserved via `extra` maps.

pub mod annotation;
pub mod form;
pub mod schema;
pub mod signature;

pub use annotation::*;
pub use form::*;
pub use signature::*;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use uuid::Uuid;

#[cfg(test)]
mod tests;

pub(crate) fn one() -> f64 {
    1.0
}

pub(crate) fn default_true() -> bool {
    true
}

/// `#RRGGBB` colour, validated on deserialization.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Color(pub String);

impl Color {
    fn is_valid(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 7 && b[0] == b'#' && s[1..].chars().all(|c| c.is_ascii_hexdigit())
    }
}

impl TryFrom<String> for Color {
    type Error = &'static str;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        if Self::is_valid(&s) {
            Ok(Self(s))
        } else {
            Err("colour must be a #RRGGBB string")
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Color {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^#[0-9A-Fa-f]{6}$"
        })
    }
}

/// Root envelope (§2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeltaDocument {
    pub schema_version: u32,
    pub document_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_id: Option<[String; 2]>,
    pub base_revision: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    pub ops: Vec<Op>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// A single delta operation (§3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Op {
    pub op_id: Uuid,
    pub ts: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub kind: OpKind,
    pub target: Target,
    /// Entity id; the fully-qualified field name for `formField`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// Create: full entity; update: RFC 7386 merge patch; delete: `{}` or `{ "reason" }`.
    pub payload: Value,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    Annotation,
    FormField,
    Signature,
}
