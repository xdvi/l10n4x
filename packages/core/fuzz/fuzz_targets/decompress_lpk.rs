#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = l10n4x_core::lpk::decompress_lpk(data);
});
