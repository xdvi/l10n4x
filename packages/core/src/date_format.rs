//! Locale-aware date and time formatting from ISO 8601 strings or Unix timestamps.
extern crate alloc;
use alloc::string::String;

/// Date formatting style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    /// Date only (locale-aware ordering and separators).
    Date,
    /// Time only (HH:MM, 24-hour).
    Time,
    /// Date and time combined.
    DateTime,
}

/// Parsed date parts.
struct DateParts {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
}

/// Parses "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS" ISO 8601 strings.
fn parse_iso(s: &str) -> Option<DateParts> {
    let s = s.trim();
    let date_part = s.split('T').next()?;
    let mut parts = date_part.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;

    let (hour, minute) = if let Some(time_part) = s.split('T').nth(1) {
        let mut tp = time_part.split(':');
        let h: u8 = tp.next()?.parse().ok()?;
        let m: u8 = tp.next()?.parse().ok()?;
        (h, m)
    } else {
        (0, 0)
    };

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(DateParts {
        year,
        month,
        day,
        hour,
        minute,
    })
}

/// Converts a Unix timestamp (integer seconds since 1970-01-01 UTC) to date parts.
fn unix_to_parts(secs: i64) -> DateParts {
    let days = (secs / 86400) as i32;
    let time_of_day = secs.rem_euclid(86400) as u32;
    let hour = (time_of_day / 3600) as u8;
    let minute = ((time_of_day % 3600) / 60) as u8;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 {
        (mp + 3) as u8
    } else {
        (mp - 9) as u8
    };
    let year = if m <= 2 { y + 1 } else { y };

    DateParts {
        year,
        month: m,
        day: d,
        hour,
        minute,
    }
}

/// Parses either an ISO 8601 string or an integer Unix timestamp string.
fn parse_value(s: &str) -> Option<DateParts> {
    if let Ok(ts) = s.trim().parse::<i64>() {
        return Some(unix_to_parts(ts));
    }
    parse_iso(s)
}

/// Formats a date string with locale-aware pattern.
/// `value` can be a Unix timestamp (integer string) or ISO 8601 date/datetime string.
pub fn format_date(value: &str, locale: &str, style: DateStyle) -> String {
    let parts = match parse_value(value) {
        Some(p) => p,
        None => return alloc::string::String::from(value),
    };

    let lang = locale.split(['-', '_']).next().unwrap_or(locale);

    let date_str = match lang.to_lowercase().as_str() {
        "en" | "zh" | "ko" | "fil" | "he" | "iw" => {
            alloc::format!("{:02}/{:02}/{}", parts.month, parts.day, parts.year)
        }
        "ja" => alloc::format!("{}年{:02}月{:02}日", parts.year, parts.month, parts.day),
        "de" | "at" | "cs" | "sk" | "pl" | "hu" | "ru" | "uk" | "be" | "bg" | "sr" | "hr"
        | "bs" | "mk" | "ro" | "sl" | "lv" | "lt" | "et" | "el" | "tr" | "ka" | "hy" | "az"
        | "kk" | "ky" | "uz" | "tk" | "mn" => {
            alloc::format!("{:02}.{:02}.{}", parts.day, parts.month, parts.year)
        }
        "fr" | "es" | "pt" | "it" | "nl" | "da" | "sv" | "nb" | "fi" | "af" | "ca" | "gl"
        | "eu" | "ar" | "fa" | "ur" | "hi" | "bn" | "pa" | "gu" | "mr" | "ta" | "te" | "kn"
        | "ml" | "si" | "my" | "km" | "lo" | "th" | "vi" | "id" | "ms" | "sw" => {
            alloc::format!("{:02}/{:02}/{}", parts.day, parts.month, parts.year)
        }
        _ => alloc::format!("{}-{:02}-{:02}", parts.year, parts.month, parts.day),
    };

    let time_str = alloc::format!("{:02}:{:02}", parts.hour, parts.minute);

    match style {
        DateStyle::Date => date_str,
        DateStyle::Time => time_str,
        DateStyle::DateTime => alloc::format!("{}, {}", date_str, time_str),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_english() {
        assert_eq!(
            format_date("2024-12-25", "en", DateStyle::Date),
            "12/25/2024"
        );
    }

    #[test]
    fn iso_date_german() {
        assert_eq!(
            format_date("2024-12-25", "de", DateStyle::Date),
            "25.12.2024"
        );
    }

    #[test]
    fn iso_date_japanese() {
        assert_eq!(
            format_date("2024-12-25", "ja", DateStyle::Date),
            "2024年12月25日"
        );
    }

    #[test]
    fn iso_date_french() {
        assert_eq!(
            format_date("2024-12-25", "fr", DateStyle::Date),
            "25/12/2024"
        );
    }

    #[test]
    fn time_style() {
        assert_eq!(
            format_date("2024-12-25T14:30:00", "en", DateStyle::Time),
            "14:30"
        );
    }

    #[test]
    fn datetime_style_english() {
        assert_eq!(
            format_date("2024-12-25T14:30:00", "en", DateStyle::DateTime),
            "12/25/2024, 14:30"
        );
    }

    #[test]
    fn unix_timestamp_integer() {
        assert_eq!(
            format_date("1735138200", "en", DateStyle::Date),
            "12/25/2024"
        );
    }

    #[test]
    fn invalid_value_returns_raw_string() {
        assert_eq!(
            format_date("not-a-date", "en", DateStyle::Date),
            "not-a-date"
        );
    }

    #[test]
    fn german_locale_format() {
        assert_eq!(
            format_date("2024-03-15", "de", DateStyle::Date),
            "15.03.2024"
        );
    }

    #[test]
    fn fallback_locale_format() {
        assert_eq!(
            format_date("2024-03-15", "xx", DateStyle::Date),
            "2024-03-15"
        );
    }

    #[test]
    fn iso_date_with_whitespace() {
        assert_eq!(
            format_date("  2024-12-25  ", "en", DateStyle::Date),
            "12/25/2024"
        );
    }

    #[test]
    fn unix_timestamp_negative() {
        let result = format_date("-86400", "en", DateStyle::Date);
        assert!(!result.is_empty());
    }

    #[test]
    fn parse_value_valid_int_timestamp() {
        let parts = parse_value("1735138200");
        assert!(parts.is_some());
    }
}
