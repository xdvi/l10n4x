extern crate alloc;
use alloc::string::String;

/// Relative time formatting style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelTimeStyle {
    /// Automatically select the best unit based on magnitude.
    Auto,
    /// Format in seconds.
    Seconds,
    /// Format in minutes.
    Minutes,
    /// Format in hours.
    Hours,
    /// Format in days.
    Days,
    /// Format in weeks.
    Weeks,
    /// Format in months.
    Months,
    /// Format in years.
    Years,
}

fn select_unit(seconds: i64) -> RelTimeStyle {
    let abs = seconds.unsigned_abs();
    if abs < 60 {
        RelTimeStyle::Seconds
    } else if abs < 3600 {
        RelTimeStyle::Minutes
    } else if abs < 86400 {
        RelTimeStyle::Hours
    } else if abs < 604800 {
        RelTimeStyle::Days
    } else if abs < 2629744 {
        RelTimeStyle::Weeks
    } else if abs < 31556926 {
        RelTimeStyle::Months
    } else {
        RelTimeStyle::Years
    }
}

fn r(s: &str) -> String {
    String::from(s)
}

fn format_reltime_en(value: i64, style: RelTimeStyle) -> String {
    let abs = value.unsigned_abs();
    let future = value >= 0;
    match style {
        RelTimeStyle::Seconds => {
            if abs == 0 {
                r("just now")
            } else if future {
                alloc::format!("in {} seconds", abs)
            } else {
                alloc::format!("{} seconds ago", abs)
            }
        }
        RelTimeStyle::Minutes => {
            let n = if abs == 1 {
                r("1 minute")
            } else {
                alloc::format!("{} minutes", abs)
            };
            if future {
                alloc::format!("in {}", n)
            } else {
                alloc::format!("{} ago", n)
            }
        }
        RelTimeStyle::Hours => {
            let n = if abs == 1 {
                r("1 hour")
            } else {
                alloc::format!("{} hours", abs)
            };
            if future {
                alloc::format!("in {}", n)
            } else {
                alloc::format!("{} ago", n)
            }
        }
        RelTimeStyle::Days => match (abs, future) {
            (1, true) => r("tomorrow"),
            (1, false) => r("yesterday"),
            (n, true) => alloc::format!("in {} days", n),
            (n, false) => alloc::format!("{} days ago", n),
        },
        RelTimeStyle::Weeks => match (abs, future) {
            (1, true) => r("next week"),
            (1, false) => r("last week"),
            (n, true) => alloc::format!("in {} weeks", n),
            (n, false) => alloc::format!("{} weeks ago", n),
        },
        RelTimeStyle::Months => match (abs, future) {
            (1, true) => r("next month"),
            (1, false) => r("last month"),
            (n, true) => alloc::format!("in {} months", n),
            (n, false) => alloc::format!("{} months ago", n),
        },
        RelTimeStyle::Years => match (abs, future) {
            (1, true) => r("next year"),
            (1, false) => r("last year"),
            (n, true) => alloc::format!("in {} years", n),
            (n, false) => alloc::format!("{} years ago", n),
        },
        RelTimeStyle::Auto => unreachable!(),
    }
}

fn format_reltime_es(value: i64, style: RelTimeStyle) -> String {
    let abs = value.unsigned_abs();
    let future = value >= 0;
    match style {
        RelTimeStyle::Seconds => {
            if abs == 0 {
                r("ahora mismo")
            } else if future {
                alloc::format!("en {} segundos", abs)
            } else {
                alloc::format!("hace {} segundos", abs)
            }
        }
        RelTimeStyle::Minutes => {
            let n = if abs == 1 {
                r("1 minuto")
            } else {
                alloc::format!("{} minutos", abs)
            };
            if future {
                alloc::format!("en {}", n)
            } else {
                alloc::format!("hace {}", n)
            }
        }
        RelTimeStyle::Hours => {
            let n = if abs == 1 {
                r("1 hora")
            } else {
                alloc::format!("{} horas", abs)
            };
            if future {
                alloc::format!("en {}", n)
            } else {
                alloc::format!("hace {}", n)
            }
        }
        RelTimeStyle::Days => match (abs, future) {
            (1, true) => r("mañana"),
            (1, false) => r("ayer"),
            (n, true) => alloc::format!("en {} días", n),
            (n, false) => alloc::format!("hace {} días", n),
        },
        RelTimeStyle::Weeks => match (abs, future) {
            (1, true) => r("la próxima semana"),
            (1, false) => r("la semana pasada"),
            (n, true) => alloc::format!("en {} semanas", n),
            (n, false) => alloc::format!("hace {} semanas", n),
        },
        RelTimeStyle::Months => match (abs, future) {
            (1, true) => r("el próximo mes"),
            (1, false) => r("el mes pasado"),
            (n, true) => alloc::format!("en {} meses", n),
            (n, false) => alloc::format!("hace {} meses", n),
        },
        RelTimeStyle::Years => match (abs, future) {
            (1, true) => r("el próximo año"),
            (1, false) => r("el año pasado"),
            (n, true) => alloc::format!("en {} años", n),
            (n, false) => alloc::format!("hace {} años", n),
        },
        RelTimeStyle::Auto => unreachable!(),
    }
}

fn convert_to_unit(seconds: i64, style: RelTimeStyle) -> i64 {
    match style {
        RelTimeStyle::Auto | RelTimeStyle::Seconds => seconds,
        RelTimeStyle::Minutes => seconds / 60,
        RelTimeStyle::Hours => seconds / 3600,
        RelTimeStyle::Days => seconds / 86400,
        RelTimeStyle::Weeks => seconds / 604800,
        RelTimeStyle::Months => seconds / 2629744,
        RelTimeStyle::Years => seconds / 31556926,
    }
}

/// Formats a time delta (in seconds) as a relative time string (e.g. "3 days ago").
/// `value` is the delta in seconds (negative = past, positive = future, 0 = now).
pub fn format_relative_time(value: i64, locale: &str, style: RelTimeStyle) -> String {
    let actual_style = if style == RelTimeStyle::Auto {
        select_unit(value)
    } else {
        style
    };
    let adjusted_value = convert_to_unit(value, actual_style);

    let lang = locale.split(['-', '_']).next().unwrap_or("en");
    match lang.to_lowercase().as_str() {
        "es" => format_reltime_es(adjusted_value, actual_style),
        _ => format_reltime_en(adjusted_value, actual_style),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_just_now() {
        assert_eq!(
            format_relative_time(0, "en", RelTimeStyle::Seconds),
            "just now"
        );
    }

    #[test]
    fn english_seconds_ago() {
        assert_eq!(
            format_relative_time(-30, "en", RelTimeStyle::Seconds),
            "30 seconds ago"
        );
    }

    #[test]
    fn english_in_hours() {
        assert_eq!(
            format_relative_time(7200, "en", RelTimeStyle::Hours),
            "in 2 hours"
        );
    }

    #[test]
    fn english_yesterday() {
        assert_eq!(
            format_relative_time(-86400, "en", RelTimeStyle::Days),
            "yesterday"
        );
    }

    #[test]
    fn english_tomorrow() {
        assert_eq!(
            format_relative_time(86400, "en", RelTimeStyle::Days),
            "tomorrow"
        );
    }

    #[test]
    fn english_auto_seconds() {
        assert_eq!(
            format_relative_time(45, "en", RelTimeStyle::Auto),
            "in 45 seconds"
        );
    }

    #[test]
    fn english_auto_minutes() {
        assert_eq!(
            format_relative_time(180, "en", RelTimeStyle::Auto),
            "in 3 minutes"
        );
    }

    #[test]
    fn spanish_ahora() {
        assert_eq!(
            format_relative_time(0, "es", RelTimeStyle::Seconds),
            "ahora mismo"
        );
    }

    #[test]
    fn spanish_ayer() {
        assert_eq!(
            format_relative_time(-86400, "es", RelTimeStyle::Days),
            "ayer"
        );
    }

    #[test]
    fn spanish_future_days() {
        assert_eq!(
            format_relative_time(172800, "es", RelTimeStyle::Auto),
            "en 2 días"
        );
    }

    #[test]
    fn english_auto_hours() {
        assert_eq!(
            format_relative_time(3600, "en", RelTimeStyle::Auto),
            "in 1 hour"
        );
    }

    #[test]
    fn english_auto_days() {
        assert_eq!(
            format_relative_time(172800, "en", RelTimeStyle::Auto),
            "in 2 days"
        );
    }

    #[test]
    fn english_auto_weeks() {
        assert_eq!(
            format_relative_time(604800, "en", RelTimeStyle::Auto),
            "next week"
        );
    }

    #[test]
    fn english_auto_months() {
        assert_eq!(
            format_relative_time(2629744, "en", RelTimeStyle::Auto),
            "next month"
        );
    }

    #[test]
    fn english_auto_years() {
        assert_eq!(
            format_relative_time(31556926, "en", RelTimeStyle::Auto),
            "next year"
        );
    }

    #[test]
    fn english_past_seconds() {
        assert_eq!(
            format_relative_time(-5, "en", RelTimeStyle::Seconds),
            "5 seconds ago"
        );
    }

    #[test]
    fn english_past_minutes() {
        assert_eq!(
            format_relative_time(-120, "en", RelTimeStyle::Minutes),
            "2 minutes ago"
        );
    }

    #[test]
    fn english_past_hours() {
        assert_eq!(
            format_relative_time(-3600, "en", RelTimeStyle::Hours),
            "1 hour ago"
        );
    }

    #[test]
    fn english_past_days_multi() {
        assert_eq!(
            format_relative_time(-172800, "en", RelTimeStyle::Days),
            "2 days ago"
        );
    }

    #[test]
    fn english_past_weeks() {
        assert_eq!(
            format_relative_time(-1209600, "en", RelTimeStyle::Weeks),
            "2 weeks ago"
        );
    }

    #[test]
    fn english_past_months() {
        assert_eq!(
            format_relative_time(-5259488, "en", RelTimeStyle::Months),
            "2 months ago"
        );
    }

    #[test]
    fn english_past_years() {
        assert_eq!(
            format_relative_time(-315569260, "en", RelTimeStyle::Years),
            "10 years ago"
        );
    }

    #[test]
    fn english_in_minutes() {
        assert_eq!(
            format_relative_time(300, "en", RelTimeStyle::Minutes),
            "in 5 minutes"
        );
    }

    #[test]
    fn english_in_weeks() {
        assert_eq!(
            format_relative_time(1209600, "en", RelTimeStyle::Weeks),
            "in 2 weeks"
        );
    }

    #[test]
    fn english_in_months() {
        assert_eq!(
            format_relative_time(5259488, "en", RelTimeStyle::Months),
            "in 2 months"
        );
    }

    #[test]
    fn english_in_years() {
        assert_eq!(
            format_relative_time(315569260, "en", RelTimeStyle::Years),
            "in 10 years"
        );
    }

    #[test]
    fn spanish_past_hours() {
        assert_eq!(
            format_relative_time(-7200, "es", RelTimeStyle::Hours),
            "hace 2 horas"
        );
    }

    #[test]
    fn spanish_future_weeks() {
        assert_eq!(
            format_relative_time(1209600, "es", RelTimeStyle::Weeks),
            "en 2 semanas"
        );
    }

    #[test]
    fn spanish_past_months() {
        assert_eq!(
            format_relative_time(-5259488, "es", RelTimeStyle::Months),
            "hace 2 meses"
        );
    }

    #[test]
    fn spanish_future_years() {
        assert_eq!(
            format_relative_time(31556926, "es", RelTimeStyle::Years),
            "el próximo año"
        );
    }

    #[test]
    fn english_singular_minute() {
        assert_eq!(
            format_relative_time(60, "en", RelTimeStyle::Minutes),
            "in 1 minute"
        );
    }

    #[test]
    fn english_singular_hour() {
        assert_eq!(
            format_relative_time(-3600, "en", RelTimeStyle::Hours),
            "1 hour ago"
        );
    }

    #[test]
    fn spanish_singular_hour() {
        assert_eq!(
            format_relative_time(3600, "es", RelTimeStyle::Hours),
            "en 1 hora"
        );
    }

    #[test]
    fn english_next_week() {
        assert_eq!(
            format_relative_time(604800, "en", RelTimeStyle::Weeks),
            "next week"
        );
    }

    #[test]
    fn english_last_week() {
        assert_eq!(
            format_relative_time(-604800, "en", RelTimeStyle::Weeks),
            "last week"
        );
    }

    #[test]
    fn english_next_month() {
        assert_eq!(
            format_relative_time(2629744, "en", RelTimeStyle::Months),
            "next month"
        );
    }

    #[test]
    fn english_last_month() {
        assert_eq!(
            format_relative_time(-2629744, "en", RelTimeStyle::Months),
            "last month"
        );
    }

    #[test]
    fn english_next_year() {
        assert_eq!(
            format_relative_time(31556926, "en", RelTimeStyle::Years),
            "next year"
        );
    }

    #[test]
    fn english_last_year() {
        assert_eq!(
            format_relative_time(-31556926, "en", RelTimeStyle::Years),
            "last year"
        );
    }

    #[test]
    fn spanish_next_week() {
        assert_eq!(
            format_relative_time(604800, "es", RelTimeStyle::Weeks),
            "la próxima semana"
        );
    }

    #[test]
    fn spanish_last_week() {
        assert_eq!(
            format_relative_time(-604800, "es", RelTimeStyle::Weeks),
            "la semana pasada"
        );
    }

    #[test]
    fn spanish_next_month() {
        assert_eq!(
            format_relative_time(2629744, "es", RelTimeStyle::Months),
            "el próximo mes"
        );
    }

    #[test]
    fn spanish_last_month() {
        assert_eq!(
            format_relative_time(-2629744, "es", RelTimeStyle::Months),
            "el mes pasado"
        );
    }

    #[test]
    fn locale_with_variant() {
        assert_eq!(
            format_relative_time(0, "en-US", RelTimeStyle::Seconds),
            "just now"
        );
        assert_eq!(
            format_relative_time(0, "es_AR", RelTimeStyle::Seconds),
            "ahora mismo"
        );
    }

    #[test]
    fn select_unit_seconds() {
        assert_eq!(select_unit(0), RelTimeStyle::Seconds);
        assert_eq!(select_unit(59), RelTimeStyle::Seconds);
    }

    #[test]
    fn select_unit_minutes() {
        assert_eq!(select_unit(60), RelTimeStyle::Minutes);
        assert_eq!(select_unit(3599), RelTimeStyle::Minutes);
    }

    #[test]
    fn select_unit_hours() {
        assert_eq!(select_unit(3600), RelTimeStyle::Hours);
        assert_eq!(select_unit(86399), RelTimeStyle::Hours);
    }

    #[test]
    fn select_unit_days() {
        assert_eq!(select_unit(86400), RelTimeStyle::Days);
        assert_eq!(select_unit(604799), RelTimeStyle::Days);
    }

    #[test]
    fn select_unit_weeks() {
        assert_eq!(select_unit(604800), RelTimeStyle::Weeks);
        assert_eq!(select_unit(2629743), RelTimeStyle::Weeks);
    }

    #[test]
    fn select_unit_months() {
        assert_eq!(select_unit(2629744), RelTimeStyle::Months);
        assert_eq!(select_unit(31556925), RelTimeStyle::Months);
    }

    #[test]
    fn select_unit_years() {
        assert_eq!(select_unit(31556926), RelTimeStyle::Years);
        assert_eq!(select_unit(i64::MAX), RelTimeStyle::Years);
    }

    #[test]
    fn convert_seconds_to_unit() {
        assert_eq!(convert_to_unit(120, RelTimeStyle::Minutes), 2);
        assert_eq!(convert_to_unit(7200, RelTimeStyle::Hours), 2);
        assert_eq!(convert_to_unit(172800, RelTimeStyle::Days), 2);
        assert_eq!(convert_to_unit(1209600, RelTimeStyle::Weeks), 2);
        assert_eq!(convert_to_unit(5259488, RelTimeStyle::Months), 2);
        assert_eq!(convert_to_unit(31556926, RelTimeStyle::Years), 1);
    }
}
