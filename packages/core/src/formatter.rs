extern crate alloc;

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

/// Parse operands for plural rules:
/// n: absolute value
/// i: integer part
/// v: visible fraction digit count
#[derive(Debug, Clone)]
struct PluralOperands {
    n: f64,
    i: u64,
    v: usize,
}

impl PluralOperands {
    fn new(val: f64) -> Self {
        let n = if val < 0.0 { -val } else { val };
        let i = n as u64;

        let fraction = n - (i as f64);
        if fraction < 1e-9 {
            PluralOperands { n, i, v: 0 }
        } else {
            // Check up to 6 decimal places
            let mut v = 0;
            let mut temp = fraction;
            for _ in 1..=6 {
                temp *= 10.0;
                v += 1;
                temp = temp - ((temp as u64) as f64);
                if temp < 1e-9 {
                    break;
                }
            }
            PluralOperands { n, i, v }
        }
    }
}

/// Resolves the CLDR plural category for a given locale and numeric value.
/// Supports major languages like English, Spanish, French, German, Russian, etc.
pub fn get_plural_category(locale: &str, value: f64) -> PluralCategory {
    let ops = PluralOperands::new(value);

    // Normalize locale to lowercase two-letter code
    let lang = if locale.len() >= 2 {
        &locale[0..2]
    } else {
        locale
    };

    match lang {
        "en" => {
            // one: i = 1 and v = 0
            if ops.i == 1 && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        "es" | "it" | "pt" | "de" | "nl" | "sv" | "da" | "no" | "fi" => {
            // one: n = 1
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        "fr" => {
            // one: i = 0, 1 and v = 0
            if (ops.i == 0 || ops.i == 1) && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        "ru" | "uk" | "be" => {
            // one: v = 0 and i % 10 = 1 and i % 100 != 11
            // few: v = 0 and i % 10 in 2..4 and i % 100 not in 12..14
            // many: v = 0 and (i % 10 = 0 or i % 10 in 5..9 or i % 100 in 11..14)
            if ops.v == 0 {
                let i10 = ops.i % 10;
                let i100 = ops.i % 100;
                if i10 == 1 && i100 != 11 {
                    PluralCategory::One
                } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                    PluralCategory::Few
                } else if i10 == 0 || (5..=9).contains(&i10) || (11..=14).contains(&i100) {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Other
            }
        }
        _ => {
            // Default generic fallback: if n == 1, One, else Other
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
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
            _ => return Err(core::fmt::Error),
        }
    }
    Ok(())
}
