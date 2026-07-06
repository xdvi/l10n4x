//! Regression tests for MF2 compile-time validation gaps.

use l10n4x_compiler::icu_parser::MessageParser;
use l10n4x_compiler::{compile_with_options, BundleMode, CompileOptions};
use std::fs;
use tempfile::TempDir;

fn assert_parse_rejects(src: &str) {
    let result = MessageParser::new(src).parse();
    assert!(
        result.is_err(),
        "expected parse error for {:?}, got {:?}",
        src,
        result
    );
}

#[test]
fn rejects_null_byte_inside_placeholder() {
    assert_parse_rejects("bad {\u{0000}placeholder}");
}

#[test]
fn rejects_option_name_empty_before_equals() {
    assert_parse_rejects("bad {:placeholder option:=x}");
}

#[test]
fn rejects_option_name_leading_colon() {
    assert_parse_rejects("bad {:placeholder :option=x}");
}

#[test]
fn rejects_option_namespace_double_colon() {
    assert_parse_rejects("bad {:placeholder option::x=y}");
}

#[test]
fn rejects_trailing_content_after_match() {
    assert_parse_rejects(".input {$x :x} .match $x * {{foo}} extra");
}

#[test]
fn compile_error_includes_locale_and_key() {
    let tmp = TempDir::new().unwrap();
    let en = tmp.path().join("en");
    fs::create_dir_all(&en).unwrap();
    // Single-string JSON file → flattened key is the file stem ("hello").
    fs::write(en.join("hello.json"), r#""{unclosed""#).unwrap();

    let out = tmp.path().join("out.lpk");
    let err = compile_with_options(tmp.path(), &out, CompileOptions::default()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Locale 'en'"), "{msg}");
    assert!(msg.contains("key 'hello'"), "{msg}");
}

#[test]
fn compile_rejects_forward_local_reference() {
    let tmp = TempDir::new().unwrap();
    let en = tmp.path().join("en");
    fs::create_dir_all(&en).unwrap();
    fs::write(
        en.join("bad.json"),
        r#"{"msg": ".local $foo = {$bar} .local $bar = {42} {{_}}"}"#,
    )
    .unwrap();

    let out = tmp.path().join("out.lpk");
    let err = compile_with_options(
        tmp.path(),
        &out,
        CompileOptions {
            bundle_mode: BundleMode::Monolith,
            ..Default::default()
        },
    )
    .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("msg"), "{msg}");
}
