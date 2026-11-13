#![doc = "nib-ffi: UniFFI surface for nib — thin facade over nib-core, nib-render, nib-sign."]
#![forbid(unsafe_code)]

mod document;
mod error;

pub use document::{
    AnnotationSummary, ApplyReport, DeltaValidationError, DirtyRegion, ExportMode, IRect, ISize,
    ImportOptions, NibDocument, OpenOptions, PageInfo, Point, Priority, Rect, Rotation, TileKey,
    TileRequest, TileResult,
};
pub use error::NibError;

uniffi::setup_scaffolding!();

#[cfg(target_os = "android")]
fn init_logger() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        android_logger::init_once(android_logger::Config::default().with_tag("nib"));
    });
}

#[cfg(not(target_os = "android"))]
fn init_logger() {}

#[uniffi::export]
pub fn nib_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[uniffi::export]
pub fn pdfium_version() -> Result<String, NibError> {
    init_logger();
    match std::panic::catch_unwind(|| nib_render::pdfium_probe(None)) {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(nib_render::ProbeError::Pdfium(m))) => Err(NibError::Pdfium(m)),
        Err(_) => Err(NibError::Internal("panic in pdfium_probe".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    #[test]
    fn version_matches_cargo() {
        assert_eq!(super::nib_version(), env!("CARGO_PKG_VERSION"));
    }

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

    /// `worker()` binds the already-mapped library unless NIB_PDFIUM_DIR is
    /// set — set it before the first document is opened.
    fn open_text() -> Arc<NibDocument> {
        std::env::set_var("NIB_PDFIUM_DIR", host_pdfium_dir());
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures/text.pdf"),
        )
        .unwrap();
        NibDocument::from_bytes(
            bytes,
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap()
    }

    const CREATE_SQUARE: &str = r##"{
        "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c01",
        "ts": "2026-11-12T14:03:25Z",
        "type": "create",
        "target": "annotation",
        "id": "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55",
        "page": 0,
        "payload": {"subtype": "Square", "page": 0, "rect": [10, 10, 50, 50], "color": "#FF0000"}
    }"##;

    #[test]
    fn apply_undo_redo_roundtrip() {
        let doc = open_text();
        let rep = doc.apply(format!("[{CREATE_SQUARE}]")).unwrap();
        assert_eq!(rep.applied.len(), 1);
        assert_eq!(doc.annotations(0).len(), 1);
        assert_eq!(doc.annotations(0)[0].subtype, "Square");
        assert!(doc.can_undo());
        assert!(doc.undo().is_some());
        assert_eq!(doc.annotations(0).len(), 0);
        assert!(doc.can_redo());
        assert!(doc.redo().is_some());
        assert_eq!(doc.annotations(0).len(), 1);
    }

    #[test]
    fn text_lines_in_rect_two_lines() {
        let doc = open_text();
        let lines = doc
            .text_lines_in_rect(
                0,
                Rect {
                    x0: 30.0,
                    y0: 140.0,
                    x1: 190.0,
                    y1: 165.0,
                },
            )
            .unwrap();
        assert_eq!(lines.len(), 2, "lines: {lines:?}");
        assert!((lines[0].y1 - lines[1].y1 - 16.0).abs() <= 0.75);
    }

    #[test]
    fn invalid_delta_error_is_json() {
        let doc = open_text();
        let bad = CREATE_SQUARE.replace("\"page\": 0", "\"page\": 99");
        match doc.apply(format!("[{bad}]")) {
            Err(NibError::InvalidDelta(s)) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                let arr = v.as_array().unwrap();
                assert_eq!(arr[0]["code"], "PAGE_OUT_OF_RANGE");
            }
            other => panic!("expected InvalidDelta, got {other:?}"),
        }
    }
}
