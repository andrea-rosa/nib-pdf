#![doc = "nib-core: PDF document model, delta engine, undo/redo, appearance-stream generation, and writer. Never depends on PDFium."]
#![forbid(unsafe_code)]

pub mod ap;
pub mod delta;
pub mod forms;
pub mod geom;
pub mod model;
pub mod text;
pub mod writer;
