//! Validation error codes (JSON_SCHEMA §3).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ErrorCode {
    #[serde(rename = "UNKNOWN_ID")]
    UnknownId,
    #[serde(rename = "DUPLICATE_ID")]
    DuplicateId,
    #[serde(rename = "SCHEMA")]
    Schema,
    #[serde(rename = "PAGE_OUT_OF_RANGE")]
    PageOutOfRange,
    #[serde(rename = "RECT_INVALID")]
    RectInvalid,
    #[serde(rename = "SIGNATURE_LOCKED")]
    SignatureLocked,
    #[serde(rename = "FIELD_READONLY")]
    FieldReadonly,
    #[serde(rename = "VALUE_NOT_IN_OPTIONS")]
    ValueNotInOptions,
    #[serde(rename = "MAXLEN_EXCEEDED")]
    MaxlenExceeded,
    #[serde(rename = "REVISION_MISMATCH")]
    RevisionMismatch,
    /// The op is well-formed but cannot be applied through this entry point
    /// (signature ops: digital signing uses the dedicated signing API).
    #[serde(rename = "UNSUPPORTED_OP")]
    UnsupportedOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub op_index: usize,
    pub path: String,
    pub code: ErrorCode,
    pub message: String,
}

impl ValidationError {
    pub fn new(
        op_index: usize,
        path: impl Into<String>,
        code: ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            op_index,
            path: path.into(),
            code,
            message: message.into(),
        }
    }
}
