#![doc = "nib-render: PDFium-backed rendering engine (pdfium-render wrapper)."]
// unsafe_code is not forbidden here: the PdfiumLibraryBindings FFI surface is unsafe.

mod engine;
#[cfg(test)]
mod golden;
mod queue;
mod renderer;
pub mod types;
mod worker;

use std::path::Path;

use pdfium_render::prelude::*;
use thiserror::Error;

pub use types::{IRect, ISize, PageInfo, Priority, RenderError, TileKey, TileRequest, TileResult};
pub use worker::RenderWorker;

/// PDFium API feature this build of pdfium-render targets.
pub const PDFIUM_API_FEATURE: &str = "pdfium_8125";

/// Binding-only probe error kept separate so `pdfium_probe` never needs the
/// full `RenderError` surface.
#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("pdfium: {0}")]
    Pdfium(String),
}

/// Loads PDFium (from `library_dir` when given, otherwise the already-mapped
/// system library) and returns a human-readable probe string.
///
/// Binding only: this never calls `Pdfium::new`, so it does not run
/// `FPDF_InitLibrary` — the render engine owns the single initialization.
pub fn pdfium_probe(library_dir: Option<&Path>) -> Result<String, ProbeError> {
    let bound = match library_dir {
        Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)),
        None => Pdfium::bind_to_system_library(),
    };
    let bindings = match bound {
        Ok(b) => b,
        // The render worker already bound and initialized the library — the
        // probe's goal (PDFium is loaded and callable) is met.
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
            return Ok(format!(
                "pdfium ok (api {PDFIUM_API_FEATURE}, nib-render {}, already bound)",
                env!("CARGO_PKG_VERSION")
            ))
        }
        Err(e) => return Err(ProbeError::Pdfium(e.to_string())),
    };
    // SAFETY: bindings were just successfully loaded; FPDF_GetLastError takes no
    // input and is safe to call on a valid library handle.
    let last_error = unsafe { bindings.FPDF_GetLastError() };
    Ok(format!(
        "pdfium ok (api {PDFIUM_API_FEATURE}, nib-render {}, FPDF_GetLastError={last_error})",
        env!("CARGO_PKG_VERSION")
    ))
}

/// Shared helpers for unit and integration tests that need the host pdfium
/// binary (fetched by `tools/fetch-pdfium.sh`).
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, OnceLock};

    use crate::RenderWorker;

    pub fn host_pdfium_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("NIB_PDFIUM_DIR") {
            return dir.into();
        }
        let triple = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            "linux-x64"
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "mac-arm64"
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            "mac-x64"
        } else {
            panic!("unsupported host; set NIB_PDFIUM_DIR");
        };
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/pdfium")
            .join(triple)
    }

    pub fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures")
    }

    /// One process-wide worker for all tests: `Pdfium::new` /
    /// `FPDF_InitLibrary` may only run once per process.
    pub fn test_worker() -> Arc<RenderWorker> {
        static WORKER: OnceLock<Arc<RenderWorker>> = OnceLock::new();
        Arc::clone(WORKER.get_or_init(|| RenderWorker::spawn(Some(host_pdfium_dir()))))
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn pdfium_probe_loads() {
        let dir = host_pdfium_dir();
        assert!(
            dir.join("libpdfium.so").exists() || dir.join("libpdfium.dylib").exists(),
            "PDFium library not found under {} — run nib-rs/tools/fetch-pdfium.sh",
            dir.display()
        );
        let msg = pdfium_probe(Some(&dir)).expect("pdfium probe");
        assert!(msg.contains("pdfium ok"));
    }
}
