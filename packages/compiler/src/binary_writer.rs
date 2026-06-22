use crate::icu_parser::{DateStyle, ListStyle, MessageNode, NumberStyle, PluralCaseKey, RelTimeStyle};

fn serialize_nodes(nodes: &[MessageNode]) -> Vec<u8> {
    let mut buf = Vec::new();
    for node in nodes {
        match node {
            MessageNode::Text(t) => {
                buf.push(0x01);
                let bytes = t.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
            MessageNode::RawVariable(v) => {
                // Unescaped variable ({- name} syntax). Emits 0x0B with raw flag.
                buf.push(0x0B);
                let bytes = v.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
                buf.push(0x01); // flags: 0x01 = raw (no escaping)
            }
            MessageNode::Variable(v) => {
                // Variables are escaped by default (opcode 0x0B with flags byte).
                // The `{- var}` unescape marker is handled at the parser level and
                // emitted as a separate RawVariable node.
                buf.push(0x0B);
                let bytes = v.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
                buf.push(0x00); // flags: 0 = escaped by default
            }
            MessageNode::Plural { var, ordinal, cases } => {
                if *ordinal {
                    buf.push(0x0A); // ordinal plural
                } else {
                    buf.push(0x03); // cardinal plural
                }
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                buf.extend_from_slice(&(cases.len() as u16).to_be_bytes());
                for (key, pattern) in cases {
                    match key {
                        PluralCaseKey::Other => buf.push(0x00),
                        PluralCaseKey::Exact(val) => {
                            buf.push(0x01);
                            buf.extend_from_slice(&val.to_be_bytes());
                        }
                        PluralCaseKey::Zero => buf.push(0x02),
                        PluralCaseKey::One => buf.push(0x03),
                        PluralCaseKey::Two => buf.push(0x04),
                        PluralCaseKey::Few => buf.push(0x05),
                        PluralCaseKey::Many => buf.push(0x06),
                    }
                    let pattern_bytecode = serialize_nodes(pattern);
                    buf.extend_from_slice(&(pattern_bytecode.len() as u32).to_be_bytes());
                    buf.extend_from_slice(&pattern_bytecode);
                }
            }
            MessageNode::Select { var, cases } => {
                buf.push(0x04);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                buf.extend_from_slice(&(cases.len() as u16).to_be_bytes());
                for (key, pattern) in cases {
                    let key_bytes = key.as_bytes();
                    buf.extend_from_slice(&(key_bytes.len() as u32).to_be_bytes());
                    buf.extend_from_slice(key_bytes);
                    let pattern_bytecode = serialize_nodes(pattern);
                    buf.extend_from_slice(&(pattern_bytecode.len() as u32).to_be_bytes());
                    buf.extend_from_slice(&pattern_bytecode);
                }
            }
            MessageNode::Number { var, style } => {
                buf.push(0x05);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                match style {
                    NumberStyle::Decimal => { buf.push(0x00); }
                    NumberStyle::Percent => { buf.push(0x01); }
                    NumberStyle::Integer => { buf.push(0x02); }
                    NumberStyle::Currency(code) => {
                        buf.push(0x03);
                        let code_bytes = code.as_bytes();
                        buf.extend_from_slice(&(code_bytes.len() as u32).to_be_bytes());
                        buf.extend_from_slice(code_bytes);
                    }
                }
            }
            MessageNode::Date { var, style } => {
                buf.push(0x06);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                let style_byte: u8 = match style {
                    DateStyle::Date     => 0x00,
                    DateStyle::Time     => 0x01,
                    DateStyle::DateTime => 0x02,
                };
                buf.push(style_byte);
            }
            MessageNode::VariableWithDefault { name, default } => {
                buf.push(0x0C);
                let name_bytes    = name.as_bytes();
                let default_bytes = default.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(name_bytes);
                buf.extend_from_slice(&(default_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(default_bytes);
                buf.push(0x00); // flags: 0 = escaped by default
            }
            MessageNode::List { var, style } => {
                buf.push(0x09);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                let style_byte: u8 = match style {
                    ListStyle::Conjunction => 0x00,
                    ListStyle::Disjunction => 0x01,
                    ListStyle::Unit        => 0x02,
                };
                buf.push(style_byte);
            }
            MessageNode::RelTime { var, style } => {
                buf.push(0x08);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                let style_byte: u8 = match style {
                    RelTimeStyle::Auto    => 0x00,
                    RelTimeStyle::Seconds => 0x01,
                    RelTimeStyle::Minutes => 0x02,
                    RelTimeStyle::Hours   => 0x03,
                    RelTimeStyle::Days    => 0x04,
                    RelTimeStyle::Weeks   => 0x05,
                    RelTimeStyle::Months  => 0x06,
                    RelTimeStyle::Years   => 0x07,
                };
                buf.push(style_byte);
            }
            MessageNode::Custom { var, format } => {
                buf.push(0x0D);
                let var_bytes = var.as_bytes();
                buf.extend_from_slice(&(var_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(var_bytes);
                let fmt_bytes = format.formatter.as_bytes();
                buf.extend_from_slice(&(fmt_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(fmt_bytes);
                let opts_str = format.options.iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",");
                let opt_bytes = opts_str.as_bytes();
                buf.extend_from_slice(&(opt_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(opt_bytes);
            }
            MessageNode::KeyRef(ref_key) => {
                // KeyRef should be resolved before writing, but emit as Text if not.
                buf.push(0x01);
                let bytes = ref_key.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
        }
    }
    buf
}

pub fn write_binary_format(
    translations: &std::collections::HashMap<String, Vec<MessageNode>>,
) -> Vec<u8> {
    let mut sorted_keys: Vec<&String> = translations.keys().collect();
    sorted_keys.sort();

    let mut data_pool = Vec::new();
    let mut index_entries = Vec::new();

    let mut current_offset = 16u32;

    for key in sorted_keys {
        let key_bytes = key.as_bytes();
        let key_offset = current_offset;
        let key_len = key_bytes.len() as u32;

        data_pool.extend_from_slice(key_bytes);
        current_offset += key_len;

        let val_nodes = translations.get(key).unwrap();
        let val_bytes = serialize_nodes(val_nodes);
        let val_offset = current_offset;
        let val_len = val_bytes.len() as u32;

        data_pool.extend_from_slice(&val_bytes);
        current_offset += val_len;

        index_entries.push((key_offset, key_len, val_offset, val_len));
    }

    let index_offset = current_offset;
    let index_count = index_entries.len() as u32;

    for (k_off, k_len, v_off, v_len) in index_entries {
        data_pool.extend_from_slice(&k_off.to_be_bytes());
        data_pool.extend_from_slice(&k_len.to_be_bytes());
        data_pool.extend_from_slice(&v_off.to_be_bytes());
        data_pool.extend_from_slice(&v_len.to_be_bytes());
    }

    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"L10N");
    buffer.extend_from_slice(&l10n4x_core::binary_format::FORMAT_VERSION.to_be_bytes());
    buffer.extend_from_slice(&index_offset.to_be_bytes());
    buffer.extend_from_slice(&index_count.to_be_bytes());
    buffer.extend_from_slice(&data_pool);

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icu_parser::{CustomFormat, DateStyle, ListStyle, NumberStyle, PluralCaseKey, RelTimeStyle};
    use std::collections::HashMap;

    #[test]
    fn test_serialize_text() {
        let nodes = vec![MessageNode::Text("Hello World".to_string())];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x01);
        let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(len, 11);
        assert_eq!(&bytes[5..5+11], b"Hello World");
    }

    #[test]
    fn test_serialize_variable() {
        let nodes = vec![MessageNode::Variable("name".to_string())];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x0B);
        let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(len, 4);
        assert_eq!(&bytes[5..9], b"name");
        assert_eq!(bytes[9], 0x00); // escaped by default
    }

    #[test]
    fn test_serialize_raw_variable() {
        let nodes = vec![MessageNode::RawVariable("html".to_string())];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x0B);
        assert_eq!(bytes[9], 0x01); // raw flag
    }

    #[test]
    fn test_serialize_plural_cardinal() {
        let nodes = vec![MessageNode::Plural {
            var: "count".to_string(),
            ordinal: false,
            cases: vec![
                (PluralCaseKey::One, vec![MessageNode::Text("item".to_string())]),
                (PluralCaseKey::Other, vec![MessageNode::Text("items".to_string())]),
            ],
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x03); // cardinal plural
        // var name
        let var_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(var_len, 5);
        assert_eq!(&bytes[5..10], b"count");
        // case count
        let num_cases = u16::from_be_bytes(bytes[10..12].try_into().unwrap());
        assert_eq!(num_cases, 2);
    }

    #[test]
    fn test_serialize_plural_ordinal() {
        let nodes = vec![MessageNode::Plural {
            var: "n".to_string(),
            ordinal: true,
            cases: vec![
                (PluralCaseKey::One, vec![MessageNode::Text("1st".to_string())]),
                (PluralCaseKey::Other, vec![MessageNode::Text("th".to_string())]),
            ],
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x0A); // ordinal plural
    }

    #[test]
    fn test_serialize_select() {
        let nodes = vec![MessageNode::Select {
            var: "gender".to_string(),
            cases: vec![
                ("male".to_string(), vec![MessageNode::Text("Mr.".to_string())]),
                ("other".to_string(), vec![MessageNode::Text("Mx.".to_string())]),
            ],
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x04); // select opcode
        let var_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(var_len, 6);
        assert_eq!(&bytes[5..11], b"gender");
    }

    #[test]
    fn test_serialize_number_decimal() {
        let nodes = vec![MessageNode::Number {
            var: "val".to_string(),
            style: NumberStyle::Decimal,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x05);
        assert_eq!(bytes[bytes.len()-1], 0x00); // decimal style
    }

    #[test]
    fn test_serialize_number_percent() {
        let nodes = vec![MessageNode::Number {
            var: "val".to_string(),
            style: NumberStyle::Percent,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[bytes.len()-1], 0x01);
    }

    #[test]
    fn test_serialize_number_integer() {
        let nodes = vec![MessageNode::Number {
            var: "val".to_string(),
            style: NumberStyle::Integer,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[bytes.len()-1], 0x02);
    }

    #[test]
    fn test_serialize_number_currency() {
        let nodes = vec![MessageNode::Number {
            var: "amt".to_string(),
            style: NumberStyle::Currency("USD".to_string()),
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[bytes.len()-1-4-3], 0x03); // currency style
        // currency code should appear
        let code_len_pos = bytes.len() - 4 - 3;
        let code_len = u32::from_be_bytes(bytes[code_len_pos..code_len_pos+4].try_into().unwrap()) as usize;
        assert_eq!(code_len, 3);
        assert_eq!(&bytes[code_len_pos+4..code_len_pos+4+3], b"USD");
    }

    #[test]
    fn test_serialize_date_styles() {
        for (style, expected_byte) in [
            (DateStyle::Date, 0x00u8),
            (DateStyle::Time, 0x01),
            (DateStyle::DateTime, 0x02),
        ] {
            let nodes = vec![MessageNode::Date {
                var: "d".to_string(),
                style,
            }];
            let bytes = serialize_nodes(&nodes);
            assert_eq!(bytes[bytes.len()-1], expected_byte, "DateStyle variant");
        }
    }

    #[test]
    fn test_serialize_variable_with_default() {
        let nodes = vec![MessageNode::VariableWithDefault {
            name: "user".to_string(),
            default: "Guest".to_string(),
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x0C);
        let name_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(name_len, 4);
        assert_eq!(&bytes[5..9], b"user");
        let default_len_pos = 9;
        let default_len = u32::from_be_bytes(bytes[default_len_pos..default_len_pos+4].try_into().unwrap()) as usize;
        assert_eq!(default_len, 5);
        assert_eq!(&bytes[default_len_pos+4..default_len_pos+4+5], b"Guest");
        assert_eq!(bytes[bytes.len()-1], 0x00); // escaped
    }

    #[test]
    fn test_serialize_list_conjunction() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Conjunction,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x09);
        assert_eq!(bytes[bytes.len()-1], 0x00);
    }

    #[test]
    fn test_serialize_list_disjunction() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Disjunction,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[bytes.len()-1], 0x01);
    }

    #[test]
    fn test_serialize_list_unit() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Unit,
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[bytes.len()-1], 0x02);
    }

    #[test]
    fn test_serialize_reltime_styles() {
        for (style, expected_byte) in [
            (RelTimeStyle::Auto, 0x00u8),
            (RelTimeStyle::Seconds, 0x01),
            (RelTimeStyle::Minutes, 0x02),
            (RelTimeStyle::Hours, 0x03),
            (RelTimeStyle::Days, 0x04),
            (RelTimeStyle::Weeks, 0x05),
            (RelTimeStyle::Months, 0x06),
            (RelTimeStyle::Years, 0x07),
        ] {
            let nodes = vec![MessageNode::RelTime {
                var: "t".to_string(),
                style,
            }];
            let bytes = serialize_nodes(&nodes);
            assert_eq!(bytes[bytes.len()-1], expected_byte);
        }
    }

    #[test]
    fn test_serialize_custom_formatter() {
        let mut opts = HashMap::new();
        opts.insert("prefix".to_string(), "<".to_string());
        opts.insert("suffix".to_string(), ">".to_string());
        let nodes = vec![MessageNode::Custom {
            var: "val".to_string(),
            format: CustomFormat {
                formatter: "wrap".to_string(),
                options: opts,
            },
        }];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x0D);
        // Should contain "wrap" and "prefix=<" and "suffix=>"
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("wrap"));
        assert!(s.contains("prefix=<"));
        assert!(s.contains("suffix=>"));
    }

    #[test]
    fn test_serialize_keyref() {
        let nodes = vec![MessageNode::KeyRef("other.key".to_string())];
        let bytes = serialize_nodes(&nodes);
        assert_eq!(bytes[0], 0x01); // emitted as text
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("other.key"));
    }

    #[test]
    fn test_write_binary_format_empty() {
        let translations = HashMap::new();
        let bytes = write_binary_format(&translations);
        assert_eq!(&bytes[0..4], b"L10N");
        let index_count = u32::from_be_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!(index_count, 0);
    }

    #[test]
    fn test_write_binary_format_single() {
        let mut translations = HashMap::new();
        translations.insert("key1".to_string(), vec![MessageNode::Text("Hello".to_string())]);
        let bytes = write_binary_format(&translations);
        assert_eq!(&bytes[0..4], b"L10N");
        let index_count = u32::from_be_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!(index_count, 1);
        // Verify we can read it back
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        let val = reader.lookup("key1").unwrap();
        assert_eq!(val, b"\x01\x00\x00\x00\x05Hello");
    }

    #[test]
    fn test_write_binary_format_multiple_sorted() {
        let mut translations = HashMap::new();
        translations.insert("b".to_string(), vec![MessageNode::Text("second".to_string())]);
        translations.insert("a".to_string(), vec![MessageNode::Text("first".to_string())]);
        let bytes = write_binary_format(&translations);
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        // keys should be sorted
        assert!(reader.lookup("a").is_some());
        assert!(reader.lookup("b").is_some());
        // Verify the bytecode values are correct
        let val_a = reader.lookup("a").unwrap();
        let val_b = reader.lookup("b").unwrap();
        let text_len_a = u32::from_be_bytes(val_a[1..5].try_into().unwrap()) as usize;
        assert_eq!(&val_a[5..5+text_len_a], b"first");
        let text_len_b = u32::from_be_bytes(val_b[1..5].try_into().unwrap()) as usize;
        assert_eq!(&val_b[5..5+text_len_b], b"second");
    }

    #[test]
    fn test_roundtrip_via_reader_and_formatter() {
        let mut translations = HashMap::new();
        translations.insert("greeting".to_string(), vec![
            MessageNode::Text("Hello ".to_string()),
            MessageNode::Variable("name".to_string()),
            MessageNode::Text("!".to_string()),
        ]);
        let bytes = write_binary_format(&translations);
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        let bc = reader.lookup("greeting").unwrap();
        let mut out = String::new();
        l10n4x_core::formatter::format_message(bc, "en", &[("name", "World")], &mut out).unwrap();
        assert_eq!(out, "Hello World!");
    }
}
