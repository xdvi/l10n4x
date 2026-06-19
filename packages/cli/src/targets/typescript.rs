use std::fs;
use std::path::Path;
use serde_json::Value;

const TS_TEMPLATE: &str = include_str!("../templates/ts_generated.ts");

pub fn generate(
    out_dir: &Path,
    sorted_keys: &[String],
    _options: &Value,
    fallback: &str,
    output_dir: &str,
    key_env: &str,
) -> Result<(), anyhow::Error> {
    let mut key_definitions = String::new();
    for k in sorted_keys {
        key_definitions.push_str(&format!("  | \"{}\"\n", k));
    }
    if key_definitions.is_empty() {
        key_definitions = "  | string".to_string();
    } else {
        key_definitions = key_definitions.trim_end().to_string();
    }

    let i18n_content = TS_TEMPLATE
        .replace("{{KEY_DEFINITIONS}}", &key_definitions)
        .replace("{{FALLBACK_LOCALE}}", fallback)
        .replace("{{OUTPUT_DIR}}", output_dir)
        .replace("{{KEY_ENV}}", key_env);

    let file_path = out_dir.join("generated.ts");
    fs::write(&file_path, i18n_content)?;
    println!("Generated TypeScript bindings at '{}'", file_path.display());

    Ok(())
}
