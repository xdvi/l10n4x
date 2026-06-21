//! `l10n4x-compiler` is the translation compilation toolkit component of `l10n4x`.
//! It parses translation templates in JSON/ICU format, flattens hierarchical namespaces,
//! and compiles them into compressed `.pak` binary assets.

pub mod binary_writer;
pub mod icu_parser;

use binary_writer::write_binary_format;
use icu_parser::MessageParser;
use l10n4x_core::envelope;
use l10n4x_core::integrity;
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

/// Compiles directories of JSON localization files into signed `.pak` files.
/// When `encrypt` is true, wraps each pak in an optional `L10E` AES-GCM envelope.
pub fn compile_translations(
    src_path: &Path,
    out_path: &Path,
    encrypt: bool,
) -> Result<(), CompileError> {
    if !src_path.is_dir() {
        return Err(CompileError::SourceNotADirectory);
    }

    if !out_path.exists() {
        fs::create_dir_all(out_path)?;
    }

    let lang_dirs = fs::read_dir(src_path)?;

    for lang_entry in lang_dirs {
        let lang_entry = lang_entry?;
        let lang_path = lang_entry.path();
        if lang_path.is_dir() {
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

            // Parse flat translations into AST/MessageNodes
            let mut parsed_translations = HashMap::new();
            for (k, template) in raw_flat_translations {
                let parser = MessageParser::new(&template);
                let nodes = parser.parse().map_err(CompileError::TemplateParseError)?;
                parsed_translations.insert(k, nodes);
            }

            // Compile into binary format
            let binary_bytes = write_binary_format(&parsed_translations);

            // Compress using DEFLATE
            let compressed_bytes = miniz_oxide::deflate::compress_to_vec(&binary_bytes, 6);

            let unsigned = build_unsigned(&compressed_bytes);
            let signature = integrity::sign(&unsigned)
                .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))?;
            let signed = seal(&unsigned, &signature);
            let pak_bytes = if encrypt {
                envelope::wrap_encrypted(&signed)
                    .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))?
            } else {
                signed
            };

            let pak_file_path = out_path.join(format!("{}.pak", lang));
            fs::write(pak_file_path, pak_bytes)?;
        }
    }

    Ok(())
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
}
