pub mod binary_writer;
pub mod icu_parser;

use binary_writer::write_binary_format;
use icu_parser::MessageParser;
use l10n4x_core::crypto::encrypt_gcm;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Recursively flattens a JSON Value into a flat string map.
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
        _ => {} // Ignore arrays and other unsupported types
    }
}

/// Compiles directories of JSON localization files into GCM-encrypted .pak files.
pub fn compile_translations(src_path: &Path, out_path: &Path) -> Result<(), &'static str> {
    if !src_path.is_dir() {
        return Err("Source is not a directory");
    }

    if !out_path.exists() {
        fs::create_dir_all(out_path).map_err(|_| "Failed to create output directory")?;
    }

    let lang_dirs = fs::read_dir(src_path).map_err(|_| "Failed to read source directory")?;

    for lang_entry in lang_dirs {
        let lang_entry = lang_entry.map_err(|_| "Failed to read language entry")?;
        let lang_path = lang_entry.path();
        if lang_path.is_dir() {
            let lang = lang_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid directory name")?
                .to_string();

            let mut raw_flat_translations = HashMap::new();
            let mut file_count = 0;

            let files = fs::read_dir(&lang_path).map_err(|_| "Failed to read locale directory")?;

            for file_entry in files {
                let file_entry = file_entry.map_err(|_| "Failed to read file entry")?;
                let file_path = file_entry.path();
                if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
                    let file_name = file_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or("Invalid filename")?
                        .to_string();

                    let content = fs::read_to_string(&file_path)
                        .map_err(|_| "Failed to read translation file")?;
                    let parsed_json: Value =
                        serde_json::from_str(&content).map_err(|_| "Failed to parse JSON")?;

                    // Flatten under the filename prefix namespace (exactly like Go compiler did)
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
                let nodes = parser
                    .parse()
                    .map_err(|_| "Failed to parse translation template")?;
                parsed_translations.insert(k, nodes);
            }

            // Compile into binary format
            let binary_bytes = write_binary_format(&parsed_translations);

            // Compress using DEFLATE
            let compressed_bytes = miniz_oxide::deflate::compress_to_vec(&binary_bytes, 6);

            // Encrypt using AES-GCM
            let encrypted_bytes = encrypt_gcm(&compressed_bytes)?;

            // Save as <locale>.pak
            let pak_file_path = out_path.join(format!("{}.pak", lang));
            fs::write(pak_file_path, encrypted_bytes)
                .map_err(|_| "Failed to write compiled pak file")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
