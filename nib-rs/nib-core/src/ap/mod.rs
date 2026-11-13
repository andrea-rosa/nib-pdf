//! Appearance-stream (`/AP /N` form XObject) generation for annotations.
//!
//! Every generator mirrors `AnnotationDrawing.kt` (the Compose overlay is the
//! reference renderer): same geometry, same defaults, y-up user-space
//! coordinates. Opacity is applied exclusively through a single `ExtGState`
//! named `GS0` emitted at the head of the content stream.

pub mod content;
pub mod fonts;
pub mod freetext;
pub mod image;
pub mod ink;
pub mod line;
pub mod markup;
pub mod note;
pub mod shape;
pub mod stamp;

#[cfg(test)]
mod tests;

use crate::geom::Rect;
use crate::model::{Annotation, BlendMode};

/// Result of [`build`]: the form-XObject `/BBox` (also the annotation
/// `/Rect`), the content stream bytes, and the resource dictionaries the
/// writer must place under `/Resources`.
pub struct ApOutput {
    pub bbox: Rect,
    pub content: Vec<u8>,
    /// `ExtGState` resources, keyed by name (e.g. `GS0`).
    pub ext_gstate: Vec<(String, lopdf::Dictionary)>,
    /// `/Font` resources, keyed by name (e.g. `Helv`).
    pub fonts: Vec<(String, lopdf::Dictionary)>,
    /// `/XObject` image resources, keyed by name (e.g. `Im0`).
    pub images: Vec<(String, ImageXObject)>,
    pub warnings: Vec<String>,
}

/// An image XObject plus its optional soft-mask (`/SMask`) XObject.
pub struct ImageXObject {
    pub stream: lopdf::Stream,
    pub smask: Option<lopdf::Stream>,
}

/// `/ExtGState` dict carrying the annotation's opacity and blend mode.
/// `/BM` is `/Multiply` for multiply, `/Normal` otherwise.
pub fn ext_gstate(opacity: f64, blend: BlendMode) -> lopdf::Dictionary {
    let mut d = lopdf::Dictionary::new();
    d.set("Type", lopdf::Object::Name(b"ExtGState".to_vec()));
    d.set("CA", lopdf::Object::Real(opacity as f32));
    d.set("ca", lopdf::Object::Real(opacity as f32));
    d.set(
        "BM",
        lopdf::Object::Name(match blend {
            BlendMode::Multiply => b"Multiply".to_vec(),
            BlendMode::Normal => b"Normal".to_vec(),
        }),
    );
    d
}

/// Source of raster assets referenced by `assetId` (stamp images).
pub trait AssetSource {
    fn asset(&self, asset_id: &str) -> Option<Vec<u8>>;
}

/// `AssetSource` that never resolves anything.
pub struct NoAssets;

impl AssetSource for NoAssets {
    fn asset(&self, _asset_id: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Source of embeddable font programs. M3 does not embed; consulted only to
/// decide whether to warn.
pub trait FontSource {
    fn font(&self, family: &str, bold: bool, italic: bool) -> Option<Vec<u8>>;
}

/// `FontSource` that never resolves anything.
pub struct NoFonts;

impl FontSource for NoFonts {
    fn font(&self, _family: &str, _bold: bool, _italic: bool) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApError {
    #[error("annotation has no usable geometry")]
    MissingGeometry,
    #[error("image: {0}")]
    Image(String),
    #[error("asset: {0}")]
    Asset(String),
}

pub(crate) fn color_or(
    c: &Option<crate::model::Color>,
    default: &str,
) -> crate::model::Color {
    c.clone()
        .unwrap_or_else(|| crate::model::Color(default.to_string()))
}

/// Build the appearance of one annotation. The returned `bbox` is both the
/// form XObject `/BBox` and the annotation `/Rect`.
pub fn build(
    a: &Annotation,
    assets: &dyn AssetSource,
    fonts: &dyn FontSource,
) -> Result<ApOutput, ApError> {
    let blend = match a {
        // Highlight defaults to multiply; every other subtype is Normal.
        Annotation::Highlight(m) => m.blend_mode.unwrap_or(BlendMode::Multiply),
        _ => BlendMode::Normal,
    };
    let mut out = match a {
        Annotation::Ink(i) => ink::gen(i),
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => markup::gen(a, m),
        Annotation::FreeText(f) => freetext::gen(f, fonts),
        Annotation::Text(t) => note::gen(t),
        Annotation::Stamp(s) => stamp::gen(s, assets),
        Annotation::Square(s) | Annotation::Circle(s) => shape::gen(a, s),
        Annotation::Line(l) => line::gen(l),
    }?;
    out.ext_gstate = vec![("GS0".to_string(), ext_gstate(a.base().opacity, blend))];
    // GS0 must be the first graphics-state applied.
    let mut content = b"/GS0 gs\n".to_vec();
    content.extend_from_slice(&out.content);
    out.content = content;
    Ok(out)
}
