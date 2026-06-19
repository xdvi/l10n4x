use serde_json::Value;
use std::fs;
use std::path::Path;

const DART_TEMPLATE: &str = include_str!("../templates/dart_i18n.dart");

pub fn generate(
    out_dir: &Path,
    sorted_keys: &[String],
    _options: &Value,
    fallback: &str,
    output_dir: &str,
    key_env: &str,
    to_lower_camel_case: fn(&str) -> String,
) -> Result<(), anyhow::Error> {
    let mut dart_definitions = String::new();
    let mut dart_helpers = String::new();
    for k in sorted_keys {
        let key_var = to_lower_camel_case(k);
        dart_definitions.push_str(&format!("  static const String {} = '{}';\n", key_var, k));
        dart_helpers.push_str(&format!(
            "  String {}({{Map<String, String>? args}}) => t(L10nKeys.{}, args: args);\n",
            key_var, key_var
        ));
    }

    let i18n_content = DART_TEMPLATE
        .replace("{{KEY_DEFINITIONS}}", dart_definitions.trim_end())
        .replace("{{FALLBACK_LOCALE}}", fallback)
        .replace("{{OUTPUT_DIR}}", output_dir)
        .replace("{{KEY_ENV}}", key_env)
        .replace("{{HELPERS}}", &dart_helpers);

    let file_path = out_dir.join("i18n_keys.dart");
    fs::write(&file_path, i18n_content)?;
    println!(
        "Generated Flutter/Dart bindings at '{}'",
        file_path.display()
    );

    Ok(())
}
