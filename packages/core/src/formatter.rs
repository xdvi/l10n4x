extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;

use crate::date_format::{format_date, DateStyle};
use crate::list_format::{format_list, ListStyle};
use crate::number_format::{format_currency, format_number, NumberStyle};
use crate::plural_rules::{get_ordinal_category, get_plural_category};
use crate::reltime::{format_relative_time, RelTimeStyle};

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(feature = "std")]
use std::sync::Mutex;

/// Type for custom formatter functions.
/// Takes (value, locale, options_map) and returns formatted string.
#[cfg(feature = "std")]
pub type CustomFormatter = Box<dyn Fn(&str, &str, &HashMap<String, String>) -> String + Send>;

#[cfg(feature = "std")]
static FORMATTER_REGISTRY: Mutex<Option<HashMap<String, CustomFormatter>>> = Mutex::new(None);

/// Registers a custom formatter function.
#[cfg(feature = "std")]
pub fn register_formatter(name: &str, formatter: CustomFormatter) {
    if let Ok(mut lock) = FORMATTER_REGISTRY.lock() {
        lock.get_or_insert_with(HashMap::new)
            .insert(name.to_string(), formatter);
    }
}

/// Formats a value using a registered custom formatter.
/// Returns `None` if no formatter with that name is registered.
#[cfg(feature = "std")]
pub fn format_with_custom(
    name: &str,
    value: &str,
    locale: &str,
    options: &HashMap<String, String>,
) -> Option<String> {
    if let Ok(lock) = FORMATTER_REGISTRY.lock() {
        if let Some(ref map) = *lock {
            if let Some(f) = map.get(name) {
                return Some(f(value, locale, options));
            }
        }
    }
    None
}

/// HTML-entity-escapes a string: & < > " ' → &amp; &lt; &gt; &quot; &#39;
pub fn html_escape(s: &str) -> String {
    let mut out = alloc::string::String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

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

#[inline]
fn param_value<'a>(
    params: &[(&'a str, &'a str)],
    index: Option<&BTreeMap<&'a str, &'a str>>,
    name: &str,
) -> Option<&'a str> {
    if let Some(map) = index {
        return map.get(name).copied();
    }
    if params.len() == 1 {
        return if params[0].0 == name {
            Some(params[0].1)
        } else {
            None
        };
    }
    params
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
}

/// Formats a bytecode compiled message into the provided writer, dynamically
/// interpolating variables and evaluating plural/select rules.
pub fn format_message<W: core::fmt::Write>(
    bytecode: &[u8],
    locale: &str,
    params: &[(&str, &str)],
    writer: &mut W,
) -> core::fmt::Result {
    // Fast path: single raw text node (bytes don't start with an opcode 0x01..0x0D)
    if !bytecode.is_empty() && (bytecode[0] == 0x00 || bytecode[0] > 0x0D) {
        let text = core::str::from_utf8(bytecode).map_err(|_| core::fmt::Error)?;
        return writer.write_str(text);
    }
    let param_index = if params.len() > 1 {
        let mut map = BTreeMap::new();
        for (k, v) in params {
            map.insert(*k, *v);
        }
        Some(map)
    } else {
        None
    };
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
                if let Some(val) = param_value(params, param_index.as_ref(), var_name) {
                    writer.write_str(val)?;
                } else {
                    writer.write_str("{")?;
                    writer.write_str(var_name)?;
                    writer.write_str("}")?;
                }
            }
            0x0B => {
                // Variable with HTML escaping (has flags byte: bit 0 = raw/unescaped)
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
                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let flags = bytecode[pos];
                pos += 1;
                if let Some(val) = param_value(params, param_index.as_ref(), var_name) {
                    if flags & 0x01 == 0 {
                        writer.write_str(&html_escape(val))?;
                    } else {
                        writer.write_str(val)?;
                    }
                } else {
                    writer.write_str("{")?;
                    writer.write_str(var_name)?;
                    writer.write_str("}")?;
                }
            }
            0x0A => {
                // Ordinal Plural Match (same binary format as 0x03, uses ordinal CLDR rules)
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

                let param_val =
                    param_value(params, param_index.as_ref(), var_name).unwrap_or("0");
                let parsed_i: i64 = param_val.parse().unwrap_or(0);
                let cat = get_ordinal_category(locale, parsed_i);

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
                        other_case_pos = Some(pat_pos);
                        other_case_len = Some(pat_len);
                    }
                    let matches = match case_type {
                        0x01 => (parsed_i as f64 - val.unwrap_or(0.0)).abs() < 1e-9,
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
                let param_val =
                    param_value(params, param_index.as_ref(), var_name).unwrap_or("0");
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
                let param_val =
                    param_value(params, param_index.as_ref(), var_name).unwrap_or("");

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

                let param_val: f64 = param_value(params, param_index.as_ref(), var_name)
                    .map(|v| v.parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(0.0);

                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let style_byte = bytecode[pos];
                pos += 1;

                if style_byte == 0x03 {
                    // Currency style — read currency code
                    if pos + 4 > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let code_len =
                        u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    if pos + code_len > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let currency_code = core::str::from_utf8(&bytecode[pos..pos + code_len])
                        .map_err(|_| core::fmt::Error)?;
                    pos += code_len;
                    let formatted = format_currency(param_val, locale, currency_code);
                    writer.write_str(&formatted)?;
                } else {
                    let style = match style_byte {
                        0x01 => NumberStyle::Percent,
                        0x02 => NumberStyle::Integer,
                        _ => NumberStyle::Decimal,
                    };
                    let formatted = format_number(param_val, locale, style);
                    writer.write_str(&formatted)?;
                }
            }
            0x06 => {
                // Date/Time formatting
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
                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let style_byte = bytecode[pos];
                pos += 1;

                let style = match style_byte {
                    0x01 => DateStyle::Time,
                    0x02 => DateStyle::DateTime,
                    _ => DateStyle::Date,
                };

                let raw_val =
                    param_value(params, param_index.as_ref(), var_name).unwrap_or("");

                let formatted = format_date(raw_val, locale, style);
                writer.write_str(&formatted)?;
            }
            0x07 => {
                // Variable with default value
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let name_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + name_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_name = core::str::from_utf8(&bytecode[pos..pos + name_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += name_len;

                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let default_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + default_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let default_val = core::str::from_utf8(&bytecode[pos..pos + default_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += default_len;

                let value = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or(default_val);

                writer.write_str(value)?;
            }
            0x08 => {
                // Relative time formatting
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
                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let style_byte = bytecode[pos];
                pos += 1;

                let style = match style_byte {
                    0x01 => RelTimeStyle::Seconds,
                    0x02 => RelTimeStyle::Minutes,
                    0x03 => RelTimeStyle::Hours,
                    0x04 => RelTimeStyle::Days,
                    0x05 => RelTimeStyle::Weeks,
                    0x06 => RelTimeStyle::Months,
                    0x07 => RelTimeStyle::Years,
                    _ => RelTimeStyle::Auto,
                };

                let raw_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or("0");
                let delta: i64 = raw_val.parse().unwrap_or(0);

                let formatted = format_relative_time(delta, locale, style);
                writer.write_str(&formatted)?;
            }
            0x09 => {
                // List formatting
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
                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let style_byte = bytecode[pos];
                pos += 1;

                let style = match style_byte {
                    0x01 => ListStyle::Disjunction,
                    0x02 => ListStyle::Unit,
                    _ => ListStyle::Conjunction,
                };

                let raw_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or("[]");

                let formatted = format_list(raw_val, locale, style);
                writer.write_str(&formatted)?;
            }
            0x0C => {
                // Variable with default value + HTML escaping (has flags byte: bit 0 = raw/unescaped)
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let name_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + name_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let var_name = core::str::from_utf8(&bytecode[pos..pos + name_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += name_len;

                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let default_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + default_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let default_val = core::str::from_utf8(&bytecode[pos..pos + default_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += default_len;

                if pos + 1 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let flags = bytecode[pos];
                pos += 1;

                let raw_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or(default_val);

                if flags & 0x01 == 0 {
                    writer.write_str(&html_escape(raw_val))?;
                } else {
                    writer.write_str(raw_val)?;
                }
            }
            0x0D => {
                // Custom formatter
                // Layout: var_len(4) + var_name(var_len) + fmt_len(4) + fmt_name(fmt_len) + opt_len(4) + options(opt_len)
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
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let fmt_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + fmt_len > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let fmt_name = core::str::from_utf8(&bytecode[pos..pos + fmt_len])
                    .map_err(|_| core::fmt::Error)?;
                pos += fmt_len;
                if pos + 4 > bytecode.len() {
                    return Err(core::fmt::Error);
                }
                let opt_len =
                    u32::from_be_bytes(bytecode[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                let options_str = if opt_len > 0 {
                    if pos + opt_len > bytecode.len() {
                        return Err(core::fmt::Error);
                    }
                    let s = core::str::from_utf8(&bytecode[pos..pos + opt_len])
                        .map_err(|_| core::fmt::Error)?;
                    pos += opt_len;
                    s
                } else {
                    ""
                };

                let raw_val = params
                    .iter()
                    .find(|(k, _)| *k == var_name)
                    .map(|(_, v)| *v)
                    .unwrap_or(var_name);

                let result: Option<alloc::string::String> = {
                    #[cfg(feature = "std")]
                    {
                        let mut opts = HashMap::new();
                        if !options_str.is_empty() {
                            for pair in options_str.split(',') {
                                if let Some(eq_pos) = pair.find('=') {
                                    opts.insert(
                                        pair[..eq_pos].trim().to_string(),
                                        pair[eq_pos + 1..].trim().to_string(),
                                    );
                                }
                            }
                        }
                        format_with_custom(fmt_name, raw_val, locale, &opts)
                    }
                    #[cfg(not(feature = "std"))]
                    {
                        let _ = fmt_name;
                        let _ = options_str;
                        None
                    }
                };

                if let Some(formatted) = result {
                    writer.write_str(&formatted)?;
                } else {
                    writer.write_str(raw_val)?;
                }
            }
            _ => return Err(core::fmt::Error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod formatter_unit_tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    fn make_text(text: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x01);
        buf.extend_from_slice(&(text.len() as u32).to_be_bytes());
        buf.extend_from_slice(text.as_bytes());
        buf
    }

    fn make_var(name: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x02);
        buf.extend_from_slice(&(name.len() as u32).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf
    }

    fn make_escaped_var(name: &str, raw: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x0B);
        buf.extend_from_slice(&(name.len() as u32).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.push(if raw { 0x01 } else { 0x00 });
        buf
    }

    fn make_var_default(name: &str, default: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x07);
        buf.extend_from_slice(&(name.len() as u32).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(default.len() as u32).to_be_bytes());
        buf.extend_from_slice(default.as_bytes());
        buf
    }

    #[test]
    fn test_html_escape_amp() {
        assert_eq!(html_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn test_html_escape_lt_gt() {
        assert_eq!(html_escape("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn test_html_escape_quotes() {
        assert_eq!(html_escape("\"hello\""), "&quot;hello&quot;");
        assert_eq!(html_escape("'world'"), "&#39;world&#39;");
    }

    #[test]
    fn test_html_escape_no_change() {
        assert_eq!(html_escape("plain text 123"), "plain text 123");
    }

    #[test]
    fn test_html_escape_empty() {
        assert_eq!(html_escape(""), "");
    }

    #[test]
    fn test_plural_category_debug() {
        let _ = format!("{:?}", PluralCategory::One);
    }

    #[test]
    fn test_text_only() {
        let bc = make_text("Hello");
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "Hello");
    }

    #[test]
    fn test_variable_substitution() {
        let mut bc = Vec::new();
        bc.extend_from_slice(&make_text("Hello "));
        bc.extend_from_slice(&make_var("name"));
        bc.extend_from_slice(&make_text("!"));
        let mut out = String::new();
        format_message(&bc, "en", &[("name", "John")], &mut out).unwrap();
        assert_eq!(out, "Hello John!");
    }

    #[test]
    fn test_variable_missing_shows_braces() {
        let bc = make_var("missing");
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "{missing}");
    }

    #[test]
    fn test_escaped_variable_escapes_html() {
        let bc = make_escaped_var("name", false);
        let mut out = String::new();
        format_message(&bc, "en", &[("name", "<b>bold</b>")], &mut out).unwrap();
        assert_eq!(out, "&lt;b&gt;bold&lt;/b&gt;");
    }

    #[test]
    fn test_raw_variable_no_escape() {
        let bc = make_escaped_var("name", true);
        let mut out = String::new();
        format_message(&bc, "en", &[("name", "<b>bold</b>")], &mut out).unwrap();
        assert_eq!(out, "<b>bold</b>");
    }

    #[test]
    fn test_variable_with_default_present() {
        let bc = make_var_default("name", "DefaultName");
        let mut out = String::new();
        format_message(&bc, "en", &[("name", "Actual")], &mut out).unwrap();
        assert_eq!(out, "Actual");
    }

    #[test]
    fn test_variable_with_default_missing() {
        let bc = make_var_default("name", "DefaultName");
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "DefaultName");
    }

    #[test]
    fn test_unknown_opcode_returns_error() {
        let bc = vec![0xFF];
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_text_len() {
        // Text opcode but only 2 bytes available for length
        let bc = vec![0x01, 0x00];
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_text_body() {
        let mut bc = vec![0x01];
        bc.extend_from_slice(&10u32.to_be_bytes()); // claims 10 bytes but only 2 follow
        bc.extend_from_slice(b"ab");
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_var_len() {
        let bc = vec![0x02, 0x00, 0x00]; // not enough for full u32
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_bytecode() {
        let bc = vec![];
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn test_select_simple() {
        // Build a select on variable "gender" with cases: "male" -> "Mr.", "female" -> "Ms.", other -> "Mx."
        let mut bc = Vec::new();
        bc.push(0x04); // select opcode
                       // var name length + name
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        // 3 cases
        bc.extend_from_slice(&3u16.to_be_bytes());
        // case "male"
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"male");
        let male_pat = make_text("Mr.");
        bc.extend_from_slice(&(male_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&male_pat);
        // case "female"
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"female");
        let female_pat = make_text("Ms.");
        bc.extend_from_slice(&(female_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&female_pat);
        // other
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"other");
        let other_pat = make_text("Mx.");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[("gender", "male")], &mut out).unwrap();
        assert_eq!(out, "Mr.");

        let mut out = String::new();
        format_message(&bc, "en", &[("gender", "female")], &mut out).unwrap();
        assert_eq!(out, "Ms.");

        let mut out = String::new();
        format_message(&bc, "en", &[("gender", "other")], &mut out).unwrap();
        assert_eq!(out, "Mx.");

        // unknown value falls to other
        let mut out = String::new();
        format_message(&bc, "en", &[("gender", "unknown")], &mut out).unwrap();
        assert_eq!(out, "Mx.");
    }

    #[test]
    fn test_select_missing_var() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"other");
        let other_pat = make_text("N/A");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "N/A");
    }

    #[test]
    fn test_number_format_opcode_decimal() {
        let mut bc = Vec::new();
        bc.push(0x05); // number opcode
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x00); // Decimal style
        let mut out = String::new();
        format_message(&bc, "en", &[("num", "1234.56")], &mut out).unwrap();
        assert_eq!(out, "1,234.56");
    }

    #[test]
    fn test_number_format_opcode_percent() {
        let mut bc = Vec::new();
        bc.push(0x05);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x01); // Percent style
        let mut out = String::new();
        format_message(&bc, "en", &[("num", "0.75")], &mut out).unwrap();
        assert_eq!(out, "75%");
    }

    #[test]
    fn test_number_format_opcode_integer() {
        let mut bc = Vec::new();
        bc.push(0x05);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x02); // Integer style
        let mut out = String::new();
        format_message(&bc, "en", &[("num", "3.14")], &mut out).unwrap();
        assert_eq!(out, "3");
    }

    #[test]
    fn test_number_format_opcode_currency() {
        let mut bc = Vec::new();
        bc.push(0x05);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"amt");
        bc.push(0x03); // Currency style
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"USD");
        let mut out = String::new();
        format_message(&bc, "en", &[("amt", "42.50")], &mut out).unwrap();
        assert_eq!(out, "$42.50");
    }

    #[test]
    fn test_reltime_opcode() {
        let mut bc = Vec::new();
        bc.push(0x08);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"time");
        bc.push(0x04); // Days style
        let mut out = String::new();
        format_message(&bc, "en", &[("time", "-86400")], &mut out).unwrap();
        assert_eq!(out, "yesterday");
    }

    #[test]
    fn test_reltime_opcode_auto() {
        let mut bc = Vec::new();
        bc.push(0x08);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"time");
        bc.push(0xFF); // unknown style -> Auto
        let mut out = String::new();
        format_message(&bc, "en", &[("time", "45")], &mut out).unwrap();
        assert_eq!(out, "in 45 seconds");
    }

    #[test]
    fn test_list_format_conjunction() {
        let mut bc = Vec::new();
        bc.push(0x09);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"list");
        bc.push(0x00); // Conjunction
        let mut out = String::new();
        format_message(&bc, "en", &[("list", r#"["A","B","C"]"#)], &mut out).unwrap();
        assert_eq!(out, "A, B, and C");
    }

    #[test]
    fn test_list_format_disjunction() {
        let mut bc = Vec::new();
        bc.push(0x09);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"list");
        bc.push(0x01); // Disjunction
        let mut out = String::new();
        format_message(&bc, "en", &[("list", r#"["X","Y"]"#)], &mut out).unwrap();
        assert_eq!(out, "X or Y");
    }

    #[test]
    fn test_list_format_unit() {
        let mut bc = Vec::new();
        bc.push(0x09);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"list");
        bc.push(0x02); // Unit
        let mut out = String::new();
        format_message(&bc, "en", &[("list", r#"["A","B"]"#)], &mut out).unwrap();
        assert_eq!(out, "A, B");
    }

    #[test]
    fn test_variable_with_default_and_escaping_present_raw() {
        let mut bc = Vec::new();
        bc.push(0x0C);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"name");
        bc.extend_from_slice(&7u32.to_be_bytes());
        bc.extend_from_slice(b"default");
        bc.push(0x01); // flags: raw
        let mut out = String::new();
        format_message(&bc, "en", &[("name", "<hey>")], &mut out).unwrap();
        assert_eq!(out, "<hey>");
    }

    #[test]
    fn test_variable_with_default_and_escaping_missing_escaped() {
        let mut bc = Vec::new();
        bc.push(0x0C);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"name");
        bc.extend_from_slice(&9u32.to_be_bytes());
        bc.extend_from_slice(b"<default>");
        bc.push(0x00); // flags: escaped
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "&lt;default&gt;");
    }

    #[test]
    fn test_plural_english_one() {
        // Build bytecode for: plural on "count" with one -> "item", other -> "items"
        let mut bc = Vec::new();
        bc.push(0x03); // plural opcode
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&2u16.to_be_bytes());
        // case: one (0x03)
        bc.push(0x03);
        let one_pat = make_text("item");
        bc.extend_from_slice(&(one_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&one_pat);
        // case: other (0x00)
        bc.push(0x00);
        let other_pat = make_text("items");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[("count", "1")], &mut out).unwrap();
        assert_eq!(out, "item");

        let mut out = String::new();
        format_message(&bc, "en", &[("count", "5")], &mut out).unwrap();
        assert_eq!(out, "items");
    }

    #[test]
    fn test_plural_exact_value() {
        // exact match: "=0" -> "none", other -> "some"
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&2u16.to_be_bytes());
        // exact case (0x01) with value 0.0
        bc.push(0x01);
        bc.extend_from_slice(&0.0f64.to_be_bytes());
        let zero_pat = make_text("none");
        bc.extend_from_slice(&(zero_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&zero_pat);
        // other
        bc.push(0x00);
        let other_pat = make_text("some");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[("count", "0")], &mut out).unwrap();
        assert_eq!(out, "none");

        let mut out = String::new();
        format_message(&bc, "en", &[("count", "3")], &mut out).unwrap();
        assert_eq!(out, "some");
    }

    #[test]
    fn test_plural_missing_var_uses_zero() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&2u16.to_be_bytes());
        // other
        bc.push(0x00);
        let other_pat = make_text("fallback");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);
        // one (won't match without var)
        bc.push(0x03);
        let one_pat = make_text("one");
        bc.extend_from_slice(&(one_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&one_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "fallback");
    }

    #[test]
    fn test_plural_no_other_case_returns_error() {
        // Only a 'few' case, no 'other' - should error for English 5
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x05); // few
        let few_pat = make_text("few");
        bc.extend_from_slice(&(few_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&few_pat);

        let mut out = String::new();
        let result = format_message(&bc, "en", &[("count", "5")], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_ordinal_plural() {
        let mut bc = Vec::new();
        bc.push(0x0A); // ordinal plural opcode
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&2u16.to_be_bytes());
        // one ordinal case
        bc.push(0x03);
        let one_pat = make_text("1st");
        bc.extend_from_slice(&(one_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&one_pat);
        // other
        bc.push(0x00);
        let other_pat = make_text("Nth");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[("num", "1")], &mut out).unwrap();
        assert_eq!(out, "1st");

        let mut out = String::new();
        format_message(&bc, "en", &[("num", "3")], &mut out).unwrap();
        assert_eq!(out, "Nth");
    }

    #[test]
    fn test_date_format() {
        let mut bc = Vec::new();
        bc.push(0x06); // date opcode
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"date");
        bc.push(0x00); // Date style
        let mut out = String::new();
        format_message(&bc, "en", &[("date", "2024-01-15")], &mut out).unwrap();
        assert!(out.contains("2024") || out.contains("Jan") || out.contains("01/15"));
    }

    #[test]
    fn test_time_format() {
        let mut bc = Vec::new();
        bc.push(0x06);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"date");
        bc.push(0x01); // Time style
        let mut out = String::new();
        format_message(&bc, "en", &[("date", "14:30:00")], &mut out).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn test_date_time_format() {
        let mut bc = Vec::new();
        bc.push(0x06);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"date");
        bc.push(0x02); // DateTime style
        let mut out = String::new();
        format_message(&bc, "en", &[("date", "2024-01-15T14:30:00")], &mut out).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn test_date_missing_var() {
        let mut bc = Vec::new();
        bc.push(0x06);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"date");
        bc.push(0x00);
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn test_custom_formatter_no_std_fallback() {
        let mut bc = Vec::new();
        bc.push(0x0D);
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"name");
        bc.extend_from_slice(&7u32.to_be_bytes());
        bc.extend_from_slice(b"customf");
        bc.extend_from_slice(&0u32.to_be_bytes());

        let mut out = String::new();
        format_message(&bc, "en", &[("name", "Diego")], &mut out).unwrap();
        assert_eq!(out, "Diego");
    }

    // ═══════════════════════════════════════════════════════════════
    //  ADVERSARIAL TESTS — trying to break the formatter
    // ═══════════════════════════════════════════════════════════════

    fn make_plural_bc(var: &str, case_type: u8, exact_val: Option<f64>) -> Vec<u8> {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&(var.len() as u32).to_be_bytes());
        bc.extend_from_slice(var.as_bytes());
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(case_type);
        if let Some(v) = exact_val {
            bc.extend_from_slice(&v.to_be_bytes());
        }
        let pat = make_text("matched");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        bc
    }

    fn make_select_bc(var: &str, cases: &[&str]) -> Vec<u8> {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&(var.len() as u32).to_be_bytes());
        bc.extend_from_slice(var.as_bytes());
        bc.extend_from_slice(&(cases.len() as u16).to_be_bytes());
        for key in cases {
            bc.extend_from_slice(&(key.len() as u32).to_be_bytes());
            bc.extend_from_slice(key.as_bytes());
            let pat = make_text(key);
            bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
            bc.extend_from_slice(&pat);
        }
        bc
    }

    #[test]
    fn adversarial_plural_exact_nan() {
        let bc = make_plural_bc("count", 0x01, Some(f64::NAN));
        let mut out = String::new();
        // NaN never matches via comparison, and there's no "other" case -> error
        let result = format_message(&bc, "en", &[("count", "5")], &mut out);
        assert!(
            result.is_err(),
            "NaN exact match should error (no fallback)"
        );
    }

    #[test]
    fn adversarial_plural_exact_infinity() {
        let bc = make_plural_bc("count", 0x01, Some(f64::INFINITY));
        let mut out = String::new();
        let result = format_message(&bc, "en", &[("count", "5")], &mut out);
        assert!(
            result.is_err(),
            "Infinity exact match should error (no fallback)"
        );
    }

    #[test]
    fn adversarial_select_no_other_case() {
        let bc = make_select_bc("gender", &["male", "female"]);
        let mut out = String::new();
        let result = format_message(&bc, "en", &[("gender", "unknown")], &mut out);
        assert!(result.is_err(), "Select without 'other' should error");
    }

    #[test]
    fn adversarial_select_empty_key_name() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.extend_from_slice(&0u32.to_be_bytes());
        bc.extend_from_slice(b"");
        let pat = make_text("fallback");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);

        let mut out = String::new();
        let result = format_message(&bc, "en", &[("gender", "")], &mut out);
        assert!(result.is_ok(), "Empty key should match empty param");
    }

    #[test]
    fn adversarial_invalid_utf8_in_text() {
        let mut bc = vec![0x01];
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(&[0xFF, 0xFE, 0x00, 0xFF]);
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err(), "Invalid UTF-8 should return error");
    }

    #[test]
    fn adversarial_invalid_utf8_in_var_name() {
        let mut bc = vec![0x02];
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(&[0xFF, 0xFE, 0x00, 0xFF]);
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err(), "Invalid UTF-8 in var name should error");
    }

    #[test]
    fn adversarial_empty_locale() {
        let bc = make_text("hello");
        let mut out = String::new();
        let result = format_message(&bc, "", &[], &mut out);
        assert!(result.is_ok(), "Empty locale should not crash");
        assert_eq!(out, "hello");
    }

    #[test]
    fn adversarial_unknown_case_type_in_plural() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x42); // unknown case type (not 0x00-0x06)
        let pat = make_text("weird");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);

        let mut out = String::new();
        let result = format_message(&bc, "en", &[("count", "5")], &mut out);
        assert!(
            result.is_err(),
            "Unknown case type with no 'other' should error"
        );
    }

    #[test]
    fn adversarial_ordinal_unknown_case_type() {
        let mut bc = Vec::new();
        bc.push(0x0A);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x99); // unknown case type
        let pat = make_text("result");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);

        let mut out = String::new();
        let result = format_message(&bc, "en", &[("num", "5")], &mut out);
        assert!(
            result.is_err(),
            "Unknown ordinal case type with no 'other' should error"
        );
    }

    #[test]
    fn adversarial_ordinal_i64_max_precision_loss() {
        let mut bc = Vec::new();
        bc.push(0x0A);
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"count");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x01); // exact match
        bc.extend_from_slice(&(i64::MAX as f64).to_be_bytes());
        let pat = make_text("large");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);

        let mut out = String::new();
        let result = format_message(&bc, "en", &[("count", &i64::MAX.to_string())], &mut out);
        // This might fail due to f64 precision loss when i64::MAX is cast to f64
        // It's a known limitation, but should not panic
        let _ = result;
    }

    #[test]
    fn adversarial_writer_error_propagated() {
        struct FailWriter;
        impl core::fmt::Write for FailWriter {
            fn write_str(&mut self, _: &str) -> core::fmt::Result {
                Err(core::fmt::Error)
            }
        }

        let bc = make_text("hello");
        let mut w = FailWriter;
        let result = format_message(&bc, "en", &[], &mut w);
        assert!(result.is_err(), "Writer error should be propagated");
    }

    #[test]
    fn adversarial_writer_error_mid_message() {
        struct FailAfterFirst;
        static CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        impl core::fmt::Write for FailAfterFirst {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                if CALLED.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(core::fmt::Error);
                }
                CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(c) = s.chars().next() {
                    write!(&mut String::new(), "{}", c)
                } else {
                    Ok(())
                }
            }
        }

        // Two text segments to trigger TWO write_str calls
        let mut bc = make_text("hello");
        bc.extend_from_slice(&make_text(" world"));
        let result = format_message(&bc, "en", &[], &mut FailAfterFirst);
        assert!(result.is_err(), "second write_str should return error");
    }

    #[test]
    fn adversarial_writer_error_on_empty_text() {
        struct FailAfterFirst;
        static CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        impl core::fmt::Write for FailAfterFirst {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                if CALLED.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(core::fmt::Error);
                }
                CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(c) = s.chars().next() {
                    write!(&mut String::new(), "{}", c)
                } else {
                    Ok(())
                }
            }
        }

        let bc = make_text("");
        let result = format_message(&bc, "en", &[], &mut FailAfterFirst);
        assert!(
            result.is_ok(),
            "empty text with single write should succeed"
        );
    }

    #[test]
    fn adversarial_number_with_nan_param() {
        let mut bc = Vec::new();
        bc.push(0x05);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x00); // Decimal
        let mut out = String::new();
        let result = format_message(&bc, "en", &[("num", "NaN")], &mut out);
        assert!(result.is_ok(), "NaN param should not crash (becomes 0.0)");
        assert_eq!(out, "0");
    }

    #[test]
    fn adversarial_number_with_inf_param() {
        let mut bc = Vec::new();
        bc.push(0x05);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x00);
        let mut out = String::new();
        let result = format_message(&bc, "en", &[("num", "inf")], &mut out);
        assert!(result.is_ok(), "inf param should not crash");
    }

    #[test]
    fn adversarial_unterminated_multibyte_utf8() {
        // Text with unterminated multi-byte UTF-8 sequence
        let mut bc = vec![0x01];
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(&[0xE2, 0x82]); // incomplete 3-byte sequence
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err(), "Incomplete UTF-8 should error");
    }

    #[test]
    fn adversarial_select_matches_first_not_later() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&3u16.to_be_bytes());
        // case "male" -> "FIRST"
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"male");
        let first_pat = make_text("FIRST");
        bc.extend_from_slice(&(first_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&first_pat);
        // case "male" (duplicate) -> "SECOND"
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"male");
        let second_pat = make_text("SECOND");
        bc.extend_from_slice(&(second_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&second_pat);
        // other
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"other");
        let other_pat = make_text("OTHER");
        bc.extend_from_slice(&(other_pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other_pat);

        let mut out = String::new();
        format_message(&bc, "en", &[("gender", "male")], &mut out).unwrap();
        assert_eq!(out, "FIRST", "Duplicate select keys should match first");
    }

    #[test]
    fn adversarial_truncated_var_body() {
        let bc = vec![0x02, 0x00, 0x00, 0x00, 0x0A, 0x00];
        let mut out = String::new();
        let result = format_message(&bc, "en", &[], &mut out);
        assert!(result.is_err(), "truncated var name body should error");
    }

    #[test]
    fn adversarial_ordinal_all_categories() {
        let mut bc = Vec::new();
        bc.push(0x0A);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&6u16.to_be_bytes());
        let cases: Vec<(u8, f64)> =
            vec![(1, 1.0), (2, 0.0), (3, 0.0), (4, 0.0), (5, 0.0), (6, 0.0)];
        for (case_type, val) in cases {
            bc.push(case_type);
            if case_type == 1 {
                bc.extend_from_slice(&val.to_be_bytes());
            }
            let pat = make_text("x");
            bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
            bc.extend_from_slice(&pat);
        }
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(
            r.is_ok(),
            "ordinal with all category types should not error"
        );
    }

    #[test]
    fn adversarial_truncated_plural_var_len() {
        let mut bc = vec![0x03];
        bc.extend_from_slice(&5u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        // Missing the num_cases u16
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err());
    }

    #[test]
    fn adversarial_reltime_all_styles() {
        for style_byte in [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07] {
            let mut bc = Vec::new();
            bc.push(0x08);
            bc.extend_from_slice(&3u32.to_be_bytes());
            bc.extend_from_slice(b"num");
            bc.push(style_byte);
            let mut out = String::new();
            let r = format_message(&bc, "en", &[("num", "3600")], &mut out);
            assert!(
                r.is_ok(),
                "reltime style_byte 0x{:02X} should work",
                style_byte
            );
        }
    }

    #[test]
    fn adversarial_custom_formatter_with_options() {
        register_formatter(
            "opts_test",
            Box::new(|value, _locale, opts| {
                let prefix = opts.get("pfx").map(String::as_str).unwrap_or("");
                let suffix = opts.get("sfx").map(String::as_str).unwrap_or("");
                format!("{}{}{}", prefix, value, suffix)
            }),
        );
        let mut bc = Vec::new();
        bc.push(0x0D);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&9u32.to_be_bytes());
        bc.extend_from_slice(b"opts_test");
        bc.extend_from_slice(&11u32.to_be_bytes());
        bc.extend_from_slice(b"pfx=<,sfx=>");
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "hello")], &mut out);
        assert!(r.is_ok(), "custom formatter with options should work");
        assert_eq!(out, "<hello>");
    }

    #[test]
    fn adversarial_custom_formatter_options_split() {
        register_formatter(
            "opts_split",
            Box::new(|_value, _locale, _opts| String::new()),
        );
        let mut bc = Vec::new();
        bc.push(0x0D);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&10u32.to_be_bytes());
        bc.extend_from_slice(b"opts_split");
        bc.extend_from_slice(&7u32.to_be_bytes());
        bc.extend_from_slice(b"a=b,c=d");
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "x")], &mut out);
        assert!(r.is_ok(), "options with multiple key=value should parse");
    }

    #[test]
    fn adversarial_custom_formatter_no_options() {
        register_formatter("no_opts", Box::new(|_value, _locale, _opts| String::new()));
        let mut bc = Vec::new();
        bc.push(0x0D);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&7u32.to_be_bytes());
        bc.extend_from_slice(b"no_opts");
        bc.extend_from_slice(&0u32.to_be_bytes());
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "x")], &mut out);
        assert!(r.is_ok(), "custom formatter with no options should work");
    }

    #[test]
    fn adversarial_escaped_var_missing_param() {
        let bc = make_escaped_var("missing", true);
        let mut out = String::new();
        format_message(&bc, "en", &[], &mut out).unwrap();
        assert_eq!(out, "{missing}", "missing escaped var should show braces");
    }

    #[test]
    fn adversarial_select_truncated_key_len() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&2u16.to_be_bytes());
        bc.extend_from_slice(&4u32.to_be_bytes()); // key_len = 4
        bc.extend_from_slice(b"mal"); // only 3 bytes instead of 4
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_err(), "truncated select key should error");
    }

    #[test]
    fn adversarial_select_truncated_key_body() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&2u16.to_be_bytes());
        bc.extend_from_slice(&10u32.to_be_bytes()); // key_len = 10
        bc.extend_from_slice(b"male"); // only 4 bytes
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_err(), "select key body truncated should error");
    }

    #[test]
    fn adversarial_select_truncated_pat_len() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.extend_from_slice(&4u32.to_be_bytes());
        bc.extend_from_slice(b"male");
        bc.extend_from_slice(&0u32.to_be_bytes()); // pat_len = 0
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_ok(), "select with empty pattern should work");
    }

    #[test]
    fn adversarial_select_truncated_var_len() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&100u32.to_be_bytes()); // var_len = 100
        bc.extend_from_slice(b"x");
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_err(), "truncated select var name should error");
    }

    #[test]
    fn adversarial_plural_truncated_case_type() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&1u16.to_be_bytes());
        // No case_type byte
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err(), "truncated plural case type should error");
    }

    #[test]
    fn adversarial_plural_truncated_f64_val() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x01); // exact match type, expects 8 bytes f64
        bc.extend_from_slice(&[0; 7]); // only 7 bytes
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err(), "truncated plural f64 value should error");
    }

    #[test]
    fn adversarial_plural_truncated_pat_len() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x00); // other type
                       // No pat_len bytes
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err(), "truncated plural pat_len should error");
    }

    #[test]
    fn adversarial_plural_truncated_pat_body() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.push(0x00); // other type
        bc.extend_from_slice(&100u32.to_be_bytes()); // pat_len = 100
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err(), "truncated plural pat body should error");
    }

    #[test]
    fn adversarial_plural_exact_match_value() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&2u16.to_be_bytes());
        // exact match case: =1
        bc.push(0x01);
        bc.extend_from_slice(&1.0f64.to_be_bytes());
        let pat = make_text("one");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // other case
        bc.push(0x00);
        let other = make_text("other");
        bc.extend_from_slice(&(other.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other);
        let mut out = String::new();
        format_message(&bc, "en", &[("num", "1")], &mut out).unwrap();
        assert_eq!(out, "one", "exact match value 1 should hit");
    }

    #[test]
    fn adversarial_plural_category_zero_and_many() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&3u16.to_be_bytes());
        // zero case (not typical for en, but tests coverage)
        bc.push(0x02);
        let pat = make_text("zero");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // one case
        bc.push(0x03);
        let pat = make_text("one");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // other
        bc.push(0x00);
        let other = make_text("other");
        bc.extend_from_slice(&(other.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other);
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_ok(), "plural with zero/one/other should work");
        assert_eq!(out, "one", "value 1 should select 'one' category");
    }

    #[test]
    fn adversarial_plural_two_few_many_categories() {
        let mut bc = Vec::new();
        bc.push(0x03);
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.extend_from_slice(&4u16.to_be_bytes());
        // two (en: not used for plurals, hits unc path)
        bc.push(0x04);
        let pat = make_text("two");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // few (en: not used for plurals, hits unc path)
        bc.push(0x05);
        let pat = make_text("few");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // many (en: not used for plurals, hits unc path)
        bc.push(0x06);
        let pat = make_text("many");
        bc.extend_from_slice(&(pat.len() as u32).to_be_bytes());
        bc.extend_from_slice(&pat);
        // other
        bc.push(0x00);
        let other = make_text("other");
        bc.extend_from_slice(&(other.len() as u32).to_be_bytes());
        bc.extend_from_slice(&other);
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "5")], &mut out);
        assert!(r.is_ok(), "plural with two/few/many/other should work");
        assert_eq!(out, "other", "value 5 should select 'other'");
    }

    #[test]
    fn adversarial_plural_truncated_var_len_header() {
        let bc = vec![0x03, 0x00];
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(
            r.is_err(),
            "plural with truncated var_len header should error"
        );
    }

    #[test]
    fn adversarial_plural_truncated_num_cases() {
        let mut bc = vec![0x03];
        bc.extend_from_slice(&3u32.to_be_bytes());
        bc.extend_from_slice(b"num");
        bc.push(0x00);
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("num", "1")], &mut out);
        assert!(r.is_err(), "plural with truncated num_cases should error");
    }

    #[test]
    fn adversarial_select_truncated_var_len_header() {
        let bc = vec![0x04];
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_err(), "select with just opcode should error");
    }

    #[test]
    fn adversarial_select_truncated_num_cases() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&6u32.to_be_bytes());
        bc.extend_from_slice(b"gender");
        bc.push(0x00);
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(r.is_err(), "select with truncated num_cases should error");
    }

    #[test]
    fn adversarial_select_truncated_key_len_field() {
        let bc = vec![
            0x04, 0x00, 0x00, 0x00, 0x01, b'x', 0x00, 0x01, 0x00, 0x00, 0x00,
        ];
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "male")], &mut out);
        assert!(
            r.is_err(),
            "select with truncated key_len field should error"
        );
    }

    #[test]
    fn adversarial_select_truncated_pat_len_field() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&1u32.to_be_bytes());
        bc.extend_from_slice(b"x");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.extend_from_slice(&1u32.to_be_bytes());
        bc.extend_from_slice(b"x");
        // No pat_len bytes
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "x")], &mut out);
        assert!(r.is_err(), "select with truncated pat_len should error");
    }

    #[test]
    fn adversarial_select_truncated_pat_body() {
        let mut bc = Vec::new();
        bc.push(0x04);
        bc.extend_from_slice(&1u32.to_be_bytes());
        bc.extend_from_slice(b"x");
        bc.extend_from_slice(&1u16.to_be_bytes());
        bc.extend_from_slice(&1u32.to_be_bytes());
        bc.extend_from_slice(b"x");
        bc.extend_from_slice(&100u32.to_be_bytes()); // pat_len = 100
        let mut out = String::new();
        let r = format_message(&bc, "en", &[("gender", "x")], &mut out);
        assert!(r.is_err(), "select with truncated pat body should error");
    }
}

#[cfg(all(test, feature = "std"))]
mod custom_formatter_tests {
    use super::*;

    #[test]
    fn register_and_call_formatter() {
        register_formatter(
            "reverse",
            Box::new(|value, _locale, _opts| value.chars().rev().collect()),
        );
        let result = format_with_custom("reverse", "hello", "en", &HashMap::new());
        assert_eq!(result, Some("olleh".to_string()));
    }

    #[test]
    fn unregistered_formatter_returns_none() {
        let result = format_with_custom("nonexistent", "test", "en", &HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn raw_text_is_written_directly() {
        let mut out = String::new();
        format_message(b"Save", "en", &[], &mut out).unwrap();
        assert_eq!(out, "Save");
    }

    #[test]
    fn raw_text_with_newlines() {
        let mut out = String::new();
        format_message(b"Line1\nLine2", "en", &[], &mut out).unwrap();
        assert_eq!(out, "Line1\nLine2");
    }

    #[test]
    fn raw_text_empty_returns_ok_no_output() {
        let mut out = String::new();
        assert!(format_message(b"", "en", &[], &mut out).is_ok());
        assert!(out.is_empty());
    }

    #[test]
    fn opcode_text_still_works_after_optimization() {
        let mut out = String::new();
        let bytes = b"\x01\x00\x00\x00\x05Hello";
        format_message(bytes, "en", &[], &mut out).unwrap();
        assert_eq!(out, "Hello");
    }

    #[test]
    fn opcode_with_params_unchanged_by_optimization() {
        let mut out = String::new();
        let bytes = b"\x0B\x00\x00\x00\x04name\x00";
        format_message(bytes, "en", &[("name", "World")], &mut out).unwrap();
        assert_eq!(out, "World");
    }

    #[test]
    fn formatter_receives_options() {
        register_formatter(
            "wrap",
            Box::new(|value, _locale, opts| {
                let prefix = opts.get("prefix").map(|s| s.as_str()).unwrap_or("");
                let suffix = opts.get("suffix").map(|s| s.as_str()).unwrap_or("");
                format!("{}{}{}", prefix, value, suffix)
            }),
        );
        let mut opts = HashMap::new();
        opts.insert("prefix".to_string(), "<".to_string());
        opts.insert("suffix".to_string(), ">".to_string());
        let result = format_with_custom("wrap", "hello", "en", &opts);
        assert_eq!(result, Some("<hello>".to_string()));
    }
}
