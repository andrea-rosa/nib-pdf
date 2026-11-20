//! AcroForm field entities (JSON_SCHEMA §7).

use super::{Color, TextAlign};
use crate::geom::Rect;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `target: "formField"` payload (§7.1). `id` on the op is the fully-qualified
/// `name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub name: String,
    pub field_type: FieldType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_state: Option<SignatureFieldState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(default, skip_serializing_if = "FieldFlags::is_default")]
    pub flags: FieldFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_len: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_appearance: Option<DefaultAppearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FieldOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<Widget>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FieldType {
    Text,
    Checkbox,
    Radio,
    PushButton,
    Combo,
    List,
    Signature,
}

/// Whether a signature field can accept a new signature, not its validity.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SignatureFieldState {
    Empty,
    Occupied,
    Unknown,
}

/// `/Ff` bits — common (1/2/3) plus the per-type flags of §7.2.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldFlags {
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub no_export: bool,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub password: bool,
    #[serde(default)]
    pub comb: bool,
    #[serde(default)]
    pub do_not_spell_check: bool,
    #[serde(default)]
    pub do_not_scroll: bool,
    #[serde(default)]
    pub rich_text: bool,
    #[serde(default)]
    pub no_toggle_to_off: bool,
    #[serde(default)]
    pub radios_in_unison: bool,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub sort: bool,
    #[serde(default)]
    pub multi_select: bool,
}

impl FieldFlags {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `/DA` (§7.1). `fontSize: 0` = auto-size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAppearance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldOption {
    pub export: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Widget annotation of a field (`/Kids`, §7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub page: u32,
    pub rect: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance_state: Option<String>,
    /// Per-kid export value (`/AP /N` non-`Off` state key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<super::BorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}
