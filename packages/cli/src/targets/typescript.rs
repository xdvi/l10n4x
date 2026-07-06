use ahash::AHashMap;
use crate::targets::GenerateContext;
use serde_json::Value;
use std::fs;
use std::path::Path;

const TS_KEYS_TEMPLATE: &str = include_str!("../templates/ts_keys.ts");

pub fn generate(
    out_dir: &Path,
    key_pairs: &[(u64, String)],
    _options: &Value,
    _ctx: &GenerateContext<'_>,
    params_map: &AHashMap<String, Vec<String>>,
) -> Result<(), anyhow::Error> {
    let mut key_entries = String::new();
    for (hash, name) in key_pairs {
        let pascal_name = crate::generator::to_pascal_case(name);
        key_entries.push_str(&format!("  {}: 0x{:016x},\n", pascal_name, hash));
    }

    let mut param_types = String::new();
    for (_, name) in key_pairs {
        if let Some(param_names) = params_map.get(name) {
            if !param_names.is_empty() {
                let pascal = crate::generator::to_pascal_case(name);
                let fields: String = param_names
                    .iter()
                    .map(|p| format!("  {}: string | number;", p))
                    .collect::<Vec<_>>()
                    .join("\n");
                param_types.push_str(&format!(
                    "export type {pascal}Params = {{\n{fields}\n}};\n\n",
                    pascal = pascal,
                    fields = fields
                ));
            }
        }
    }

    let content = TS_KEYS_TEMPLATE
        .replace("{{KEY_ENTRIES}}", key_entries.trim_end())
        .replace("{{PARAM_TYPES}}", param_types.trim_end());

    let file_path = out_dir.join("generated.ts");
    fs::write(&file_path, content)?;
    println!(
        "Generated thin TypeScript keys at '{}' (use @l10n4x/runtime + framework adapter)",
        file_path.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::GenerateContext;

    fn test_ctx() -> GenerateContext<'static> {
        GenerateContext {
            fallback: "en",
            output_dir: "",
            source_dir: "",
            verify_key_bytes: "000000000000000000000000000000000000000000000000000000000000000000",
            verify_public_key_hex: "",
            encrypt: false,
            encrypt_key_env: "",
        }
    }

    #[test]
    fn generates_thin_keys_only() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "welcome.title".to_string())];
        let params = AHashMap::new();
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("WelcomeTitle: 0xabcdef0123456789"));
        assert!(content.contains("export const Keys"));
        assert!(!content.contains("l10n4x-wasm"));
        assert!(!content.contains("useTranslation"));
    }

    #[test]
    fn generates_param_types() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "greeting".to_string())];
        let mut params = AHashMap::new();
        params.insert("greeting".to_string(), vec!["name".to_string()]);
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("export type GreetingParams"));
        assert!(content.contains("name: string | number"));
    }
}
