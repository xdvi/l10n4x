//! `l10n4x-wasm` — WebAssembly bindings for `l10n4x`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn l10n4x_set_verify_key(key: &[u8]) -> bool {
    l10n4x_core::integrity::set_verify_key(key)
}

#[wasm_bindgen]
pub fn l10n4x_set_decrypt_key(key: &[u8]) -> bool {
    l10n4x_core::encryption::set_decrypt_key(key)
}

#[wasm_bindgen]
pub fn l10n4x_set_fallback_locale(locale: &str) {
    l10n4x_core::store::set_fallback_locale(locale);
}

#[wasm_bindgen]
pub fn l10n4x_load_pak_bytes(bytes: &[u8], locale: &str) -> Result<(), JsValue> {
    if !l10n4x_core::integrity::verify_key_configured() {
        return Err(JsValue::from_str(
            "Signature verification failed: verify key not set or invalid",
        ));
    }
    match l10n4x_core::pak::decompress_pak(bytes) {
        Ok(decompressed) => {
            if l10n4x_core::loader::load_raw_bytes(locale, &decompressed) {
                Ok(())
            } else {
                Err(JsValue::from_str(
                    "Failed to load decompressed pak bytes into store",
                ))
            }
        }
        Err(err) => {
            let msg = format!("Invalid format or decompression failed: {}", err);
            Err(JsValue::from_str(&msg))
        }
    }
}

#[wasm_bindgen]
pub fn l10n4x_translate(locale: &str, key: &str) -> String {
    l10n4x_core::store::translate(locale, key, &[])
}

#[wasm_bindgen]
pub fn l10n4x_translate_with_params(
    locale: &str,
    key: &str,
    param_keys: Vec<String>,
    param_values: Vec<String>,
) -> String {
    if param_keys.len() != param_values.len() {
        return key.to_string();
    }
    let params: Vec<(&str, &str)> = param_keys
        .iter()
        .zip(param_values.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    l10n4x_core::store::translate(locale, key, &params)
}

#[wasm_bindgen]
pub fn l10n4x_clear() {
    l10n4x_core::store::clear_translations();
}
