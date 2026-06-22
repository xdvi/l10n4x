use crate::targets::GenerateContext;
use serde_json::Value;
use std::fs;
use std::path::Path;

const DART_TEMPLATE: &str = include_str!("../templates/dart_i18n.dart");

fn dart_decrypt_key_init(ctx: &GenerateContext<'_>) -> String {
    if !ctx.encrypt {
        return String::new();
    }
    format!(
        r#"
    final encRaw = Platform.environment['{env}'];
    if (encRaw == null || encRaw.length != 32) {{
      throw StateError('l10n4c: encryptKeyEnv not set or wrong length');
    }}
    final encPtr = calloc<ffi.Uint8>(32);
    for (var i = 0; i < 32; i++) {{
      encPtr[i] = encRaw.codeUnitAt(i);
    }}
    if (_setDecryptKey(encPtr, 32) != 0) {{
      calloc.free(encPtr);
      throw StateError('l10n4c: invalid decrypt key');
    }}
    calloc.free(encPtr);
"#,
        env = ctx.encrypt_key_env
    )
}

fn dart_decrypt_key_typedefs(encrypt: bool) -> String {
    if !encrypt {
        return String::new();
    }
    r#"
typedef l10n4c_set_decrypt_key_func = ffi.Int32 Function(ffi.Pointer<ffi.Uint8> key, int key_len);
typedef L10n4cSetDecryptKey = int Function(ffi.Pointer<ffi.Uint8> key, int key_len);
"#
    .to_string()
}

fn dart_decrypt_key_fields(encrypt: bool) -> String {
    if !encrypt {
        return String::new();
    }
    "  static late final L10n4cSetDecryptKey _setDecryptKey;\n".to_string()
}

fn dart_decrypt_key_lookup(encrypt: bool) -> String {
    if !encrypt {
        return String::new();
    }
    "    _setDecryptKey = _lib.lookup<ffi.NativeFunction<l10n4c_set_decrypt_key_func>>('l10n4c_set_decrypt_key').asFunction();\n".to_string()
}

pub fn generate(
    out_dir: &Path,
    key_pairs: &[(u64, String)],
    _options: &Value,
    ctx: &GenerateContext<'_>,
    to_lower_camel_case: fn(&str) -> String,
) -> Result<(), anyhow::Error> {
    let mut dart_definitions = String::new();
    let mut dart_helpers = String::new();
    for (hash, name) in key_pairs {
        let key_var = to_lower_camel_case(name);
        dart_definitions.push_str(&format!(
            "  static const int {} = 0x{:016x};\n",
            key_var, hash
        ));
        dart_helpers.push_str(&format!(
            "  String {}({{Map<String, String>? args}}) => t(L10nKeys.{}, args: args);\n",
            key_var, key_var
        ));
    }

    let i18n_content = DART_TEMPLATE
        .replace("{{KEY_DEFINITIONS}}", dart_definitions.trim_end())
        .replace("{{FALLBACK_LOCALE}}", ctx.fallback)
        .replace("{{OUTPUT_DIR}}", ctx.output_dir)
        .replace("{{VERIFY_KEY_BYTES}}", ctx.verify_key_bytes)
        .replace(
            "{{DECRYPT_KEY_TYPEDEFS}}",
            &dart_decrypt_key_typedefs(ctx.encrypt),
        )
        .replace(
            "{{DECRYPT_KEY_FIELDS}}",
            &dart_decrypt_key_fields(ctx.encrypt),
        )
        .replace(
            "{{DECRYPT_KEY_LOOKUP}}",
            &dart_decrypt_key_lookup(ctx.encrypt),
        )
        .replace("{{DECRYPT_KEY_INIT}}", &dart_decrypt_key_init(ctx))
        .replace("{{HELPERS}}", &dart_helpers);

    let file_path = out_dir.join("i18n_keys.dart");
    fs::write(&file_path, i18n_content)?;
    println!(
        "Generated Flutter/Dart bindings at '{}'",
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
    fn generates_key_definitions() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![
            (0xabcdef0123456789, "common.welcome".to_string()),
            (0x123456789abcdef0, "user.name".to_string()),
        ];
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), |s| {
            let pascal: String = s
                .split('.')
                .map(|part| {
                    let mut c = part.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                })
                .collect();
            let mut chars = pascal.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_ascii_lowercase().to_string() + chars.as_str(),
            }
        })
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("i18n_keys.dart")).unwrap();
        assert!(content.contains("static const int"));
        assert!(content.contains("commonWelcome"));
        assert!(content.contains("userName"));
    }

    #[test]
    fn generates_helper_methods() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "greeting".to_string())];
        generate(dir.path(), &key_pairs, &Value::Null, &test_ctx(), |s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_ascii_lowercase().to_string() + chars.as_str(),
            }
        })
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("i18n_keys.dart")).unwrap();
        assert!(content.contains("L10nKeys"));
    }

    #[test]
    fn generates_bindings_with_encryption() {
        let dir = tempfile::tempdir().unwrap();
        let key_pairs: Vec<(u64, String)> = vec![(0xabcdef0123456789, "greeting".to_string())];
        let mut ctx = test_ctx();
        ctx.encrypt = true;
        ctx.encrypt_key_env = "TEST_ENCRYPT_KEY_ENV";
        generate(dir.path(), &key_pairs, &Value::Null, &ctx, |s| {
            s.to_string()
        })
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("i18n_keys.dart")).unwrap();
        assert!(content.contains("l10n4c_set_decrypt_key"));
        assert!(content.contains("Platform.environment['TEST_ENCRYPT_KEY_ENV']"));
    }
}
