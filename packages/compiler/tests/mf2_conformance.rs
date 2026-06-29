//! ICU MessageFormat 2.0 syntax conformance tests.
//! Fixtures from https://github.com/unicode-org/message-format-wg/tree/main/test/tests

use l10n4x_compiler::binary_writer::serialize_message;
use l10n4x_compiler::icu_parser::MessageParser;
use l10n4x_compiler::mf2_parser::validate_data_model;
use l10n4x_core::formatter::format_message;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mf2")
}

fn load_tests(filename: &str) -> Vec<Value> {
    let path = fixtures_dir().join(filename);
    let raw = fs::read_to_string(path).expect("fixture file");
    let doc: Value = serde_json::from_str(&raw).expect("valid JSON fixture");
    doc["tests"].as_array().expect("tests array").clone()
}

#[test]
fn mf2_syntax_fixtures_parse_successfully() {
    let tests = load_tests("syntax.json");
    let mut passed = 0usize;
    let mut failed = Vec::new();

    for case in &tests {
        let src = case["src"].as_str().unwrap_or("");
        let result = MessageParser::new(src).parse();
        if result.is_ok() {
            passed += 1;
        } else {
            let desc = case["description"].as_str().unwrap_or(src).to_string();
            failed.push((desc, result.unwrap_err()));
        }
    }

    assert!(
        failed.is_empty(),
        "MF2 syntax conformance: {}/{} passed. Failures:\n{}",
        passed,
        tests.len(),
        failed
            .iter()
            .take(10)
            .map(|(d, e)| format!("  - {d}: {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn mf2_syntax_error_fixtures_are_rejected() {
    let tests = load_tests("syntax-errors.json");
    let mut passed = 0usize;
    let mut failed = Vec::new();

    for case in &tests {
        let src = case["src"].as_str().unwrap_or("");
        let result = MessageParser::new(src).parse();
        if result.is_err() {
            passed += 1;
        } else {
            let desc = case["description"].as_str().unwrap_or(src).to_string();
            failed.push(desc);
        }
    }

    assert!(
        failed.is_empty(),
        "MF2 syntax-error conformance: {}/{} rejected. Accepted invalid:\n{}",
        passed,
        tests.len(),
        failed
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn format_mf2_fixture(src: &str, params: &[(&str, &str)]) -> Result<String, String> {
    let nodes = MessageParser::new(src).parse().map_err(|e| e.to_string())?;
    let bytecode = serialize_message(&nodes);
    let mut out = String::new();
    format_message(&bytecode, "und", params, &mut out).map_err(|_| "format error".to_string())?;
    Ok(out)
}

#[test]
fn mf2_pattern_selection_fixtures_format_correctly() {
    let tests = load_tests("pattern-selection.json");
    let mut passed = 0usize;
    let mut failed = Vec::new();

    for case in &tests {
        let src = case["src"].as_str().unwrap_or("");
        let exp = case["exp"].as_str().unwrap_or("");
        let mut param_pairs: Vec<(String, String)> = Vec::new();
        if let Some(arr) = case["params"].as_array() {
            for p in arr {
                let name = p["name"].as_str().unwrap_or("").to_string();
                let value = match &p["value"] {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                param_pairs.push((name, value));
            }
        }
        let param_refs: Vec<(&str, &str)> = param_pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        match format_mf2_fixture(src, &param_refs) {
            Ok(got) if got == exp => passed += 1,
            Ok(got) => {
                let desc = case["description"].as_str().unwrap_or(src).to_string();
                failed.push(format!("  - {desc}: expected {exp:?}, got {got:?}"));
            }
            Err(e) => {
                let desc = case["description"].as_str().unwrap_or(src).to_string();
                failed.push(format!("  - {desc}: {e}"));
            }
        }
    }

    assert!(
        failed.is_empty(),
        "MF2 pattern-selection conformance: {}/{} passed. Failures:\n{}",
        passed,
        tests.len(),
        failed.join("\n")
    );
}

#[test]
fn mf2_data_model_error_fixtures_are_rejected() {
    let tests = load_tests("data-model-errors.json");
    let mut passed = 0usize;
    let mut failed = Vec::new();

    for case in &tests {
        let src = case["src"].as_str().unwrap_or("");
        if let Some(exp) = case.get("exp").and_then(|v| v.as_str()) {
            let nodes = match MessageParser::new(src).parse() {
                Ok(n) => n,
                Err(e) => {
                    failed.push(format!("valid case should parse: {src}: {e}"));
                    continue;
                }
            };
            if validate_data_model(&nodes).is_err() {
                failed.push(format!("valid case failed data-model check: {src}"));
                continue;
            }
            match format_mf2_fixture(src, &[]) {
                Ok(got) if got == exp => passed += 1,
                Ok(got) => failed.push(format!("expected {exp:?}, got {got:?} for {src}")),
                Err(e) => failed.push(format!("format error for valid case {src}: {e}")),
            }
            continue;
        }

        let parse_err = MessageParser::new(src).parse().is_err();
        let model_err = MessageParser::new(src)
            .parse()
            .ok()
            .and_then(|nodes| validate_data_model(&nodes).err());
        if parse_err || model_err.is_some() {
            passed += 1;
        } else {
            let desc = case["description"].as_str().unwrap_or(src).to_string();
            failed.push(desc);
        }
    }

    assert!(
        failed.is_empty(),
        "MF2 data-model-error conformance: {}/{} rejected. Accepted invalid:\n{}",
        passed,
        tests.len(),
        failed
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
