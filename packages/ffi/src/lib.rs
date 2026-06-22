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

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Arc;

use l10n4x_core::binary_format::fnv1a_64;
use l10n4x_core::encryption;
use l10n4x_core::integrity;
use l10n4x_core::loader::{load_pak_directory, try_load_pak_locale, try_load_static_bytes};
use l10n4x_core::metrics;
use l10n4x_core::store::{
    clear_translations, get_fallback_locale, set_fallback_locale, translate_to_writer_with_status,
    TranslateStatus,
};

/// C-compatible function pointer type for the missing key callback.
pub type L10n4cMissingKeyFn = unsafe extern "C" fn(locale: *const c_char, key_hash: u64);

static C_MISSING_KEY_HANDLER: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

fn c_missing_key_bridge(locale: &str, key_hash: u64) {
    let ptr = C_MISSING_KEY_HANDLER.load(core::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    let f: L10n4cMissingKeyFn = unsafe { core::mem::transmute(ptr) };
    if let Ok(lc) = CString::new(locale) {
        unsafe {
            f(lc.as_ptr(), key_hash);
        }
    }
}

/// Typed key-value interpolation parameter for C callers.
#[repr(C)]
pub struct L10n4cParam {
    /// Parameter name (e.g. `"name"`).
    pub key: *const c_char,
    /// Parameter value as UTF-8 string.
    pub value: *const c_char,
}

struct TranslateOutcome {
    text: Arc<str>,
    key_found: bool,
    locale_loaded: bool,
}

struct CachedTranslate {
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
    text: Arc<str>,
    key_found: bool,
    locale_loaded: bool,
}

struct TranslateScratch {
    locale: String,
    param_pairs: Vec<(String, String)>,
}

thread_local! {
    static LAST_TRANSLATE: RefCell<Option<CachedTranslate>> = const { RefCell::new(None) };
    static SCRATCH: RefCell<TranslateScratch> = const {
        RefCell::new(TranslateScratch {
            locale: String::new(),
            param_pairs: Vec::new(),
        })
    };
}

const MAX_STACK_PARAMS: usize = 8;

fn hash_params(params: &[(&str, &str)]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for (key, value) in params {
        h ^= fnv1a_64(key.as_bytes());
        h = h.wrapping_mul(0x100000001b3);
        h ^= fnv1a_64(value.as_bytes());
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn cache_lookup(
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
) -> Option<TranslateOutcome> {
    LAST_TRANSLATE.with(|cell| {
        let cached = cell.borrow();
        let entry = cached.as_ref()?;
        if entry.locale_hash == locale_hash
            && entry.key_hash == key_hash
            && entry.context_hash == context_hash
            && entry.params_key == params_key
        {
            Some(TranslateOutcome {
                text: Arc::clone(&entry.text),
                key_found: entry.key_found,
                locale_loaded: entry.locale_loaded,
            })
        } else {
            None
        }
    })
}

fn cache_store(
    locale_hash: u64,
    key_hash: u64,
    context_hash: Option<u64>,
    params_key: u64,
    outcome: &TranslateOutcome,
) {
    LAST_TRANSLATE.with(|cell| {
        *cell.borrow_mut() = Some(CachedTranslate {
            locale_hash,
            key_hash,
            context_hash,
            params_key,
            text: Arc::clone(&outcome.text),
            key_found: outcome.key_found,
            locale_loaded: outcome.locale_loaded,
        });
    });
}

fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, i32> {
    if ptr.is_null() {
        return Err(L10N4C_INVALID_PARAMS);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| L10N4C_INVALID_ENCODING)
}

fn resolve_translation(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> TranslateOutcome {
    let locale_hash = fnv1a_64(locale.as_bytes());
    let params_key = hash_params(params);
    if let Some(cached) = cache_lookup(locale_hash, key_hash, context_hash, params_key) {
        return cached;
    }

    let mut resolved = String::new();
    let status = translate_to_writer_with_status(
        locale,
        key_hash,
        context_hash,
        params,
        &mut resolved,
    )
    .unwrap_or(TranslateStatus {
        key_found: false,
        locale_loaded: false,
    });
    let outcome = TranslateOutcome {
        text: Arc::from(resolved),
        key_found: status.key_found,
        locale_loaded: status.locale_loaded,
    };
    cache_store(locale_hash, key_hash, context_hash, params_key, &outcome);
    outcome
}

fn outcome_status(outcome: &TranslateOutcome) -> i32 {
    if outcome.key_found {
        L10N4C_OK
    } else if outcome.locale_loaded {
        L10N4C_KEY_NOT_FOUND
    } else {
        L10N4C_LOCALE_NOT_LOADED
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

fn fill_locale_buf(locale_ptr: *const c_char, locale: &mut String) -> Result<(), i32> {
    locale.clear();
    if locale_ptr.is_null() {
        locale.push_str(&get_fallback_locale());
    } else {
        locale.push_str(cstr_to_str(locale_ptr)?);
    }
    Ok(())
}

fn fill_typed_params(
    params: *const L10n4cParam,
    param_count: usize,
    storage: &mut Vec<(String, String)>,
) -> Result<(), i32> {
    storage.clear();
    if param_count == 0 {
        return Ok(());
    }
    if params.is_null() {
        return Err(L10N4C_INVALID_PARAMS);
    }
    let size_of_param = std::mem::size_of::<L10n4cParam>();
    let _total_size = size_of_param
        .checked_mul(param_count)
        .ok_or(L10N4C_BUFFER_OVERFLOW)?;

    storage.reserve(param_count);
    let slice = unsafe { std::slice::from_raw_parts(params, param_count) };
    for (i, p) in slice.iter().enumerate() {
        let key = cstr_to_str(p.key)?;
        let value = cstr_to_str(p.value)?;
        if i < storage.len() {
            storage[i].0.clear();
            storage[i].0.push_str(key);
            storage[i].1.clear();
            storage[i].1.push_str(value);
        } else {
            storage.push((key.to_owned(), value.to_owned()));
        }
    }
    storage.truncate(param_count);
    Ok(())
}

fn resolve_with_param_pairs(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    pairs: &[(String, String)],
) -> TranslateOutcome {
    if pairs.len() <= MAX_STACK_PARAMS {
        let mut buf = [("", ""); MAX_STACK_PARAMS];
        for (i, (k, v)) in pairs.iter().enumerate() {
            buf[i] = (k.as_str(), v.as_str());
        }
        resolve_translation(locale, key_hash, context_hash, &buf[..pairs.len()])
    } else {
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        resolve_translation(locale, key_hash, context_hash, &refs)
    }
}

fn translate_core(
    locale_ptr: *const c_char,
    key_hash: u64,
    context_hash: Option<u64>,
    params: Option<(*const L10n4cParam, usize)>,
) -> Result<TranslateOutcome, i32> {
    SCRATCH.with(|scratch_cell| {
        let mut scratch = scratch_cell.borrow_mut();
        fill_locale_buf(locale_ptr, &mut scratch.locale)?;
        match params {
            None => Ok(resolve_translation(&scratch.locale, key_hash, context_hash, &[])),
            Some((ptr, count)) => {
                fill_typed_params(ptr, count, &mut scratch.param_pairs)?;
                Ok(resolve_with_param_pairs(
                    &scratch.locale,
                    key_hash,
                    context_hash,
                    &scratch.param_pairs,
                ))
            }
        }
    })
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

/// Sets the ordered fallback locale chain (first match wins).
/// `locales` is an array of `count` null-terminated UTF-8 locale strings.
/// Returns `L10N4C_OK` on success or `L10N4C_INVALID_PARAMS` if `locales` is null.
///
/// # Safety
/// `locales` must point to `count` valid, non-null, null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn l10n4c_set_fallback_chain(
    locales: *const *const c_char,
    count: usize,
) -> i32 {
    if locales.is_null() || count == 0 {
        return L10N4C_INVALID_PARAMS;
    }
    let mut chain: Vec<&str> = Vec::with_capacity(count.min(16));
    for i in 0..count.min(16) {
        // SAFETY: caller guarantees each pointer is a valid null-terminated UTF-8 string.
        let ptr = unsafe { *locales.add(i) };
        match cstr_to_str(ptr) {
            Ok(s) => chain.push(s),
            Err(e) => return e,
        }
    }
    l10n4x_core::store::set_fallback_chain(&chain);
    L10N4C_OK
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
    match try_load_pak_locale(locale_str, path_str) {
        Ok(()) => L10N4C_OK,
        Err(e) => core_error_to_ffi(e),
    }
}

/// Loads a static (compile-time embedded) L10N buffer into the store.
///
/// `data` must point to a valid L10N-format buffer that lives for the program's lifetime
/// (e.g., a `static` variable declared in C). The caller retains ownership of `data`.
///
/// `already_verified`: if non-zero, the caller asserts the data was cryptographically
/// verified at build time and runtime will not re-verify it.
///
/// Returns `L10N4C_OK` on success, or `L10N4C_INVALID_PARAMS` if pointers are null or length is 0.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_static_bytes(
    locale: *const c_char,
    data: *const u8,
    data_len: usize,
    already_verified: i32,
) -> i32 {
    let locale_str = match cstr_to_str(locale) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if data.is_null() || data_len == 0 {
        return L10N4C_INVALID_PARAMS;
    }
    let slice = unsafe { core::slice::from_raw_parts(data, data_len) };
    // SAFETY: The caller promises the buffer lives for the program's lifetime
    // (e.g., a C static variable or mmap'd read-only section).
    let static_slice: &'static [u8] = unsafe { core::mem::transmute(slice) };
    let verified = already_verified != 0;
    match try_load_static_bytes(locale_str, static_slice, verified) {
        Ok(()) => L10N4C_OK,
        Err(e) => core_error_to_ffi(e),
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
    key_hash: u64,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let outcome = match translate_core(locale, key_hash, None, None) {
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
    outcome_status(&outcome)
}

/// Translates a key into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate(
    locale: *const c_char,
    key_hash: u64,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let outcome = match translate_core(locale, key_hash, None, None) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => outcome_status(&outcome),
        Err(L10N4C_BUFFER_TOO_SMALL) => L10N4C_BUFFER_TOO_SMALL,
        Err(e) => e,
    }
}

/// Returns the buffer size needed for a typed-parameter translation.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params_required_size(
    locale: *const c_char,
    key_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let outcome = match translate_core(locale, key_hash, None, Some((params, param_count))) {
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
    outcome_status(&outcome)
}

/// Translates a key with typed parameters into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params(
    locale: *const c_char,
    key_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let outcome = match translate_core(locale, key_hash, None, Some((params, param_count))) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => outcome_status(&outcome),
        Err(e) => e,
    }
}

/// Allocates and returns a translated string. Caller must free with [`l10n4c_free_string`].
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_alloc(locale: *const c_char, key_hash: u64) -> *mut c_char {
    match translate_core(locale, key_hash, None, None) {
        Ok(o) => string_to_c(&o.text),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_params_alloc(
    locale: *const c_char,
    key_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
) -> *mut c_char {
    match translate_core(locale, key_hash, None, Some((params, param_count))) {
        Ok(o) => string_to_c(&o.text),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Returns the buffer size needed for a context-suffix translation.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context_required_size(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let outcome = match translate_core(locale, key_hash, Some(context_hash), None) {
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
    outcome_status(&outcome)
}

/// Translates a key with context suffix into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let outcome = match translate_core(locale, key_hash, Some(context_hash), None) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => outcome_status(&outcome),
        Err(e) => e,
    }
}

/// Allocates and returns a context-suffix translation.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context_alloc(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
) -> *mut c_char {
    match translate_core(locale, key_hash, Some(context_hash), None) {
        Ok(o) => string_to_c(&o.text),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Returns the buffer size needed for a context-suffix translation with parameters.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context_and_params_required_size(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return L10N4C_INVALID_PARAMS;
    }
    let outcome = match translate_core(
        locale,
        key_hash,
        Some(context_hash),
        Some((params, param_count)),
    ) {
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
    outcome_status(&outcome)
}

/// Translates a key with context suffix and parameters into a caller-provided buffer.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context_and_params(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
    buf: *mut u8,
    max_len: usize,
) -> i32 {
    let outcome = match translate_core(
        locale,
        key_hash,
        Some(context_hash),
        Some((params, param_count)),
    ) {
        Ok(o) => o,
        Err(e) => return e,
    };
    match write_to_c_buffer(&outcome.text, buf, max_len) {
        Ok(_) => outcome_status(&outcome),
        Err(e) => e,
    }
}

/// Allocates and returns a context-suffix translation with parameters.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_translate_with_context_and_params_alloc(
    locale: *const c_char,
    key_hash: u64,
    context_hash: u64,
    params: *const L10n4cParam,
    param_count: usize,
) -> *mut c_char {
    match translate_core(
        locale,
        key_hash,
        Some(context_hash),
        Some((params, param_count)),
    ) {
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

/// Registers a C callback invoked when a translation key is not found.
/// Pass NULL to remove the callback.
///
/// # Safety
/// `handler` must remain valid for the lifetime of the program (or until replaced).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn l10n4c_set_missing_key_handler(handler: Option<L10n4cMissingKeyFn>) {
    match handler {
        Some(f) => {
            C_MISSING_KEY_HANDLER.store(f as *mut (), core::sync::atomic::Ordering::Release);
            l10n4x_core::store::set_missing_key_handler(c_missing_key_bridge);
        }
        None => {
            C_MISSING_KEY_HANDLER
                .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Release);
            l10n4x_core::store::clear_missing_key_handler();
        }
    }
}

/// Returns the library version string (e.g. "0.2.0").
/// The returned string is owned by the caller and must be freed with l10n4c_free_string.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_get_version() -> *mut c_char {
    string_to_c(env!("CARGO_PKG_VERSION"))
}

/// Custom formatter function type for C callers.
/// Takes (value, locale, options_json) and returns allocated string.
pub type L10n4cCustomFormatter = unsafe extern "C" fn(
    value: *const c_char,
    locale: *const c_char,
    options: *const c_char,
) -> *mut c_char;

/// Registers a custom formatter with the given name.
/// The formatter is called for ICU message syntax like `{var, formatterName}`.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_register_formatter(
    name: *const c_char,
    formatter: Option<L10n4cCustomFormatter>,
) -> i32 {
    let name_str = match cstr_to_str(name) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let Some(f) = formatter else {
        return L10N4C_INVALID_PARAMS;
    };
    l10n4x_core::formatter::register_formatter(
        &name_str,
        Box::new(move |value, locale, _options| {
            let c_value = CString::new(value).unwrap_or_default();
            let c_locale = CString::new(locale).unwrap_or_default();
            let result = unsafe { f(c_value.as_ptr(), c_locale.as_ptr(), core::ptr::null()) };
            if result.is_null() {
                return value.to_string();
            }
            let s = unsafe { CStr::from_ptr(result) }
                .to_string_lossy()
                .into_owned();
            unsafe {
                let _ = CString::from_raw(result);
            }
            s
        }),
    );
    L10N4C_OK
}

/// Clears all loaded translations from the global store.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_clear() {
    clear_translations();
}

/// Writes comma-separated loaded locale codes into `out_buf` (up to `out_len` bytes).
/// Returns the number of bytes written (excluding null terminator),
/// or `L10N4C_BUFFER_TOO_SMALL` if the buffer is too small.
/// On success, the buffer is null-terminated.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_get_loaded_locales(out_buf: *mut u8, out_len: usize) -> i32 {
    if out_buf.is_null() || out_len == 0 {
        return L10N4C_INVALID_PARAMS;
    }
    let locales = l10n4x_core::store::read_store(|store| {
        let codes: Vec<&str> = store.locales.iter().map(|(loc, _)| loc.as_str()).collect();
        codes.join(",")
    });
    let bytes = locales.as_bytes();
    let len = bytes.len();
    if len + 1 > out_len {
        return L10N4C_BUFFER_TOO_SMALL;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, len);
        *out_buf.add(len) = 0;
    }
    len as i32
}

/// Returns comma-separated metrics counters: total translations, cache hits,
/// cache misses, locale loads, format errors — as a UTF-8 string.
/// Returns the number of bytes written, or L10N4C_BUFFER_TOO_SMALL.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_get_metrics(out_buf: *mut u8, out_len: usize) -> i32 {
    if out_buf.is_null() || out_len == 0 {
        return L10N4C_INVALID_PARAMS;
    }
    let s = metrics::metrics_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len + 1 > out_len {
        return L10N4C_BUFFER_TOO_SMALL;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, len);
        *out_buf.add(len) = 0;
    }
    len as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn error_code_values() {
        assert_eq!(L10N4C_OK, 0);
        assert_eq!(L10N4C_KEY_NOT_FOUND, 1);
        assert_eq!(L10N4C_LOCALE_NOT_LOADED, 2);
        assert_eq!(L10N4C_BUFFER_TOO_SMALL, 3);
        assert_eq!(L10N4C_INVALID_PARAMS, 4);
        assert_eq!(L10N4C_INTERNAL_ERROR, 5);
        assert_eq!(L10N4C_INVALID_ENCODING, 6);
        assert_eq!(L10N4C_IO_ERROR, 7);
        assert_eq!(L10N4C_SIGNATURE_INVALID, 8);
        assert_eq!(L10N4C_VERIFY_KEY_NOT_SET, 9);
        assert_eq!(L10N4C_NOT_INITIALIZED, 10);
        assert_eq!(L10N4C_DECRYPT_KEY_NOT_SET, 11);
        assert_eq!(L10N4C_BUFFER_OVERFLOW, 12);
    }

    #[test]
    fn cstr_to_str_null_returns_invalid_params() {
        let result = cstr_to_str(std::ptr::null());
        assert_eq!(result, Err(L10N4C_INVALID_PARAMS));
    }

    #[test]
    fn cstr_to_str_valid() {
        let s = CString::new("hello").unwrap();
        let result = cstr_to_str(s.as_ptr());
        assert_eq!(result, Ok("hello"));
    }

    #[test]
    fn write_to_c_buffer_null_buffer() {
        let result = write_to_c_buffer("test", std::ptr::null_mut(), 10);
        assert_eq!(result, Err(L10N4C_BUFFER_TOO_SMALL));
    }

    #[test]
    fn write_to_c_buffer_too_small() {
        let mut buf = [0u8; 2];
        let result = write_to_c_buffer("hello", buf.as_mut_ptr(), 2);
        assert_eq!(result, Err(L10N4C_BUFFER_TOO_SMALL));
    }

    #[test]
    fn write_to_c_buffer_success() {
        let mut buf = [0u8; 16];
        let result = write_to_c_buffer("hello", buf.as_mut_ptr(), 16);
        assert_eq!(result, Ok(5));
        assert_eq!(&buf[..6], b"hello\0");
    }

    #[test]
    fn fill_typed_params_null_with_count() {
        let mut pairs = Vec::new();
        let result = fill_typed_params(std::ptr::null(), 1, &mut pairs);
        assert_eq!(result, Err(L10N4C_INVALID_PARAMS));
    }

    #[test]
    fn fill_typed_params_zero_count() {
        let param = L10n4cParam {
            key: std::ptr::null(),
            value: std::ptr::null(),
        };
        let mut pairs = Vec::new();
        let result = fill_typed_params(&param, 0, &mut pairs);
        assert_eq!(result, Ok(()));
        assert!(pairs.is_empty());
    }

    #[test]
    fn fill_typed_params_success() {
        let k = CString::new("name").unwrap();
        let v = CString::new("John").unwrap();
        let param = L10n4cParam {
            key: k.as_ptr(),
            value: v.as_ptr(),
        };
        let mut pairs = Vec::new();
        let result = fill_typed_params(&param, 1, &mut pairs);
        assert_eq!(result, Ok(()));
        assert_eq!(pairs, vec![("name".to_string(), "John".to_string())]);
    }

    #[test]
    fn string_to_c_normal() {
        let ptr = string_to_c("hello");
        assert!(!ptr.is_null());
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(s, "hello");
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }

    #[test]
    fn string_to_c_with_nul_bytes() {
        // Interior nul bytes cause error -> null
        let ptr = string_to_c("he\0llo");
        assert!(ptr.is_null());
    }

    #[test]
    fn set_verify_key_null() {
        let result = l10n4c_set_verify_key(std::ptr::null(), 32);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn set_verify_key_wrong_len() {
        let key = [0u8; 16];
        let result = l10n4c_set_verify_key(key.as_ptr(), 16);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn set_decrypt_key_null() {
        let result = l10n4c_set_decrypt_key(std::ptr::null(), 32);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn set_decrypt_key_wrong_len() {
        let key = [0u8; 16];
        let result = l10n4c_set_decrypt_key(key.as_ptr(), 16);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn set_fallback_locale_invalid_utf8() {
        let invalid = b"en_\xff\xff\x00";
        let result = l10n4c_set_fallback_locale(invalid.as_ptr() as *const c_char);
        assert_eq!(result, L10N4C_INVALID_ENCODING);
    }

    #[test]
    fn set_fallback_locale_success() {
        let locale = CString::new("fr").unwrap();
        let result = l10n4c_set_fallback_locale(locale.as_ptr());
        assert_eq!(result, L10N4C_OK);
        // reset
        let en = CString::new("en").unwrap();
        l10n4c_set_fallback_locale(en.as_ptr());
    }

    #[test]
    fn clear_is_safe() {
        l10n4c_clear();
    }

    #[test]
    fn get_version_returns_non_null() {
        let ptr = l10n4c_get_version();
        assert!(!ptr.is_null());
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(!s.is_empty());
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }

    #[test]
    fn free_string_null_is_safe() {
        l10n4c_free_string(std::ptr::null_mut());
    }

    #[test]
    fn set_missing_key_handler_null_clears() {
        unsafe {
            l10n4c_set_missing_key_handler(None);
        }
        // should not panic
    }

    #[test]
    fn translate_required_size_null_out() {
        let result = l10n4c_translate_required_size(std::ptr::null(), 0, std::ptr::null_mut());
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn translate_null_buffer() {
        let result = l10n4c_translate(std::ptr::null(), 0, std::ptr::null_mut(), 0);
        assert!(
            result == L10N4C_BUFFER_TOO_SMALL
                || result == L10N4C_KEY_NOT_FOUND
                || result == L10N4C_LOCALE_NOT_LOADED
                || result == L10N4C_INVALID_PARAMS
        );
    }

    #[test]
    fn get_loaded_locales_null_buffer() {
        let result = l10n4c_get_loaded_locales(std::ptr::null_mut(), 10);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn get_loaded_locales_zero_len() {
        let mut buf = [0u8; 10];
        let result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 0);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn get_loaded_locales_empty_store() {
        l10n4c_clear();
        let mut buf = [0u8; 64];
        let result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 64);
        assert!(result >= 0);
        assert_eq!(&buf[..result as usize], b"");
        assert_eq!(buf[result as usize], 0);
    }

    #[test]
    fn get_metrics_null_buffer() {
        let result = l10n4c_get_metrics(std::ptr::null_mut(), 10);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn get_metrics_returns_values() {
        let mut buf = [0u8; 64];
        let result = l10n4c_get_metrics(buf.as_mut_ptr(), 64);
        assert!(result > 0);
        assert_eq!(buf[result as usize], 0);
        let s = std::str::from_utf8(&buf[..result as usize]).unwrap();
        assert_eq!(s.split(',').count(), 5);
    }

    #[test]
    fn register_formatter_null_name() {
        let result = l10n4c_register_formatter(std::ptr::null(), None);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn register_formatter_null_formatter() {
        let name = CString::new("myformat").unwrap();
        let result = l10n4c_register_formatter(name.as_ptr(), None);
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn translate_with_params_required_size_null_out() {
        let result = l10n4c_translate_with_params_required_size(
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
        );
        assert_eq!(result, L10N4C_INVALID_PARAMS);
    }

    #[test]
    fn set_verify_key_valid() {
        let key = [42u8; 32];
        let result = l10n4c_set_verify_key(key.as_ptr(), 32);
        assert_eq!(result, L10N4C_OK);
    }

    #[test]
    fn set_decrypt_key_valid() {
        let key = [42u8; 32];
        let result = l10n4c_set_decrypt_key(key.as_ptr(), 32);
        assert_eq!(result, L10N4C_OK);
    }

    #[test]
    fn get_loaded_locales_empty_after_clear() {
        l10n4c_clear();
        let mut buf = [0u8; 64];
        let result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 64);
        assert_eq!(result, 0);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn get_metrics_small_buffer() {
        let mut buf = [0u8; 1];
        let result = l10n4c_get_metrics(buf.as_mut_ptr(), 1);
        assert_eq!(result, L10N4C_BUFFER_TOO_SMALL);
    }
}
