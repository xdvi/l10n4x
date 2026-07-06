//! Scoped store FFI tests (P2.5 multi-tenant isolation).

use std::ffi::CString;

use l10n4c::{
    l10n4c_store_create, l10n4c_store_destroy, l10n4c_store_translate, l10n4c_translate, L10N4C_OK,
};
use l10n4x_core::binary_format::{fnv1a_64, pack_l10n, RUNTIME_VERSION};
use l10n4x_core::loader::try_load_static_bytes_for_store;
use l10n4x_core::store_registry::StoreHandle;

fn pak_with_text(hash: u64, text: &[u8]) -> Vec<u8> {
    let entries: Vec<(u64, Vec<u8>)> = vec![(hash, text.to_vec())];
    pack_l10n(&entries, RUNTIME_VERSION, 1, None)
}

fn store_translate_helper(store_handle: u32, locale: &CString, key_hash: u64) -> String {
    let mut buf = [0u8; 64];
    let code = l10n4c_store_translate(
        store_handle,
        locale.as_ptr(),
        key_hash,
        buf.as_mut_ptr(),
        buf.len(),
    );
    assert_eq!(code, L10N4C_OK);
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..nul]).unwrap().to_string()
}

#[test]
fn ffi_scoped_store_translate_isolated() {
    let h = l10n4c_store_create();
    assert_ne!(h, 0);

    let handle = StoreHandle::from_raw(h).unwrap();
    let key = fnv1a_64(b"hello");
    let pak = pak_with_text(key, b"scoped");
    try_load_static_bytes_for_store(Some(handle), "en", &pak, true).unwrap();

    let locale = CString::new("en").unwrap();
    let mut global_buf = [0u8; 64];
    let global_code = l10n4c_translate(
        locale.as_ptr(),
        key,
        global_buf.as_mut_ptr(),
        global_buf.len(),
    );
    assert!(
        global_code == L10N4C_OK
            || global_code == l10n4c::L10N4C_KEY_NOT_FOUND
            || global_code == l10n4c::L10N4C_LOCALE_NOT_LOADED
    );
    let global_nul = global_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(global_buf.len());
    let global = std::str::from_utf8(&global_buf[..global_nul]).unwrap();
    assert_ne!(global, "scoped");

    assert_eq!(store_translate_helper(h, &locale, key), "scoped");

    let rc = l10n4c_store_destroy(h);
    assert_eq!(rc, L10N4C_OK);
}
