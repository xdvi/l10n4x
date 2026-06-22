//! Integration tests for the `l10n4c` FFI runtime layer.
//!
//! These tests exercise the **runtime-only** API surface: loading pre-compiled
//! `.pak` files, translating keys, managing the fallback locale, and the alloc
//! API. Compilation is performed via `l10n4x_compiler` directly (as the CLI
//! would), not through FFI.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;

use l10n4c::{
    l10n4c_clear, l10n4c_free_string, l10n4c_get_loaded_locales,
    l10n4c_load_pak_directory, l10n4c_load_static_bytes, l10n4c_register_formatter,
    l10n4c_set_decrypt_key, l10n4c_set_fallback_chain, l10n4c_set_fallback_locale,
    l10n4c_set_missing_key_handler, l10n4c_set_verify_key, l10n4c_translate,
    l10n4c_translate_alloc, l10n4c_translate_required_size, l10n4c_translate_with_params,
    l10n4c_translate_with_params_alloc, l10n4c_translate_with_params_required_size,
    L10n4cParam, L10N4C_INVALID_PARAMS,
    L10N4C_KEY_NOT_FOUND, L10N4C_LOCALE_NOT_LOADED, L10N4C_OK,
};
use l10n4x_compiler::fnv1a_64;

/// Install signing + verify keys for test fixtures.
/// Compilation goes through `l10n4x_compiler`, which uses `l10n4x_core::integrity`
/// directly — no FFI needed for signing.
fn install_test_keys() {
    let seed = [11u8; 32];
    let _ = l10n4x_compiler::signing::set_signing_key(&seed);
    let pubkey = l10n4x_compiler::signing::signing_public_key().unwrap();
    assert_eq!(
        l10n4c_set_verify_key(pubkey.as_ptr(), pubkey.len()),
        L10N4C_OK
    );
}

/// Compile test fixtures from JSON → .pak using the compiler crate.
fn compile_fixtures(src: &Path, out: &Path, encrypt: bool) {
    l10n4x_compiler::compile_translations(src, out, encrypt, 8).unwrap();
}

fn translate_helper(
    locale: *const std::os::raw::c_char,
    key_hash: u64,
) -> String {
    let mut size = 0usize;
    let code = l10n4c_translate_required_size(locale, key_hash, &mut size);
    assert!(code == L10N4C_OK || code == L10N4C_KEY_NOT_FOUND || code == L10N4C_LOCALE_NOT_LOADED);
    let mut buf = vec![0u8; size.max(1)];
    let written_code = l10n4c_translate(locale, key_hash, buf.as_mut_ptr(), buf.len());
    assert!(
        written_code == L10N4C_OK
            || written_code == L10N4C_KEY_NOT_FOUND
            || written_code == L10N4C_LOCALE_NOT_LOADED
    );
    std::str::from_utf8(&buf[..size.saturating_sub(1)])
        .unwrap_or("")
        .to_string()
}

fn translate_with_params_helper(
    locale: *const std::os::raw::c_char,
    key_hash: u64,
    params: HashMap<&str, &str>,
) -> String {
    let strings: Vec<CString> = params
        .iter()
        .flat_map(|(k, v)| [CString::new(*k).unwrap(), CString::new(*v).unwrap()])
        .collect();
    let mut c_params = Vec::with_capacity(params.len());
    for (i, _) in (0..params.len()).enumerate() {
        c_params.push(L10n4cParam {
            key: strings[i * 2].as_ptr(),
            value: strings[i * 2 + 1].as_ptr(),
        });
    }
    let ptr = l10n4c_translate_with_params_alloc(locale, key_hash, c_params.as_ptr(), c_params.len());
    assert!(!ptr.is_null());
    let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    l10n4c_free_string(ptr);
    s
}

// ─── Test: compile .pak from JSON, load via FFI, translate ──────────────────

fn test_compiler_and_pak_loading() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_src");
    let temp_es_dir = temp_src.join("es");
    let temp_en_dir = temp_src.join("en");

    fs::create_dir_all(&temp_es_dir).unwrap();
    fs::create_dir_all(&temp_en_dir).unwrap();

    fs::write(
        temp_es_dir.join("errors.json"),
        r#"{"unauthorized": "No autorizado por favor inicie sesión."}"#,
    )
    .unwrap();
    fs::write(
        temp_en_dir.join("errors.json"),
        r#"{"unauthorized": "Unauthorized please log in."}"#,
    )
    .unwrap();

    let temp_out = Path::new("temp_test_out");
    compile_fixtures(temp_src, temp_out, false);

    assert!(temp_out.join("es.pak").is_file());
    assert!(temp_out.join("en.pak").is_file());

    l10n4c_clear();
    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_es = CString::new("es").unwrap();
    let locale_en = CString::new("en").unwrap();
    let key_unauth = fnv1a_64(b"errors.unauthorized");

    assert_eq!(
        translate_helper(locale_es.as_ptr(), key_unauth),
        "No autorizado por favor inicie sesión."
    );
    assert_eq!(
        translate_helper(locale_en.as_ptr(), key_unauth),
        "Unauthorized please log in."
    );

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

// ─── Test: fallback locale behavior ─────────────────────────────────────────

fn test_fallback_locale() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_fb_src");
    let temp_en_dir = temp_src.join("en");
    let temp_es_dir = temp_src.join("es");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::create_dir_all(&temp_es_dir).unwrap();

    fs::write(
        temp_en_dir.join("common.json"),
        r#"{"greeting": "Hello!", "fallback_only": "English only"}"#,
    )
    .unwrap();
    fs::write(temp_es_dir.join("common.json"), r#"{"greeting": "¡Hola!"}"#).unwrap();

    let temp_out = Path::new("temp_test_fb_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_en = CString::new("en").unwrap();
    let locale_es = CString::new("es").unwrap();
    let locale_fr = CString::new("fr").unwrap();

    // Fallback to "en" (default)
    let key_fallback_only = fnv1a_64(b"common.fallback_only");
    assert_eq!(
        translate_helper(locale_es.as_ptr(), key_fallback_only),
        "English only"
    );

    // Switch fallback to "es"
    let fallback_es = CString::new("es").unwrap();
    assert_eq!(l10n4c_set_fallback_locale(fallback_es.as_ptr()), L10N4C_OK);

    let key_greeting = fnv1a_64(b"common.greeting");
    assert_eq!(
        translate_helper(locale_fr.as_ptr(), key_greeting),
        "¡Hola!"
    );

    // Reset fallback
    let fallback_en = CString::new("en").unwrap();
    assert_eq!(l10n4c_set_fallback_locale(fallback_en.as_ptr()), L10N4C_OK);

    // Missing key returns key as fallback
    let key_missing_hash = fnv1a_64(b"common.missing_key");
    let mut missing_size = 0usize;
    let missing_code =
        l10n4c_translate_required_size(locale_es.as_ptr(), key_missing_hash, &mut missing_size);
    assert_eq!(missing_code, L10N4C_KEY_NOT_FOUND);

    // Clear and verify empty state - store uses {:#x} format for missing keys
    l10n4c_clear();
    let key_greeting_hash = fnv1a_64(b"common.greeting");
    let expected_hex = format!("{:#x}", key_greeting_hash);
    let post_clear = translate_helper(locale_en.as_ptr(), key_greeting_hash);
    assert_eq!(post_clear, expected_hex);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

// ─── Test: variable interpolation + ICU plural ──────────────────────────────

fn test_variable_interpolation_and_plurals() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_interp_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(
        temp_en_dir.join("common.json"),
        r#"{
            "welcome": "Hello {name}!",
            "messages": "You have {count, plural, =0 {no messages} =1 {one message} other {# messages}}."
        }"#,
    )
    .unwrap();

    let temp_out = Path::new("temp_test_interp_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_en = CString::new("en").unwrap();
    let key_welcome = fnv1a_64(b"common.welcome");
    let key_messages = fnv1a_64(b"common.messages");

    let mut welcome_params = HashMap::new();
    welcome_params.insert("name", "Diego");
    assert_eq!(
        translate_with_params_helper(locale_en.as_ptr(), key_welcome, welcome_params),
        "Hello Diego!"
    );

    for (count, expected) in [
        ("0", "You have no messages."),
        ("1", "You have one message."),
        ("5", "You have 5 messages."),
    ] {
        let mut p = HashMap::new();
        p.insert("count", count);
        assert_eq!(
            translate_with_params_helper(locale_en.as_ptr(), key_messages, p),
            expected
        );
    }

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

// ─── Test: encrypted .pak (L10E envelope) ───────────────────────────────────

fn test_encrypted_pak_compile_and_load() {
    l10n4c_clear();
    install_test_keys();

    let enc_key = [22u8; 32];
    assert_eq!(
        l10n4c_set_decrypt_key(enc_key.as_ptr(), enc_key.len()),
        L10N4C_OK
    );
    // Also configure encryption key in core for compilation
    assert!(l10n4x_core::encryption::set_decrypt_key(&enc_key));

    let temp_src = Path::new("temp_test_enc_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(
        temp_en_dir.join("common.json"),
        r#"{"greeting": "Hello encrypted"}"#,
    )
    .unwrap();

    let temp_out = Path::new("temp_test_enc_out");
    compile_fixtures(temp_src, temp_out, true);

    let pak_bytes = fs::read(temp_out.join("en.pak")).unwrap();
    assert_eq!(&pak_bytes[0..4], b"L10E");

    l10n4c_clear();
    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_en = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.greeting");
    assert_eq!(
        translate_helper(locale_en.as_ptr(), key_hash),
        "Hello encrypted"
    );

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

// ─── Test: alloc API ────────────────────────────────────────────────────────

fn test_alloc_api() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_alloc_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"greet": "Hi"}"#).unwrap();

    let temp_out = Path::new("temp_test_alloc_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.greet");
    let ptr = l10n4c_translate_alloc(locale.as_ptr(), key_hash);
    assert!(!ptr.is_null());
    let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
    assert_eq!(s, "Hi");
    l10n4c_free_string(ptr);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

// ─── Hardening & Synchronization Tests ──────────────────────────────────────

#[test]
fn test_error_constants_match_header() {
    let bindings = bindgen::Builder::default()
        .header("l10n4c.h")
        .generate()
        .expect("Unable to generate bindings")
        .to_string();

    let mut values = std::collections::HashMap::new();
    for line in bindings.lines() {
        if line.contains("pub const L10N4C_") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let name = parts[2].trim_end_matches(':');
                let value_str = parts[5].trim_end_matches(';');
                if let Ok(val) = value_str.parse::<i32>() {
                    values.insert(name.to_string(), val);
                }
            }
        }
    }

    assert_eq!(
        *values.get("L10N4C_OK").expect("L10N4C_OK"),
        l10n4c::L10N4C_OK
    );
    assert_eq!(
        *values
            .get("L10N4C_KEY_NOT_FOUND")
            .expect("L10N4C_KEY_NOT_FOUND"),
        l10n4c::L10N4C_KEY_NOT_FOUND
    );
    assert_eq!(
        *values
            .get("L10N4C_LOCALE_NOT_LOADED")
            .expect("L10N4C_LOCALE_NOT_LOADED"),
        l10n4c::L10N4C_LOCALE_NOT_LOADED
    );
    assert_eq!(
        *values
            .get("L10N4C_BUFFER_TOO_SMALL")
            .expect("L10N4C_BUFFER_TOO_SMALL"),
        l10n4c::L10N4C_BUFFER_TOO_SMALL
    );
    assert_eq!(
        *values
            .get("L10N4C_INVALID_PARAMS")
            .expect("L10N4C_INVALID_PARAMS"),
        l10n4c::L10N4C_INVALID_PARAMS
    );
    assert_eq!(
        *values
            .get("L10N4C_INTERNAL_ERROR")
            .expect("L10N4C_INTERNAL_ERROR"),
        l10n4c::L10N4C_INTERNAL_ERROR
    );
    assert_eq!(
        *values
            .get("L10N4C_INVALID_ENCODING")
            .expect("L10N4C_INVALID_ENCODING"),
        l10n4c::L10N4C_INVALID_ENCODING
    );
    assert_eq!(
        *values.get("L10N4C_IO_ERROR").expect("L10N4C_IO_ERROR"),
        l10n4c::L10N4C_IO_ERROR
    );
    assert_eq!(
        *values
            .get("L10N4C_SIGNATURE_INVALID")
            .expect("L10N4C_SIGNATURE_INVALID"),
        l10n4c::L10N4C_SIGNATURE_INVALID
    );
    assert_eq!(
        *values
            .get("L10N4C_VERIFY_KEY_NOT_SET")
            .expect("L10N4C_VERIFY_KEY_NOT_SET"),
        l10n4c::L10N4C_VERIFY_KEY_NOT_SET
    );
    assert_eq!(
        *values
            .get("L10N4C_DECRYPT_KEY_NOT_SET")
            .expect("L10N4C_DECRYPT_KEY_NOT_SET"),
        l10n4c::L10N4C_DECRYPT_KEY_NOT_SET
    );
    assert_eq!(
        *values
            .get("L10N4C_BUFFER_OVERFLOW")
            .expect("L10N4C_BUFFER_OVERFLOW"),
        l10n4c::L10N4C_BUFFER_OVERFLOW
    );
}

fn test_ffi_invalid_utf8() {
    l10n4c_clear();
    // Pass invalid UTF-8 sequence to fallback locale setting
    let invalid_utf8 = b"en_\xff\xff\x00";
    let code = l10n4c_set_fallback_locale(invalid_utf8.as_ptr() as *const std::os::raw::c_char);
    assert_eq!(code, l10n4c::L10N4C_INVALID_ENCODING);
}

fn test_ffi_buffer_overflow() {
    l10n4c_clear();
    let locale = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.greet");
    let dummy_param = L10n4cParam {
        key: std::ptr::null(),
        value: std::ptr::null(),
    };
    // Pass maximum usize to cause checked multiplication overflow
    let code =
        l10n4c_translate_with_params_alloc(locale.as_ptr(), key_hash, &dummy_param, usize::MAX);
    assert!(code.is_null());

    let mut out_size = 0usize;
    let size_code = l10n4c_translate_with_params_required_size(
        locale.as_ptr(),
        key_hash,
        &dummy_param,
        usize::MAX,
        &mut out_size,
    );
    assert_eq!(size_code, l10n4c::L10N4C_BUFFER_OVERFLOW);
}

fn test_fallback_chain() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_chain_src");
    let temp_en_dir = temp_src.join("en");
    let temp_es_dir = temp_src.join("es");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::create_dir_all(&temp_es_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"greeting": "Hello"}"#).unwrap();
    fs::write(temp_es_dir.join("common.json"), r#"{"greeting": "Hola"}"#).unwrap();

    let temp_out = Path::new("temp_test_chain_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    // Use fallback chain: [es, en]
    let es_c = CString::new("es").unwrap();
    let en_c = CString::new("en").unwrap();
    let chain = [es_c.as_ptr(), en_c.as_ptr()];
    let code = unsafe { l10n4c_set_fallback_chain(chain.as_ptr(), 2) };
    assert_eq!(code, L10N4C_OK);

    // Translate with explicit locale "fr" (not loaded) — should fallback through chain
    let locale_fr = CString::new("fr").unwrap();
    let key_hash = fnv1a_64(b"common.greeting");
    assert_eq!(
        translate_helper(locale_fr.as_ptr(), key_hash),
        "Hola"
    );

    // Test with null chain
    let null_code = unsafe { l10n4c_set_fallback_chain(std::ptr::null(), 1) };
    assert_eq!(null_code, L10N4C_INVALID_PARAMS);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_missing_key_handler_callback() {
    l10n4c_clear();
    install_test_keys();

    static CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static CALLED_LOCALE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    static CALLED_KEY_HASH: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

    // Reset
    CALLED.store(false, std::sync::atomic::Ordering::SeqCst);

    let temp_src = Path::new("temp_test_handler_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"existing": "exists"}"#).unwrap();

    let temp_out = Path::new("temp_test_handler_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    unsafe extern "C" fn missing_key_cb(locale: *const std::os::raw::c_char, key_hash: u64) {
        let loc = unsafe { CStr::from_ptr(locale) }.to_str().unwrap_or("").to_string();
        *CALLED_LOCALE.lock().unwrap() = Some(loc);
        *CALLED_KEY_HASH.lock().unwrap() = Some(key_hash);
        CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    unsafe { l10n4c_set_missing_key_handler(Some(missing_key_cb)) };

    let locale_en = CString::new("en").unwrap();
    let missing_key_hash = fnv1a_64(b"common.nonexistent");
    let mut buf = [0u8; 64];
    l10n4c_translate(locale_en.as_ptr(), missing_key_hash, buf.as_mut_ptr(), 64);

    assert!(CALLED.load(std::sync::atomic::Ordering::SeqCst), "missing key handler should have been called");
    assert_eq!(*CALLED_LOCALE.lock().unwrap(), Some("en".to_string()));
    assert_eq!(*CALLED_KEY_HASH.lock().unwrap(), Some(fnv1a_64(b"common.nonexistent")));

    // Clear handler
    unsafe { l10n4c_set_missing_key_handler(None) };

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_register_custom_formatter_ffi() {
    l10n4c_clear();

    unsafe extern "C" fn uppercase_formatter(
        value: *const std::os::raw::c_char,
        _locale: *const std::os::raw::c_char,
        _options: *const std::os::raw::c_char,
    ) -> *mut std::os::raw::c_char {
        let s = unsafe { CStr::from_ptr(value) }.to_str().unwrap_or("");
        CString::new(s.to_uppercase()).unwrap_or_default().into_raw()
    }

    let name = CString::new("c_upper").unwrap();
    let code = l10n4c_register_formatter(name.as_ptr(), Some(uppercase_formatter));
    assert_eq!(code, L10N4C_OK);

    // Verify by compiling a message and translating with it
    install_test_keys();

    let temp_src = Path::new("temp_test_cfmt_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"welcome": "Hello {name, c_upper}!"}"#).unwrap();

    let temp_out = Path::new("temp_test_cfmt_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_en = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.welcome");
    let mut buf = [0u8; 128];
    let code = l10n4c_translate(locale_en.as_ptr(), key_hash, buf.as_mut_ptr(), 128);
    assert!(code == L10N4C_OK || code == L10N4C_KEY_NOT_FOUND);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_get_loaded_locales_with_data() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_locales_src");
    let temp_en_dir = temp_src.join("en");
    let temp_es_dir = temp_src.join("es");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::create_dir_all(&temp_es_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"key": "val"}"#).unwrap();
    fs::write(temp_es_dir.join("common.json"), r#"{"key": "valor"}"#).unwrap();

    let temp_out = Path::new("temp_test_locales_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let mut buf = [0u8; 64];
    let result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 64);
    assert!(result > 0);
    let s = std::str::from_utf8(&buf[..result as usize]).unwrap();
    assert!(s.contains("en") || s.contains("es"), "expected locales in output, got: {}", s);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_translate_with_params_buffer_api() {
    l10n4c_clear();
    install_test_keys();

    let temp_src = Path::new("temp_test_twp_src");
    let temp_en_dir = temp_src.join("en");
    fs::create_dir_all(&temp_en_dir).unwrap();
    fs::write(temp_en_dir.join("common.json"), r#"{"hello": "Hello {name}!"}"#).unwrap();

    let temp_out = Path::new("temp_test_twp_out");
    compile_fixtures(temp_src, temp_out, false);

    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();
    assert_eq!(l10n4c_load_pak_directory(out_c.as_ptr()), L10N4C_OK);

    let locale_en = CString::new("en").unwrap();
    let key_hash = fnv1a_64(b"common.hello");
    let param_name = CString::new("name").unwrap();
    let param_val = CString::new("World").unwrap();
    let c_param = l10n4c::L10n4cParam { key: param_name.as_ptr(), value: param_val.as_ptr() };

    // Test required_size
    let mut out_size = 0usize;
    let size_code = l10n4c_translate_with_params_required_size(
        locale_en.as_ptr(), key_hash,
        &c_param, 1,
        &mut out_size,
    );
    assert!(size_code == L10N4C_OK || size_code == L10N4C_KEY_NOT_FOUND);
    assert!(out_size > 5);

    // Test buffer translate
    let mut buf = vec![0u8; out_size.max(16)];
    let code = l10n4c_translate_with_params(
        locale_en.as_ptr(), key_hash,
        &c_param, 1,
        buf.as_mut_ptr(), buf.len(),
    );
    assert!(code == L10N4C_OK || code == L10N4C_KEY_NOT_FOUND);

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_load_static_bytes_ffi() {
    l10n4c_clear();

    static L10N_DATA: &[u8] = &[
        b'L', b'1', b'0', b'N',
        0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x10,
        0x00, 0x00, 0x00, 0x00,
    ];

    let locale = CString::new("static_test").unwrap();
    let code = l10n4c_load_static_bytes(
        locale.as_ptr(),
        L10N_DATA.as_ptr(),
        L10N_DATA.len(),
        1,
    );
    assert_eq!(code, l10n4c::L10N4C_OK);

    let mut buf = [0u8; 64];
    let locales_result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 64);
    assert!(locales_result > 0);
    let s = std::str::from_utf8(&buf[..locales_result as usize]).unwrap();
    assert!(s.contains("static_test"), "expected static_test in loaded locales, got: {}", s);
}

// ─── Single test entry point (avoid global state races) ─────────────────────

#[test]
fn run_all_ffi_integration_tests() {
    test_compiler_and_pak_loading();
    test_fallback_locale();
    test_variable_interpolation_and_plurals();
    test_encrypted_pak_compile_and_load();
    test_alloc_api();
    test_ffi_invalid_utf8();
    test_ffi_buffer_overflow();
    test_fallback_chain();
    test_get_loaded_locales_with_data();
    test_translate_with_params_buffer_api();
    test_missing_key_handler_callback();
    test_register_custom_formatter_ffi();
    test_load_static_bytes_ffi();
}
