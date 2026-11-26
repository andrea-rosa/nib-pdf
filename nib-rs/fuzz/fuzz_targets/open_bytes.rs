#![no_main]

use libfuzzer_sys::fuzz_target;
use nib_core::writer::Writer;

fuzz_target!(|data: &[u8]| {
    let _ = Writer::open(data.to_vec());
});
