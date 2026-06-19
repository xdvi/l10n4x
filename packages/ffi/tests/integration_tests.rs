use std::ffi::CString;
use std::fs;
use std::path::Path;

use l10n4c::{
    l10n4c_clear, l10n4c_load_locale, l10n4c_translate,
    l10n4c_set_fallback_locale, l10n4c_set_encryption_key, l10n4c_compile,
    l10n4c_load_pak_directory, l10n4c_translate_with_params,
};

fn translate_helper(locale: *const std::os::raw::c_char, key: *const std::os::raw::c_char) -> String {
    let mut buf = [0u8; 512];
    let written = l10n4c_translate(locale, key, buf.as_mut_ptr(), buf.len());
    std::str::from_utf8(&buf[..written]).unwrap().to_string()
}

fn translate_with_params_helper(
    locale: *const std::os::raw::c_char,
    key: *const std::os::raw::c_char,
    params_json: *const std::os::raw::c_char,
) -> String {
    let mut buf = [0u8; 512];
    let written = l10n4c_translate_with_params(locale, key, params_json, buf.as_mut_ptr(), buf.len());
    std::str::from_utf8(&buf[..written]).unwrap().to_string()
}

fn test_dynamic_translation_flow() {
    l10n4c_clear();

    let locale_en = CString::new("en").unwrap();
    let en_json = r#"{
        "errors.unauthorized": "Unauthorized. Please log in to continue.",
        "errors.database.connection_failed": "Failed to connect to the database."
    }"#;
    let c_en_json = CString::new(en_json).unwrap();
    let load_en_res = l10n4c_load_locale(locale_en.as_ptr(), c_en_json.as_ptr(), std::ptr::null());
    assert!(load_en_res);

    let locale_es = CString::new("es").unwrap();
    let es_errors_json = r#"{
        "unauthorized": "No autorizado. Inicie sesión para continuar.",
        "database": {
            "connection_failed": "Error al conectar con la base de datos."
        }
    }"#;
    let c_es_json = CString::new(es_errors_json).unwrap();
    let prefix_errors = CString::new("errors").unwrap();
    let load_es_res = l10n4c_load_locale(locale_es.as_ptr(), c_es_json.as_ptr(), prefix_errors.as_ptr());
    assert!(load_es_res);

    let key_unauth = CString::new("errors.unauthorized").unwrap();
    let key_db_conn = CString::new("errors.database.connection_failed").unwrap();

    let str_en_unauth = translate_helper(locale_en.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_en_unauth, "Unauthorized. Please log in to continue.");

    let str_es_unauth = translate_helper(locale_es.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_es_unauth, "No autorizado. Inicie sesión para continuar.");

    let str_es_db = translate_helper(locale_es.as_ptr(), key_db_conn.as_ptr());
    assert_eq!(str_es_db, "Error al conectar con la base de datos.");

    let en_fallback_json = r#"{
        "errors.fallback_test": "Fallback text"
    }"#;
    let c_en_fallback = CString::new(en_fallback_json).unwrap();
    assert!(l10n4c_load_locale(locale_en.as_ptr(), c_en_fallback.as_ptr(), std::ptr::null()));

    let key_fallback = CString::new("errors.fallback_test").unwrap();
    let str_fallback = translate_helper(locale_es.as_ptr(), key_fallback.as_ptr());
    assert_eq!(str_fallback, "Fallback text");

    let locale_fr = CString::new("fr").unwrap();
    let fallback_es = CString::new("es").unwrap();
    assert!(l10n4c_set_fallback_locale(fallback_es.as_ptr()));

    let str_fr_unauth = translate_helper(locale_fr.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_fr_unauth, "No autorizado. Inicie sesión para continuar.");

    let fallback_en = CString::new("en").unwrap();
    assert!(l10n4c_set_fallback_locale(fallback_en.as_ptr()));

    let key_missing = CString::new("errors.missing_key").unwrap();
    let str_missing = translate_helper(locale_es.as_ptr(), key_missing.as_ptr());
    assert_eq!(str_missing, "errors.missing_key");

    l10n4c_clear();
    let str_post_clear = translate_helper(locale_en.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_post_clear, "errors.unauthorized");
}

fn test_compiler_and_pak_loading() {
    l10n4c_clear();

    let temp_src = Path::new("temp_test_src");
    let temp_es_dir = temp_src.join("es");
    let temp_en_dir = temp_src.join("en");

    fs::create_dir_all(&temp_es_dir).unwrap();
    fs::create_dir_all(&temp_en_dir).unwrap();

    // Set custom encryption key for compiler and loading to verify it's dynamic
    let custom_key = b"my-custom-32-byte-secret-key-123";
    assert!(l10n4c_set_encryption_key(custom_key.as_ptr(), custom_key.len()));

    // Write test raw JSON files
    let es_errors_content = r#"{
        "unauthorized": "No autorizado por favor inicie sesión."
    }"#;
    fs::write(temp_es_dir.join("errors.json"), es_errors_content).unwrap();

    let en_errors_content = r#"{
        "unauthorized": "Unauthorized please log in."
    }"#;
    fs::write(temp_en_dir.join("errors.json"), en_errors_content).unwrap();

    // Compile
    let temp_out = Path::new("temp_test_out");
    let src_c = CString::new(temp_src.to_str().unwrap()).unwrap();
    let out_c = CString::new(temp_out.to_str().unwrap()).unwrap();

    let compile_success = l10n4c_compile(src_c.as_ptr(), out_c.as_ptr());
    assert!(compile_success);

    assert!(temp_out.join("es.pak").is_file());
    assert!(temp_out.join("en.pak").is_file());

    l10n4c_clear();

    // Load the compiled pak directory
    let load_success = l10n4c_load_pak_directory(out_c.as_ptr());
    assert!(load_success);

    let locale_es = CString::new("es").unwrap();
    let locale_en = CString::new("en").unwrap();
    let key_unauth = CString::new("errors.unauthorized").unwrap();

    let str_es = translate_helper(locale_es.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_es, "No autorizado por favor inicie sesión.");

    let str_en = translate_helper(locale_en.as_ptr(), key_unauth.as_ptr());
    assert_eq!(str_en, "Unauthorized please log in.");

    // Reset to default key
    let default_key = b"polyglot-default-key-32-bytes!!!";
    assert!(l10n4c_set_encryption_key(default_key.as_ptr(), default_key.len()));

    let _ = fs::remove_dir_all(temp_src);
    let _ = fs::remove_dir_all(temp_out);
}

fn test_variable_interpolation_and_plurals() {
    l10n4c_clear();

    let locale_en = CString::new("en").unwrap();
    let json_en = r#"{
        "welcome": "Hello {name}!",
        "messages": "You have {count, plural, =0 {no messages} =1 {one message} other {# messages}}."
    }"#;
    let c_json_en = CString::new(json_en).unwrap();
    assert!(l10n4c_load_locale(locale_en.as_ptr(), c_json_en.as_ptr(), std::ptr::null()));

    let key_welcome = CString::new("welcome").unwrap();
    let key_messages = CString::new("messages").unwrap();

    // Test simple variables
    let params_welcome = CString::new(r#"{"name": "Diego"}"#).unwrap();
    let str_welcome = translate_with_params_helper(locale_en.as_ptr(), key_welcome.as_ptr(), params_welcome.as_ptr());
    assert_eq!(str_welcome, "Hello Diego!");

    // Test plural zero
    let params_zero = CString::new(r#"{"count": "0"}"#).unwrap();
    let str_zero = translate_with_params_helper(locale_en.as_ptr(), key_messages.as_ptr(), params_zero.as_ptr());
    assert_eq!(str_zero, "You have no messages.");

    // Test plural one
    let params_one = CString::new(r#"{"count": "1"}"#).unwrap();
    let str_one = translate_with_params_helper(locale_en.as_ptr(), key_messages.as_ptr(), params_one.as_ptr());
    assert_eq!(str_one, "You have one message.");

    // Test plural other (e.g. 5)
    let params_other = CString::new(r#"{"count": "5"}"#).unwrap();
    let str_other = translate_with_params_helper(locale_en.as_ptr(), key_messages.as_ptr(), params_other.as_ptr());
    assert_eq!(str_other, "You have 5 messages.");
}

#[test]
fn run_all_ffi_integration_tests() {
    test_dynamic_translation_flow();
    test_compiler_and_pak_loading();
    test_variable_interpolation_and_plurals();
}
