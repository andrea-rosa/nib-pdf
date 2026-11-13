//! Annotation entities (JSON_SCHEMA §4–§6.1).

use super::{default_true, one, Color};
use crate::geom::{Point, QuadPoints, Rect};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Annotation discriminator: `subtype` (§4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "subtype")]
pub enum Annotation {
    Ink(InkAnnot),
    Highlight(MarkupAnnot),
    Underline(MarkupAnnot),
    StrikeOut(MarkupAnnot),
    Squiggly(MarkupAnnot),
    FreeText(FreeTextAnnot),
    Text(TextAnnot),
    Stamp(StampAnnot),
    Square(ShapeAnnot),
    Circle(ShapeAnnot),
    Line(LineAnnot),
}

impl Annotation {
    pub fn base(&self) -> &AnnotBase {
        match self {
            Self::Ink(a) => &a.base,
            Self::Highlight(a) | Self::Underline(a) | Self::StrikeOut(a) | Self::Squiggly(a) => {
                &a.base
            }
            Self::FreeText(a) => &a.base,
            Self::Text(a) => &a.base,
            Self::Stamp(a) => &a.base,
            Self::Square(a) | Self::Circle(a) => &a.base,
            Self::Line(a) => &a.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut AnnotBase {
        match self {
            Self::Ink(a) => &mut a.base,
            Self::Highlight(a) | Self::Underline(a) | Self::StrikeOut(a) | Self::Squiggly(a) => {
                &mut a.base
            }
            Self::FreeText(a) => &mut a.base,
            Self::Text(a) => &mut a.base,
            Self::Stamp(a) => &mut a.base,
            Self::Square(a) | Self::Circle(a) => &mut a.base,
            Self::Line(a) => &mut a.base,
        }
    }
}

/// Keys shared by all annotations (§4), flattened into each subtype payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnnotBase {
    pub id: Uuid,
    pub page: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default = "one")]
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "AnnotFlags::is_default")]
    pub flags: AnnotFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popup: Option<Popup>,
    #[serde(default, skip_serializing_if = "Appearance::is_default")]
    pub appearance: Appearance,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub custom: serde_json::Map<String, Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// `/F` bits 2,3,8,7,6 (§4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnnotFlags {
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "default_true")]
    pub print: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub no_view: bool,
}

impl Default for AnnotFlags {
    fn default() -> Self {
        Self {
            hidden: false,
            print: true,
            locked: false,
            read_only: false,
            no_view: false,
        }
    }
}

impl AnnotFlags {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `/AP` handling (§4): `regenerate` (default) rebuilds `/AP`; otherwise
/// `apHash` preserves an imported foreign appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    #[serde(default = "default_true")]
    pub regenerate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ap_hash: Option<String>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            regenerate: true,
            ap_hash: None,
        }
    }
}

impl Appearance {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Popup {
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
}

/// FreeText callout (`/CL`, `/LE`, §5.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Callout {
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<LineEnding>,
}

/// `/BS` (§5.3/§5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BorderStyle {
    pub style: BorderStyleKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
}

/// Cloudy-border effect `/BE` (§5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cloudy {
    pub intensity: u8,
}

/// Line caption `/Cap`, `/CP` (§5.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineCaption {
    pub text: String,
    pub position: CaptionPosition,
}

/// Signer metadata for visual e-signatures (§6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VisualSigner {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_hint: Option<String>,
}

// ---------------------------------------------------------------- §5.1 Ink

/// One ink stroke; `points` rows are `[x, y, pressure?, tMs?]` — arity 2–4 is
/// checked by delta validation, not serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub points: Vec<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<InkTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Smoothing {
    pub algorithm: SmoothingAlgorithm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tension: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resample_spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplify_tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_error: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InkAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_cap: Option<LineCap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_join: Option<LineJoin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_sensitive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothing: Option<Smoothing>,
    pub strokes: Vec<Stroke>,
    // Visual-signature extension (§6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<VisualSigner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_to_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_mode: Option<FitMode>,
}

// ------------------------------------------------- §5.2 text markup

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MarkupAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    pub quad_points: Vec<QuadPoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_mode: Option<BlendMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width_factor: Option<f64>,
}

// ------------------------------------------------------- §5.3 FreeText

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FreeTextAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<FontWeight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<FontStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<VerticalAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<BorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callout: Option<Callout>,
}

// ----------------------------------------------------- §5.4 Text (note)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<TextIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ReviewState>,
}

// --------------------------------------------------------- §5.5 Stamp

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StampAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<StampKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<StampImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<VectorStamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub preserve_aspect: bool,
    // Visual-signature extension (§6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<VisualSigner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_to_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_mode: Option<FitMode>,
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StampImage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VectorStamp {
    pub paths: Vec<VectorPath>,
    pub view_box: [f64; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VectorPath {
    /// SVG-path-like mini-language (`M L C Z` only).
    pub d: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
}

// --------------------------------------------- §5.6 Square / Circle

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShapeAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<BorderStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloudy: Option<Cloudy>,
}

// ---------------------------------------------------------- §5.7 Line

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineAnnot {
    #[serde(flatten)]
    pub base: AnnotBase,
    pub start: Point,
    pub end: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_endings: Option<Vec<LineEnding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_extension: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<LineCaption>,
}

// ------------------------------------------------------------- enums

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineCap {
    Round,
    Butt,
    Square,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineJoin {
    Round,
    Miter,
    Bevel,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SmoothingAlgorithm {
    None,
    CatmullRom,
    BezierFit,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InkTool {
    Stylus,
    Finger,
    Mouse,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Signature,
    Initials,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    Multiply,
    Normal,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TextIcon {
    Comment,
    Key,
    Note,
    Help,
    NewParagraph,
    Paragraph,
    Insert,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReviewState {
    Accepted,
    Rejected,
    Cancelled,
    Completed,
    Marked,
    Unmarked,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StampKind {
    Image,
    Vector,
    Standard,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyleKind {
    Solid,
    Dashed,
    Beveled,
    Inset,
    Underline,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    #[serde(rename = "rOpenArrow")]
    ROpenArrow,
    #[serde(rename = "rClosedArrow")]
    RClosedArrow,
    Slash,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    Contain,
    Stretch,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Normal,
    Bold,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Italic,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CaptionPosition {
    Inline,
    Top,
}
