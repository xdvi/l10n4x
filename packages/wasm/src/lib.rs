//! `l10n4x-wasm` — WebAssembly bindings for `l10n4x`.
//!
//! Mirrors the runtime API of the `l10n4c` C FFI layer, including context-suffix
//! translation and locale-change callbacks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use l10n4x_core::binary_format::fnv1a_64;
use l10n4x_core::ota::{ota_can_rollback, try_ota_reload_lpk, try_ota_rollback};
use l10n4x_core::store::{hash_params, translate_to_writer_with_status, TranslateStatus};
use wasm_bindgen::prelude::*;

struct CachedTranslate {
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
    text: Arc<str>,
}

thread_local! {
    static LAST_TRANSLATE: RefCell<Option<CachedTranslate>> = const { RefCell::new(None) };
    static TRANSLATE_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

fn cache_lookup(
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
) -> Option<Arc<str>> {
    LAST_TRANSLATE.with(|cell| {
        let cached = cell.borrow();
        let entry = cached.as_ref()?;
        if entry.locale_hash == locale_hash
            && entry.key_hash == key_hash
            && entry.context_hash == context_hash
            && entry.params_key == params_key
        {
            Some(Arc::clone(&entry.text))
        } else {
            None
        }
    })
}

fn clear_translate_cache() {
    LAST_TRANSLATE.with(|cell| *cell.borrow_mut() = None);
}

fn cache_store(
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
    text: Arc<str>,
) {
    LAST_TRANSLATE.with(|cell| {
        *cell.borrow_mut() = Some(CachedTranslate {
            locale_hash,
            key_hash,
            context_hash,
            params_key,
            text,
        });
    });
}

fn translate_cached(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> String {
    let locale_hash = fnv1a_64(locale.as_bytes());
    let params_key = if params.is_empty() {
        if let Some(cached) = cache_lookup(locale_hash, key_hash, context_hash, 0) {
            return cached.to_string();
        }
        0
    } else {
        let pk = hash_params(params);
        if let Some(cached) = cache_lookup(locale_hash, key_hash, context_hash, pk) {
            return cached.to_string();
        }
        pk
    };

    let (text, key_found) = TRANSLATE_BUF.with(|cell| {
        let mut guard = cell.borrow_mut();
        guard.clear();
        let status =
            translate_to_writer_with_status(locale, key_hash, context_hash, params, &mut *guard)
                .unwrap_or(TranslateStatus {
                    key_found: false,
                    locale_loaded: false,
                });
        // clone() instead of mem::take: taking would zero the buffer's
        // capacity and force it to re-grow on every call.
        (guard.clone(), status.key_found)
    });

    if key_found {
        cache_store(
            locale_hash,
            key_hash,
            context_hash,
            params_key,
            Arc::<str>::from(text.as_str()),
        );
    }
    text
}

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

/// Merges a signed namespace `.lpk` into an existing locale (modular bundle mode).
#[wasm_bindgen]
pub fn l10n4x_load_namespace_bytes(
    bytes: &[u8],
    locale: &str,
    namespace: &str,
) -> Result<(), JsValue> {
    if !l10n4x_core::integrity::verify_key_configured() {
        return Err(JsValue::from_str(
            "Signature verification failed: verify key not set or invalid",
        ));
    }
    match l10n4x_core::lpk::decompress_lpk(bytes) {
        Ok(decompressed) => {
            match l10n4x_core::loader::try_load_namespace_bytes(locale, namespace, decompressed) {
                Ok(()) => {
                    clear_translate_cache();
                    Ok(())
                }
                Err(err) => Err(JsValue::from_str(&format!(
                    "Namespace load failed: {}",
                    err
                ))),
            }
        }
        Err(err) => Err(JsValue::from_str(&format!(
            "Invalid format or decompression failed: {}",
            err
        ))),
    }
}

#[wasm_bindgen]
pub fn l10n4x_load_lpk_bytes(bytes: &[u8], locale: &str) -> Result<(), JsValue> {
    if !l10n4x_core::integrity::verify_key_configured() {
        return Err(JsValue::from_str(
            "Signature verification failed: verify key not set or invalid",
        ));
    }
    match l10n4x_core::lpk::decompress_lpk(bytes) {
        Ok(decompressed) => {
            if l10n4x_core::loader::load_raw_bytes(locale, decompressed) {
                clear_translate_cache();
                Ok(())
            } else {
                Err(JsValue::from_str(
                    "Failed to load decompressed lpk bytes into store",
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
pub fn l10n4x_translate(locale: &str, key_hash: u64) -> String {
    translate_cached(locale, key_hash, None, &[])
}

#[wasm_bindgen]
pub fn l10n4x_translate_with_params(
    locale: &str,
    key_hash: u64,
    param_keys: Vec<String>,
    param_values: Vec<String>,
) -> Result<String, JsValue> {
    if param_keys.len() != param_values.len() {
        return Err(JsValue::from_str(&format!(
            "param_keys/param_values length mismatch: {} keys vs {} values",
            param_keys.len(),
            param_values.len()
        )));
    }
    let params: Vec<(&str, &str)> = param_keys
        .iter()
        .zip(param_values.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    Ok(translate_cached(locale, key_hash, None, &params))
}

/// Translates a key with context suffix support (e.g. `friend_male` → key = `friend`).
#[wasm_bindgen]
pub fn l10n4x_translate_with_context(locale: &str, key_hash: u64, context_hash: u64) -> String {
    translate_cached(locale, key_hash, Some(context_hash), &[])
}

/// Translate with context and parameters.
#[wasm_bindgen]
pub fn l10n4x_translate_with_context_and_params(
    locale: &str,
    key_hash: u64,
    context_hash: u64,
    param_keys: Vec<String>,
    param_values: Vec<String>,
) -> Result<String, JsValue> {
    if param_keys.len() != param_values.len() {
        return Err(JsValue::from_str(&format!(
            "param_keys/param_values length mismatch: {} keys vs {} values",
            param_keys.len(),
            param_values.len()
        )));
    }
    let params: Vec<(&str, &str)> = param_keys
        .iter()
        .zip(param_values.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    Ok(translate_cached(
        locale,
        key_hash,
        Some(context_hash),
        &params,
    ))
}

#[wasm_bindgen]
pub fn l10n4x_register_formatter(name: &str, callback: js_sys::Function) {
    let cb = callback.clone();
    l10n4x_core::formatter::register_formatter(
        name,
        Box::new(
            move |value: &str, _locale: &str, _options: &HashMap<String, String>| {
                let this = wasm_bindgen::JsValue::UNDEFINED;
                let arg1 = wasm_bindgen::JsValue::from_str(value);
                let result = cb.call1(&this, &arg1);
                match result {
                    Ok(val) => val.as_string().unwrap_or_else(|| value.to_string()),
                    Err(_) => value.to_string(),
                }
            },
        ),
    );
}

#[wasm_bindgen]
pub fn l10n4x_clear() {
    l10n4x_core::store::clear_translations();
    clear_translate_cache();
}

/// Registers a JS callback invoked when a locale is loaded or cleared.
/// The callback receives the locale code that was loaded.
#[wasm_bindgen]
pub fn l10n4x_on_locale_changed(callback: js_sys::Function) {
    let cb = callback;
    l10n4x_core::store::on_locale_changed_boxed(Box::new(move |locale: &str| {
        let arg = wasm_bindgen::JsValue::from_str(locale);
        let _ = cb.call1(&wasm_bindgen::JsValue::UNDEFINED, &arg);
    }));
}

/// Returns `true` if the given locale's lpk has been successfully loaded.
#[wasm_bindgen]
pub fn l10n4x_locale_loaded(locale: &str) -> bool {
    l10n4x_core::store::locale_loaded(locale)
}

/// Returns `true` if `key` exists in `locale` or the configured fallback chain.
#[wasm_bindgen]
pub fn l10n4x_key_exists(locale: &str, key_hash: u64) -> bool {
    l10n4x_core::store::key_exists(locale, key_hash, None)
}

/// Returns `true` if a context-suffixed key exists in `locale` or the fallback chain.
#[wasm_bindgen]
pub fn l10n4x_key_exists_with_context(locale: &str, key_hash: u64, context_hash: u64) -> bool {
    l10n4x_core::store::key_exists(locale, key_hash, Some(context_hash))
}

/// Atomically reloads a signed locale `.lpk`, retaining one retired snapshot for rollback.
#[wasm_bindgen]
pub fn l10n4x_ota_reload_lpk(locale: &str, bytes: &[u8]) -> Result<(), JsValue> {
    if !l10n4x_core::integrity::verify_key_configured() {
        return Err(JsValue::from_str(
            "Signature verification failed: verify key not set or invalid",
        ));
    }
    match try_ota_reload_lpk(locale, bytes) {
        Ok(()) => {
            clear_translate_cache();
            Ok(())
        }
        Err(err) => Err(JsValue::from_str(&format!("OTA reload failed: {}", err))),
    }
}

/// Restores the retired OTA snapshot for `locale` when available.
#[wasm_bindgen]
pub fn l10n4x_ota_rollback(locale: &str) -> Result<(), JsValue> {
    match try_ota_rollback(locale) {
        Ok(()) => {
            clear_translate_cache();
            Ok(())
        }
        Err(err) => Err(JsValue::from_str(&format!("OTA rollback failed: {}", err))),
    }
}

/// Returns `true` when an OTA rollback snapshot exists for `locale`.
#[wasm_bindgen]
pub fn l10n4x_ota_can_rollback(locale: &str) -> bool {
    ota_can_rollback(locale)
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
    fn key_exists_returns_false_without_lpk() {
        super::l10n4x_clear();
        assert!(!super::l10n4x_key_exists("en", 0));
    }

    #[test]
    fn get_loaded_locales_empty_after_clear() {
        super::l10n4x_clear();
        assert!(super::l10n4x_get_loaded_locales().is_empty());
    }

    #[test]
    fn translate_cache_hit_param_free() {
        super::l10n4x_clear();
        let key = 0xdead_beef_u64;
        let r1 = super::translate_cached("en", key, None, &[]);
        let r2 = super::translate_cached("en", key, None, &[]);
        assert_eq!(r1, r2);
    }
}
