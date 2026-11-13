//! Shared geometry types: user-space ↔ device-space coordinate math.
//! Pure Rust, no PDFium — usable from nib-render and nib-ffi alike.

mod matrix;
mod point;
mod quad;
mod rect;
mod transform;

pub use matrix::Matrix;
pub use point::Point;
pub use quad::QuadPoints;
pub use rect::Rect;
pub use transform::{PageTransform, Rotation, RotationError};
