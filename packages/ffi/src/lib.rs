//! `l10n4c` is the C-FFI compatibility layer for `l10n4x`.
//! It exposes stable C-compatible symbols for loading compiled `.pak` files
//! and performing localization lookups using caller-allocated buffers.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;

use l10n4x_compiler::compile_translations;
use l10n4x_core::crypto::set_encryption_key;
use l10n4x_core::loader::{load_pak_directory, load_pak_locale};
use l10n4x_core::store::{clear_translations, get_fallback_locale, set_fallback_locale};

fn write_to_c_buffer(s: &str, buf: *mut u8, max_len: usize) -> usize {
    let len = s.len();
    let needed = len + 1;
    if buf.is_null() || max_len < needed {
        needed
    } else {
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), buf, len);
            *buf.add(len) = 0; // null terminator
        }
        len
    }
}

/// Sets the global 32-byte AES key for GCM encryption and decryption.
/// Returns true if successful, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_set_encryption_key(key: *const u8, key_len: usize) -> bool {
    if key.is_null() || key_len != 32 {
        return false;
    }
    let key_slice = unsafe { std::slice::from_raw_parts(key, key_len) };
    set_encryption_key(key_slice)
}

/// Sets the global fallback locale (defaults to "en").
/// Returns true if successful, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_set_fallback_locale(locale: *const c_char) -> bool {
    if locale.is_null() {
        return false;
    }
    if let Ok(locale_str) = unsafe { CStr::from_ptr(locale) }.to_str() {
        set_fallback_locale(locale_str)
    } else {
        false
    }
}

/// Compiles a source directory of raw JSON localization folders into GCM-encrypted .pak files.
/// Output folder is created if it does not exist.
/// Returns true if successful, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_compile(src_dir: *const c_char, out_dir: *const c_char) -> bool {
    if src_dir.is_null() || out_dir.is_null() {
        return false;
    }

    let src_path_str = match unsafe { CStr::from_ptr(src_dir) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let out_path_str = match unsafe { CStr::from_ptr(out_dir) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    compile_translations(Path::new(src_path_str), Path::new(out_path_str)).is_ok()
}

/// Loads translations from a JSON string into the global store for a given locale.
/// An optional prefix can be provided to namespace the keys.
/// Returns true if loading succeeded, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_locale(
    locale: *const c_char,
    json_content: *const c_char,
    prefix: *const c_char,
) -> bool {
    if locale.is_null() || json_content.is_null() {
        return false;
    }

    let locale_str = match unsafe { CStr::from_ptr(locale) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(json_content) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let prefix_str = if prefix.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(prefix) }
            .to_str()
            .unwrap_or_default()
    };

    let parsed_json: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let mut raw_flat = std::collections::HashMap::new();
    l10n4x_compiler::flatten_value(prefix_str.to_string(), &parsed_json, &mut raw_flat);

    let mut parsed_translations = std::collections::HashMap::new();
    for (k, template) in raw_flat {
        let parser = l10n4x_compiler::icu_parser::MessageParser::new(&template);
        if let Ok(nodes) = parser.parse() {
            parsed_translations.insert(k, nodes);
        } else {
            return false;
        }
    }

    let binary_bytes = l10n4x_compiler::binary_writer::write_binary_format(&parsed_translations);
    l10n4x_core::loader::load_raw_bytes(locale_str, &binary_bytes)
}

/// Decrypts and loads a single .pak file for a given locale.
/// Returns true if loading succeeded, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_pak_locale(locale: *const c_char, file_path: *const c_char) -> bool {
    if locale.is_null() || file_path.is_null() {
        return false;
    }

    let locale_str = match unsafe { CStr::from_ptr(locale) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    load_pak_locale(locale_str, path_str)
}

/// Scans a directory for all .pak files and automatically loads them.
/// Uses the filename stem as the locale.
/// Returns true if at least one pak was loaded successfully, false otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_pak_directory(dir_path: *const c_char) -> bool {
    if dir_path.is_null() {
        return false;
    }

    let dir_path_str = match unsafe { CStr::from_ptr(dir_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    load_pak_directory(dir_path_str)
}

/// Translates a key for a given locale.
/// Returns the size of the buffer needed (including null terminator) if `buf` is null or `max_len` is too small.
/// Otherwise, copies the translation to `buf`, appends a null terminator, and returns the number of bytes written.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate(
    locale: *const c_char,
    key: *const c_char,
    buf: *mut u8,
    max_len: usize,
) -> usize {
    if locale.is_null() || key.is_null() {
        return 0;
    }

    let fallback = get_fallback_locale();
    let locale_str = match unsafe { CStr::from_ptr(locale) }.to_str() {
        Ok(s) => s,
        Err(_) => &fallback,
    };

    let key_str = unsafe { CStr::from_ptr(key) }.to_str().unwrap_or_default();

    let mut resolved_str = String::new();
    if l10n4x_core::store::translate_to_writer(locale_str, key_str, &[], &mut resolved_str).is_err()
    {
        resolved_str = key_str.to_string();
    }

    write_to_c_buffer(&resolved_str, buf, max_len)
}

/// Translates a key for a given locale, parsing and interpolating JSON variables.
/// Returns the size of the buffer needed (including null terminator) if `buf` is null or `max_len` is too small.
/// Otherwise, copies the translation to `buf`, appends a null terminator, and returns the number of bytes written.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params(
    locale: *const c_char,
    key: *const c_char,
    params_json: *const c_char,
    buf: *mut u8,
    max_len: usize,
) -> usize {
    if locale.is_null() || key.is_null() {
        return 0;
    }

    let fallback = get_fallback_locale();
    let locale_str = match unsafe { CStr::from_ptr(locale) }.to_str() {
        Ok(s) => s,
        Err(_) => &fallback,
    };

    let key_str = unsafe { CStr::from_ptr(key) }.to_str().unwrap_or_default();

    let mut params = std::collections::HashMap::new();
    if !params_json.is_null() {
        if let Ok(json_str) = unsafe { CStr::from_ptr(params_json) }.to_str() {
            if let Ok(parsed) =
                serde_json::from_str::<std::collections::HashMap<String, String>>(json_str)
            {
                params = parsed;
            }
        }
    }

    let params_vec: Vec<(&str, &str)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let mut resolved_str = String::new();
    if l10n4x_core::store::translate_to_writer(locale_str, key_str, &params_vec, &mut resolved_str)
        .is_err()
    {
        resolved_str = key_str.to_string();
    }

    write_to_c_buffer(&resolved_str, buf, max_len)
}

/// Clears all loaded translations from the global store.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_clear() {
    clear_translations();
}
