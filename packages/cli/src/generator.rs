use crate::config::Target;
use crate::targets;
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
    if pascal.is_empty() {
        return pascal;
    }
    let mut chars = pascal.chars();
    let first = chars.next().unwrap().to_ascii_lowercase();
    format!("{}{}", first, chars.collect::<String>())
}

pub fn generate_bindings(
    targets: &[Target],
    keys: &HashSet<String>,
    fallback: &str,
    output_dir: &str,
    key_env: &str,
) -> Result<(), anyhow::Error> {
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
                    fallback,
                    output_dir,
                    key_env,
                    to_pascal_case,
                )?;
            }
            "typescript" => {
                targets::typescript::generate(
                    out_dir,
                    &sorted_keys,
                    &target.options,
                    fallback,
                    output_dir,
                    key_env,
                )?;
            }
            "flutter" => {
                targets::flutter::generate(
                    out_dir,
                    &sorted_keys,
                    &target.options,
                    fallback,
                    output_dir,
                    key_env,
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
