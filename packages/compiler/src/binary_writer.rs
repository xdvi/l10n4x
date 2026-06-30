use ahash::AHashMap;
use std::io::{self, Write};
use crate::icu_parser::{
    DateStyle, ListStyle, MessageNode, NumberStyle, PluralCaseKey, RelTimeStyle,
};

/// Serialize parsed message nodes to ICU bytecode.
pub fn serialize_message(nodes: &[MessageNode]) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_nodes(nodes, &mut buf).unwrap();
    buf
}

fn serialize_nodes<W: Write>(nodes: &[MessageNode], w: &mut W) -> io::Result<()> {
    // Optimization: single Text node emits raw bytes (no opcode 0x01 prefix)
    if nodes.len() == 1 {
        if let MessageNode::Text(t) = &nodes[0] {
            w.write_all(t.as_bytes())?;
            return Ok(());
        }
    }
    for node in nodes {
        match node {
            MessageNode::Text(t) => {
                w.write_all(&[0x01])?;
                let bytes = t.as_bytes();
                w.write_all(&(bytes.len() as u32).to_be_bytes())?;
                w.write_all(bytes)?;
            }
            MessageNode::RawVariable(v) => {
                // Unescaped variable ({- name} syntax). Emits 0x0B with raw flag.
                w.write_all(&[0x0B])?;
                let bytes = v.as_bytes();
                w.write_all(&(bytes.len() as u32).to_be_bytes())?;
                w.write_all(bytes)?;
                w.write_all(&[0x01])?; // flags: 0x01 = raw (no escaping)
            }
            MessageNode::Variable(v) => {
                // Variables are escaped by default (opcode 0x0B with flags byte).
                // The `{- var}` unescape marker is handled at the parser level and
                // emitted as a separate RawVariable node.
                w.write_all(&[0x0B])?;
                let bytes = v.as_bytes();
                w.write_all(&(bytes.len() as u32).to_be_bytes())?;
                w.write_all(bytes)?;
                w.write_all(&[0x00])?; // flags: 0 = escaped by default
            }
            MessageNode::Plural {
                var,
                ordinal,
                cases,
            } => {
                if *ordinal {
                    w.write_all(&[0x0A])?; // ordinal plural
                } else {
                    w.write_all(&[0x03])?; // cardinal plural
                }
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                w.write_all(&(cases.len() as u16).to_be_bytes())?;
                for (key, pattern) in cases {
                    match key {
                        PluralCaseKey::Other => w.write_all(&[0x00])?,
                        PluralCaseKey::Exact(val) => {
                            w.write_all(&[0x01])?;
                            w.write_all(&val.to_be_bytes())?;
                        }
                        PluralCaseKey::Zero => w.write_all(&[0x02])?,
                        PluralCaseKey::One => w.write_all(&[0x03])?,
                        PluralCaseKey::Two => w.write_all(&[0x04])?,
                        PluralCaseKey::Few => w.write_all(&[0x05])?,
                        PluralCaseKey::Many => w.write_all(&[0x06])?,
                        PluralCaseKey::Range(min, max) => {
                            w.write_all(&[0x07])?;
                            w.write_all(&min.to_be_bytes())?;
                            w.write_all(&max.to_be_bytes())?;
                        }
                    }
                    let mut sub = Vec::new();
                    serialize_nodes(pattern, &mut sub)?;
                    w.write_all(&(sub.len() as u32).to_be_bytes())?;
                    w.write_all(&sub)?;
                }
            }
            MessageNode::Select { var, cases } => {
                w.write_all(&[0x04])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                w.write_all(&(cases.len() as u16).to_be_bytes())?;
                for (key, pattern) in cases {
                    let key_bytes = key.as_bytes();
                    w.write_all(&(key_bytes.len() as u32).to_be_bytes())?;
                    w.write_all(key_bytes)?;
                    let mut sub = Vec::new();
                    serialize_nodes(pattern, &mut sub)?;
                    w.write_all(&(sub.len() as u32).to_be_bytes())?;
                    w.write_all(&sub)?;
                }
            }
            MessageNode::Number { var, style } => {
                w.write_all(&[0x05])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                match style {
                    NumberStyle::Decimal => {
                        w.write_all(&[0x00])?;
                    }
                    NumberStyle::Percent => {
                        w.write_all(&[0x01])?;
                    }
                    NumberStyle::Integer => {
                        w.write_all(&[0x02])?;
                    }
                    NumberStyle::Currency(code) => {
                        w.write_all(&[0x03])?;
                        let code_bytes = code.as_bytes();
                        w.write_all(&(code_bytes.len() as u32).to_be_bytes())?;
                        w.write_all(code_bytes)?;
                    }
                }
            }
            MessageNode::Date { var, style } => {
                w.write_all(&[0x06])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                let style_byte: u8 = match style {
                    DateStyle::Date => 0x00,
                    DateStyle::Time => 0x01,
                    DateStyle::DateTime => 0x02,
                };
                w.write_all(&[style_byte])?;
            }
            MessageNode::VariableWithDefault { name, default } => {
                w.write_all(&[0x0C])?;
                let name_bytes = name.as_bytes();
                let default_bytes = default.as_bytes();
                w.write_all(&(name_bytes.len() as u32).to_be_bytes())?;
                w.write_all(name_bytes)?;
                w.write_all(&(default_bytes.len() as u32).to_be_bytes())?;
                w.write_all(default_bytes)?;
                w.write_all(&[0x00])?; // flags: 0 = escaped by default
            }
            MessageNode::List { var, style } => {
                w.write_all(&[0x09])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                let style_byte: u8 = match style {
                    ListStyle::Conjunction => 0x00,
                    ListStyle::Disjunction => 0x01,
                    ListStyle::Unit => 0x02,
                };
                w.write_all(&[style_byte])?;
            }
            MessageNode::RelTime { var, style } => {
                w.write_all(&[0x08])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                let style_byte: u8 = match style {
                    RelTimeStyle::Auto => 0x00,
                    RelTimeStyle::Seconds => 0x01,
                    RelTimeStyle::Minutes => 0x02,
                    RelTimeStyle::Hours => 0x03,
                    RelTimeStyle::Days => 0x04,
                    RelTimeStyle::Weeks => 0x05,
                    RelTimeStyle::Months => 0x06,
                    RelTimeStyle::Years => 0x07,
                };
                w.write_all(&[style_byte])?;
            }
            MessageNode::Markup { .. } => {}
            MessageNode::Custom {
                var,
                literal_operand,
                format,
            } => {
                w.write_all(&[0x0D])?;
                let var_bytes = var.as_bytes();
                w.write_all(&(var_bytes.len() as u32).to_be_bytes())?;
                w.write_all(var_bytes)?;
                let lit = literal_operand.as_deref().unwrap_or("");
                let lit_bytes = lit.as_bytes();
                w.write_all(&(lit_bytes.len() as u32).to_be_bytes())?;
                w.write_all(lit_bytes)?;
                let fmt_bytes = format.formatter.as_bytes();
                w.write_all(&(fmt_bytes.len() as u32).to_be_bytes())?;
                w.write_all(fmt_bytes)?;
                let opts_str = format
                    .options
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",");
                let opt_bytes = opts_str.as_bytes();
                w.write_all(&(opt_bytes.len() as u32).to_be_bytes())?;
                w.write_all(opt_bytes)?;
            }
            MessageNode::KeyRef(ref_key) => {
                // KeyRef should be resolved before writing, but emit as Text if not.
                w.write_all(&[0x01])?;
                let bytes = ref_key.as_bytes();
                w.write_all(&(bytes.len() as u32).to_be_bytes())?;
                w.write_all(bytes)?;
            }
            MessageNode::Mf2Match {
                selectors,
                inputs,
                locals,
                variants,
            } => {
                w.write_all(&[0x0E])?;
                w.write_all(&[selectors.len() as u8])?;
                for sel in selectors {
                    let b = sel.as_bytes();
                    w.write_all(&(b.len() as u32).to_be_bytes())?;
                    w.write_all(b)?;
                }
                w.write_all(&(inputs.len() as u16).to_be_bytes())?;
                for (name, expr) in inputs {
                    let nb = name.as_bytes();
                    w.write_all(&(nb.len() as u32).to_be_bytes())?;
                    w.write_all(nb)?;
                    serialize_decl_expr(expr, w)?;
                }
                w.write_all(&(locals.len() as u16).to_be_bytes())?;
                for (name, expr) in locals {
                    let nb = name.as_bytes();
                    w.write_all(&(nb.len() as u32).to_be_bytes())?;
                    w.write_all(nb)?;
                    serialize_decl_expr(expr, w)?;
                }
                w.write_all(&(variants.len() as u16).to_be_bytes())?;
                for (keys, pattern) in variants {
                    w.write_all(&[keys.len() as u8])?;
                    for key in keys {
                        let kb = key.as_bytes();
                        w.write_all(&(kb.len() as u32).to_be_bytes())?;
                        w.write_all(kb)?;
                    }
                    let mut sub = Vec::new();
                    serialize_nodes(pattern, &mut sub)?;
                    w.write_all(&(sub.len() as u32).to_be_bytes())?;
                    w.write_all(&sub)?;
                }
            }
        }
    }
    Ok(())
}

fn serialize_decl_expr<W: Write>(node: &MessageNode, w: &mut W) -> io::Result<()> {
    let (var, literal, formatter, options) = match node {
        MessageNode::Custom {
            var,
            literal_operand,
            format,
        } => (
            var.as_str(),
            literal_operand.as_deref().unwrap_or(""),
            format.formatter.as_str(),
            format
                .options
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(","),
        ),
        MessageNode::Variable(name) => (name.as_str(), "", "", String::new()),
        MessageNode::VariableWithDefault { name, default } => {
            (name.as_str(), default.as_str(), "string", String::new())
        }
        _ => ("", "", "", String::new()),
    };
    for s in [var, literal, formatter, options.as_str()] {
        let b = s.as_bytes();
        w.write_all(&(b.len() as u32).to_be_bytes())?;
        w.write_all(b)?;
    }
    Ok(())
}

pub fn write_binary_format(
    translations: &AHashMap<u64, Vec<MessageNode>>,
) -> Vec<u8> {
    write_binary_format_with_keys(translations, None)
}

pub fn write_binary_format_with_keys(
    translations: &AHashMap<u64, Vec<MessageNode>>,
    key_names: Option<&AHashMap<u64, String>>,
) -> Vec<u8> {
    use std::collections::BTreeMap;
    let mut entries = BTreeMap::new();
    for (&hash, nodes) in translations {
        entries.insert(hash, serialize_message(nodes));
    }
    #[cfg(feature = "debug-keys")]
    let debug = key_names.map(|m| {
        let mut out = BTreeMap::new();
        for (&hash, name) in m {
            out.insert(hash, name.clone());
        }
        out
    });
    #[cfg(feature = "debug-keys")]
    let debug_ref = debug.as_ref();
    #[cfg(not(feature = "debug-keys"))]
    let _ = key_names;
    l10n4x_core::binary_format::pack_l10n(
        &entries,
        l10n4x_core::binary_format::RUNTIME_VERSION,
        l10n4x_core::locale_data::LOCALE_DATA_VERSION,
        #[cfg(feature = "debug-keys")]
        debug_ref,
        #[cfg(not(feature = "debug-keys"))]
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fnv1a_64;
    use crate::icu_parser::{
        CustomFormat, DateStyle, ListStyle, NumberStyle, PluralCaseKey, RelTimeStyle,
    };
    use std::collections::HashMap;

    use ahash::AHashMap;

    fn serialize_nodes_vec(nodes: &[MessageNode]) -> Vec<u8> {
        let mut buf = Vec::new();
        serialize_nodes(nodes, &mut buf).unwrap();
        buf
    }

    #[test]
    fn test_serialize_text() {
        let nodes = vec![MessageNode::Text("Hello World".to_string())];
        let bytes = serialize_nodes_vec(&nodes);
        // Single text node emits raw bytes with no opcode prefix
        assert_eq!(&bytes, b"Hello World");
    }

    #[test]
    fn test_serialize_variable() {
        let nodes = vec![MessageNode::Variable("name".to_string())];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x0B);
        let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(len, 4);
        assert_eq!(&bytes[5..9], b"name");
        assert_eq!(bytes[9], 0x00); // escaped by default
    }

    #[test]
    fn test_serialize_raw_variable() {
        let nodes = vec![MessageNode::RawVariable("html".to_string())];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x0B);
        assert_eq!(bytes[9], 0x01); // raw flag
    }

    #[test]
    fn test_serialize_plural_cardinal() {
        let nodes = vec![MessageNode::Plural {
            var: "count".to_string(),
            ordinal: false,
            cases: vec![
                (
                    PluralCaseKey::One,
                    vec![MessageNode::Text("item".to_string())],
                ),
                (
                    PluralCaseKey::Other,
                    vec![MessageNode::Text("items".to_string())],
                ),
            ],
        }];
        let bytes = serialize_nodes_vec(&nodes);
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
    fn test_serialize_plural_range() {
        let nodes = vec![MessageNode::Plural {
            var: "count".to_string(),
            ordinal: false,
            cases: vec![
                (
                    PluralCaseKey::Range(4, 500),
                    vec![MessageNode::Text("many".to_string())],
                ),
                (
                    PluralCaseKey::Range(7, i32::MAX),
                    vec![MessageNode::Text("lots".to_string())],
                ),
            ],
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x03);
        let var_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        let mut pos = 5 + var_len + 2;
        assert_eq!(bytes[pos], 0x07);
        pos += 1;
        assert_eq!(
            i32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()),
            4
        );
        pos += 4;
        assert_eq!(
            i32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()),
            500
        );
    }

    #[test]
    fn test_serialize_plural_ordinal() {
        let nodes = vec![MessageNode::Plural {
            var: "n".to_string(),
            ordinal: true,
            cases: vec![
                (
                    PluralCaseKey::One,
                    vec![MessageNode::Text("1st".to_string())],
                ),
                (
                    PluralCaseKey::Other,
                    vec![MessageNode::Text("th".to_string())],
                ),
            ],
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x0A); // ordinal plural
    }

    #[test]
    fn test_serialize_select() {
        let nodes = vec![MessageNode::Select {
            var: "gender".to_string(),
            cases: vec![
                (
                    "male".to_string(),
                    vec![MessageNode::Text("Mr.".to_string())],
                ),
                (
                    "other".to_string(),
                    vec![MessageNode::Text("Mx.".to_string())],
                ),
            ],
        }];
        let bytes = serialize_nodes_vec(&nodes);
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
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x05);
        assert_eq!(bytes[bytes.len() - 1], 0x00); // decimal style
    }

    #[test]
    fn test_serialize_number_percent() {
        let nodes = vec![MessageNode::Number {
            var: "val".to_string(),
            style: NumberStyle::Percent,
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[bytes.len() - 1], 0x01);
    }

    #[test]
    fn test_serialize_number_integer() {
        let nodes = vec![MessageNode::Number {
            var: "val".to_string(),
            style: NumberStyle::Integer,
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[bytes.len() - 1], 0x02);
    }

    #[test]
    fn test_serialize_number_currency() {
        let nodes = vec![MessageNode::Number {
            var: "amt".to_string(),
            style: NumberStyle::Currency("USD".to_string()),
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[bytes.len() - 1 - 4 - 3], 0x03); // currency style
                                                          // currency code should appear
        let code_len_pos = bytes.len() - 4 - 3;
        let code_len =
            u32::from_be_bytes(bytes[code_len_pos..code_len_pos + 4].try_into().unwrap()) as usize;
        assert_eq!(code_len, 3);
        assert_eq!(&bytes[code_len_pos + 4..code_len_pos + 4 + 3], b"USD");
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
            let bytes = serialize_nodes_vec(&nodes);
            assert_eq!(bytes[bytes.len() - 1], expected_byte, "DateStyle variant");
        }
    }

    #[test]
    fn test_serialize_variable_with_default() {
        let nodes = vec![MessageNode::VariableWithDefault {
            name: "user".to_string(),
            default: "Guest".to_string(),
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x0C);
        let name_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(name_len, 4);
        assert_eq!(&bytes[5..9], b"user");
        let default_len_pos = 9;
        let default_len = u32::from_be_bytes(
            bytes[default_len_pos..default_len_pos + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(default_len, 5);
        assert_eq!(
            &bytes[default_len_pos + 4..default_len_pos + 4 + 5],
            b"Guest"
        );
        assert_eq!(bytes[bytes.len() - 1], 0x00); // escaped
    }

    #[test]
    fn test_serialize_list_conjunction() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Conjunction,
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x09);
        assert_eq!(bytes[bytes.len() - 1], 0x00);
    }

    #[test]
    fn test_serialize_list_disjunction() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Disjunction,
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[bytes.len() - 1], 0x01);
    }

    #[test]
    fn test_serialize_list_unit() {
        let nodes = vec![MessageNode::List {
            var: "items".to_string(),
            style: ListStyle::Unit,
        }];
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[bytes.len() - 1], 0x02);
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
            let bytes = serialize_nodes_vec(&nodes);
            assert_eq!(bytes[bytes.len() - 1], expected_byte);
        }
    }

    #[test]
    fn test_serialize_custom_formatter() {
        let mut opts = HashMap::new();
        opts.insert("prefix".to_string(), "<".to_string());
        opts.insert("suffix".to_string(), ">".to_string());
        let nodes = vec![MessageNode::Custom {
            literal_operand: None,
            var: "val".to_string(),
            format: CustomFormat {
                formatter: "wrap".to_string(),
                options: opts,
            },
        }];
        let bytes = serialize_nodes_vec(&nodes);
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
        let bytes = serialize_nodes_vec(&nodes);
        assert_eq!(bytes[0], 0x01); // emitted as text
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("other.key"));
    }

    #[test]
    fn test_write_binary_format_empty() {
        let translations = AHashMap::new();
        let bytes = write_binary_format(&translations);
        assert_eq!(&bytes[0..4], b"L10N");
        let version = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(version, l10n4x_core::binary_format::FORMAT_VERSION_V3);
        let index_count = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!(index_count, 0);
    }

    #[test]
    fn test_write_binary_format_single() {
        let mut translations = AHashMap::new();
        translations.insert(
            fnv1a_64(b"key1"),
            vec![MessageNode::Text("Hello".to_string())],
        );
        let bytes = write_binary_format(&translations);
        assert_eq!(&bytes[0..4], b"L10N");
        let index_count = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!(index_count, 1);
        // Verify we can read it back
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        let val = reader.lookup(fnv1a_64(b"key1")).unwrap();
        // Single text node: stored as raw bytes without opcode prefix
        assert_eq!(val, b"Hello");
    }

    #[test]
    fn test_write_binary_format_multiple_sorted() {
        let mut translations = AHashMap::new();
        translations.insert(
            fnv1a_64(b"b"),
            vec![MessageNode::Text("second".to_string())],
        );
        translations.insert(fnv1a_64(b"a"), vec![MessageNode::Text("first".to_string())]);
        let bytes = write_binary_format(&translations);
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        // keys should be sorted
        assert!(reader.lookup(fnv1a_64(b"a")).is_some());
        assert!(reader.lookup(fnv1a_64(b"b")).is_some());
        // Verify the bytecode values are correct (single text = raw bytes)
        let val_a = reader.lookup(fnv1a_64(b"a")).unwrap();
        let val_b = reader.lookup(fnv1a_64(b"b")).unwrap();
        assert_eq!(val_a, b"first");
        assert_eq!(val_b, b"second");
    }

    #[test]
    fn test_roundtrip_via_reader_and_formatter() {
        let mut translations = AHashMap::new();
        translations.insert(
            fnv1a_64(b"greeting"),
            vec![
                MessageNode::Text("Hello ".to_string()),
                MessageNode::Variable("name".to_string()),
                MessageNode::Text("!".to_string()),
            ],
        );
        let bytes = write_binary_format(&translations);
        let reader = l10n4x_core::binary_format::BinaryFormatReader::new(&bytes).unwrap();
        let bc = reader.lookup(fnv1a_64(b"greeting")).unwrap();
        let mut out = String::new();
        l10n4x_core::formatter::format_message(bc, "en", &[("name", "World")], &mut out).unwrap();
        assert_eq!(out, "Hello World!");
    }
}
