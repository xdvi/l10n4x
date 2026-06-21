use crate::targets::GenerateContext;
use serde_json::Value;
use std::fs;
use std::path::Path;

const TS_TEMPLATE: &str = include_str!("../templates/ts_generated.ts");

fn ts_encrypt_block(ctx: &GenerateContext<'_>) -> String {
    if !ctx.encrypt {
        return String::new();
    }
    format!(
        r#"
const ENCRYPT_ENABLED = true;
const ENCRYPT_KEY_ENV = "{env}";

function loadDecryptKey(options?: {{ decryptKey?: Uint8Array }}): Uint8Array {{
  if (options?.decryptKey && options.decryptKey.length === 32) {{
    return options.decryptKey;
  }}
  if (typeof process !== "undefined" && process.env?.[ENCRYPT_KEY_ENV]) {{
    const raw = process.env[ENCRYPT_KEY_ENV]!;
    if (raw.length !== 32) {{
      throw new Error(`l10n4x: ${{ENCRYPT_KEY_ENV}} must be exactly 32 bytes`);
    }}
    return new TextEncoder().encode(raw);
  }}
  throw new Error("l10n4x: decrypt key required (pass options.decryptKey or set env)");
}}
"#,
        env = ctx.encrypt_key_env
    )
}

fn ts_decrypt_key_import(encrypt: bool) -> &'static str {
    if encrypt {
        "  l10n4x_set_decrypt_key,\n"
    } else {
        ""
    }
}

fn ts_decrypt_key_init(encrypt: bool) -> String {
    if !encrypt {
        return String::new();
    }
    "  l10n4x_set_decrypt_key(loadDecryptKey(options));\n".to_string()
}

pub fn generate(
    out_dir: &Path,
    sorted_keys: &[String],
    _options: &Value,
    ctx: &GenerateContext<'_>,
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
        .replace("{{FALLBACK_LOCALE}}", ctx.fallback)
        .replace("{{OUTPUT_DIR}}", ctx.output_dir)
        .replace("{{VERIFY_PUBLIC_KEY}}", ctx.verify_public_key_hex)
        .replace("{{ENCRYPT_BLOCK}}", &ts_encrypt_block(ctx))
        .replace("{{DECRYPT_KEY_IMPORT}}", ts_decrypt_key_import(ctx.encrypt))
        .replace("{{DECRYPT_KEY_INIT}}", &ts_decrypt_key_init(ctx.encrypt));

    let file_path = out_dir.join("generated.ts");
    fs::write(&file_path, i18n_content)?;
    println!("Generated TypeScript bindings at '{}'", file_path.display());

    Ok(())
}
