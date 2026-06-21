//! CLDR plural rules — covers 12 language families and ~120 locales.
//! Source: https://unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html
extern crate alloc;

pub use crate::formatter::PluralCategory;

/// Parse operands from a numeric value.
/// n = absolute value, i = integer part, v = visible fraction digit count.
struct Ops {
    n: f64,
    i: u64,
    v: usize,
}

impl Ops {
    fn from(val: f64) -> Self {
        let n = if val < 0.0 { -val } else { val };
        let i = n as u64;
        let frac = n - (i as f64);
        let v = if frac < 1e-9 {
            0
        } else {
            let mut count = 0usize;
            let mut tmp = frac;
            for _ in 1..=6 {
                tmp *= 10.0;
                count += 1;
                tmp -= (tmp as u64) as f64;
                if tmp < 1e-9 {
                    break;
                }
            }
            count
        };
        Ops { n, i, v }
    }
}

/// Returns the CLDR plural category for a given locale tag and numeric value.
/// Locale matching is done on the first two characters (language subtag) in lowercase.
pub fn get_plural_category(locale: &str, value: f64) -> PluralCategory {
    let ops = Ops::from(value);
    let lang = locale
        .split(['-', '_'])
        .next()
        .unwrap_or(locale);

    match lang.to_lowercase().as_str() {
        // ── Family 0: invariable — always Other ──────────────────────────────
        // Japanese, Korean, Chinese, Vietnamese, Thai, Burmese, Indonesian, Khmer, Malay, Lao
        "ja" | "ko" | "zh" | "vi" | "th" | "my" | "id" | "km" | "ms" | "lo"
        | "bo" | "dz" | "ig" | "ii" | "jv" | "kde" | "kea" | "nqo" | "ses"
        | "sg" | "wo" | "yo" | "yue" => PluralCategory::Other,

        // ── Family 1: one/other (n = 1) ───────────────────────────────────────
        "af" | "az" | "bg" | "bn" | "ca" | "da" | "de" | "el" | "eo" | "es"
        | "et" | "eu" | "fi" | "fy" | "gl" | "gu" | "hu" | "it" | "kk"
        | "ky" | "lb" | "mn" | "mr" | "ne" | "nl" | "or" | "pa" | "rm"
        | "sq" | "sw" | "ta" | "te" | "tk" | "tr" | "ug" | "ur" | "uz" => {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── English: one if i=1 and v=0 ──────────────────────────────────────
        "en" => {
            if ops.i == 1 && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Portuguese: one if n=1 ───────────────────────────────────────────
        "pt" => {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 2: French-style — one if i=0 or i=1 ───────────────────────
        "fr" | "ff" | "hy" | "kab" => {
            if (ops.i == 0 || ops.i == 1) && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 3: Slavic 3-way (Russian/Ukrainian/Belarusian) ────────────
        "ru" | "uk" | "be" => {
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

        // ── Family 4: South Slavic (Serbian/Croatian/Bosnian) ────────────────
        "hr" | "sr" | "bs" | "sh" => {
            let i10 = ops.i % 10;
            let i100 = ops.i % 100;
            if ops.v == 0 {
                if i10 == 1 && i100 != 11 {
                    PluralCategory::One
                } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            } else {
                let f = (ops.n * 10.0) as u64 % 10;
                let f100 = (ops.n * 100.0) as u64 % 100;
                if f == 1 && f100 != 11 {
                    PluralCategory::One
                } else if (2..=4).contains(&f) && !(12..=14).contains(&f100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
        }

        // ── Family 5: Polish ─────────────────────────────────────────────────
        "pl" => {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else if ops.v == 0 {
                let i10 = ops.i % 10;
                let i100 = ops.i % 100;
                if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                    PluralCategory::Few
                } else if i10 == 0
                    || i10 == 1
                    || (5..=9).contains(&i10)
                    || (12..=14).contains(&i100)
                {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 6: Czech / Slovak ─────────────────────────────────────────
        "cs" | "sk" => {
            if (ops.n - 1.0).abs() < 1e-9 && ops.v == 0 {
                PluralCategory::One
            } else if ops.n >= 2.0 && ops.n <= 4.0 && ops.v == 0 {
                PluralCategory::Few
            } else if ops.v != 0 {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 7: Arabic — 6 forms ────────────────────────────────────────
        "ar" | "ckb" => {
            if ops.v == 0 {
                let n100 = ops.i % 100;
                if ops.i == 0 {
                    PluralCategory::Zero
                } else if ops.i == 1 {
                    PluralCategory::One
                } else if ops.i == 2 {
                    PluralCategory::Two
                } else if (3..=10).contains(&n100) {
                    PluralCategory::Few
                } else if (11..=99).contains(&n100) {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 8: Latvian ────────────────────────────────────────────────
        "lv" | "prg" => {
            if ops.v == 0 {
                let i10 = ops.i % 10;
                let i100 = ops.i % 100;
                if ops.i == 0 || (11..=19).contains(&i100) {
                    PluralCategory::Zero
                } else if i10 == 1 && i100 != 11 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 9: Lithuanian ─────────────────────────────────────────────
        "lt" => {
            if ops.v != 0 {
                PluralCategory::Many
            } else {
                let i10 = ops.i % 10;
                let i100 = ops.i % 100;
                if i10 == 1 && !(11..=19).contains(&i100) {
                    PluralCategory::One
                } else if (2..=9).contains(&i10) && !(11..=19).contains(&i100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
        }

        // ── Family 10: Romanian ──────────────────────────────────────────────
        "ro" | "mo" => {
            if (ops.n - 1.0).abs() < 1e-9 && ops.v == 0 {
                PluralCategory::One
            } else if ops.v == 0 {
                let i100 = ops.i % 100;
                if ops.i == 0 || (1..=19).contains(&i100) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Few
            }
        }

        // ── Family 11: Hebrew ────────────────────────────────────────────────
        "he" | "iw" => {
            if ops.v == 0 {
                if ops.i == 1 {
                    PluralCategory::One
                } else if ops.i == 2 {
                    PluralCategory::Two
                } else if ops.i >= 10 && ops.i % 10 == 0 {
                    PluralCategory::Many
                } else {
                    PluralCategory::Other
                }
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 12: Macedonian ─────────────────────────────────────────────
        "mk" => {
            if ops.v == 0 && (ops.i % 10 == 1 || ops.i == 11) {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Default fallback: n=1 → One, else Other ──────────────────────────
        _ => {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_six_forms() {
        assert_eq!(get_plural_category("ar", 0.0), PluralCategory::Zero);
        assert_eq!(get_plural_category("ar", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("ar", 2.0), PluralCategory::Two);
        assert_eq!(get_plural_category("ar", 5.0), PluralCategory::Few);
        assert_eq!(get_plural_category("ar", 15.0), PluralCategory::Many);
        assert_eq!(get_plural_category("ar", 100.0), PluralCategory::Other);
    }

    #[test]
    fn polish_four_forms() {
        assert_eq!(get_plural_category("pl", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("pl", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("pl", 5.0), PluralCategory::Many);
        assert_eq!(get_plural_category("pl", 1.5), PluralCategory::Other);
    }

    #[test]
    fn east_asian_always_other() {
        for lang in &["ja", "ko", "zh", "vi", "th", "my", "id", "km", "ms"] {
            assert_eq!(
                get_plural_category(lang, 1.0),
                PluralCategory::Other,
                "lang={} expected Other for n=1",
                lang
            );
        }
    }

    #[test]
    fn russian_three_forms() {
        assert_eq!(get_plural_category("ru", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("ru", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("ru", 5.0), PluralCategory::Many);
        assert_eq!(get_plural_category("ru", 11.0), PluralCategory::Many);
        assert_eq!(get_plural_category("ru", 21.0), PluralCategory::One);
    }

    #[test]
    fn czech_four_forms() {
        assert_eq!(get_plural_category("cs", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("cs", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("cs", 5.0), PluralCategory::Other);
        assert_eq!(get_plural_category("cs", 1.5), PluralCategory::Many);
    }

    #[test]
    fn latvian_special() {
        assert_eq!(get_plural_category("lv", 0.0), PluralCategory::Zero);
        assert_eq!(get_plural_category("lv", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("lv", 2.0), PluralCategory::Other);
        assert_eq!(get_plural_category("lv", 11.0), PluralCategory::Zero);
        assert_eq!(get_plural_category("lv", 21.0), PluralCategory::One);
    }

    #[test]
    fn romanian_special() {
        assert_eq!(get_plural_category("ro", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("ro", 0.0), PluralCategory::Few);
        assert_eq!(get_plural_category("ro", 19.0), PluralCategory::Few);
        assert_eq!(get_plural_category("ro", 20.0), PluralCategory::Other);
    }

    #[test]
    fn french_zero_one() {
        assert_eq!(get_plural_category("fr", 0.0), PluralCategory::One);
        assert_eq!(get_plural_category("fr", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("fr", 2.0), PluralCategory::Other);
    }

    #[test]
    fn english_standard() {
        assert_eq!(get_plural_category("en", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("en", 2.0), PluralCategory::Other);
        assert_eq!(get_plural_category("en-US", 1.0), PluralCategory::One);
    }
}
