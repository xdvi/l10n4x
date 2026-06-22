extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// List formatting style (conjunction type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    /// Conjunction style ("A, B, and C").
    Conjunction,
    /// Disjunction style ("A, B, or C").
    Disjunction,
    /// Unit style ("A, B, C" — comma only).
    Unit,
}

/// Parse a JSON array of strings manually (no serde dependency in core).
fn parse_json_array(s: &str) -> Option<Vec<String>> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return None;
    }
    let inner = s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let mut result = Vec::new();
    let mut remaining = inner;
    while !remaining.is_empty() {
        remaining = remaining.trim();
        if remaining.starts_with('"') {
            let mut escaped = false;
            let mut value = String::new();
            let mut chars = remaining[1..].chars();
            for c in &mut chars {
                if escaped {
                    value.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    result.push(value);
                    remaining = chars.as_str();
                    break;
                } else {
                    value.push(c);
                }
            }
            remaining = remaining.trim_start();
            if remaining.starts_with(',') {
                remaining = &remaining[1..];
            }
        } else if remaining.starts_with('n') {
            // null
            if let Some(end) = remaining.find([',', ']']) {
                remaining = &remaining[end..];
                if remaining.starts_with(',') {
                    remaining = &remaining[1..];
                }
            } else {
                break;
            }
        } else {
            // number or other — skip to next comma
            if let Some(end) = remaining.find([',', ']']) {
                remaining = &remaining[end..];
                if remaining.starts_with(',') {
                    remaining = &remaining[1..];
                }
            } else {
                break;
            }
        }
    }
    Some(result)
}

fn format_list_en(items: &[String], style: ListStyle) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => {
            let conj = match style {
                ListStyle::Conjunction => " and ",
                ListStyle::Disjunction => " or ",
                ListStyle::Unit => ", ",
            };
            alloc::format!("{}{}{}", items[0], conj, items[1])
        }
        _ => {
            let conj = match style {
                ListStyle::Conjunction => ", and ",
                ListStyle::Disjunction => ", or ",
                ListStyle::Unit => ", ",
            };
            let head = items[..items.len() - 1].join(", ");
            alloc::format!("{}{}{}", head, conj, items[items.len() - 1])
        }
    }
}

fn format_list_es(items: &[String], style: ListStyle) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => {
            let conj = match style {
                ListStyle::Conjunction => " y ",
                ListStyle::Disjunction => " o ",
                ListStyle::Unit => ", ",
            };
            alloc::format!("{}{}{}", items[0], conj, items[1])
        }
        _ => {
            let head = items[..items.len() - 1].join(", ");
            let conj = match style {
                ListStyle::Conjunction => " y ",
                ListStyle::Disjunction => " o ",
                ListStyle::Unit => ", ",
            };
            alloc::format!("{}{}{}", head, conj, items[items.len() - 1])
        }
    }
}

/// Formats a JSON array of strings with locale-aware list conjunction.
/// `items_json` should be a JSON string like `["A","B","C"]`.
pub fn format_list(items_json: &str, locale: &str, style: ListStyle) -> String {
    let items = match parse_json_array(items_json) {
        Some(v) => v,
        None => return alloc::string::String::from(items_json),
    };

    let lang = crate::locale_util::lang_subtag(locale);
    if crate::locale_util::lang_eq(lang, "es") {
        format_list_es(&items, style)
    } else {
        format_list_en(&items, style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(items: &[&str]) -> String {
        let mut s = String::from("[");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push('"');
            s.push_str(item);
            s.push('"');
        }
        s.push(']');
        s
    }

    #[test]
    fn english_single_item() {
        assert_eq!(format_list(&j(&["A"]), "en", ListStyle::Conjunction), "A");
    }

    #[test]
    fn english_two_items_conjunction() {
        assert_eq!(
            format_list(&j(&["A", "B"]), "en", ListStyle::Conjunction),
            "A and B"
        );
    }

    #[test]
    fn english_three_items_conjunction() {
        assert_eq!(
            format_list(&j(&["A", "B", "C"]), "en", ListStyle::Conjunction),
            "A, B, and C"
        );
    }

    #[test]
    fn english_disjunction() {
        assert_eq!(
            format_list(&j(&["A", "B", "C"]), "en", ListStyle::Disjunction),
            "A, B, or C"
        );
    }

    #[test]
    fn english_unit() {
        assert_eq!(
            format_list(&j(&["A", "B", "C"]), "en", ListStyle::Unit),
            "A, B, C"
        );
    }

    #[test]
    fn spanish_conjunction() {
        assert_eq!(
            format_list(&j(&["A", "B", "C"]), "es", ListStyle::Conjunction),
            "A, B y C"
        );
    }

    #[test]
    fn invalid_json_returns_raw() {
        assert_eq!(
            format_list("not-json", "en", ListStyle::Conjunction),
            "not-json"
        );
    }

    #[test]
    fn empty_list() {
        assert_eq!(format_list("[]", "en", ListStyle::Conjunction), "");
    }

    #[test]
    fn english_two_items_disjunction() {
        assert_eq!(
            format_list(&j(&["A", "B"]), "en", ListStyle::Disjunction),
            "A or B"
        );
    }

    #[test]
    fn english_two_items_unit() {
        assert_eq!(format_list(&j(&["A", "B"]), "en", ListStyle::Unit), "A, B");
    }

    #[test]
    fn spanish_two_items() {
        assert_eq!(
            format_list(&j(&["A", "B"]), "es", ListStyle::Conjunction),
            "A y B"
        );
    }

    #[test]
    fn spanish_two_items_disjunction() {
        assert_eq!(
            format_list(&j(&["A", "B"]), "es", ListStyle::Disjunction),
            "A o B"
        );
    }

    #[test]
    fn spanish_two_items_unit() {
        assert_eq!(format_list(&j(&["A", "B"]), "es", ListStyle::Unit), "A, B");
    }

    #[test]
    fn parse_json_array_null_element() {
        // null elements are skipped
        let result = format_list(r#"["A",null,"B"]"#, "en", ListStyle::Conjunction);
        assert_eq!(result, "A and B");
    }

    #[test]
    fn parse_json_array_number_element() {
        // number elements are skipped
        let result = format_list(r#"["A",42,"B"]"#, "en", ListStyle::Conjunction);
        assert_eq!(result, "A and B");
    }

    #[test]
    fn parse_json_array_escaped_quote() {
        let result = format_list(r#"["he\"llo","world"]"#, "en", ListStyle::Conjunction);
        assert_eq!(result, "he\"llo and world");
    }

    #[test]
    fn format_list_es_three_items() {
        assert_eq!(
            format_list(r#"["A","B","C"]"#, "es", ListStyle::Disjunction),
            "A, B o C"
        );
    }

    #[test]
    fn format_list_es_unit() {
        assert_eq!(
            format_list(r#"["A","B","C"]"#, "es", ListStyle::Unit),
            "A, B, C"
        );
    }
}
