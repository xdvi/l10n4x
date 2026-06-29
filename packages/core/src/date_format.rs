//! Locale-aware date and time formatting from ISO 8601 strings or Unix timestamps.
extern crate alloc;
use crate::locale_data::resolve_timezone_offset_minutes;
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

/// Parses "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS[Z|±offset]" ISO 8601 strings to UTC seconds.
fn parse_to_utc_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (body, offset_minutes) = split_timezone_suffix(s)?;
    let date_part = body.split('T').next()?;
    let mut parts = date_part.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;

    let (hour, minute) = if let Some(time_part) = body.split('T').nth(1) {
        let time_part = time_part.trim_end_matches('Z').trim_end_matches('z');
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

    let local_secs = civil_to_unix(year, month, day, hour, minute)?;
    Some(local_secs - (offset_minutes as i64) * 60)
}

fn split_timezone_suffix(s: &str) -> Option<(&str, i32)> {
    if let Some(idx) = s.rfind('Z').or_else(|| s.rfind('z')) {
        if idx + 1 == s.len() {
            return Some((&s[..idx], 0));
        }
    }
    if let Some(idx) = s.rfind(['+', '-']) {
        if s[..idx].contains('T') {
            let (body, tz) = s.split_at(idx);
            let off = resolve_timezone_offset_minutes(tz)?;
            return Some((body, off));
        }
    }
    Some((s, 0))
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i32 {
    let mut y = year;
    if month <= 2 {
        y -= 1;
    }
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * u32::from(month_shift) + 2) / 5 + u32::from(day) - 1;
    let doe = yoe as i32 * 365 + yoe as i32 / 4 - yoe as i32 / 100 + doy as i32;
    era * 146097 + doe - 719468
}

fn civil_from_days(days: i32) -> (i32, u8, u8) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (yoe * 365 + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 {
        (mp + 3) as u8
    } else {
        (mp - 9) as u8
    };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

fn civil_to_unix(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Option<i64> {
    let days = days_from_civil(year, month, day);
    Some(i64::from(days) * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60)
}

/// Converts a Unix timestamp (integer seconds since 1970-01-01 UTC) to date parts in UTC.
fn unix_to_parts_utc(secs: i64) -> DateParts {
    let days = (secs / 86400) as i32;
    let time_of_day = secs.rem_euclid(86400) as u32;
    let hour = (time_of_day / 3600) as u8;
    let minute = ((time_of_day % 3600) / 60) as u8;
    let (year, month, day) = civil_from_days(days);

    DateParts {
        year,
        month,
        day,
        hour,
        minute,
    }
}

fn unix_to_parts_tz(secs: i64, tz_offset_minutes: i32) -> DateParts {
    unix_to_parts_utc(secs + (tz_offset_minutes as i64) * 60)
}

/// Parses either an ISO 8601 string or an integer Unix timestamp string.
fn parse_value_to_utc_secs(s: &str) -> Option<i64> {
    if let Ok(ts) = s.trim().parse::<i64>() {
        return Some(ts);
    }
    parse_to_utc_secs(s)
}

fn format_parts(parts: &DateParts, locale: &str, style: DateStyle) -> String {
    let lang = crate::locale_util::lang_subtag(locale);

    let date_str = match () {
        _ if crate::locale_util::lang_matches_any(lang, &["en", "zh", "ko", "fil", "he", "iw"]) => {
            alloc::format!("{:02}/{:02}/{}", parts.month, parts.day, parts.year)
        }
        _ if crate::locale_util::lang_eq(lang, "ja") => {
            alloc::format!("{}年{:02}月{:02}日", parts.year, parts.month, parts.day)
        }
        _ if crate::locale_util::lang_matches_any(
            lang,
            &[
                "de", "at", "cs", "sk", "pl", "hu", "ru", "uk", "be", "bg", "sr", "hr", "bs", "mk",
                "ro", "sl", "lv", "lt", "et", "el", "tr", "ka", "hy", "az", "kk", "ky", "uz", "tk",
                "mn",
            ],
        ) =>
        {
            alloc::format!("{:02}.{:02}.{}", parts.day, parts.month, parts.year)
        }
        _ if crate::locale_util::lang_matches_any(
            lang,
            &[
                "fr", "es", "pt", "it", "nl", "da", "sv", "nb", "fi", "af", "ca", "gl", "eu", "ar",
                "fa", "ur", "hi", "bn", "pa", "gu", "mr", "ta", "te", "kn", "ml", "si", "my", "km",
                "lo", "th", "vi", "id", "ms", "sw",
            ],
        ) =>
        {
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

/// Formats a date string with locale-aware pattern (UTC / embedded ISO offset).
pub fn format_date(value: &str, locale: &str, style: DateStyle) -> String {
    format_date_tz(value, locale, style, None)
}

/// Formats a date/time value in the given locale, optionally re-projecting UTC instants into `tz`.
pub fn format_date_tz(value: &str, locale: &str, style: DateStyle, tz: Option<&str>) -> String {
    let utc_secs = match parse_value_to_utc_secs(value) {
        Some(s) => s,
        None => return String::from(value),
    };
    let tz_offset = tz.and_then(resolve_timezone_offset_minutes).unwrap_or(0);
    let parts = unix_to_parts_tz(utc_secs, tz_offset);
    format_parts(&parts, locale, style)
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
        assert!(parse_value_to_utc_secs("1735138200").is_some());
    }

    #[test]
    fn utc_z_suffix_parsed() {
        let secs = parse_to_utc_secs("2024-12-25T15:00:00Z").unwrap();
        let parts = unix_to_parts_utc(secs);
        assert_eq!(parts.hour, 15);
    }

    #[test]
    fn timezone_param_shifts_display() {
        // 2024-12-25T15:00:00Z → 10:00 in America/Bogota (UTC-5)
        let got = format_date_tz(
            "2024-12-25T15:00:00Z",
            "en",
            DateStyle::Time,
            Some("America/Bogota"),
        );
        assert_eq!(got, "10:00");
    }
}
