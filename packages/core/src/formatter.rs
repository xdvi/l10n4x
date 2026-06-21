extern crate alloc;

use crate::number_format::{format_number, NumberStyle};
use crate::plural_rules::get_plural_category;

/// Represents standard CLDR plural categories used by plural rule selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    /// Zero category (e.g. "=0" or special rules).
    Zero,
    /// One category (e.g. "=1" or singular cases).
    One,
    /// Two category (dual).
    Two,
    /// Few category.
    Few,
    /// Many category.
    Many,
    /// Fallback default category.
    Other,
}



/// Formats a bytecode compiled message into the provided writer, dynamically
/// interpolating variables and evaluating plural/select rules.
pub fn format_message<W: core::fmt::Write>(
    bytecode: &[u8],
    locale: &str,
    params: &[(&str, &str)],
    writer: &mut W,
) -> core::fmt::Result {
    let mut pos = 0;
    while pos < bytecode.len() {
        if pos + 1 > bytecode.len() {
            return Err(core::fmt::Error);
        }
        let opcode = bytecode[pos];
        pos += 1;
        match opcode {
            0x01 => {
                // Text
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let len = u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let text = core::str::from_utf8(&bytecode[pos..pos + len])
                    .map_err(|_| core::fmt::Error)?;
                pos += len;
                writer.write_str(text)?;
            }
            0x02 => {
                // Variable
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let len = u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_name = core::str::from_utf8(&bytecode[pos..pos + len])
                    .map_err(|_| core::fmt::Error)?;
                pos += len;
                if let Some((_, val)) = params.iter().find(|(k, _)| *k == var_name) {
                    writer.write_str(val)?;
                } else {
                    writer.write_str("{")?;
                    writer.write_str(var_name)?;
                    writer.write_str("}")?;
                }
            }
            0x03 => {
                // Plural Match
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + var_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_name = core::str::from_utf8(&bytecode[pos..pos + var_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += var_len;

                if pos + 2 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let num_cases =
                    u16::from_be_bytes(bytecode[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;

                // Lookup parameter value
                let param_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or("0");
                let parsed_val: f64 = param_val.parse().unwrap_or(0.0);

                let cat = get_plural_category(locale, parsed_val);

                let mut best_case_pos = None;
                let mut best_case_len = None;
                let mut other_case_pos = None;
                let mut other_case_len = None;

                for _ in 0..num_cases {
                    if pos + 1 > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let case_type = bytecode[pos];
                    pos += 1;

                    let has_val = case_type == 0x01;
                    let val = if has_val {
                        if pos + 8 > bytecode.len() {
                            return Err(core::fmt::Error);
                        }
                        let v = f64::from_be_bytes(bytecode[pos..pos + 8].try_into().unwrap());
                        pos += 8;
                        Some(v)
                    } else {
                        None
                    };

                    if pos + 4 > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let pat_len =
                        u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;

                    if pos + pat_len > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let pat_pos = pos;
                    pos += pat_len;

                    if case_type == 0x00 {
                        // other
                        other_case_pos = Some(pat_pos);
                        other_case_len = Some(pat_len);
                    }

                    let matches = match case_type {
                        0x01 => (parsed_val - val.unwrap()).abs() < 1e-9,
                        0x02 => cat == PluralCategory::Zero,
                        0x03 => cat == PluralCategory::One,
                        0x04 => cat == PluralCategory::Two,
                        0x05 => cat == PluralCategory::Few,
                        0x06 => cat == PluralCategory::Many,
                        _ => false,
                    };

                    if matches && best_case_pos.is_none() {
                        best_case_pos = Some(pat_pos);
                        best_case_len = Some(pat_len);
                    }
                }

                let (selected_pos, selected_len) = best_case_pos
                    .map(|p| (p, best_case_len.unwrap()))
                    .or_else(|| other_case_pos.map(|p| (p, other_case_len.unwrap())))
                    .ok_or(core::fmt::Error)?;

                let sub_bytecode = &bytecode[selected_pos..selected_pos + selected_len];
                format_message(sub_bytecode, locale, params, writer)?;
            }
            0x04 => {
                // Select Match
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + var_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_name = core::str::from_utf8(&bytecode[pos..pos + var_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += var_len;

                if pos + 2 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let num_cases =
                    u16::from_be_bytes(bytecode[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;

                // Lookup parameter value
                let param_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or("");

                let mut best_case_pos = None;
                let mut best_case_len = None;
                let mut other_case_pos = None;
                let mut other_case_len = None;

                for _ in 0..num_cases {
                    if pos + 4 > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let key_len =
                        u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    if pos + key_len > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let key_name = core::str::from_utf8(&bytecode[pos..pos + key_len])
                        .map_err(|_| core::fmt::Error)?;
                    pos += key_len;

                    if pos + 4 > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let pat_len =
                        u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    if pos + pat_len > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let pat_pos = pos;
                    pos += pat_len;

                    if key_name == "other" {
                        other_case_pos = Some(pat_pos);
                        other_case_len = Some(pat_len);
                    }

                    if key_name == param_val && best_case_pos.is_none() {
                        best_case_pos = Some(pat_pos);
                        best_case_len = Some(pat_len);
                    }
                }

                let (selected_pos, selected_len) = best_case_pos
                    .map(|p| (p, best_case_len.unwrap()))
                    .or_else(|| other_case_pos.map(|p| (p, other_case_len.unwrap())))
                    .ok_or(core::fmt::Error)?;

                let sub_bytecode = &bytecode[selected_pos..selected_pos + selected_len];
                format_message(sub_bytecode, locale, params, writer)?;
            }
            0x05 => {
                // Number formatting
                if pos + 4 > bytecode.len() { return Err(core::fmt::Error); }
                let var_len = u32::from_be_bytes(bytecode[pos..pos+4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + var_len > bytecode.len() { return Err(core::fmt::Error); }
                let var_name = core::str::from_utf8(&bytecode[pos..pos+var_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += var_len;
                if pos + 1 > bytecode.len() { return Err(core::fmt::Error); }
                let style_byte = bytecode[pos];
                pos += 1;

                let style = match style_byte {
                    0x01 => NumberStyle::Percent,
                    0x02 => NumberStyle::Integer,
                    _    => NumberStyle::Decimal,
                };

                let param_val: f64 = params.iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| v.parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(0.0);

                let formatted = format_number(param_val, locale, style);
                writer.write_str(&formatted)?;
            }
            _ => return Err(core::fmt::Error),
        }
    }
    Ok(())
}
