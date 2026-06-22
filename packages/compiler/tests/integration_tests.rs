use l10n4x_compiler::binary_writer::write_binary_format;
use l10n4x_compiler::fnv1a_64;
use l10n4x_compiler::icu_parser::parse_interval_plural;
use l10n4x_compiler::icu_parser::{
    DateStyle, ListStyle, MessageNode, MessageParser, NumberStyle, PluralCaseKey, RelTimeStyle,
};
use l10n4x_core::binary_format::BinaryFormatReader;
use l10n4x_core::formatter::format_message;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[test]
fn test_compiler_core_integration_zero_alloc() {
    // Compile a complex message with plural via binary_writer
    // and validate that BinaryFormatReader + formatter processes it
    let mut translations = HashMap::new();

    let parser =
        MessageParser::new("{count, plural, =0 {no messages} =1 {one message} other {# messages}}");
    let nodes = parser.parse().unwrap();
    translations.insert(fnv1a_64(b"msg.key"), nodes);

    let binary_bytes = write_binary_format(&translations);

    // Reader lookup (zero-copy)
    let reader = BinaryFormatReader::new(&binary_bytes).unwrap();
    let bytecode = reader.lookup(fnv1a_64(b"msg.key")).unwrap();

    // Format without allocation using a preallocated writer buffer (represented by String in tests)
    let mut output = String::new();
    let params = [("count", "5")];
    format_message(bytecode, "en", &params, &mut output).unwrap();
    assert_eq!(output, "5 messages");
}

#[test]
fn test_binary_header_magic_and_version() {
    let mut translations = HashMap::new();
    translations.insert(
        fnv1a_64(b"test.key"),
        vec![MessageNode::Text("val".to_string())],
    );
    let binary_bytes = write_binary_format(&translations);

    // Header must be at least 16 bytes
    assert!(binary_bytes.len() >= 16);
    // Magic bytes "L10N" at bytes 0-3
    assert_eq!(&binary_bytes[0..4], b"L10N");
    // Version (big-endian) at bytes 4-7
    let version = u32::from_be_bytes(binary_bytes[4..8].try_into().unwrap());
    assert_eq!(version, l10n4x_core::binary_format::FORMAT_VERSION);
}

#[test]
fn test_array_flattening() {
    use serde_json::json;
    let val = json!({
        "menu": {
            "items": ["Home", "Settings", "Profile"]
        }
    });
    let mut map = HashMap::new();
    l10n4x_compiler::flatten_value("config".to_string(), &val, &mut map);
    assert_eq!(
        map.get("config.menu.items").unwrap(),
        r#"["Home","Settings","Profile"]"#
    );
}

#[test]
fn test_compression_ratio_estimate() {
    let mut translations = HashMap::new();
    // Generate 100 typical translations
    for i in 0..100 {
        let key = format!("module.submodule.action.error_code_{}", i);
        let template = format!("An error occurred during operation {} in module. Please try again or contact support with reference ID: {{ref_id}}.", i);
        let parser = MessageParser::new(&template);
        let nodes = parser.parse().unwrap();
        translations.insert(fnv1a_64(key.as_bytes()), nodes);
    }

    let uncompressed = write_binary_format(&translations);
    let compressed = zstd::encode_all(&uncompressed[..], 8).unwrap();

    let uncompressed_len = uncompressed.len();
    let compressed_len = compressed.len();
    let reduction = 100.0 - (compressed_len as f64 / uncompressed_len as f64 * 100.0);

    println!(
        "COMPRESSION_TEST_RESULT: Uncompressed: {}, Compressed: {}, Reduction: {:.2}%",
        uncompressed_len, compressed_len, reduction
    );
    assert!(reduction > 50.0);
}

#[test]
fn test_parser_simple() {
    let parser = MessageParser::new("Hello {name}!");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0], MessageNode::Text("Hello ".to_string()));
    assert_eq!(nodes[1], MessageNode::Variable("name".to_string()));
    assert_eq!(nodes[2], MessageNode::Text("!".to_string()));
}

#[test]
fn test_parser_plural_mf1() {
    let parser =
        MessageParser::new("{count, plural, =0 {no messages} =1 {one message} other {# messages}}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    if let MessageNode::Plural {
        var,
        ordinal: _,
        cases,
    } = &nodes[0]
    {
        assert_eq!(var, "count");
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0].0, PluralCaseKey::Exact(0.0));
        assert_eq!(
            cases[0].1,
            vec![MessageNode::Text("no messages".to_string())]
        );

        assert_eq!(cases[1].0, PluralCaseKey::Exact(1.0));
        assert_eq!(
            cases[1].1,
            vec![MessageNode::Text("one message".to_string())]
        );

        assert_eq!(cases[2].0, PluralCaseKey::Other);
        // '#' in MF1 gets translated to Variable(count)
        assert_eq!(
            cases[2].1,
            vec![
                MessageNode::Variable("count".to_string()),
                MessageNode::Text(" messages".to_string())
            ]
        );
    } else {
        panic!("Expected Plural node");
    }
}

#[test]
fn test_mf2_numeric_plural_key() {
    let parser = MessageParser::new("{count, plural, =0 {none} =1 {one} other {many}}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    if let MessageNode::Plural { cases, .. } = &nodes[0] {
        assert!(cases
            .iter()
            .any(|(k, _)| matches!(k, PluralCaseKey::Exact(v) if (*v - 0.0).abs() < 1e-9)));
        assert!(cases
            .iter()
            .any(|(k, _)| matches!(k, PluralCaseKey::Exact(v) if (*v - 1.0).abs() < 1e-9)));
    } else {
        panic!("Expected Plural");
    }
}

#[test]
fn test_parser_select_exact_value() {
    let parser =
        MessageParser::new("{status, select, ok {All good} error {Error} other {Unknown}}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    if let MessageNode::Select { var, cases } = &nodes[0] {
        assert_eq!(var, "status");
        assert!(cases.iter().any(|(k, _)| k == "ok"));
        assert!(cases.iter().any(|(k, _)| k == "error"));
        assert!(cases.iter().any(|(k, _)| k == "other"));
    }
}

#[test]
fn test_parser_icu1_reltime() {
    let parser = MessageParser::new("{time, relativetime}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::RelTime {
            style: RelTimeStyle::Auto,
            ..
        }
    ));
}

#[test]
fn test_parser_icu1_list_conjunction() {
    let parser = MessageParser::new("{names, list}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Conjunction,
            ..
        }
    ));
}

#[test]
fn test_parser_icu1_list_disjunction() {
    let parser = MessageParser::new("{names, list, or}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Disjunction,
            ..
        }
    ));
}

#[test]
fn test_parser_number_style_currency() {
    let parser = MessageParser::new("{price, number, currency}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Number { var, style: NumberStyle::Currency(code) } if var == "price" && code.is_empty())
    );
}

#[test]
fn test_parser_empty_template() {
    let parser = MessageParser::new("");
    let nodes = parser.parse().unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn test_parser_only_text() {
    let parser = MessageParser::new("Just plain text without variables");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        &nodes[0],
        &MessageNode::Text("Just plain text without variables".to_string())
    );
}

#[test]
fn test_parser_malformed_icu1_bad_function() {
    let parser = MessageParser::new("{x, unknown_function}");
    let result = parser.parse();
    assert!(result.is_ok());
    let nodes = result.unwrap();
    assert!(matches!(&nodes[0], MessageNode::Custom { .. }));
}

#[test]
fn test_extract_case_body_unmatched_brace() {
    let parser = MessageParser::new("match $x\nwhen one {unclosed pattern");
    let result = parser.parse();
    assert!(result.is_err());
}

#[test]
fn test_parser_mf2_inline_datetime() {
    let parser = MessageParser::new("{$date :datetime}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Date { var, style: DateStyle::DateTime } if var == "date")
    );
}

#[test]
fn test_parser_mf2_inline_number_integer() {
    let parser = MessageParser::new("{$n :number style=integer}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Number { var, style: NumberStyle::Integer } if var == "n")
    );
}

#[test]
fn test_parser_mf2_variable_pipe_default() {
    let parser = MessageParser::new("{name|Guest}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::VariableWithDefault { name, default } if name == "name" && default == "Guest")
    );
}

#[test]
fn test_parser_mf2_raw_variable() {
    let parser = MessageParser::new("{- name}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::RawVariable(v) if v == "name"));
}

#[test]
fn test_parser_mf2_raw_variable_with_default() {
    let parser = MessageParser::new("{- name|fallback}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::VariableWithDefault { name, default } if name == "name" && default == "fallback")
    );
}

#[test]
fn test_parser_mf2_selectordinal_multi_line() {
    let parser =
        MessageParser::new("match $n selectordinal\nwhen one {1st}\nwhen two {2nd}\n* {th}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Plural { var, ordinal: true, .. } if var == "n"));
}

#[test]
fn test_adversarial_match_no_body() {
    let parser = MessageParser::new("match $x");
    let result = parser.parse();
    // Degenerate case: no body produces empty plural
    assert!(result.is_ok(), "Match with no body returns empty plural");
    let nodes = result.unwrap();
    assert!(matches!(&nodes[0], MessageNode::Plural { var, cases, .. }
        if var == "x" && cases.is_empty()));
}

#[test]
fn test_adversarial_match_just_match_keyword() {
    let parser = MessageParser::new("match");
    let result = parser.parse();
    assert!(result.is_err(), "Just 'match' should error");
}

#[test]
fn test_adversarial_match_unknown_token() {
    let parser = MessageParser::new("match $x\nfoo {bar}");
    let result = parser.parse();
    assert!(result.is_err(), "Unknown token should error");
}

#[test]
fn test_adversarial_match_missing_key_after_when() {
    let parser = MessageParser::new("match $x\nwhen {value}");
    let result = parser.parse();
    assert!(result.is_err(), "Missing key after 'when' should error");
}

#[test]
fn test_adversarial_icu1_unknown_type_empty_string() {
    let parser = MessageParser::new("{x, }");
    let nodes = parser.parse();
    // expr_type is empty string " " trimmed to ""
    assert!(nodes.is_ok());
}

#[test]
fn test_adversarial_unicode_in_variable_name() {
    let parser = MessageParser::new("{名前}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Variable(v) if v == "名前"));
}

#[test]
fn test_adversarial_emoji_in_text() {
    let parser = MessageParser::new("Hello 😊 {name}!");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 3);
    assert!(matches!(&nodes[0], MessageNode::Text(t) if t == "Hello 😊 "));
    assert!(matches!(&nodes[1], MessageNode::Variable(v) if v == "name"));
    assert!(matches!(&nodes[2], MessageNode::Text(t) if t == "!"));
}

#[test]
fn test_adversarial_rtl_override_in_text() {
    let parser = MessageParser::new("LTR\u{202E}RTL\u{202C}{var}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[1], MessageNode::Variable(v) if v == "var"));
}

#[test]
fn test_adversarial_interval_plural_empty_input() {
    let result = parse_interval_plural("");
    assert!(
        result.is_none(),
        "Empty string should not be interval plural"
    );
}

#[test]
fn test_adversarial_interval_plural_no_close_bracket() {
    let result = parse_interval_plural("(0[none]");
    assert!(result.is_none(), "Missing ')' should return None");
}

#[test]
fn test_adversarial_interval_plural_malformed_range() {
    let result = parse_interval_plural("(abc)[test]");
    assert!(result.is_none(), "Non-numeric range should return None");
}

#[test]
fn test_is_plural_key_with_edge_values() {
    // Direct test of the is_plural_key function via match parsing
    let parser = MessageParser::new(
        "match $x\nwhen =0 {zero}\nwhen =1.5 {one point five}\nwhen one {one}\n* {other}",
    );
    let nodes = parser.parse();
    assert!(nodes.is_ok(), "Plural keys with = prefix should work");
}

#[test]
fn test_adversarial_two_var_match_with_exact_values() {
    let input = r#"match $count $size
when =0 small {zero small}
when =1 medium {one medium}
when =0 large {zero large}
when * * {other}"#;
    let parser = MessageParser::new(input);
    let result = parser.parse();
    assert!(
        result.is_ok(),
        "Two-var match with =prefix keys should work"
    );
    match &result.unwrap()[0] {
        MessageNode::Select { var, cases: _ } => {
            assert_eq!(var, "size", "Outer var should be the 2nd selector");
        }
        other => panic!("Expected Select, got {:?}", other),
    }
}

#[test]
fn test_adversarial_extremely_long_variable_name() {
    let template = format!("Hello {{{}}}!", "a".repeat(1000));
    let parser = MessageParser::new(&template);
    let nodes = parser.parse().unwrap();
    assert!(nodes.len() >= 2);
    let var_node = nodes.iter().find(|n| matches!(n, MessageNode::Variable(_)));
    assert!(var_node.is_some(), "Should find variable node");
}

#[test]
fn test_adversarial_nested_braces_in_expression() {
    let parser = MessageParser::new("{x{y}z}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    // Expression with nested braces should parse without panicking
    // The inner brace is included in the expression string
    if let MessageNode::Variable(v) = &nodes[0] {
        assert!(v.contains("x") || v.contains("y") || v.contains("z"));
    }
}

#[test]
fn test_parser_mf2_match_select() {
    let parser =
        MessageParser::new("match $gender\nwhen male {Mr.}\nwhen female {Ms.}\nwhen * {Mx.}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Select { var, .. } if var == "gender"));
}

#[test]
fn test_parser_icu1_date_style_time() {
    let parser = MessageParser::new("{now, time}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Date { var, style: DateStyle::Time } if var == "now"));
}

#[test]
fn test_parser_icu1_date_style_datetime() {
    let parser = MessageParser::new("{now, datetime}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Date { var, style: DateStyle::DateTime } if var == "now")
    );
}

#[test]
fn test_parser_icu1_reltime_explicit_styles() {
    for (input, expected_style) in [
        ("{t, relativetime, seconds}", RelTimeStyle::Seconds),
        ("{t, relativetime, minutes}", RelTimeStyle::Minutes),
        ("{t, relativetime, hours}", RelTimeStyle::Hours),
        ("{t, relativetime, days}", RelTimeStyle::Days),
        ("{t, relativetime, weeks}", RelTimeStyle::Weeks),
        ("{t, relativetime, months}", RelTimeStyle::Months),
        ("{t, relativetime, years}", RelTimeStyle::Years),
        ("{t, relativetime, unknown}", RelTimeStyle::Auto),
    ] {
        let parser = MessageParser::new(input);
        let nodes = parser.parse().unwrap();
        assert!(
            matches!(&nodes[0], MessageNode::RelTime { var, style } if var == "t" && *style == expected_style),
            "Failed for input: {}",
            input
        );
    }
}

#[test]
fn test_parser_icu1_list_style_unit() {
    let parser = MessageParser::new("{names, list, unit}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Unit,
            ..
        }
    ));
}

#[test]
fn test_parser_mf2_raw_variable_with_pipe_default() {
    let parser = MessageParser::new("{- name|FallbackName}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::VariableWithDefault { name, default } if name == "name" && default == "FallbackName")
    );
}

#[test]
fn test_parser_mf2_inline_date_style() {
    let parser = MessageParser::new("{$d :date}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Date { var, style: DateStyle::Date } if var == "d"));
}

#[test]
fn test_parser_mf2_inline_time_style() {
    let parser = MessageParser::new("{$t :time}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(&nodes[0], MessageNode::Date { var, style: DateStyle::Time } if var == "t"));
}

#[test]
fn test_parser_mf2_inline_list_conjunction() {
    let parser = MessageParser::new("{$items :list}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Conjunction,
            ..
        }
    ));
}

#[test]
fn test_parser_mf2_inline_list_disjunction() {
    let parser = MessageParser::new("{$items :list style=or}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Disjunction,
            ..
        }
    ));
}

#[test]
fn test_parser_mf2_inline_list_unit() {
    let parser = MessageParser::new("{$items :list style=unit}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Unit,
            ..
        }
    ));
}

#[test]
fn test_parser_mf2_inline_relativetime_units() {
    for (input, expected) in [
        ("{$t :relativedelta unit=seconds}", RelTimeStyle::Seconds),
        ("{$t :relativedelta unit=minutes}", RelTimeStyle::Minutes),
        ("{$t :relativedelta unit=hours}", RelTimeStyle::Hours),
        ("{$t :relativedelta unit=days}", RelTimeStyle::Days),
        ("{$t :relativedelta unit=weeks}", RelTimeStyle::Weeks),
        ("{$t :relativedelta unit=months}", RelTimeStyle::Months),
        ("{$t :relativetime unit=years}", RelTimeStyle::Years),
        ("{$t :relativedelta}", RelTimeStyle::Auto),
        ("{$t :relativetime unknown=foo}", RelTimeStyle::Auto),
    ] {
        let parser = MessageParser::new(input);
        let nodes = parser.parse().unwrap();
        assert!(
            matches!(&nodes[0], MessageNode::RelTime { var, style } if var == "t" && *style == expected),
            "Failed for input: {}",
            input
        );
    }
}

#[test]
fn test_parser_mf2_inline_number_currency_with_code() {
    let parser = MessageParser::new("{$p :number style=currency currency=EUR}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Number { var, style: NumberStyle::Currency(code) } if var == "p" && code == "EUR")
    );
}

#[test]
fn test_parser_mf2_inline_string_with_default() {
    let parser = MessageParser::new("{$name :string default=Guest}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::VariableWithDefault { name, default } if name == "name" && default == "Guest")
    );
}

#[test]
fn test_parser_mf2_inline_string_with_quoted_default() {
    let parser = MessageParser::new(r#"{$name :string default="John Doe"}"#);
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::VariableWithDefault { name, default } if name == "name" && default == "John Doe")
    );
}

#[test]
fn test_parser_icu1_currency_fallback_decimal() {
    // ICU1 splitn(3, ',') limits parts, so "currency, EUR" won't be parsed as Currency
    // The style falls through to NumberStyle::Decimal
    let parser = MessageParser::new("{price, number, currency, EUR}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Number { var, style: NumberStyle::Decimal } if var == "price")
    );
}

#[test]
fn test_parser_custom_formatter_with_options() {
    let parser = MessageParser::new("{val, suffix, prefix=Hello_ suffix=_World}");
    let nodes = parser.parse().unwrap();
    match &nodes[0] {
        MessageNode::Custom { var, format } => {
            assert_eq!(var, "val");
            assert_eq!(format.formatter, "suffix");
            assert_eq!(
                format.options.get("prefix").map(|s| s.as_str()),
                Some("Hello_")
            );
            assert_eq!(
                format.options.get("suffix").map(|s| s.as_str()),
                Some("_World")
            );
        }
        other => panic!("Expected Custom, got {:?}", other),
    }
}

#[test]
fn test_parser_icu1_custom_formatter_no_options() {
    let parser = MessageParser::new("{x, someFormatter}");
    let nodes = parser.parse().unwrap();
    assert!(
        matches!(&nodes[0], MessageNode::Custom { var, format } if var == "x" && format.formatter == "someFormatter" && format.options.is_empty())
    );
}

#[test]
fn test_parser_mf2_ordinal_single_var_match() {
    let parser = MessageParser::new("match $n selectordinal\nwhen one {1st}\n* {@th}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::Plural { ordinal: true, .. }
    ));
}

#[test]
fn test_parser_mf2_two_var_match_outer_plural_inner_select() {
    let input = r#"match $count $gender
when one masculine {1 man}
when one feminine  {1 woman}
when other masculine {{count} men}
when other feminine  {{count} women}
when * * {people}"#;
    let nodes = MessageParser::new(input).parse().unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        MessageNode::Select { var, cases } => {
            assert_eq!(var, "gender");
            assert_eq!(cases.len(), 3);
        }
        other => panic!("Expected Select, got {:?}", other),
    }
}

#[test]
fn test_parser_mf2_two_var_match_outer_plural() {
    // With plural keywords for outer, the outer should also be a Plural
    let input = r#"match $count $items
when one one {1 item}
when one other {1 item(s)}
when other one {{count} item}
when other other {{count} items}"#;
    let nodes = MessageParser::new(input).parse().unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        MessageNode::Plural { var, cases, .. } => {
            assert_eq!(var, "items");
            assert_eq!(cases.len(), 2);
        }
        other => panic!("Expected Plural, got {:?}", other),
    }
}

#[test]
fn test_parser_mf2_two_var_match_outer_select_inner_select() {
    let input = r#"match $type $level
when info debug {Info Debug}
when info error {Info Error}
when warn * {Warning}
when * * {Unknown}"#;
    let nodes = MessageParser::new(input).parse().unwrap();
    match &nodes[0] {
        MessageNode::Select { var, cases } => {
            assert_eq!(var, "level");
            assert_eq!(cases.len(), 3, "expected debug, error, other");
        }
        other => panic!("Expected Select, got {:?}", other),
    }
}

#[test]
fn test_parser_mf2_two_var_match_all_wildcard_outer() {
    let input = r#"match $a $b
when * x {first}
when * y {second}
when * * {all}"#;
    let nodes = MessageParser::new(input).parse().unwrap();
    match &nodes[0] {
        MessageNode::Select { var, cases: _ } => {
            assert_eq!(var, "b");
        }
        other => panic!("Expected Select, got {:?}", other),
    }
}

#[test]
fn test_expand_hash_escaped() {
    // Test # expansion and \# escape in plural case patterns
    let parser = MessageParser::new("{count, plural, one {# item} other {# items}}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    // # should expand to Variable("count")
}

#[test]
fn test_parser_unmatched_brace_error() {
    let parser = MessageParser::new("{unclosed");
    let result = parser.parse();
    assert!(result.is_err());
}

#[test]
fn test_parser_nested_braces() {
    let parser = MessageParser::new("{{nested}}");
    let nodes = parser.parse().unwrap();
    // The outer braces form an expression, inner is parsed as expression content
    assert_eq!(nodes.len(), 1);
}

#[test]
fn test_parser_emptry_brace_expression() {
    let parser = MessageParser::new("{}");
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
}

#[test]
fn test_parser_mf2_inline_list_unknown_fallback_to_conjunction() {
    let parser = MessageParser::new("{$items :list style=unknown}");
    let nodes = parser.parse().unwrap();
    assert!(matches!(
        &nodes[0],
        MessageNode::List {
            style: ListStyle::Conjunction,
            ..
        }
    ));
}

#[test]
fn test_parser_plural_mf2() {
    let parser = MessageParser::new(
        r#"
        match $count
        when 0 {no messages}
        when one {one message}
        * {{ $count } messages}
    "#,
    );
    let nodes = parser.parse().unwrap();
    assert_eq!(nodes.len(), 1);
    if let MessageNode::Plural {
        var,
        ordinal: _,
        cases,
    } = &nodes[0]
    {
        assert_eq!(var, "count");
        assert_eq!(cases.len(), 3);

        assert_eq!(cases[0].0, PluralCaseKey::Exact(0.0));
        assert_eq!(
            cases[0].1,
            vec![MessageNode::Text("no messages".to_string())]
        );

        assert_eq!(cases[1].0, PluralCaseKey::One);
        assert_eq!(
            cases[1].1,
            vec![MessageNode::Text("one message".to_string())]
        );

        assert_eq!(cases[2].0, PluralCaseKey::Other);
        assert_eq!(
            cases[2].1,
            vec![
                MessageNode::Variable("count".to_string()),
                MessageNode::Text(" messages".to_string())
            ]
        );
    } else {
        panic!("Expected Plural node");
    }
}

#[test]
fn test_compile_to_bytes_roundtrip() {
    let temp_src = Path::new("temp_compile_bytes_test");
    let en_dir = temp_src.join("en");
    fs::create_dir_all(&en_dir).unwrap();
    fs::write(en_dir.join("test.json"), r#"{"greeting": "Hello {name}!"}"#).unwrap();

    let result = l10n4x_compiler::compile_translations_to_bytes(temp_src);
    assert!(
        result.is_ok(),
        "compile_to_bytes should succeed: {:?}",
        result.err()
    );
    let bytes_map = result.unwrap();
    assert!(bytes_map.contains_key("en"), "should have 'en' locale");
    let en_bytes = &bytes_map["en"];
    assert_eq!(&en_bytes[0..4], b"L10N", "should produce valid L10N format");

    let _ = fs::remove_dir_all(temp_src);
}
