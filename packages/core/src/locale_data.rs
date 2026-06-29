//! Versioned locale data metadata and locale property helpers (CLDR-lite).

/// CLDR data revision embedded in this runtime build.
/// Bump when plural, number, date, or RTL tables change incompatibly.
pub const LOCALE_DATA_VERSION: u32 = 1;

/// Maximum `locale_data_version` this runtime understands.
pub const SUPPORTED_LOCALE_DATA_VERSION: u32 = LOCALE_DATA_VERSION;

/// Reserved param name for IANA timezone or fixed offset (`America/Bogota`, `+05:30`, `UTC`).
pub const TIMEZONE_PARAM: &str = "tz";

const RTL_LANGS: &[&str] = &[
    "ar", "fa", "he", "iw", "ur", "ps", "sd", "ug", "yi", "ckb", "dv",
];

/// Returns true when the locale uses right-to-left script by default.
pub fn is_rtl_locale(locale: &str) -> bool {
    let lang = crate::locale_util::lang_subtag(locale);
    RTL_LANGS
        .iter()
        .any(|tag| crate::locale_util::lang_eq(lang, tag))
}

/// Resolves a timezone identifier to offset minutes east of UTC.
/// Supports `UTC`/`Z`, fixed offsets (`+05:30`, `-05:00`), and a built-in IANA subset.
pub fn resolve_timezone_offset_minutes(tz: &str) -> Option<i32> {
    let tz = tz.trim();
    if tz.is_empty() || tz.eq_ignore_ascii_case("UTC") || tz == "Z" {
        return Some(0);
    }
    if let Some(off) = parse_fixed_offset_minutes(tz) {
        return Some(off);
    }
    TZ_OFFSETS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(tz))
        .map(|(_, minutes)| *minutes)
}

fn parse_fixed_offset_minutes(tz: &str) -> Option<i32> {
    let bytes = tz.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &tz[1..];
    let (hours, minutes) = if let Some((h, m)) = rest.split_once(':') {
        (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?)
    } else if rest.len() >= 2 {
        (rest.parse::<i32>().ok()?, 0)
    } else {
        return None;
    };
    if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

/// Standard (non-DST) offsets for common IANA zones.
const TZ_OFFSETS: &[(&str, i32)] = &[
    ("America/Bogota", -300),
    ("America/Mexico_City", -360),
    ("America/New_York", -300),
    ("America/Chicago", -360),
    ("America/Denver", -420),
    ("America/Los_Angeles", -480),
    ("America/Sao_Paulo", -180),
    ("Europe/London", 0),
    ("Europe/Paris", 60),
    ("Europe/Berlin", 60),
    ("Europe/Moscow", 180),
    ("Asia/Dubai", 240),
    ("Asia/Kolkata", 330),
    ("Asia/Tokyo", 540),
    ("Asia/Shanghai", 480),
    ("Australia/Sydney", 600),
    ("Pacific/Auckland", 720),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_arabic_hebrew() {
        assert!(is_rtl_locale("ar"));
        assert!(is_rtl_locale("ar-SA"));
        assert!(is_rtl_locale("he-IL"));
        assert!(!is_rtl_locale("en-US"));
    }

    #[test]
    fn timezone_utc_and_fixed() {
        assert_eq!(resolve_timezone_offset_minutes("UTC"), Some(0));
        assert_eq!(resolve_timezone_offset_minutes("+05:30"), Some(330));
        assert_eq!(resolve_timezone_offset_minutes("-05:00"), Some(-300));
    }

    #[test]
    fn timezone_iana_subset() {
        assert_eq!(
            resolve_timezone_offset_minutes("America/Bogota"),
            Some(-300)
        );
    }
}
