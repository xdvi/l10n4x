//! Locale-aware number formatting using a built-in CLDR separator table.
//! No external dependencies required.
extern crate alloc;
use alloc::string::String;

/// Number formatting style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberStyle {
    /// Locale decimal and grouping separators (default).
    Decimal,
    /// Multiply by 100 and append "%" symbol.
    Percent,
    /// Truncate to integer, apply grouping.
    Integer,
}

/// Returns (decimal_sep, grouping_sep) for the given locale language tag.
fn separators(locale: &str) -> (char, char) {
    let lang = locale.split(['-', '_']).next().unwrap_or(locale);
    match lang.to_lowercase().as_str() {
        // Period decimal, comma grouping (English-like)
        "en" | "zh" | "ja" | "ko" | "th" | "vi" | "hi" | "ms" | "fil" | "sw"
        | "af" | "my" | "km" | "lo" | "bn" | "gu" | "kn" | "ml" | "mr"
        | "ne" | "or" | "pa" | "si" | "ta" | "te" | "ur" => ('.', ','),
        // Comma decimal, period grouping (Continental European)
        "de" | "nl" | "da" | "sv" | "nb" | "fi" | "et" | "lv" | "lt" | "hu"
        | "hr" | "sr" | "bs" | "mk" | "sq" | "sk" | "cs" | "pl" | "ro"
        | "bg" | "ru" | "uk" | "be" | "ka" | "hy" | "az" | "kk" | "ky"
        | "uz" | "tk" | "mn" | "id" => (',', '.'),
        // Comma decimal, space grouping (French-like)
        "fr" | "es" | "it" | "pt" | "ca" | "gl" | "eu" | "eo" | "tr" | "ar"
        | "he" | "fa" | "el" => (',', '\u{202f}'),
        _ => ('.', ','),
    }
}

/// Formats `value` with locale-aware number representation.
pub fn format_number(value: f64, locale: &str, style: NumberStyle) -> String {
    let (decimal_sep, group_sep) = separators(locale);

    let (n, suffix) = match style {
        NumberStyle::Percent => (value * 100.0, "%"),
        NumberStyle::Decimal | NumberStyle::Integer => (value, ""),
    };

    let abs_n = if n < 0.0 { -n } else { n };
    let sign = if n < 0.0 { "-" } else { "" };
    let int_part = abs_n as u64;
    let frac = abs_n - (int_part as f64);

    // Format integer part with grouping separator (every 3 digits)
    let int_str = {
        let digits = alloc::format!("{}", int_part);
        let mut grouped = String::new();
        for (i, ch) in digits.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                grouped.push(group_sep);
            }
            grouped.push(ch);
        }
        grouped.chars().rev().collect::<String>()
    };

    // Format fractional part (max 2 decimal places, strip trailing zeros)
    let frac_str = if matches!(style, NumberStyle::Integer) || frac < 1e-9 {
        String::new()
    } else {
        let rounded = (frac * 100.0).round() as u64;
        if rounded == 0 {
            String::new()
        } else if rounded % 10 == 0 {
            alloc::format!("{}{}", decimal_sep, rounded / 10)
        } else {
            alloc::format!("{}{:02}", decimal_sep, rounded)
        }
    };

    alloc::format!("{}{}{}{}", sign, int_str, frac_str, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_decimal_separator() {
        assert_eq!(format_number(1234.56, "en", NumberStyle::Decimal), "1,234.56");
        assert_eq!(format_number(1000.0, "en", NumberStyle::Decimal), "1,000");
        assert_eq!(format_number(0.5, "en", NumberStyle::Decimal), "0.5");
    }

    #[test]
    fn german_decimal_separator() {
        assert_eq!(format_number(1234.56, "de", NumberStyle::Decimal), "1.234,56");
        assert_eq!(format_number(1000.0, "de", NumberStyle::Decimal), "1.000");
    }

    #[test]
    fn percent_style() {
        assert_eq!(format_number(0.75, "en", NumberStyle::Percent), "75%");
        assert_eq!(format_number(1.0, "en", NumberStyle::Percent), "100%");
    }

    #[test]
    fn integer_style_truncates_fraction() {
        assert_eq!(format_number(3.14, "en", NumberStyle::Integer), "3");
        assert_eq!(format_number(1234.9, "en", NumberStyle::Integer), "1,234");
    }

    #[test]
    fn small_number_no_grouping() {
        assert_eq!(format_number(42.0, "en", NumberStyle::Decimal), "42");
        assert_eq!(format_number(999.0, "en", NumberStyle::Decimal), "999");
    }
}
