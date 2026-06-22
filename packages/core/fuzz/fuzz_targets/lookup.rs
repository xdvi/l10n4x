#![no_main]

extern crate alloc;
use alloc::vec::Vec;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Create minimal valid binary format
    let mut buf = Vec::new();
    buf.extend_from_slice(b"L10N");
    buf.extend_from_slice(&1u32.to_be_bytes()); // format version
    buf.extend_from_slice(&16u32.to_be_bytes()); // index offset
    buf.extend_from_slice(&0u32.to_be_bytes()); // index count
    if let Ok(reader) = l10n4x_core::binary_format::BinaryFormatReader::new(&buf) {
        if let Ok(key_str) = core::str::from_utf8(data) {
            let _ = reader.lookup(key_str);
        }
    }
});
