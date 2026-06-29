//! CLDR plural rules — covers 12 language families and ~120 locales.
//! Source: <https://unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html>
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

/// Returns the CLDR **ordinal** plural category for a given locale tag and integer value.
/// Supports ordinals like "1st", "2nd", "3rd" in English.
pub fn get_ordinal_category(locale: &str, value: i64) -> PluralCategory {
    let lang = crate::locale_util::lang_subtag(locale);

    match () {
        // ── English ordinal: special 1st, 2nd, 3rd, 11th-13th ─────────────────
        _ if crate::locale_util::lang_eq(lang, "en") => {
            let mod100 = value % 100;
            let mod10 = value % 10;
            if mod10 == 1 && mod100 != 11 {
                PluralCategory::One // 1st, 21st, 101st
            } else if mod10 == 2 && mod100 != 12 {
                PluralCategory::Two // 2nd, 22nd, 102nd
            } else if mod10 == 3 && mod100 != 13 {
                PluralCategory::Few // 3rd, 23rd, 103rd
            } else {
                PluralCategory::Other // 4th, 11th-13th, etc.
            }
        }
        // ── French: 1er, 2e ────────────────────────────────────────────────────
        _ if crate::locale_util::lang_matches_any(lang, &["fr", "ff", "kab"]) => {
            if value == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        // ── Spanish, Italian, Portuguese: default ordinal ─────────────────────
        _ if crate::locale_util::lang_matches_any(
            lang,
            &["es", "it", "pt", "ca", "gl", "eu", "eo", "ro", "mo"],
        ) =>
        {
            PluralCategory::Other
        }
        // ── German, Dutch, Swedish: always other ──────────────────────────────
        _ if crate::locale_util::lang_matches_any(
            lang,
            &[
                "de", "nl", "sv", "da", "nb", "fi", "et", "lv", "lt", "hu", "af", "sq", "sw", "tr",
                "az", "kk", "ky", "uz", "tk", "mn",
            ],
        ) =>
        {
            PluralCategory::Other
        }
        // ── Russian ordinals: one for 1, 2, 3, 4? Actually Russian ordinals
        //    follow same pattern as cardinals (1→One, 2-4→Few, 5+→Many, 11-14→Many)
        _ if crate::locale_util::lang_matches_any(lang, &["ru", "uk", "be"]) => {
            let mod100 = value % 100;
            let mod10 = value % 10;
            if mod10 == 1 && mod100 != 11 {
                PluralCategory::One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else {
                PluralCategory::Other
            }
        }
        // ── Arabic ordinals: always other ─────────────────────────────────────
        _ if crate::locale_util::lang_matches_any(lang, &["ar", "ckb"]) => PluralCategory::Other,
        // ── Chinese, Japanese, Korean, Vietnamese: always other ───────────────
        _ if crate::locale_util::lang_matches_any(
            lang,
            &["zh", "ja", "ko", "vi", "th", "my", "id", "km", "ms", "lo"],
        ) =>
        {
            PluralCategory::Other
        }
        // ── Default: n=1 → One, else Other ────────────────────────────────────
        _ => {
            if value == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

/// Returns the CLDR plural category for a given locale tag and numeric value.
/// Locale matching is done on the first two characters (language subtag) in lowercase.
pub fn get_plural_category(locale: &str, value: f64) -> PluralCategory {
    let ops = Ops::from(value);
    let lang = crate::locale_util::lang_subtag(locale);

    match () {
        // ── Family 0: invariable — always Other ──────────────────────────────
        // Japanese, Korean, Chinese, Vietnamese, Thai, Burmese, Indonesian, Khmer, Malay, Lao
        _ if crate::locale_util::lang_matches_any(
            lang,
            &[
                "ja", "ko", "zh", "vi", "th", "my", "id", "km", "ms", "lo", "bo", "dz", "ig", "ii",
                "jv", "kde", "kea", "nqo", "ses", "sg", "wo", "yo", "yue",
            ],
        ) =>
        {
            PluralCategory::Other
        }

        // ── Family 1: one/other (n = 1) ───────────────────────────────────────
        _ if crate::locale_util::lang_matches_any(
            lang,
            &[
                "af", "az", "bg", "bn", "ca", "da", "de", "el", "eo", "es", "et", "eu", "fi", "fy",
                "gl", "gu", "hu", "it", "kk", "ky", "lb", "mn", "mr", "ne", "nl", "or", "pa", "rm",
                "sq", "sw", "ta", "te", "tk", "tr", "ug", "ur", "uz",
            ],
        ) =>
        {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── English: one if i=1 and v=0 ──────────────────────────────────────
        _ if crate::locale_util::lang_eq(lang, "en") => {
            if ops.i == 1 && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Portuguese: one if n=1 ───────────────────────────────────────────
        _ if crate::locale_util::lang_eq(lang, "pt") => {
            if (ops.n - 1.0).abs() < 1e-9 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 2: French-style — one if i=0 or i=1 ───────────────────────
        _ if crate::locale_util::lang_matches_any(lang, &["fr", "ff", "hy", "kab"]) => {
            if (ops.i == 0 || ops.i == 1) && ops.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }

        // ── Family 3: Slavic 3-way (Russian/Ukrainian/Belarusian) ────────────
        _ if crate::locale_util::lang_matches_any(lang, &["ru", "uk", "be"]) => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["hr", "sr", "bs", "sh"]) => {
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
        _ if crate::locale_util::lang_eq(lang, "pl") => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["cs", "sk"]) => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["ar", "ckb"]) => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["lv", "prg"]) => {
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
        _ if crate::locale_util::lang_eq(lang, "lt") => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["ro", "mo"]) => {
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
        _ if crate::locale_util::lang_matches_any(lang, &["he", "iw"]) => {
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
        _ if crate::locale_util::lang_eq(lang, "mk") => {
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
mod ordinal_tests {
    use super::*;

    #[test]
    fn english_ordinal_one() {
        assert_eq!(get_ordinal_category("en", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("en", 21), PluralCategory::One);
        assert_eq!(get_ordinal_category("en", 101), PluralCategory::One);
    }

    #[test]
    fn english_ordinal_two() {
        assert_eq!(get_ordinal_category("en", 2), PluralCategory::Two);
        assert_eq!(get_ordinal_category("en", 22), PluralCategory::Two);
        assert_eq!(get_ordinal_category("en", 102), PluralCategory::Two);
    }

    #[test]
    fn english_ordinal_few() {
        assert_eq!(get_ordinal_category("en", 3), PluralCategory::Few);
        assert_eq!(get_ordinal_category("en", 23), PluralCategory::Few);
    }

    #[test]
    fn english_ordinal_other() {
        assert_eq!(get_ordinal_category("en", 4), PluralCategory::Other);
        assert_eq!(get_ordinal_category("en", 11), PluralCategory::Other);
        assert_eq!(get_ordinal_category("en", 12), PluralCategory::Other);
        assert_eq!(get_ordinal_category("en", 13), PluralCategory::Other);
        assert_eq!(get_ordinal_category("en", 100), PluralCategory::Other);
    }

    #[test]
    fn french_ordinal() {
        assert_eq!(get_ordinal_category("fr", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("fr", 2), PluralCategory::Other);
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

    #[test]
    fn negative_value() {
        assert_eq!(get_plural_category("en", -1.0), PluralCategory::One);
        assert_eq!(get_plural_category("en", -5.0), PluralCategory::Other);
    }

    #[test]
    fn large_number() {
        assert_eq!(get_plural_category("en", 9999999.0), PluralCategory::Other);
    }

    #[test]
    fn german_plural() {
        assert_eq!(get_plural_category("de", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("de", 2.0), PluralCategory::Other);
    }

    #[test]
    fn ukrainian_plural() {
        assert_eq!(get_plural_category("uk", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("uk", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("uk", 5.0), PluralCategory::Many);
    }

    #[test]
    fn unknown_locale_defaults_to_one_other() {
        assert_eq!(get_plural_category("zz", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("zz", 2.0), PluralCategory::Other);
    }

    #[test]
    fn ordinal_negative_falls_to_other() {
        assert_eq!(get_ordinal_category("en", -1), PluralCategory::Other);
        assert_eq!(get_ordinal_category("en", -3), PluralCategory::Other);
    }

    #[test]
    fn ordinal_spanish() {
        assert_eq!(get_ordinal_category("es", 1), PluralCategory::Other);
        assert_eq!(get_ordinal_category("es", 2), PluralCategory::Other);
    }

    #[test]
    fn ordinal_unknown_locale_defaults_to_one() {
        assert_eq!(get_ordinal_category("zz", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("zz", 2), PluralCategory::Other);
    }

    #[test]
    fn plural_fractional_values() {
        assert_eq!(get_plural_category("en", 1.5), PluralCategory::Other);
        assert_eq!(get_plural_category("pl", 0.5), PluralCategory::Other);
    }

    #[test]
    fn hebrew_plural() {
        assert_eq!(get_plural_category("he", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("he", 2.0), PluralCategory::Two);
        assert_eq!(get_plural_category("he", 10.0), PluralCategory::Many);
        assert_eq!(get_plural_category("he", 20.0), PluralCategory::Many);
        assert_eq!(get_plural_category("he", 3.0), PluralCategory::Other);
        assert_eq!(get_plural_category("he", 1.5), PluralCategory::Other);
    }

    #[test]
    fn macedonian_plural() {
        assert_eq!(get_plural_category("mk", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("mk", 11.0), PluralCategory::One);
        assert_eq!(get_plural_category("mk", 2.0), PluralCategory::Other);
        assert_eq!(get_plural_category("mk", 12.0), PluralCategory::Other);
    }

    #[test]
    fn lithuanian_plural() {
        assert_eq!(get_plural_category("lt", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("lt", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("lt", 9.0), PluralCategory::Few);
        assert_eq!(get_plural_category("lt", 10.0), PluralCategory::Other);
        assert_eq!(get_plural_category("lt", 20.0), PluralCategory::Other);
        assert_eq!(get_plural_category("lt", 21.0), PluralCategory::One);
        assert_eq!(get_plural_category("lt", 1.5), PluralCategory::Many);
    }

    #[test]
    fn south_slavic_plural() {
        assert_eq!(get_plural_category("hr", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("hr", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("hr", 5.0), PluralCategory::Other);
        assert_eq!(get_plural_category("hr", 21.0), PluralCategory::One);
        assert_eq!(get_plural_category("sr", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("bs", 1.0), PluralCategory::One);
    }

    #[test]
    fn south_slavic_fractional() {
        // v != 0 path uses fractional digits
        assert_eq!(get_plural_category("hr", 0.1), PluralCategory::One);
        assert_eq!(get_plural_category("hr", 0.2), PluralCategory::Few);
        assert_eq!(get_plural_category("hr", 0.5), PluralCategory::Other);
        assert_eq!(get_plural_category("hr", 1.2), PluralCategory::Few);
    }

    #[test]
    fn arabic_fractional_returns_other() {
        // ar with v != 0 always returns Other
        assert_eq!(get_plural_category("ar", 1.5), PluralCategory::Other);
        assert_eq!(get_plural_category("ar", 0.5), PluralCategory::Other);
    }

    #[test]
    fn romanian_fractional_always_few() {
        assert_eq!(get_plural_category("ro", 1.5), PluralCategory::Few);
        assert_eq!(get_plural_category("ro", 2.5), PluralCategory::Few);
    }

    #[test]
    fn polish_v_not_zero_path() {
        // pl with v != 0 -> Other (1.5%1 != 0 => v != 0 => Other)
        assert_eq!(get_plural_category("pl", 1.5), PluralCategory::Other);
        assert_eq!(get_plural_category("pl", 2.5), PluralCategory::Other);
    }

    #[test]
    fn portuguese_plural() {
        assert_eq!(get_plural_category("pt", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("pt", 2.0), PluralCategory::Other);
    }

    #[test]
    fn extra_east_asian_locales_always_other() {
        for lang in &[
            "bo", "dz", "ig", "ii", "jv", "kde", "kea", "nqo", "ses", "sg", "wo", "yo", "yue",
        ] {
            assert_eq!(get_plural_category(lang, 1.0), PluralCategory::Other);
            assert_eq!(get_plural_category(lang, 5.0), PluralCategory::Other);
        }
    }

    #[test]
    fn ordinal_russian() {
        assert_eq!(get_ordinal_category("ru", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("ru", 2), PluralCategory::Few);
        assert_eq!(get_ordinal_category("ru", 3), PluralCategory::Few);
        assert_eq!(get_ordinal_category("ru", 4), PluralCategory::Few);
        assert_eq!(get_ordinal_category("ru", 5), PluralCategory::Other);
        assert_eq!(get_ordinal_category("ru", 11), PluralCategory::Other);
        assert_eq!(get_ordinal_category("ru", 12), PluralCategory::Other);
        assert_eq!(get_ordinal_category("ru", 21), PluralCategory::One);
    }

    #[test]
    fn ordinal_germanic() {
        for lang in &["de", "nl", "sv", "da", "nb", "fi", "et", "lv", "lt", "hu"] {
            assert_eq!(get_ordinal_category(lang, 1), PluralCategory::Other);
            assert_eq!(get_ordinal_category(lang, 2), PluralCategory::Other);
        }
    }

    #[test]
    fn ordinal_arabic() {
        assert_eq!(get_ordinal_category("ar", 1), PluralCategory::Other);
        assert_eq!(get_ordinal_category("ckb", 1), PluralCategory::Other);
    }

    #[test]
    fn ordinal_east_asian() {
        for lang in &["zh", "ja", "ko", "vi", "th", "id"] {
            assert_eq!(get_ordinal_category(lang, 1), PluralCategory::Other);
        }
    }

    #[test]
    fn ordinal_french_ff_kab() {
        assert_eq!(get_ordinal_category("fr", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("ff", 1), PluralCategory::One);
        assert_eq!(get_ordinal_category("kab", 1), PluralCategory::One);
    }

    #[test]
    fn czech_range_few() {
        assert_eq!(get_plural_category("cs", 2.0), PluralCategory::Few);
        assert_eq!(get_plural_category("cs", 3.0), PluralCategory::Few);
        assert_eq!(get_plural_category("cs", 4.0), PluralCategory::Few);
    }

    #[test]
    fn latvian_fractional_other() {
        // lv with v != 0 -> Other
        assert_eq!(get_plural_category("lv", 1.5), PluralCategory::Other);
        assert_eq!(get_plural_category("lv", 2.5), PluralCategory::Other);
        assert_eq!(get_plural_category("lv", 11.5), PluralCategory::Other);
    }

    #[test]
    fn slovenian_default_one_other() {
        // sl falls to default: n=1 -> One, else Other
        assert_eq!(get_plural_category("sl", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("sl", 2.0), PluralCategory::Other);
    }

    #[test]
    fn welsh_default_one_other() {
        // cy falls to default: n=1 -> One, else Other
        assert_eq!(get_plural_category("cy", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("cy", 2.0), PluralCategory::Other);
    }

    #[test]
    fn maltese_default_one_other() {
        // mt falls to default: n=1 -> One, else Other
        assert_eq!(get_plural_category("mt", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("mt", 2.0), PluralCategory::Other);
    }

    #[test]
    fn scottish_gaelic_default_one_other() {
        // gd falls to default: n=1 -> One, else Other
        assert_eq!(get_plural_category("gd", 1.0), PluralCategory::One);
        assert_eq!(get_plural_category("gd", 2.0), PluralCategory::Other);
    }
}
