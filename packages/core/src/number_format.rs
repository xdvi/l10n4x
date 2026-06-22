//! Locale-aware number formatting using a built-in CLDR separator table.
//! No external dependencies required.
extern crate alloc;
use alloc::string::String;

/// Number formatting style.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberStyle {
    /// Locale decimal and grouping separators (default).
    Decimal,
    /// Multiply by 100 and append "%" symbol.
    Percent,
    /// Truncate to integer, apply grouping.
    Integer,
    /// Currency — prepends/appends the locale-appropriate symbol.
    Currency(alloc::string::String),
}

/// Returns (decimal_sep, grouping_sep) for the given locale language tag.
fn separators(locale: &str) -> (char, char) {
    let lang = crate::locale_util::lang_subtag(locale);
    if crate::locale_util::lang_matches_any(
        lang,
        &[
            "en", "zh", "ja", "ko", "th", "vi", "hi", "ms", "fil", "sw", "af", "my", "km", "lo",
            "bn", "gu", "kn", "ml", "mr", "ne", "or", "pa", "si", "ta", "te", "ur",
        ],
    ) {
        ('.', ',')
    } else if crate::locale_util::lang_matches_any(
        lang,
        &[
            "de", "nl", "da", "sv", "nb", "fi", "et", "lv", "lt", "hu", "hr", "sr", "bs", "mk",
            "sq", "sk", "cs", "pl", "ro", "bg", "ru", "uk", "be", "ka", "hy", "az", "kk", "ky",
            "uz", "tk", "mn", "id",
        ],
    ) {
        (',', '.')
    } else if crate::locale_util::lang_matches_any(
        lang,
        &[
            "fr", "es", "it", "pt", "ca", "gl", "eu", "eo", "tr", "ar", "he", "fa", "el",
        ],
    ) {
        (',', '\u{202f}')
    } else {
        ('.', ',')
    }
}

#[inline]
fn currency_eq(code: &str, tag: &str) -> bool {
    code.eq_ignore_ascii_case(tag)
}

/// Returns `(symbol, is_prefix)` where `prefix=true` means the symbol precedes the amount.
fn currency_symbol(currency_code: &str, locale: &str) -> (&'static str, bool) {
    let lang = crate::locale_util::lang_subtag(locale);
    if currency_eq(currency_code, "USD") {
        ("$", true)
    } else if currency_eq(currency_code, "CAD") {
        ("CA$", true)
    } else if currency_eq(currency_code, "AUD") {
        ("A$", true)
    } else if currency_eq(currency_code, "GBP") {
        ("£", true)
    } else if currency_eq(currency_code, "JPY") || currency_eq(currency_code, "CNY") {
        ("¥", true)
    } else if currency_eq(currency_code, "KRW") {
        ("₩", true)
    } else if currency_eq(currency_code, "INR") {
        ("₹", true)
    } else if currency_eq(currency_code, "BRL") {
        ("R$", true)
    } else if currency_eq(currency_code, "MXN") {
        ("MX$", true)
    } else if currency_eq(currency_code, "CHF") {
        ("Fr", true)
    } else if currency_eq(currency_code, "SEK")
        || currency_eq(currency_code, "NOK")
        || currency_eq(currency_code, "DKK")
    {
        ("kr", false)
    } else if currency_eq(currency_code, "EUR") {
        if crate::locale_util::lang_matches_any(lang, &["de", "nl", "fi", "et", "lv", "lt"]) {
            ("€", false)
        } else {
            ("€", true)
        }
    } else if currency_eq(currency_code, "RUB") {
        ("₽", false)
    } else if currency_eq(currency_code, "PLN") {
        ("zł", false)
    } else if currency_eq(currency_code, "CZK") {
        ("Kč", false)
    } else if currency_eq(currency_code, "HUF") {
        ("Ft", false)
    } else if currency_eq(currency_code, "RON") {
        ("lei", false)
    } else if currency_eq(currency_code, "TRY") {
        ("₺", true)
    } else if currency_eq(currency_code, "ILS") {
        ("₪", true)
    } else if currency_eq(currency_code, "SAR") {
        ("﷼", true)
    } else if currency_eq(currency_code, "AED") {
        ("د.إ", true)
    } else if currency_eq(currency_code, "THB") {
        ("฿", true)
    } else if currency_eq(currency_code, "SGD") {
        ("S$", true)
    } else if currency_eq(currency_code, "HKD") {
        ("HK$", true)
    } else if currency_eq(currency_code, "NZD") {
        ("NZ$", true)
    } else if currency_eq(currency_code, "ZAR") {
        ("R", true)
    } else {
        ("$", true)
    }
}

/// Formats `value` as a locale-aware currency amount.
/// `currency_code` is an ISO 4217 code (e.g., `"USD"`, `"EUR"`).
pub fn format_currency(value: f64, locale: &str, currency_code: &str) -> String {
    let (symbol, is_prefix) = currency_symbol(currency_code, locale);
    let (decimal_sep, group_sep) = separators(locale);

    let no_decimal = crate::locale_util::lang_matches_any(
        currency_code,
        &["JPY", "KRW", "VND", "CLP", "IDR", "HUF", "RWF", "UGX"],
    );

    let abs_val = if value < 0.0 { -value } else { value };
    let sign = if value < 0.0 { "-" } else { "" };

    let int_part = if no_decimal {
        (abs_val + 0.5) as u64
    } else {
        abs_val as u64
    };

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

    let formatted = if no_decimal {
        alloc::format!("{}{}", sign, int_str)
    } else {
        let cents = (((abs_val * 100.0) + 0.5) as u64) % 100;
        alloc::format!("{}{}{}{:02}", sign, int_str, decimal_sep, cents)
    };

    if is_prefix {
        alloc::format!("{}{}", symbol, formatted)
    } else {
        alloc::format!("{} {}", formatted, symbol)
    }
}

/// Formats `value` with locale-aware number representation.
pub fn format_number(value: f64, locale: &str, style: NumberStyle) -> String {
    let (decimal_sep, group_sep) = separators(locale);

    let (n, suffix) = match style {
        NumberStyle::Percent => (value * 100.0, "%"),
        NumberStyle::Decimal | NumberStyle::Integer | NumberStyle::Currency(_) => (value, ""),
    };

    let sign = if n < 0.0 { "-" } else { "" };

    // Round to 2 decimal places using integer arithmetic (works in no_std)
    let total_hundredths = ((n.abs() * 100.0) + 0.5) as u64;
    let int_part = total_hundredths / 100;
    let frac_part = total_hundredths % 100;

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
    let frac_str =
        if matches!(style, NumberStyle::Integer | NumberStyle::Currency(_)) || frac_part == 0 {
            String::new()
        } else if frac_part % 10 == 0 {
            alloc::format!("{}{}", decimal_sep, frac_part / 10)
        } else {
            alloc::format!("{}{:02}", decimal_sep, frac_part)
        };

    alloc::format!("{}{}{}{}", sign, int_str, frac_str, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_decimal_separator() {
        assert_eq!(
            format_number(1234.56, "en", NumberStyle::Decimal),
            "1,234.56"
        );
        assert_eq!(format_number(1000.0, "en", NumberStyle::Decimal), "1,000");
        assert_eq!(format_number(0.5, "en", NumberStyle::Decimal), "0.5");
    }

    #[test]
    fn german_decimal_separator() {
        assert_eq!(
            format_number(1234.56, "de", NumberStyle::Decimal),
            "1.234,56"
        );
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

    #[test]
    fn currency_usd() {
        let s = format_currency(1234.56, "en", "USD");
        assert_eq!(s, "$1,234.56");
    }

    #[test]
    fn currency_eur_prefix() {
        let s = format_currency(99.95, "fr", "EUR");
        assert_eq!(s, "\u{20ac}99,95");
    }

    #[test]
    fn currency_eur_suffix() {
        let s = format_currency(99.95, "de", "EUR");
        assert_eq!(s, "99,95 €");
    }

    #[test]
    fn currency_jpy_no_decimal() {
        let s = format_currency(1000.0, "en", "JPY");
        assert_eq!(s, "¥1,000");
    }

    #[test]
    fn currency_negative() {
        let s = format_currency(-50.0, "en", "USD");
        assert_eq!(s, "$-50.00");
    }

    #[test]
    fn currency_gbp() {
        let s = format_currency(42.0, "en", "GBP");
        assert_eq!(s, "£42.00");
    }

    #[test]
    fn currency_cny() {
        let s = format_currency(999.99, "zh", "CNY");
        assert_eq!(s, "¥999.99");
    }

    #[test]
    fn currency_sek_suffix() {
        let s = format_currency(200.0, "sv", "SEK");
        assert_eq!(s, "200,00 kr");
    }

    #[test]
    fn currency_default_symbol() {
        let s = format_currency(10.0, "en", "XYZ");
        assert_eq!(s, "$10.00");
    }

    #[test]
    fn negative_number() {
        assert_eq!(
            format_number(-1234.56, "en", NumberStyle::Decimal),
            "-1,234.56"
        );
    }

    #[test]
    fn percent_multiplies() {
        assert_eq!(format_number(0.5, "en", NumberStyle::Percent), "50%");
        assert_eq!(format_number(0.0, "en", NumberStyle::Percent), "0%");
    }

    #[test]
    fn integer_with_large_value() {
        assert_eq!(
            format_number(1234567.89, "en", NumberStyle::Integer),
            "1,234,567"
        );
    }

    #[test]
    fn french_locale() {
        assert_eq!(
            format_number(1234.56, "fr", NumberStyle::Decimal),
            "1\u{202f}234,56"
        );
    }

    #[test]
    fn spanish_locale() {
        assert_eq!(
            format_number(1234.56, "es", NumberStyle::Decimal),
            "1\u{202f}234,56"
        );
    }

    #[test]
    fn number_rounding_two_decimals() {
        assert_eq!(format_number(3.14159, "en", NumberStyle::Decimal), "3.14");
    }

    #[test]
    fn number_no_fractional() {
        assert_eq!(format_number(100.0, "en", NumberStyle::Decimal), "100");
    }

    #[test]
    fn number_truncates_fraction() {
        assert_eq!(format_number(0.001, "en", NumberStyle::Decimal), "0");
    }
}
