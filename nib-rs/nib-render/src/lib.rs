#![doc = "nib-render: PDFium-backed rendering engine (pdfium-render wrapper)."]
// unsafe_code is not forbidden here: the PdfiumLibraryBindings FFI surface is unsafe.

use std::path::Path;

use pdfium_render::prelude::*;
use thiserror::Error;

/// PDFium API feature this build of pdfium-render targets.
pub const PDFIUM_API_FEATURE: &str = "pdfium_8125";

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("pdfium: {0}")]
    Pdfium(String),
}

/// Loads PDFium (from `library_dir` when given, otherwise the already-mapped
/// system library) and returns a human-readable probe string.
pub fn pdfium_probe(library_dir: Option<&Path>) -> Result<String, RenderError> {
    let bindings = match library_dir {
        Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|e| RenderError::Pdfium(e.to_string()))?;
    // SAFETY: bindings were just successfully loaded; FPDF_GetLastError takes no
    // input and is safe to call on a valid library handle.
    let last_error = unsafe { bindings.FPDF_GetLastError() };
    Ok(format!(
        "pdfium ok (api {PDFIUM_API_FEATURE}, nib-render {}, FPDF_GetLastError={last_error})",
        env!("CARGO_PKG_VERSION")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn host_pdfium_dir() -> PathBuf {
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
