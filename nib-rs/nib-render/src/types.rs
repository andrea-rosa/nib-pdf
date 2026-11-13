//! Shared types for the tile-rendering surface.

use nib_core::geom::{Rect, Rotation};

pub use nib_core::text::CharBox;

/// Identifies one cached tile. `flags` encodes render options
/// (render_annotations, render_forms, grayscale).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub page: u16,
    /// Zoom bucket; scale = 2^(bucket/2).
    pub bucket: i8,
    pub tx: u16,
    pub ty: u16,
    /// Document revision the tile was rendered from (minimum, not equality).
    pub revision: u32,
    pub flags: u8,
}

/// Render priority: visible > prefetch > thumbnail.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Thumbnail = 0,
    Prefetch = 1,
    Visible = 2,
}

/// Integer rect in device pixels.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Integer size in device pixels.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ISize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
pub struct TileRequest {
    pub key: TileKey,
    /// Tile rect within the page bitmap at `key.bucket`.
    pub device_rect: IRect,
    /// Whole-page bitmap size at `key.bucket`.
    pub page_device_size: ISize,
    /// Scheduling generation; jobs older than the current generation are skipped.
    pub generation: u64,
    pub priority: Priority,
    /// Distance from viewport centre; ties after priority break by distance.
    pub distance: u32,
}

/// A rendered tile. `pixels` is RGBA (BGRA rendered with
/// `FPDF_REVERSE_BYTE_ORDER`), tightly packed with `stride = width * 4`.
#[derive(Debug, Clone)]
pub struct TileResult {
    pub key: TileKey,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
    pub render_ms: u16,
}

/// Per-page geometry and metadata in PDF user space.
#[derive(Debug, Clone)]
pub struct PageInfo {
    /// `/CropBox` ∩ `/MediaBox` (what the viewer shows).
    pub crop_box: Rect,
    pub media_box: Rect,
    /// `/Rotate` of the page.
    pub rotation: Rotation,
    /// `/PageLabels` entry, if any.
    pub label: Option<String>,
    /// Display size in points (post-`/Rotate`; swapped for 90/270).
    pub width_pt: f64,
    pub height_pt: f64,
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("pdfium: {0}")]
    Pdfium(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("document is password-protected; encrypted PDFs are unsupported")]
    EncryptedUnsupported,
    #[error("cancelled")]
    Cancelled,
    #[error("internal: {0}")]
    Internal(String),
}
