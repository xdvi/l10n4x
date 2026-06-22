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

fn react_block() -> &'static str {
    r#"
// ── React integration ─────────────────────────────────────────────────────────
// Only included when options.react = true in l10n4x.config.json target entry.
import { useState, useEffect, useCallback } from "react";

interface UseTranslationResult {
  t: (key: LocaleKey, params?: Record<string, string | number>) => string;
  locale: string;
  setLocale: (locale: string) => void;
  isLoading: boolean;
}

/**
 * React hook for l10n4x translations.
 *
 * @example
 * const { t, setLocale } = useTranslation("en");
 * return <h1>{t("welcome.title")}</h1>;
 */
export function useTranslation(initialLocale = _FALLBACK_LOCALE): UseTranslationResult {
  const [locale, setLocaleState] = useState(initialLocale);
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    setIsLoading(true);
    loadLocale(locale)
      .then(() => setIsLoading(false))
      .catch(() => setIsLoading(false));
  }, [locale]);

  const setLocale = useCallback((next: string) => {
    setLocaleState(next);
  }, []);

  const tFn = useCallback(
    (key: LocaleKey, params?: Record<string, string | number>) =>
      t(locale, key as LocaleKey, params),
    [locale]
  );

  return { t: tFn, locale, setLocale, isLoading };
}
"#
}

pub fn generate(
    out_dir: &Path,
    key_pairs: &[(u64, String)],
    options: &Value,
    ctx: &GenerateContext<'_>,
    params_map: &std::collections::HashMap<String, Vec<String>>,
) -> Result<(), anyhow::Error> {
    let mut key_definitions = String::new();
    for (hash, name) in key_pairs {
        let pascal_name = crate::generator::to_pascal_case(name);
        key_definitions.push_str(&format!(
            "export const {} = 0x{:016x} as const;\n",
            pascal_name, hash
        ));
    }

    // Build typed t() overloads for keys that have parameters
    let mut typed_overloads = String::new();
    for (_, name) in key_pairs {
        if let Some(param_names) = params_map.get(name) {
            if !param_names.is_empty() {
                let params_type: String = param_names
                    .iter()
                    .map(|p| format!("  {}: string | number", p))
                    .collect::<Vec<_>>()
                    .join(";\n");
                typed_overloads.push_str(&format!(
                    "export function t(locale: string, key: number, params: {{\n{params}\n}}): string;\n",
                    params = params_type
                ));
            }
        }
    }

    let react = options
        .get("react")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let react_block_content = if react { react_block() } else { "" };

    let i18n_content = TS_TEMPLATE
        .replace("{{KEY_DEFINITIONS}}", &key_definitions)
        .replace("{{FALLBACK_LOCALE}}", ctx.fallback)
        .replace("{{OUTPUT_DIR}}", ctx.output_dir)
        .replace("{{VERIFY_PUBLIC_KEY}}", ctx.verify_public_key_hex)
        .replace("{{ENCRYPT_BLOCK}}", &ts_encrypt_block(ctx))
        .replace("{{DECRYPT_KEY_IMPORT}}", ts_decrypt_key_import(ctx.encrypt))
        .replace("{{DECRYPT_KEY_INIT}}", &ts_decrypt_key_init(ctx.encrypt))
        .replace("{{REACT_BLOCK}}", react_block_content);

    let i18n_content = if !typed_overloads.is_empty() {
        i18n_content.replace(
            "export function t(",
            &format!("{}\nexport function t(", typed_overloads.trim_end()),
        )
    } else {
        i18n_content
    };

    let file_path = out_dir.join("generated.ts");
    fs::write(&file_path, i18n_content)?;
    println!("Generated TypeScript bindings at '{}'", file_path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::GenerateContext;
    use std::collections::HashMap;

    fn test_ctx() -> GenerateContext<'static> {
        GenerateContext {
            fallback: "en",
            output_dir: "",
            source_dir: "",
            verify_key_bytes: "000000000000000000000000000000000000000000000000000000000000000000",
            verify_public_key_hex: "abcd",
            encrypt: false,
            encrypt_key_env: "",
        }
    }

    fn test_ctx_encrypt() -> GenerateContext<'static> {
        GenerateContext {
            fallback: "en",
            output_dir: "",
            source_dir: "",
            verify_key_bytes: "000000000000000000000000000000000000000000000000000000000000000000",
            verify_public_key_hex: "abcd",
            encrypt: true,
            encrypt_key_env: "MY_KEY",
        }
    }

    #[test]
    fn ts_encrypt_block_when_disabled() {
        let ctx = test_ctx();
        assert_eq!(ts_encrypt_block(&ctx), "");
    }

    #[test]
    fn ts_encrypt_block_when_enabled() {
        let ctx = test_ctx_encrypt();
        let block = ts_encrypt_block(&ctx);
        assert!(block.contains("ENCRYPT_ENABLED = true"));
        assert!(block.contains("MY_KEY"));
    }

    #[test]
    fn ts_decrypt_key_import_disabled() {
        assert_eq!(ts_decrypt_key_import(false), "");
    }

    #[test]
    fn ts_decrypt_key_import_enabled() {
        assert!(ts_decrypt_key_import(true).contains("l10n4x_set_decrypt_key"));
    }

    #[test]
    fn ts_decrypt_key_init_disabled() {
        assert_eq!(ts_decrypt_key_init(false), "");
    }

    #[test]
    fn ts_decrypt_key_init_enabled() {
        assert!(ts_decrypt_key_init(true).contains("l10n4x_set_decrypt_key"));
    }

    #[test]
    fn react_block_content() {
        assert!(react_block().contains("useTranslation"));
    }

    #[test]
    fn generates_type_definitions() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "welcome.title".to_string())];
        let params = HashMap::new();
        generate(
            dir.path(),
            &key_pairs,
            &serde_json::Value::Null,
            &test_ctx(),
            &params,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("export const WelcomeTitle = 0xabcdef0123456789 as const;"));
        assert!(content.contains("export function t("));
    }

    #[test]
    fn key_definitions_are_typed() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![
            (0xabcdef0123456789, "a.b".to_string()),
            (0x123456789abcdef0, "c.d".to_string()),
        ];
        let params = HashMap::new();
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("export const AB = 0xabcdef0123456789 as const;"));
        assert!(content.contains("export const CD = 0x123456789abcdef0 as const;"));
    }

    #[test]
    fn generates_with_params() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "greeting".to_string())];
        let mut params = HashMap::new();
        params.insert("greeting".to_string(), vec!["name".to_string()]);
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("name: string | number"));
    }

    #[test]
    fn generates_with_react() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "key".to_string())];
        let params = HashMap::new();
        let opts = serde_json::json!({"react": true});
        generate(dir.path(), &key_pairs, &opts, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("useTranslation"));
    }

    #[test]
    fn generates_with_encrypt() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "key".to_string())];
        let params = HashMap::new();
        generate(
            dir.path(),
            &key_pairs,
            &Value::Null,
            &test_ctx_encrypt(),
            &params,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("ENCRYPT_ENABLED"));
    }

    #[test]
    fn generates_empty_keys_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![];
        let params = HashMap::new();
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), &params).unwrap();
        let content = std::fs::read_to_string(dir.path().join("generated.ts")).unwrap();
        assert!(content.contains("number"));
    }
}
