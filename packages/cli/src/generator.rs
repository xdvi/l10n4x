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

pub fn generate_bindings(
    targets: &[Target],
    keys: &HashSet<String>,
    fallback: &str,
    output_dir: &str,
    verify_public_key_hex: &str,
    encrypt: bool,
    encrypt_key_env: &str,
) -> Result<(), anyhow::Error> {
    let verify_key_bytes = crate::config::format_verify_key_bytes(verify_public_key_hex)?;
    let ctx = GenerateContext {
        fallback,
        output_dir,
        verify_key_bytes: &verify_key_bytes,
        verify_public_key_hex,
        encrypt,
        encrypt_key_env,
    };
    let mut sorted_keys: Vec<String> = keys.iter().cloned().collect();
    sorted_keys.sort();

    for target in targets {
        let out_dir = Path::new(&target.out_dir);
        fs::create_dir_all(out_dir)?;

        match target.r#type.as_str() {
            "go" => {
                targets::go::generate(
                    out_dir,
                    &sorted_keys,
                    &target.options,
                    &ctx,
                    to_pascal_case,
                )?;
            }
            "typescript" => {
                targets::typescript::generate(out_dir, &sorted_keys, &target.options, &ctx)?;
            }
            "flutter" => {
                targets::flutter::generate(
                    out_dir,
                    &sorted_keys,
                    &target.options,
                    &ctx,
                    to_lower_camel_case,
                )?;
            }
            "c" => {
                targets::c::generate(out_dir, &sorted_keys, &target.options, to_upper_snake_case)?;
            }
            "python" => {
                targets::python::generate(
                    out_dir,
                    &sorted_keys,
                    &target.options,
                    to_upper_snake_case,
                )?;
            }
            other => {
                println!("Warning: Unknown target type '{}' ignored.", other);
            }
        }
    }
    Ok(())
}
