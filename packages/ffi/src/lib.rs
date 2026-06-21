//! `l10n4c` is the C-FFI runtime layer for `l10n4x`.
//!
//! This crate exposes **runtime-only** operations: loading signed `.pak` files,
//! translating keys, and managing the fallback locale. Compilation is handled
//! exclusively by the `l10n4x-toolkit` CLI, enforcing cryptographic integrity
//! by architecture.
//!
//! # Thread safety
//!
//! Translation lookups (`l10n4c_translate*`) are thread-safe: the underlying store uses
//! lock-free RCU pointer swapping for concurrent reads.
//!
//! Load and clear operations are **not** thread-safe. Serialize them externally
//! or restrict them to application startup/shutdown.
//!
//! # Memory management
//!
//! Functions returning `*mut c_char` allocate via the Rust global allocator. Callers must
//! release results with [`l10n4c_free_string`].

#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod error;

pub use error::*;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use l10n4x_core::encryption;
use l10n4x_core::integrity;
use l10n4x_core::loader::{load_pak_directory, load_pak_locale};
use l10n4x_core::store::{clear_translations, get_fallback_locale, set_fallback_locale};

/// Typed key-value interpolation parameter for C callers.
#[repr(C)]
pub struct L10n4cParam {
    /// Parameter name (e.g. `"name"`).
    pub key: *const c_char,
    /// Parameter value as UTF-8 string.
    pub value: *const c_char,
}

struct TranslateOutcome {
    text: String,
    key_found: bool,
    locale_loaded: bool,
}

fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, i32> {
    if ptr.is_null() {
        return Err(L10N4C_INVALID_PARAMS);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| L10N4C_INVALID_ENCODING)
}

fn resolve_translation(locale: &str, key: &str, params: &[(&str, &str)]) -> TranslateOutcome {
    let locale_loaded = l10n4x_core::store::locale_loaded(locale);
    let key_found = l10n4x_core::store::key_exists(locale, key);
    let mut resolved = String::new();
    let _ = l10n4x_core::store::translate_to_writer(locale, key, params, &mut resolved);
    TranslateOutcome {
        text: resolved,
        key_found,
        locale_loaded,
    }
}

fn required_size(text: &str) -> Option<usize> {
    text.len().checked_add(1)
}

fn write_to_c_buffer(s: &str, buf: *mut u8, max_len: usize) -> Result<usize, i32> {
    let needed = required_size(s).ok_or(L10N4C_BUFFER_OVERFLOW)?;
    if buf.is_null() || max_len < needed {
        return Err(L10N4C_BUFFER_TOO_SMALL);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    Ok(s.len())
}

fn parse_typed_params_owned(
    params: *const L10n4cParam,
    param_count: usize,
) -> Result<Vec<(String, String)>, i32> {
    if param_count == 0 {
        return Ok(Vec::new());
    }
    if params.is_null() {
        return Err(L10N4C_INVALID_PARAMS);
    }
    let size_of_param = std::mem::size_of::<L10n4cParam>();
    let _total_size = size_of_param
        .checked_mul(param_count)
        .ok_or(L10N4C_BUFFER_OVERFLOW)?;

    let slice = unsafe { std::slice::from_raw_parts(params, param_count) };
    let mut out = Vec::with_capacity(param_count);
    for p in slice {
        let key = cstr_to_str(p.key)?.to_string();
        let value = cstr_to_str(p.value)?.to_string();
        out.push((key, value));
    }
    Ok(out)
}

fn translate_core(
    locale_ptr: *const c_char,
    key_ptr: *const c_char,
    params: &[(&str, &str)],
) -> Result<TranslateOutcome, i32> {
    let key = cstr_to_str(key_ptr)?;
    let locale = if locale_ptr.is_null() {
        get_fallback_locale().to_string()
    } else {
        cstr_to_str(locale_ptr)?.to_string()
    };
    Ok(resolve_translation(&locale, key, params))
}

fn string_to_c(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Installs the 32-byte Ed25519 public key used to verify `.pak` signatures at runtime.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_set_verify_key(key: *const u8, key_len: usize) -> i32 {
    if key.is_null() || key_len != 32 {
        return L10N4C_INVALID_PARAMS;
    }
    let slice = unsafe { std::slice::from_raw_parts(key, key_len) };
    if integrity::set_verify_key(slice) {
        L10N4C_OK
    } else {
        L10N4C_INVALID_PARAMS
    }
}

/// Installs the 32-byte AES key for optional `L10E` envelope decryption (and compile-time encryption).
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_set_decrypt_key(key: *const u8, key_len: usize) -> i32 {
    if key.is_null() || key_len != 32 {
        return L10N4C_INVALID_PARAMS;
    }
    let slice = unsafe { std::slice::from_raw_parts(key, key_len) };
    if encryption::set_decrypt_key(slice) {
        L10N4C_OK
    } else {
        L10N4C_INVALID_PARAMS
    }
}

/// Sets the global fallback locale (defaults to `"en"`).
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_set_fallback_locale(locale: *const c_char) -> i32 {
    match cstr_to_str(locale) {
        Ok(s) => {
            set_fallback_locale(s);
            L10N4C_OK
        }
        Err(e) => e,
    }
}

/// Loads a single `.pak` file for a given locale.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_pak_locale(locale: *const c_char, file_path: *const c_char) -> i32 {
    let locale_str = match cstr_to_str(locale) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let path_str = match cstr_to_str(file_path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if load_pak_locale(locale_str, path_str) {
        L10N4C_OK
    } else {
        L10N4C_IO_ERROR
    }
}

/// Scans a directory for all `.pak` files and loads them (filename stem = locale).
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_pak_directory(dir_path: *const c_char) -> i32 {
    let dir = match cstr_to_str(dir_path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if load_pak_directory(dir) {
        L10N4C_OK
    } else {
        L10N4C_IO_ERROR
    }
}

/// Returns the buffer size (including null terminator) needed for a translation.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_required_size(
    locale: *const c_char,
    key: *const c_char,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let outcome = match translate_core(locale, key, &[]) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let needed = match required_size(&outcome.text) {
        Some(sz) => sz,
        None => return L10N4C_BUFFER_OVERFLOW,
    };
    unsafe {
        *out_size = needed;
    }
    if outcome.key_found {
        L10N4C_OK
    } else if outcome.locale_loaded {
        L10N4C_KEY_NOT_FOUND
    } else {
        L10N4C_LOCALE_NOT_LOADED
    }
}

/// Translates a key into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate(
    locale: *const c_char,
    key: *const c_char,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let outcome = match translate_core(locale, key, &[]) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => {
            if outcome.key_found {
                L10N4C_OK
            } else if outcome.locale_loaded {
                L10N4C_KEY_NOT_FOUND
            } else {
                L10N4C_LOCALE_NOT_LOADED
            }
        }
        Err(L10N4C_BUFFER_TOO_SMALL) => L10N4C_BUFFER_TOO_SMALL,
        Err(e) => e,
    }
}

/// Returns the buffer size needed for a typed-parameter translation.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params_required_size(
    locale: *const c_char,
    key: *const c_char,
    params: *const L10n4cParam,
    param_count: usize,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let parsed = match parse_typed_params_owned(params, param_count) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let refs: Vec<(&str, &str)> = parsed
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let outcome = match translate_core(locale, key, &refs) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let needed = match required_size(&outcome.text) {
        Some(sz) => sz,
        None => return L10N4C_BUFFER_OVERFLOW,
    };
    unsafe {
        *out_size = needed;
    }
    if outcome.key_found {
        L10N4C_OK
    } else if outcome.locale_loaded {
        L10N4C_KEY_NOT_FOUND
    } else {
        L10N4C_LOCALE_NOT_LOADED
    }
}

/// Translates a key with typed parameters into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params(
    locale: *const c_char,
    key: *const c_char,
    params: *const L10n4cParam,
    param_count: usize,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let parsed = match parse_typed_params_owned(params, param_count) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let refs: Vec<(&str, &str)> = parsed
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let outcome = match translate_core(locale, key, &refs) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => {
            if outcome.key_found {
                L10N4C_OK
            } else if outcome.locale_loaded {
                L10N4C_KEY_NOT_FOUND
            } else {
                L10N4C_LOCALE_NOT_LOADED
            }
        }
        Err(e) => e,
    }
}

/// Allocates and returns a translated string. Caller must free with [`l10n4c_free_string`].
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_alloc(locale: *const c_char, key: *const c_char) -> *mut c_char {
    match translate_core(locale, key, &[]) {
        Ok(o) => string_to_c(&o.text),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params_alloc(
    locale: *const c_char,
    key: *const c_char,
    params: *const L10n4cParam,
    param_count: usize,
) -> *mut c_char {
    let parsed = match parse_typed_params_owned(params, param_count) {
        Ok(p) => p,
        Err(_) => return std::ptr::null_mut(),
    };
    let refs: Vec<(&str, &str)> = parsed
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    match translate_core(locale, key, &refs) {
        Ok(o) => string_to_c(&o.text),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees a string previously returned by an `*_alloc` function.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

/// Clears all loaded translations from the global store.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_clear() {
    clear_translations();
}
