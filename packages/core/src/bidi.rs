//! Explicit bidirectional text isolates for mixed RTL/LTR message output.

use crate::locale_data::is_rtl_locale;

const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';

fn is_ltr_strong(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '/' | ':' | '-' | '_' | '#')
}

/// Wraps left-to-right runs with first-strong isolates when the locale is RTL.
pub fn format_segment(locale: &str, segment: &str) -> alloc::string::String {
    if !is_rtl_locale(locale) || !segment.chars().any(is_ltr_strong) {
        return alloc::string::String::from(segment);
    }
    let mut out = alloc::string::String::new();
    let mut in_ltr = false;
    for c in segment.chars() {
        let strong = is_ltr_strong(c);
        if strong && !in_ltr {
            out.push(FSI);
            in_ltr = true;
        } else if !strong && in_ltr {
            out.push(PDI);
            in_ltr = false;
        }
        out.push(c);
    }
    if in_ltr {
        out.push(PDI);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltr_locale_unchanged() {
        assert_eq!(format_segment("en", "Hello 42"), "Hello 42");
    }

    #[test]
    fn rtl_isolates_embedded_latin() {
        let got = format_segment("ar", "رسائل 42");
        assert!(got.contains(FSI));
        assert!(got.contains(PDI));
        assert!(got.contains('4'));
    }
}
