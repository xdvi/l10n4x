//! `l10n4x-wasm` is the WebAssembly bindings crate for `l10n4x`.
//! It exposes JS-compatible interfaces for loading `.pak` buffers and resolving translations.

use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// Sets the global 32-byte AES key for GCM decryption.
#[wasm_bindgen]
pub fn l10n4x_set_encryption_key(key: &[u8]) -> bool {
    l10n4x_core::crypto::set_encryption_key(key)
}

/// Sets the global fallback locale (defaults to "en").
#[wasm_bindgen]
pub fn l10n4x_set_fallback_locale(locale: &str) -> bool {
    l10n4x_core::store::set_fallback_locale(locale)
}

/// Decrypts and loads a single `.pak` file buffer for a given locale.
#[wasm_bindgen]
pub fn l10n4x_load_pak_bytes(bytes: &[u8], locale: &str) -> bool {
    l10n4x_core::loader::load_pak_bytes(locale, bytes)
}

/// Resolves the translation for a key and locale, dynamically interpolating JSON parameters.
#[wasm_bindgen]
pub fn l10n4x_translate(locale: &str, key: &str, params_json: &str) -> String {
    let mut params = HashMap::new();
    if !params_json.is_empty() {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(params_json) {
            params = parsed;
        }
    }

    let params_vec: Vec<(&str, &str)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let mut resolved_str = String::new();
    if l10n4x_core::store::translate_to_writer(locale, key, &params_vec, &mut resolved_str).is_err()
    {
        resolved_str = key.to_string();
    }

    resolved_str
}

/// Clears all loaded translations from the global store.
#[wasm_bindgen]
pub fn l10n4x_clear() {
    l10n4x_core::store::clear_translations();
}
