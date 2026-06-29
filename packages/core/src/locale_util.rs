//! Shared locale subtag helpers without heap allocation.

/// Returns the primary language subtag (`"en-US"` → `"en"`).
pub(crate) fn lang_subtag(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or(locale)
}

/// Case-insensitive ASCII comparison against a BCP-47 language tag.
pub(crate) fn lang_eq(lang: &str, tag: &str) -> bool {
    lang.eq_ignore_ascii_case(tag)
}

/// Returns `true` if `lang` matches any tag in `tags` (ASCII case-insensitive).
pub(crate) fn lang_matches_any(lang: &str, tags: &[&str]) -> bool {
    tags.iter().any(|tag| lang_eq(lang, tag))
}
