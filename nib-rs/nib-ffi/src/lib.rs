#![doc = "nib-ffi: UniFFI surface for nib — thin facade over nib-core, nib-render, nib-sign."]
#![forbid(unsafe_code)]

mod document;
mod error;

pub use document::{
    IRect, ISize, NibDocument, OpenOptions, PageInfo, Point, Priority, Rect, Rotation, TileKey,
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
    #[test]
    fn version_matches_cargo() {
        assert_eq!(super::nib_version(), env!("CARGO_PKG_VERSION"));
    }
}
