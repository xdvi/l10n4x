//! Explicit bidirectional text isolates for mixed RTL/LTR message output.

use crate::locale_data::is_rtl_locale;

const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';

fn is_ltr_strong(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '/' | ':' | '-' | '_' | '#')
}

/// Wraps left-to-right runs with first-strong isolates when the locale is RTL.
pub fn format_segment(locale: &str, segment: &str) -> alloc::string::String {
    if !needs_isolates(locale, segment) {
        return alloc::string::String::from(segment);
    }
    let mut out = alloc::string::String::with_capacity(segment.len() + 8);
    let _ = write_isolated(&mut out, segment);
    out
}

/// True when `segment` contains LTR runs that must be isolated for an RTL locale.
#[inline]
pub fn needs_isolates(locale: &str, segment: &str) -> bool {
    is_rtl_locale(locale) && segment.chars().any(is_ltr_strong)
}

/// Streams `segment` into `writer`, wrapping LTR runs in first-strong isolates.
/// Callers must have checked [`needs_isolates`]; plain LTR text should go
/// straight to `writer.write_str` without any allocation.
pub fn write_isolated<W: core::fmt::Write>(writer: &mut W, segment: &str) -> core::fmt::Result {
    let mut in_ltr = false;
    for c in segment.chars() {
        let strong = is_ltr_strong(c);
        if strong && !in_ltr {
            writer.write_char(FSI)?;
            in_ltr = true;
        } else if !strong && in_ltr {
            writer.write_char(PDI)?;
            in_ltr = false;
        }
        writer.write_char(c)?;
    }
    if in_ltr {
        writer.write_char(PDI)?;
    }
    Ok(())
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
