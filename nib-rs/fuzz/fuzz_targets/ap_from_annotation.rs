#![no_main]

use libfuzzer_sys::fuzz_target;
use nib_core::ap::{build, NoAssets, NoFonts};
use nib_core::model::Annotation;

fuzz_target!(|data: &[u8]| {
    if let Ok(annotation) = serde_json::from_slice::<Annotation>(data) {
        let _ = build(&annotation, &NoAssets, &NoFonts);
    }
});
