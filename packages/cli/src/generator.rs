use crate::config::Target;
use crate::targets::{self, GenerateContext};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '.' || c == '_' || c == '-' {
            capitalize_next = true;
        } else if c.is_alphanumeric() {
            if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
    }
    result
}

pub fn to_upper_snake_case(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c == '.' || c == '-' {
            result.push('_');
        } else if c.is_alphanumeric() {
            result.push(c.to_ascii_uppercase());
        }
    }
    result
}

pub fn to_lower_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    let first = match chars.next() {
        Some(c) => c.to_ascii_lowercase(),
        None => return String::new(),
    };
    format!("{}{}", first, chars.collect::<String>())
}

#[allow(clippy::too_many_arguments)]
pub fn generate_bindings(
    targets: &[Target],
    _keys: &HashSet<String>,
    fallback: &str,
    source_dir: &str,
    output_dir: &str,
    verify_public_key_hex: &str,
    encrypt: bool,
    encrypt_key_env: &str,
) -> Result<(), anyhow::Error> {
    let verify_key_bytes = crate::config::format_verify_key_bytes(verify_public_key_hex)?;
    let ctx = GenerateContext {
        fallback,
        output_dir,
        source_dir,
        verify_key_bytes: &verify_key_bytes,
        verify_public_key_hex,
        encrypt,
        encrypt_key_env,
    };
    let key_pairs = l10n4x_compiler::compile_key_pairs(Path::new(source_dir))?;

    for target in targets {
        let out_dir = Path::new(&target.out_dir);
        fs::create_dir_all(out_dir)?;

        match target.r#type.as_str() {
            "go" => {
                targets::go::generate(
                    out_dir,
                    &key_pairs,
                    &target.options,
                    &ctx,
                    to_pascal_case,
                )?;
            }
            "typescript" => {
                let params_map = l10n4x_compiler::extract_params_map(
                    std::path::Path::new(ctx.source_dir)
                ).unwrap_or_default();
                targets::typescript::generate(out_dir, &key_pairs, &target.options, &ctx, &params_map)?;
            }
            "flutter" => {
                targets::flutter::generate(
                    out_dir,
                    &key_pairs,
                    &target.options,
                    &ctx,
                    to_lower_camel_case,
                )?;
            }
            "c" => {
                targets::c::generate(out_dir, &key_pairs, &target.options, to_upper_snake_case)?;
            }
            "python" => {
                targets::python::generate(
                    out_dir,
                    &key_pairs,
                    &target.options,
                    to_upper_snake_case,
                )?;
            }
            "vue" => {
                let params_map = l10n4x_compiler::extract_params_map(
                    std::path::Path::new(ctx.source_dir)
                ).unwrap_or_default();
                targets::vue::generate(out_dir, &key_pairs, &target.options, &ctx, &params_map)?;
            }
            "svelte" => {
                let params_map = l10n4x_compiler::extract_params_map(
                    std::path::Path::new(ctx.source_dir)
                ).unwrap_or_default();
                targets::svelte::generate(out_dir, &key_pairs, &target.options, &ctx, &params_map)?;
            }
            "angular" => {
                let params_map = l10n4x_compiler::extract_params_map(
                    std::path::Path::new(ctx.source_dir)
                ).unwrap_or_default();
                targets::angular::generate(out_dir, &key_pairs, &target.options, &ctx, &params_map)?;
            }
            other => {
                println!("Warning: Unknown target type '{}' ignored.", other);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Target;

    #[test]
    fn to_pascal_case_dot_separated() {
        assert_eq!(to_pascal_case("common.welcome"), "CommonWelcome");
    }

    #[test]
    fn to_pascal_case_already_pascal() {
        assert_eq!(to_pascal_case("HelloWorld"), "HelloWorld");
    }

    #[test]
    fn to_upper_snake_case_dot_separated() {
        assert_eq!(to_upper_snake_case("common.welcome"), "COMMON_WELCOME");
    }

    #[test]
    fn to_lower_camel_case_dot_separated() {
        assert_eq!(to_lower_camel_case("common.welcome"), "commonWelcome");
    }

    #[test]
    fn to_lower_camel_case_single() {
        assert_eq!(to_lower_camel_case("hello"), "hello");
    }

    #[test]
    fn to_pascal_case_with_underscore() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
    }

    #[test]
    fn to_pascal_case_with_dash() {
        assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
    }

    #[test]
    fn to_pascal_case_empty() {
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn to_upper_snake_case_with_dash() {
        assert_eq!(to_upper_snake_case("hello-world"), "HELLO_WORLD");
    }

    #[test]
    fn to_upper_snake_case_simple() {
        assert_eq!(to_upper_snake_case("hello"), "HELLO");
    }

    #[test]
    fn to_lower_camel_case_empty() {
        assert_eq!(to_lower_camel_case(""), "");
    }

    #[test]
    fn generate_bindings_unknown_target_warns() {
        let targets = vec![Target {
            r#type: "nonexistent".to_string(),
            out_dir: "/tmp".to_string(),
            options: serde_json::json!({}),
        }];
        let keys: HashSet<String> = ["a.b".to_string()].into_iter().collect();
        let result = generate_bindings(&targets, &keys, "en", ".", "/tmp", "0000000000000000000000000000000000000000000000000000000000000000", false, "");
        assert!(result.is_ok());
    }

    #[test]
    fn generate_bindings_known_targets() {
        let temp = tempfile::tempdir().unwrap();
        let targets = vec![
            Target {
                r#type: "go".to_string(),
                out_dir: temp.path().join("go").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "typescript".to_string(),
                out_dir: temp.path().join("ts").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "flutter".to_string(),
                out_dir: temp.path().join("flutter").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "c".to_string(),
                out_dir: temp.path().join("c").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "python".to_string(),
                out_dir: temp.path().join("py").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "vue".to_string(),
                out_dir: temp.path().join("vue").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "svelte".to_string(),
                out_dir: temp.path().join("svelte").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
            Target {
                r#type: "angular".to_string(),
                out_dir: temp.path().join("angular").to_str().unwrap().to_string(),
                options: serde_json::json!({}),
            },
        ];
        let keys: HashSet<String> = ["a.b".to_string()].into_iter().collect();
        let result = generate_bindings(
            &targets,
            &keys,
            "en",
            ".",
            temp.path().to_str().unwrap(),
            "0000000000000000000000000000000000000000000000000000000000000000",
            false,
            "",
        );
        assert!(result.is_ok());
    }
}
