//! `l10n4x-wasm` — WebAssembly bindings for `l10n4x`.

use std::collections::HashMap;
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
pub fn l10n4x_set_fallback_chain(locales: Vec<String>) {
    let refs: Vec<&str> = locales.iter().map(|s| s.as_str()).collect();
    l10n4x_core::store::set_fallback_chain(&refs);
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
            if l10n4x_core::loader::load_raw_bytes(locale, decompressed) {
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
    l10n4x_core::store::translate(locale, key, None, &[])
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
    l10n4x_core::store::translate(locale, key, None, &params)
}

/// Translates a key with context suffix support (e.g. `friend_male` → key = `friend`).
#[wasm_bindgen]
pub fn l10n4x_translate_with_context(locale: &str, key: &str, context: Option<String>) -> String {
    let ctx = context.as_deref();
    l10n4x_core::store::translate(locale, key, ctx, &[])
}

/// Translate with context and parameters.
#[wasm_bindgen]
pub fn l10n4x_translate_with_context_and_params(
    locale: &str,
    key: &str,
    context: Option<String>,
    param_keys: Vec<String>,
    param_values: Vec<String>,
) -> String {
    let ctx = context.as_deref();
    if param_keys.len() != param_values.len() {
        return key.to_string();
    }
    let params: Vec<(&str, &str)> = param_keys
        .iter()
        .zip(param_values.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    l10n4x_core::store::translate(locale, key, ctx, &params)
}

#[wasm_bindgen]
pub fn l10n4x_register_formatter(name: &str, callback: js_sys::Function) {
    let cb = callback.clone();
    l10n4x_core::formatter::register_formatter(name, Box::new(move |value: &str, _locale: &str, _options: &HashMap<String, String>| {
        let this = wasm_bindgen::JsValue::UNDEFINED;
        let arg1 = wasm_bindgen::JsValue::from_str(value);
        let result = cb.call1(&this, &arg1);
        match result {
            Ok(val) => val.as_string().unwrap_or_else(|| value.to_string()),
            Err(_) => value.to_string(),
        }
    }));
}

#[wasm_bindgen]
pub fn l10n4x_clear() {
    l10n4x_core::store::clear_translations();
}

/// Registers a JS callback invoked when a locale is loaded or cleared.
/// The callback receives the locale code that was loaded.
#[wasm_bindgen]
pub fn l10n4x_on_locale_changed(callback: js_sys::Function) {
    let cb = callback;
    l10n4x_core::store::on_locale_changed_boxed(Box::new(move |_locale: &str| {
        let _ = cb.call0(&wasm_bindgen::JsValue::UNDEFINED);
    }));
}

/// Returns `true` if the given locale's pak has been successfully loaded.
#[wasm_bindgen]
pub fn l10n4x_locale_loaded(locale: &str) -> bool {
    l10n4x_core::store::locale_loaded(locale)
}

/// Returns `true` if `key` exists in `locale` or the configured fallback chain.
#[wasm_bindgen]
pub fn l10n4x_key_exists(locale: &str, key: &str) -> bool {
    l10n4x_core::store::key_exists(locale, key, None)
}

/// Returns the list of locale codes that are currently loaded in memory.
#[wasm_bindgen]
pub fn l10n4x_get_loaded_locales() -> Vec<String> {
    l10n4x_core::store::read_store(|store| {
        store.locales.iter().map(|(loc, _)| loc.clone()).collect()
    })
}

#[cfg(test)]
mod export_tests {
    #[test]
    fn locale_loaded_returns_false_for_unknown() {
        super::l10n4x_clear();
        assert!(!super::l10n4x_locale_loaded("xx"));
    }

    #[test]
    fn key_exists_returns_false_without_pak() {
        super::l10n4x_clear();
        assert!(!super::l10n4x_key_exists("en", "any.key"));
    }

    #[test]
    fn get_loaded_locales_empty_after_clear() {
        super::l10n4x_clear();
        assert!(super::l10n4x_get_loaded_locales().is_empty());
    }
}
