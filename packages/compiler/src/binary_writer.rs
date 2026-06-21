use crate::icu_parser::{MessageNode, NumberStyle, PluralCaseKey};

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
            MessageNode::Variable(v) => {
                buf.push(0x02);
                let bytes = v.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
            MessageNode::Plural { var, cases } => {
                buf.push(0x03);
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
                let style_byte: u8 = match style {
                    NumberStyle::Decimal => 0x00,
                    NumberStyle::Percent => 0x01,
                    NumberStyle::Integer => 0x02,
                };
                buf.push(style_byte);
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
