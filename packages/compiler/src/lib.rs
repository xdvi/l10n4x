//! `l10n4x-compiler` is the translation compilation toolkit component of `l10n4x`.
//! It parses translation templates in JSON/ICU format, flattens hierarchical namespaces,
//! and compiles them into compressed `.pak` binary assets.

pub mod binary_writer;
pub mod icu_parser;
pub mod signing;

use binary_writer::write_binary_format;
use icu_parser::MessageParser;
use l10n4x_core::envelope;
use l10n4x_core::pak::{build_unsigned, seal};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Recursively flattens a JSON Value into a flat string map.
///
/// Arrays of primitives are stored as a single JSON literal at the array key
/// (e.g. `menu.items` -> `["Home","Settings"]`). Arrays of objects require
/// semantic keys inside each element; numeric index flattening is not supported.
pub fn flatten_value(prefix: String, value: &Value, map: &mut HashMap<String, String>) {
    match value {
        Value::Object(obj) => {
            for (k, v) in obj {
                let new_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_value(new_prefix, v, map);
            }
        }
        Value::Array(arr) => {
            if arr.iter().all(|v| {
                matches!(
                    v,
                    Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
                )
            }) {
                if let Ok(literal) = serde_json::to_string(arr) {
                    map.insert(prefix, literal);
                }
            } else {
                for v in arr {
                    if let Value::Object(obj) = v {
                        for (k, inner) in obj {
                            let new_prefix = if prefix.is_empty() {
                                k.clone()
                            } else {
                                format!("{}.{}", prefix, k)
                            };
                            flatten_value(new_prefix, inner, map);
                        }
                    }
                }
            }
        }
        Value::String(s) => {
            map.insert(prefix, s.clone());
        }
        Value::Number(n) => {
            map.insert(prefix, n.to_string());
        }
        Value::Bool(b) => {
            map.insert(prefix, b.to_string());
        }
        Value::Null => {
            map.insert(prefix, String::new());
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("Source is not a directory")]
    SourceNotADirectory,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization/parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid filename")]
    InvalidFileName,
    #[error("Invalid directory name")]
    InvalidDirectoryName,
    #[error("Core integrity error: {0}")]
    CoreIntegrityError(String),
    #[error("Failed to parse translation template: {0}")]
    TemplateParseError(String),
}

/// Resolves `MessageNode::KeyRef` cross-references by inlining the target key's nodes.
/// Performs a single-pass resolution with cycle detection.
/// Missing or cyclic references are replaced with a `Text` node containing the key name.
pub fn resolve_key_refs(translations: &mut HashMap<String, Vec<icu_parser::MessageNode>>) {
    use icu_parser::MessageNode;

    let keys_with_refs: Vec<String> = translations
        .iter()
        .filter(|(_, nodes)| nodes.iter().any(|n| matches!(n, MessageNode::KeyRef(_))))
        .map(|(k, _)| k.clone())
        .collect();

    let mut resolving: std::collections::HashSet<String> = std::collections::HashSet::new();
    for key in keys_with_refs {
        resolve_single(key, translations, &mut resolving);
    }
}

fn resolve_single(
    key: String,
    translations: &mut HashMap<String, Vec<icu_parser::MessageNode>>,
    resolving: &mut std::collections::HashSet<String>,
) {
    use icu_parser::MessageNode;

    if resolving.contains(&key) {
        return;
    }
    resolving.insert(key.clone());

    let nodes = match translations.get(&key) {
        Some(n) if n.iter().any(|nd| matches!(nd, MessageNode::KeyRef(_))) => n.clone(),
        _ => { resolving.remove(&key); return; }
    };

    let mut resolved: Vec<MessageNode> = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            MessageNode::KeyRef(ref_key) => {
                if !resolving.contains(&ref_key) {
                    resolve_single(ref_key.clone(), translations, resolving);
                }
                match translations.get(&ref_key) {
                    Some(target_nodes) if !target_nodes.iter().any(|n| matches!(n, MessageNode::KeyRef(_))) => {
                        resolved.extend_from_slice(target_nodes);
                    }
                    _ => {
                        resolved.push(MessageNode::Text(ref_key));
                    }
                }
            }
            other => resolved.push(other),
        }
    }

    translations.insert(key.clone(), resolved);
    resolving.remove(&key);
}

/// Compiles directories of JSON localization files into signed `.pak` files.
/// When `encrypt` is true, wraps each pak in an optional `L10E` AES-GCM envelope.
pub fn compile_translations(
    src_path: &Path,
    out_path: &Path,
    encrypt: bool,
    compression_level: i32,
) -> Result<(), CompileError> {
    let compiled = compile_pipeline(src_path)?;

    if !out_path.exists() {
        fs::create_dir_all(out_path)?;
    }

    for (locale, nodes) in &compiled {
        let binary_bytes = write_binary_format(nodes);

        let compressed_bytes = zstd::encode_all(&binary_bytes[..], compression_level)
            .map_err(|e| CompileError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let unsigned = build_unsigned(&compressed_bytes);
        let signature = signing::sign(&unsigned)
            .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))?;
        let signed = seal(&unsigned, &signature);
        let pak_bytes = if encrypt {
            envelope::wrap_encrypted(&signed)
                .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))?
        } else {
            signed
        };

        let pak_file_path = out_path.join(format!("{}.pak", locale));
        fs::write(pak_file_path, pak_bytes)?;
    }

    Ok(())
}

/// Parses all JSON locale files in `src_path` and returns a map of
/// key → sorted list of interpolation variable names extracted from that key's message.
/// Uses only the first locale directory found (all locales share the same keys).
pub fn extract_params_map(src_path: &Path) -> Result<HashMap<String, Vec<String>>, CompileError> {
    if !src_path.is_dir() {
        return Err(CompileError::SourceNotADirectory);
    }
    let first_lang_dir = std::fs::read_dir(src_path)?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir());

    let lang_path = match first_lang_dir {
        Some(e) => e.path(),
        None => return Ok(HashMap::new()),
    };

    let mut result: HashMap<String, Vec<String>> = HashMap::new();

    for file_entry in std::fs::read_dir(&lang_path)? {
        let file_entry = file_entry?;
        let file_path = file_entry.path();
        if !file_path.is_file() || file_path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let file_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or(CompileError::InvalidFileName)?
            .to_string();
        let content = std::fs::read_to_string(&file_path)?;
        let parsed_json: serde_json::Value = serde_json::from_str(&content)?;
        let mut flat: HashMap<String, String> = HashMap::new();
        flatten_value(file_name, &parsed_json, &mut flat);

        for (key, template) in flat {
            let parser = MessageParser::new(&template);
            let nodes = parser.parse().map_err(CompileError::TemplateParseError)?;
            let mut params = icu_parser::extract_params(&nodes);
            params.sort();
            if !params.is_empty() {
                result.insert(key, params);
            }
        }
    }
    Ok(result)
}

/// Internal: read translations from a source directory, parse ICU, resolve refs.
/// Returns a map of locale → compiled MessageNode AST.
///
/// This is the core pipeline shared by `compile_translations` and
/// `compile_translations_to_bytes`.
fn compile_pipeline(
    src_path: &Path,
) -> Result<HashMap<String, HashMap<String, Vec<icu_parser::MessageNode>>>, CompileError> {
    if !src_path.is_dir() {
        return Err(CompileError::SourceNotADirectory);
    }

    let lang_dirs = fs::read_dir(src_path)?;
    let mut all_translations: HashMap<String, HashMap<String, Vec<icu_parser::MessageNode>>> =
        HashMap::new();

    for lang_entry in lang_dirs {
        let lang_entry = lang_entry?;
        let lang_path = lang_entry.path();
        if !lang_path.is_dir() {
            continue;
        }

        let lang = lang_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(CompileError::InvalidDirectoryName)?
            .to_string();

        let mut raw_flat_translations = HashMap::new();
        let mut file_count = 0;

        let files = fs::read_dir(&lang_path)?;

        for file_entry in files {
            let file_entry = file_entry?;
            let file_path = file_entry.path();
            if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
                let file_name = file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or(CompileError::InvalidFileName)?
                    .to_string();

                let content = fs::read_to_string(&file_path)?;
                let parsed_json: Value = serde_json::from_str(&content)?;

                flatten_value(file_name, &parsed_json, &mut raw_flat_translations);
                file_count += 1;
            }
        }

        if file_count == 0 {
            continue;
        }

        let mut parsed_translations: HashMap<String, Vec<icu_parser::MessageNode>> =
            HashMap::new();
        for (k, template) in raw_flat_translations {
            if let Some(interval_cases) = icu_parser::parse_interval_plural(&template) {
                let nodes = vec![icu_parser::MessageNode::Plural {
                    var: "count".to_string(),
                    ordinal: false,
                    cases: interval_cases,
                }];
                parsed_translations.insert(k, nodes);
            } else {
                let parser = MessageParser::new(&template);
                let nodes = parser.parse().map_err(CompileError::TemplateParseError)?;
                parsed_translations.insert(k, nodes);
            }
        }

        resolve_key_refs(&mut parsed_translations);

        all_translations.insert(lang, parsed_translations);
    }

    Ok(all_translations)
}

/// Compiles translations from a source directory into raw L10N binary bytes.
///
/// This function **never** applies compression, signing, or encryption.
/// It ONLY produces the raw L10N-format bytes. This is intentional:
/// the caller (typically a `build.rs`) decides whether and how to apply
/// those transforms.
///
/// Unlike `compile_translations`:
/// - Does NOT write to disk.
/// - Does NOT compress, sign, or encrypt the output.
/// - Returns the raw L10N-format bytes ready for embed via `include_bytes!`.
///
/// This is the primary API intended for `build.rs` usage.
///
/// # Signature verification
///
/// The returned bytes are NOT signed. If you need signature verification
/// (recommended for production), you MUST apply it in your build script
/// using `l10n4x_compiler::signing::sign()` before embedding.
pub fn compile_translations_to_bytes(
    src_path: &Path,
) -> Result<HashMap<String, Vec<u8>>, CompileError> {
    let compiled = compile_pipeline(src_path)?;
    let mut result = HashMap::new();
    for (locale, nodes) in &compiled {
        let bytes = write_binary_format(nodes);
        result.insert(locale.clone(), bytes);
    }
    Ok(result)
}

#[cfg(test)]
mod key_ref_tests {
    use super::*;
    use crate::icu_parser::{MessageNode, MessageParser};

    #[test]
    fn key_ref_is_inlined_at_compile_time() {
        let mut translations: HashMap<String, Vec<MessageNode>> = HashMap::new();
        translations.insert("common.ok".to_string(),
            MessageParser::new("OK").parse().unwrap());
        translations.insert("button.save".to_string(),
            MessageParser::new("$t(common.ok)").parse().unwrap());

        resolve_key_refs(&mut translations);

        let nodes = translations.get("button.save").unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MessageNode::Text(t) if t == "OK"));
    }

    #[test]
    fn cycle_detection_does_not_panic() {
        let mut translations: HashMap<String, Vec<MessageNode>> = HashMap::new();
        translations.insert("a".to_string(),
            MessageParser::new("$t(b)").parse().unwrap());
        translations.insert("b".to_string(),
            MessageParser::new("$t(a)").parse().unwrap());

        resolve_key_refs(&mut translations);
    }

    #[test]
    fn missing_ref_target_becomes_key_literal() {
        let mut translations: HashMap<String, Vec<MessageNode>> = HashMap::new();
        translations.insert("greeting".to_string(),
            MessageParser::new("$t(nonexistent.key)").parse().unwrap());

        resolve_key_refs(&mut translations);

        let nodes = translations.get("greeting").unwrap();
        assert!(matches!(&nodes[0], MessageNode::Text(t) if t.contains("nonexistent.key")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_primitive_array_as_literal() {
        let val = json!({ "items": ["A", "B"] });
        let mut map = HashMap::new();
        flatten_value("menu".to_string(), &val, &mut map);
        assert_eq!(map.get("menu.items").unwrap(), r#"["A","B"]"#);
    }

    #[test]
    fn flatten_object_array_with_semantic_keys() {
        let val = json!({
            "items": [
                { "home": "Home" },
                { "settings": "Settings" }
            ]
        });
        let mut map = HashMap::new();
        flatten_value("menu".to_string(), &val, &mut map);
        assert_eq!(map.get("menu.items.home").unwrap(), "Home");
        assert_eq!(map.get("menu.items.settings").unwrap(), "Settings");
    }

    #[test]
    fn flatten_string_value() {
        let val = json!("Just a string");
        let mut map = HashMap::new();
        flatten_value("key".to_string(), &val, &mut map);
        assert_eq!(map.get("key").unwrap(), "Just a string");
    }

    #[test]
    fn flatten_number_value() {
        let val = json!(42);
        let mut map = HashMap::new();
        flatten_value("num".to_string(), &val, &mut map);
        assert_eq!(map.get("num").unwrap(), "42");
    }

    #[test]
    fn flatten_boolean_value() {
        let val = json!(true);
        let mut map = HashMap::new();
        flatten_value("flag".to_string(), &val, &mut map);
        assert_eq!(map.get("flag").unwrap(), "true");
    }

    #[test]
    fn flatten_null_value() {
        let val = json!(null);
        let mut map = HashMap::new();
        flatten_value("empty".to_string(), &val, &mut map);
        assert_eq!(map.get("empty").unwrap(), "");
    }

    #[test]
    fn flatten_nested_object() {
        let val = json!({ "a": { "b": { "c": "deep" } } });
        let mut map = HashMap::new();
        flatten_value("".to_string(), &val, &mut map);
        assert_eq!(map.get("a.b.c").unwrap(), "deep");
    }

    #[test]
    fn compile_error_display_source_not_a_directory() {
        let err = CompileError::SourceNotADirectory;
        assert_eq!(format!("{}", err), "Source is not a directory");
    }

    #[test]
    fn compile_error_display_invalid_file_name() {
        let err = CompileError::InvalidFileName;
        assert_eq!(format!("{}", err), "Invalid filename");
    }

    #[test]
    fn compile_error_display_invalid_directory_name() {
        let err = CompileError::InvalidDirectoryName;
        assert_eq!(format!("{}", err), "Invalid directory name");
    }

    #[test]
    fn compile_error_display_core_integrity() {
        let err = CompileError::CoreIntegrityError("bad sig".to_string());
        assert_eq!(format!("{}", err), "Core integrity error: bad sig");
    }

    #[test]
    fn compile_error_display_template_parse() {
        let err = CompileError::TemplateParseError("parse failed".to_string());
        assert_eq!(format!("{}", err), "Failed to parse translation template: parse failed");
    }

    #[test]
    fn compile_error_is_debug() {
        let err = CompileError::SourceNotADirectory;
        let _ = format!("{:?}", err);
    }

    #[test]
    fn resolve_single_no_change_for_non_keyref() {
        let mut translations: HashMap<String, Vec<icu_parser::MessageNode>> = HashMap::new();
        translations.insert("key".to_string(),
            icu_parser::MessageParser::new("simple text").parse().unwrap());
        resolve_key_refs(&mut translations);
        let nodes = translations.get("key").unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn resolve_single_direct_ref() {
        let mut translations: HashMap<String, Vec<icu_parser::MessageNode>> = HashMap::new();
        translations.insert("target".to_string(),
            icu_parser::MessageParser::new("hello").parse().unwrap());
        translations.insert("source".to_string(),
            icu_parser::MessageParser::new("$t(target)").parse().unwrap());
        resolve_key_refs(&mut translations);
        let nodes = translations.get("source").unwrap();
        assert!(matches!(&nodes[0], icu_parser::MessageNode::Text(t) if t == "hello"));
    }

    #[test]
    fn compile_translations_empty_source() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_test_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let out = tmp.join("out");
        // Empty dir — should succeed but produce nothing
        let result = compile_translations(&tmp, &out, false, 6);
        assert!(result.is_ok());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compile_translations_single_locale() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_test_single");
        let _ = fs::remove_dir_all(&tmp);
        let en_dir = tmp.join("en");
        fs::create_dir_all(&en_dir).unwrap();
        fs::write(en_dir.join("common.json"), r#"{"hello": "Hello World"}"#).unwrap();

        // Set up signing key
        let seed = [22u8; 32];
        let _ = crate::signing::set_signing_key(&seed);

        let out = tmp.join("out");
        let result = compile_translations(&tmp, &out, false, 6);
        assert!(result.is_ok());
        assert!(out.join("en.pak").is_file());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compile_translations_with_encrypt() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_test_enc");
        let _ = fs::remove_dir_all(&tmp);
        let en_dir = tmp.join("en");
        fs::create_dir_all(&en_dir).unwrap();
        fs::write(en_dir.join("common.json"), r#"{"hello": "Hello"}"#).unwrap();

        let seed = [22u8; 32];
        let _ = crate::signing::set_signing_key(&seed);
        // Encrypt needs a key configured
        let enc_key = [33u8; 32];
        l10n4x_core::encryption::set_decrypt_key(&enc_key);

        let out = tmp.join("out");
        let result = compile_translations(&tmp, &out, true, 6);
        assert!(result.is_ok());
        let pak = fs::read(out.join("en.pak")).unwrap();
        assert_eq!(&pak[0..4], b"L10E");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compile_translations_with_interval_plural() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_test_int");
        let _ = fs::remove_dir_all(&tmp);
        let en_dir = tmp.join("en");
        fs::create_dir_all(&en_dir).unwrap();
        fs::write(en_dir.join("common.json"), r#"{"messages": "(0)[none];(1)[one];(2-7)[few];(7-inf)[many]"}"#).unwrap();

        let seed = [22u8; 32];
        let _ = crate::signing::set_signing_key(&seed);

        let out = tmp.join("out");
        let result = compile_translations(&tmp, &out, false, 6);
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compile_translations_not_a_directory() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_test_file.txt");
        fs::write(&tmp, "not a dir").unwrap();
        let out = std::env::temp_dir().join("l10n4x_out");
        let result = compile_translations(&tmp, &out, false, 6);
        assert!(result.is_err());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn extract_params_map_empty_dir() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_params_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let result = extract_params_map(&tmp);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_params_map_with_data() {
        use std::fs;
        let tmp = std::env::temp_dir().join("l10n4x_params");
        let _ = fs::remove_dir_all(&tmp);
        let en_dir = tmp.join("en");
        fs::create_dir_all(&en_dir).unwrap();
        fs::write(en_dir.join("common.json"), r#"{"greeting": "Hello {name}!"}"#).unwrap();
        let result = extract_params_map(&tmp);
        assert!(result.is_ok());
        let map = result.unwrap();
        assert!(map.contains_key("common.greeting"));
        assert!(map.get("common.greeting").unwrap().contains(&"name".to_string()));
        let _ = fs::remove_dir_all(&tmp);
    }
}
