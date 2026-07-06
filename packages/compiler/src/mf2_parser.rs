//! ICU MessageFormat 2.0 (MF2) complex-message parsing and expression lowering.

use crate::icu_parser::{
    is_plural_key, DateStyle, IStr, ListStyle, MarkupKind, MessageNode, NumberStyle, PluralCaseKey,
    RelTimeStyle,
};
use std::collections::{HashMap, HashSet};

/// Reject NULL bytes and Unicode noncharacters in MF2 source text.
pub fn validate_source_text(input: &str) -> Result<(), String> {
    if input.contains('\0') {
        return Err("NULL in message".to_string());
    }
    validate_unicode_escapes(input)?;
    if input.chars().any(|c| {
        let cp = c as u32;
        matches!(cp, 0xFDD0..=0xFDEF | 0xFFFE | 0xFFFF)
    }) {
        return Err("Noncharacter in message".to_string());
    }
    Ok(())
}

fn validate_unicode_escapes(input: &str) -> Result<(), String> {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 5 < bytes.len() && bytes[i + 1] == b'u' {
            let hex = &input[i + 2..i + 6];
            if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                let code = u32::from_str_radix(hex, 16).unwrap_or(0);
                if code == 0 {
                    return Err("NULL in message".to_string());
                }
                if matches!(code, 0xFDD0..=0xFDEF | 0xFFFE | 0xFFFF) {
                    return Err("Noncharacter in message".to_string());
                }
            }
            i += 6;
            continue;
        }
        i += 1;
    }
    Ok(())
}

fn is_reserved_sigil(c: char) -> bool {
    matches!(c, '^' | '!' | '&' | '%' | '*' | '<' | '>' | '?' | '~' | '+')
}

/// Returns true when the message is an MF2 complex message (declarations / quoted pattern / `.match`).
pub fn is_complex_message(input: &str) -> bool {
    let t = input.trim();
    t.starts_with('.') || t.starts_with("{{")
}

/// Parse an MF2 complex message into message nodes.
pub fn parse_complex(input: &str) -> Result<Vec<MessageNode>, String> {
    validate_source_text(input)?;
    let mut remaining = input.trim();
    let mut locals: HashMap<String, MessageNode> = HashMap::new();
    let mut inputs: HashMap<String, MessageNode> = HashMap::new();
    let mut declared: HashSet<String> = HashSet::new();
    let mut declaration_order: Vec<(String, MessageNode, bool)> = Vec::new();

    while remaining.starts_with('.') {
        if let Some(rest) = remaining.strip_prefix(".input") {
            let (var, expr, next) = parse_input_declaration(rest)?;
            if declared.contains(&var) {
                return Err("Duplicate declaration".to_string());
            }
            declared.insert(var.clone());
            declaration_order.push((var.clone(), expr.clone(), true));
            inputs.insert(var, expr);
            remaining = next;
        } else if let Some(rest) = remaining.strip_prefix(".local") {
            let (var, expr, next) = parse_local_declaration(rest)?;
            if declared.contains(&var) {
                return Err("Duplicate declaration".to_string());
            }
            declared.insert(var.clone());
            declaration_order.push((var.clone(), expr.clone(), false));
            locals.insert(var, expr);
            remaining = next;
        } else if let Some(rest) = remaining.strip_prefix(".match") {
            if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
                return Err("Invalid .match declaration".to_string());
            }
            remaining = rest;
            break;
        } else {
            return Err(format!("Unknown MF2 declaration: {}", remaining));
        }
        remaining = remaining.trim();
    }

    validate_declaration_order_vec(&declaration_order)?;

    if remaining.starts_with("{{") {
        let (pattern, next) = parse_quoted_pattern(remaining)?;
        remaining = next.trim();
        if !remaining.is_empty() {
            return Err(format!(
                "Trailing content after quoted pattern: {}",
                remaining
            ));
        }
        let pattern = substitute_locals_in_nodes(&pattern, &locals);
        if inputs.is_empty() && locals.is_empty() {
            return Ok(pattern);
        }
        return Ok(vec![MessageNode::Mf2Match {
            selectors: vec![],
            inputs,
            locals,
            variants: vec![(vec![], pattern)],
        }]);
    }

    if remaining.starts_with("match") {
        remaining = remaining.strip_prefix("match").unwrap_or(remaining).trim();
    }

    if !remaining.is_empty() {
        let node = parse_mf2_match_body(remaining, &locals, &inputs)?;
        return Ok(vec![node]);
    }

    Ok(vec![])
}

fn parse_input_declaration(rest: &str) -> Result<(String, MessageNode, &str), String> {
    let rest = rest.trim_start();
    let (expr_src, next) = parse_braced_expression(rest)?;
    let expr = parse_mf2_expression(expr_src, true)?;
    let var = extract_variable_name(expr_src)?;
    Ok((var, expr, next))
}

fn parse_local_declaration(rest: &str) -> Result<(String, MessageNode, &str), String> {
    let rest = rest.trim_start();
    let eq = rest.find('=').ok_or("Expected '=' in .local declaration")?;
    let var_part = rest[..eq].trim();
    if !var_part.starts_with('$') {
        return Err("Invalid variable name in .local".to_string());
    }
    let var = var_part.trim_start_matches('$').trim().to_string();
    if var.is_empty()
        || var.starts_with('#')
        || var.contains('.')
        || !var
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        return Err("Invalid variable name in .local".to_string());
    }
    let after_eq = rest[eq + 1..].trim_start();
    let (expr_src, next) = parse_braced_expression(after_eq)?;
    let expr = parse_mf2_expression(expr_src, true)?;
    Ok((var, expr, next))
}

fn parse_braced_expression(s: &str) -> Result<(&str, &str), String> {
    let s = s.trim_start();
    if !s.starts_with('{') {
        return Err("Expected '{'".to_string());
    }
    let mut depth = 0;
    let mut end = None;
    for (i, c) in s.char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let end = end.ok_or("Unmatched '{' in expression")?;
    Ok((&s[1..end], &s[end + 1..]))
}

fn extract_variable_name(expr: &str) -> Result<String, String> {
    let stripped = strip_attributes(expr.trim());
    let name = stripped
        .trim_start_matches('$')
        .split_whitespace()
        .next()
        .ok_or("Expected variable in .input")?
        .to_string();
    Ok(name)
}

/// Parse MF2 expression content (inside `{...}`).
pub fn parse_mf2_expression(raw: &str, mf2_mode: bool) -> Result<MessageNode, String> {
    validate_mf2_expression_syntax(raw)?;
    let s = strip_attributes(raw.trim());
    if s.is_empty() {
        return Err("Empty expression".to_string());
    }

    if let Some(rest) = s.strip_prefix('#') {
        return parse_markup_open(rest);
    }
    if let Some(rest) = s.strip_prefix('/') {
        return Ok(MessageNode::Markup {
            kind: MarkupKind::Close,
            name: parse_markup_name(rest)?,
        });
    }
    if let Some(rest) = s.strip_prefix(':') {
        return parse_function_expression(rest);
    }
    if let Some(rest) = s.strip_prefix('$') {
        return parse_variable_expression(rest);
    }
    if let Some(rest) = s.strip_prefix('-') {
        let bare = rest.trim();
        if let Some(pipe) = bare.find('|') {
            let name = bare[..pipe].trim().into();
            let default = bare[pipe + 1..].trim().into();
            return Ok(MessageNode::VariableWithDefault { name, default });
        }
        return Ok(MessageNode::RawVariable(bare.into()));
    }
    if s.starts_with('|') {
        return parse_quoted_literal_expression(s);
    }

    if let Some(pipe) = s.find('|') {
        let name = s[..pipe].trim().into();
        let default = s[pipe + 1..].trim().into();
        return Ok(MessageNode::VariableWithDefault { name, default });
    }

    if let Some((literal, func_spec)) = split_literal_function(s) {
        validate_function_spec(func_spec)?;
        let (formatter, options) = parse_function_spec(func_spec);
        return Ok(MessageNode::Custom {
            var: "".into(),
            literal_operand: Some(literal.into()),
            format: crate::icu_parser::CustomFormat { formatter, options },
        });
    }

    if let Some((name, func_spec)) = split_bare_name_function(s) {
        validate_function_spec(func_spec)?;
        if func_spec.starts_with("string") || func_spec.starts_with(":string") {
            let default = extract_default_option(func_spec);
            return Ok(MessageNode::VariableWithDefault {
                name: name.into(),
                default: default.into(),
            });
        }
        let (formatter, options) = parse_function_spec(func_spec);
        return Ok(MessageNode::Custom {
            var: "".into(),
            literal_operand: Some(name.into()),
            format: crate::icu_parser::CustomFormat { formatter, options },
        });
    }

    if mf2_mode {
        return Ok(MessageNode::Text(s.into()));
    }

    Ok(MessageNode::Variable(s.into()))
}

/// Splits `{1 :test:select}`-style literal + function annotation.
fn split_bare_name_function(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let start = i;
    if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == start {
        return None;
    }
    if i >= bytes.len() || !bytes[i].is_ascii_whitespace() {
        return None;
    }
    let func = s[i..].trim_start();
    if !(func.starts_with(':') || func.contains(':')) {
        return None;
    }
    let func = func.trim_start_matches(':');
    Some((s[start..i].trim_end().to_string(), func))
}

fn split_literal_function(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }
    let start = i;
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        saw_digit = true;
        i += 1;
    }
    if !saw_digit {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i >= bytes.len() || !bytes[i].is_ascii_whitespace() {
        return None;
    }
    let func = s[i..].trim_start();
    if func.is_empty() || !(func.starts_with(':') || func.contains(':')) {
        return None;
    }
    let func = func.trim_start_matches(':');
    Some((s[start..i].trim_end().to_string(), func))
}

fn parse_variable_expression(rest: &str) -> Result<MessageNode, String> {
    let rest = rest.trim();
    if rest.contains(' ') && !rest.contains(':') {
        return Err("Invalid variable expression".to_string());
    }
    if let Some(colon) = rest.find(':') {
        let var = rest[..colon].trim().into();
        let func = rest[colon + 1..].trim();
        validate_function_spec(func)?;
        return parse_inline_function(var, func);
    }
    if let Some(pipe) = rest.find('|') {
        let name = rest[..pipe].trim().into();
        let default = rest[pipe + 1..].trim().into();
        return Ok(MessageNode::VariableWithDefault { name, default });
    }
    let name = rest
        .split_whitespace()
        .next()
        .ok_or("Empty variable name")?;
    Ok(MessageNode::Variable(name.into()))
}

fn parse_inline_function(
    var: crate::icu_parser::IStr,
    func_spec: &str,
) -> Result<MessageNode, String> {
    if func_spec.starts_with("number") || func_spec.starts_with("Number") {
        let style = if func_spec.contains("style=percent") {
            NumberStyle::Percent
        } else if func_spec.contains("style=integer") {
            NumberStyle::Integer
        } else if func_spec.contains("style=currency") {
            let code = func_spec
                .split_whitespace()
                .find(|s| s.starts_with("currency="))
                .and_then(|s| s.split('=').nth(1))
                .unwrap_or("USD")
                .to_string();
            NumberStyle::Currency(code)
        } else {
            NumberStyle::Decimal
        };
        return Ok(MessageNode::Number { var, style });
    }
    if func_spec.starts_with("string") || func_spec.starts_with(":string") {
        let default = extract_default_option(func_spec);
        return Ok(MessageNode::VariableWithDefault {
            name: var,
            default: default.into(),
        });
    }
    if matches!(func_spec, "date" | ":date" | "Date") {
        return Ok(MessageNode::Date {
            var,
            style: DateStyle::Date,
        });
    }
    if matches!(func_spec, "time" | ":time" | "Time") {
        return Ok(MessageNode::Date {
            var,
            style: DateStyle::Time,
        });
    }
    if matches!(func_spec, "datetime" | ":datetime" | "DateTime") {
        return Ok(MessageNode::Date {
            var,
            style: DateStyle::DateTime,
        });
    }
    if func_spec.starts_with("list") || func_spec.starts_with(":list") {
        let style = if func_spec.contains("style=or") {
            ListStyle::Disjunction
        } else if func_spec.contains("style=unit") {
            ListStyle::Unit
        } else {
            ListStyle::Conjunction
        };
        return Ok(MessageNode::List { var, style });
    }
    if func_spec.contains("relativetime") || func_spec.contains("relativedelta") {
        let style = if func_spec.contains("unit=seconds") {
            RelTimeStyle::Seconds
        } else if func_spec.contains("unit=minutes") {
            RelTimeStyle::Minutes
        } else if func_spec.contains("unit=hours") {
            RelTimeStyle::Hours
        } else if func_spec.contains("unit=days") {
            RelTimeStyle::Days
        } else if func_spec.contains("unit=weeks") {
            RelTimeStyle::Weeks
        } else if func_spec.contains("unit=months") {
            RelTimeStyle::Months
        } else if func_spec.contains("unit=years") {
            RelTimeStyle::Years
        } else {
            RelTimeStyle::Auto
        };
        return Ok(MessageNode::RelTime { var, style });
    }

    let (formatter, options) = parse_function_spec(func_spec);
    Ok(MessageNode::Custom {
        var,
        literal_operand: None,
        format: crate::icu_parser::CustomFormat { formatter, options },
    })
}

fn parse_function_expression(rest: &str) -> Result<MessageNode, String> {
    validate_function_spec(rest.trim())?;
    let (formatter, options) = parse_function_spec(rest.trim());
    Ok(MessageNode::Custom {
        var: "".into(),
        literal_operand: None,
        format: crate::icu_parser::CustomFormat { formatter, options },
    })
}

fn validate_function_spec(spec: &str) -> Result<(), String> {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with(':') {
        return Err("Empty function name".to_string());
    }
    let mut parts = spec.split_whitespace();
    let name = parts.next().unwrap_or("").trim_start_matches(':');
    if name.is_empty() || name.ends_with(':') || name.contains("::") {
        return Err("Invalid function name".to_string());
    }
    validate_option_tokens(parts)?;
    Ok(())
}

fn validate_option_tokens<'a, I: Iterator<Item = &'a str>>(parts: I) -> Result<(), String> {
    let mut rest: &str = &parts.collect::<Vec<_>>().join(" ");
    let mut seen = HashSet::new();
    while !rest.is_empty() {
        rest = rest.trim_start();
        let eq = rest
            .find('=')
            .ok_or_else(|| "Option requires '='".to_string())?;
        let key = rest[..eq].trim();
        if key.is_empty() {
            return Err("Invalid option".to_string());
        }
        if key.starts_with(':') || key.contains("::") || key.ends_with(':') {
            return Err("Invalid option name".to_string());
        }
        if !seen.insert(key.to_string()) {
            return Err("Duplicate option name".to_string());
        }
        rest = rest[eq + 1..].trim_start();
        if rest.starts_with('"') {
            let close = rest[1..]
                .find('"')
                .ok_or_else(|| "Unclosed option string".to_string())?;
            if close == 0 {
                return Err("Empty option value".to_string());
            }
            rest = &rest[1 + close + 1..];
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            if end == 0 {
                return Err("Empty option value".to_string());
            }
            rest = &rest[end..];
        }
    }
    Ok(())
}

fn parse_function_spec(spec: &str) -> (String, HashMap<String, String>) {
    let mut parts = spec.split_whitespace();
    let name = parts
        .next()
        .unwrap_or("")
        .trim_start_matches(':')
        .to_string();
    let mut options = HashMap::new();
    for opt in parts {
        if let Some((k, v)) = opt.split_once('=') {
            options.insert(k.to_string(), v.trim_matches('"').to_string());
        }
    }
    (name, options)
}

fn extract_default_option(func_spec: &str) -> String {
    if let Some(start) = func_spec.find("default=\"") {
        let after = &func_spec[start + 9..];
        after.split('"').next().unwrap_or("").to_string()
    } else if let Some(start) = func_spec.find("default=") {
        let after = &func_spec[start + 8..];
        after.split_whitespace().next().unwrap_or("").to_string()
    } else {
        String::new()
    }
}

fn parse_markup_open(rest: &str) -> Result<MessageNode, String> {
    let rest = rest.trim();
    if let Some(name_end) = rest.find('/') {
        let name = rest[..name_end].trim().into();
        return Ok(MessageNode::Markup {
            kind: MarkupKind::Standalone,
            name,
        });
    }
    Ok(MessageNode::Markup {
        kind: MarkupKind::Open,
        name: parse_markup_name(rest)?,
    })
}

fn parse_markup_name(s: &str) -> Result<crate::icu_parser::IStr, String> {
    let name = s.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() {
        return Err("Empty markup name".to_string());
    }
    Ok(name.into())
}

fn parse_quoted_literal_expression(s: &str) -> Result<MessageNode, String> {
    let (content, remainder) = split_quoted_literal(s)?;
    let rem = remainder.trim();
    if rem.is_empty() {
        return Ok(MessageNode::Text(content.into()));
    }
    if let Some(func) = rem.strip_prefix(':') {
        validate_function_spec(func.trim())?;
        let (formatter, options) = parse_function_spec(func.trim());
        return Ok(MessageNode::Custom {
            var: "".into(),
            literal_operand: Some(content.into()),
            format: crate::icu_parser::CustomFormat { formatter, options },
        });
    }
    if rem.chars().next().is_some_and(is_reserved_sigil) {
        return Err("Reserved annotation after literal".to_string());
    }
    Err("Invalid content after quoted literal".to_string())
}

/// Returns `(literal_text, remainder_after_closing_pipe)`.
fn split_quoted_literal(s: &str) -> Result<(String, &str), String> {
    if !s.starts_with('|') {
        return Err("Expected quoted literal".to_string());
    }
    let inner = &s[1..];
    let mut out = String::new();
    let mut chars = inner.char_indices().peekable();
    let mut close_idx = None;
    while let Some((i, c)) = chars.next() {
        if c == '|' {
            if chars.peek().map(|(_, nc)| *nc) == Some('|') {
                chars.next();
                out.push('|');
            } else {
                close_idx = Some(i);
                break;
            }
        } else if c == '\\' {
            let next = chars.next().ok_or("Dangling escape in quoted literal")?.1;
            match next {
                '\\' => out.push('\\'),
                '{' => out.push('{'),
                '}' => out.push('}'),
                '|' => out.push('|'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    let close_idx = close_idx.ok_or("Unclosed quoted literal")?;
    Ok((out, &inner[close_idx + 1..]))
}

pub fn parse_quoted_pattern(input: &str) -> Result<(Vec<MessageNode>, &str), String> {
    let input = input.trim_start();
    if !input.starts_with("{{") {
        return Err("Expected '{{'".to_string());
    }
    let inner = &input[2..];
    let mut expr_depth = 0i32;
    let mut i = 0usize;
    while i < inner.len() {
        let c = inner[i..].chars().next().ok_or("Unclosed quoted pattern")?;
        let clen = c.len_utf8();
        if c == '\\' {
            i += clen;
            if i < inner.len() {
                i += inner[i..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if c == '{' {
            expr_depth += 1;
        } else if c == '}' {
            if expr_depth > 0 {
                expr_depth -= 1;
            } else if inner[i..].starts_with("}}") {
                let pattern = &inner[..i];
                let nodes = parse_mf2_pattern(pattern)?;
                return Ok((nodes, &input[2 + i + 2..]));
            } else {
                return Err("Unmatched '}' in quoted pattern".to_string());
            }
        }
        i += clen;
    }
    Err("Unclosed quoted pattern".to_string())
}

pub fn parse_mf2_pattern(pattern: &str) -> Result<Vec<MessageNode>, String> {
    let mut nodes = Vec::new();
    let mut text = String::new();
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            let next = chars.next().ok_or("Dangling escape in pattern")?;
            match next {
                '\\' => text.push('\\'),
                '{' => text.push('{'),
                '}' => text.push('}'),
                '|' => text.push('|'),
                other => {
                    text.push('\\');
                    text.push(other);
                }
            }
        } else if c == '{' {
            if !text.is_empty() {
                nodes.push(MessageNode::Text(std::mem::take(&mut text).into()));
            }
            let mut depth = 1;
            let mut expr = String::new();
            for ch in chars.by_ref() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                expr.push(ch);
            }
            if depth > 0 {
                return Err("Unmatched '{' in pattern".to_string());
            }
            nodes.push(parse_mf2_expression(&expr, true)?);
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        nodes.push(MessageNode::Text(text.into()));
    }
    Ok(nodes)
}

fn parse_mf2_match_body(
    body: &str,
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
) -> Result<MessageNode, String> {
    let mut remaining = body.trim();
    let mut selectors = Vec::new();

    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.starts_with("{{") {
            break;
        }
        if remaining.starts_with('$') {
            let name = read_identifier(&mut remaining)?;
            selectors.push(name);
            if remaining.starts_with('$') {
                return Err("Missing whitespace between selectors".to_string());
            }
            if !remaining.is_empty()
                && !remaining.starts_with(char::is_whitespace)
                && !remaining.starts_with("{{")
            {
                return Err("Missing whitespace after selector".to_string());
            }
            remaining = remaining.trim_start();
        } else {
            break;
        }
    }
    if selectors.is_empty() {
        return Err("Match requires at least one selector".to_string());
    }

    let mut entries: Vec<(Vec<String>, Vec<MessageNode>)> = Vec::new();
    let mut pending_keys: Vec<String> = Vec::new();

    while !remaining.is_empty() {
        let had_leading_ws = remaining.starts_with(char::is_whitespace);
        if had_leading_ws {
            remaining = remaining.trim_start();
        }
        if remaining.starts_with("{{") {
            let (pattern, next) = parse_quoted_pattern(remaining)?;
            let keys = if pending_keys.is_empty() {
                vec!["*".to_string(); selectors.len()]
            } else {
                std::mem::take(&mut pending_keys)
            };
            if keys.len() != selectors.len() {
                return Err(format!(
                    "Expected {} keys in variant, got {}",
                    selectors.len(),
                    keys.len()
                ));
            }
            entries.push((keys, substitute_locals_in_nodes(&pattern, locals)));
            remaining = next;
        } else if remaining.starts_with('*') {
            if !pending_keys.is_empty()
                && pending_keys.last().is_some_and(|k| k == "*")
                && !had_leading_ws
            {
                return Err("Missing whitespace between variant keys".to_string());
            }
            pending_keys.push("*".to_string());
            remaining = &remaining[1..];
        } else {
            let key = read_key_literal(&mut remaining)?;
            pending_keys.push(key);
        }
    }

    if entries.is_empty() {
        return Err("No variants in match".to_string());
    }
    if !pending_keys.is_empty() || !remaining.trim().is_empty() {
        return Err("Trailing content after match".to_string());
    }
    build_match_tree(&selectors, entries, locals, inputs)
}

fn read_identifier(s: &mut &str) -> Result<String, String> {
    let t = s.trim_start();
    if !t.starts_with('$') {
        return Err("Expected '$'".to_string());
    }
    let rest = &t[1..];
    let end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .unwrap_or(rest.len());
    let name = rest[..end].to_string();
    *s = &t[1 + end..];
    Ok(name)
}

fn read_key_literal(s: &mut &str) -> Result<String, String> {
    let t = s.trim_start();
    if let Some(rest) = t.strip_prefix('|') {
        let close = rest.find('|').ok_or("Unclosed quoted key")?;
        let key = rest[..close].to_string();
        *s = &t[1 + close + 1..];
        return Ok(format!("|{key}|"));
    }
    let end = t
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(t.len());
    if end == 0 {
        return Err("Empty variant key".to_string());
    }
    let key = t[..end].to_string();
    *s = &t[end..];
    Ok(key)
}

/// Validates MF2 data-model constraints on an already-parsed message AST.
pub fn validate_data_model(nodes: &[MessageNode]) -> Result<(), String> {
    for node in nodes {
        validate_data_model_node(node)?;
    }
    Ok(())
}

fn validate_data_model_node(node: &MessageNode) -> Result<(), String> {
    match node {
        MessageNode::Mf2Match {
            selectors,
            inputs,
            locals,
            variants,
        } => {
            validate_declaration_order(inputs, locals)?;
            validate_match_variants(selectors, variants, locals, inputs)
        }
        MessageNode::Select { var, cases } => validate_match_variants(
            std::slice::from_ref(var),
            &cases
                .iter()
                .map(|(k, p)| (vec![k.clone()], p.clone()))
                .collect::<Vec<_>>(),
            &HashMap::new(),
            &HashMap::new(),
        ),
        MessageNode::Plural { var, cases, .. } => {
            let entries: Vec<(Vec<IStr>, Vec<MessageNode>)> = cases
                .iter()
                .map(|(k, p)| (vec![plural_key_label(k)], p.clone()))
                .collect();
            validate_match_variants(
                std::slice::from_ref(var),
                &entries,
                &HashMap::new(),
                &HashMap::new(),
            )
        }
        _ => Ok(()),
    }
}

fn plural_key_label(key: &PluralCaseKey) -> IStr {
    match key {
        PluralCaseKey::Other => "*".into(),
        PluralCaseKey::Exact(v) => v.to_string().into(),
        PluralCaseKey::Range(min, max) => format!("{min}-{max}").into(),
        PluralCaseKey::Zero => "zero".into(),
        PluralCaseKey::One => "one".into(),
        PluralCaseKey::Two => "two".into(),
        PluralCaseKey::Few => "few".into(),
        PluralCaseKey::Many => "many".into(),
    }
}

fn validate_declaration_order(
    inputs: &HashMap<String, MessageNode>,
    locals: &HashMap<String, MessageNode>,
) -> Result<(), String> {
    let mut ordered: Vec<(String, MessageNode, bool)> = inputs
        .iter()
        .map(|(name, expr)| (name.clone(), expr.clone(), true))
        .collect();
    ordered.extend(
        locals
            .iter()
            .map(|(name, expr)| (name.clone(), expr.clone(), false)),
    );
    validate_declaration_order_vec(&ordered)
}

fn validate_declaration_order_vec(order: &[(String, MessageNode, bool)]) -> Result<(), String> {
    let all_names: HashSet<String> = order.iter().map(|(name, _, _)| name.clone()).collect();
    let mut declared: HashSet<String> = HashSet::new();
    for (name, expr, is_input) in order {
        if !is_input {
            validate_local_declaration_references(expr, &declared, &all_names)?;
        }
        declared.insert(name.clone());
    }
    Ok(())
}

fn validate_local_declaration_references(
    expr: &MessageNode,
    declared: &HashSet<String>,
    all_names: &HashSet<String>,
) -> Result<(), String> {
    match expr {
        MessageNode::Variable(name) => {
            if all_names.contains(&**name) && !declared.contains(&**name) {
                return Err("Duplicate declaration".to_string());
            }
        }
        MessageNode::Custom { var, format, .. } => {
            if !var.is_empty() && all_names.contains(&**var) && !declared.contains(&**var) {
                return Err("Duplicate declaration".to_string());
            }
            for value in format.options.values() {
                let name = value.trim_start_matches('$');
                if value.starts_with('$') && all_names.contains(name) && !declared.contains(name) {
                    return Err("Duplicate declaration".to_string());
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_match_variants(
    selectors: &[IStr],
    entries: &[(Vec<IStr>, Vec<MessageNode>)],
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    if !inputs.is_empty() || !locals.is_empty() {
        for sel in selectors {
            if !decl_has_selector_annotation(sel, inputs, locals) {
                return Err("Missing selector annotation".to_string());
            }
        }
    }
    let has_fallback = entries
        .iter()
        .any(|(keys, _)| keys.len() == selectors.len() && keys.iter().all(|k| &**k == "*"));
    if !has_fallback {
        return Err("Missing fallback variant".to_string());
    }
    let mut seen = HashSet::new();
    for (keys, _) in entries {
        let normalized: Vec<String> = keys.iter().map(|k| normalize_variant_key(k)).collect();
        if !seen.insert(normalized) {
            return Err("Duplicate variant".to_string());
        }
    }
    Ok(())
}

fn normalize_variant_key(key: &str) -> String {
    if key.len() >= 2 && key.starts_with('|') && key.ends_with('|') {
        let inner = &key[1..key.len() - 1];
        if inner == "*" {
            return key.to_string();
        }
        inner.to_string()
    } else {
        key.to_string()
    }
}

fn decl_has_selector_annotation(
    name: &str,
    inputs: &HashMap<String, MessageNode>,
    locals: &HashMap<String, MessageNode>,
) -> bool {
    let mut visiting = HashSet::new();
    decl_has_selector_annotation_inner(name, inputs, locals, &mut visiting)
}

fn decl_has_selector_annotation_inner(
    name: &str,
    inputs: &HashMap<String, MessageNode>,
    locals: &HashMap<String, MessageNode>,
    visiting: &mut HashSet<String>,
) -> bool {
    if !visiting.insert(name.to_string()) {
        return false;
    }
    let ok = if let Some(expr) = inputs.get(name) {
        expr_is_selector_annotated(expr, inputs, locals, visiting)
    } else if let Some(expr) = locals.get(name) {
        expr_is_selector_annotated(expr, inputs, locals, visiting)
    } else {
        false
    };
    visiting.remove(name);
    ok
}

fn expr_is_selector_annotated(
    expr: &MessageNode,
    inputs: &HashMap<String, MessageNode>,
    locals: &HashMap<String, MessageNode>,
    visiting: &mut HashSet<String>,
) -> bool {
    match expr {
        MessageNode::Custom { format, .. } => !format.formatter.is_empty(),
        MessageNode::VariableWithDefault { .. } => true,
        MessageNode::Number { .. }
        | MessageNode::Date { .. }
        | MessageNode::List { .. }
        | MessageNode::RelTime { .. } => true,
        MessageNode::Variable(name) => {
            decl_has_selector_annotation_inner(name, inputs, locals, visiting)
        }
        _ => false,
    }
}

fn build_match_tree(
    selectors: &[String],
    entries: Vec<(Vec<String>, Vec<MessageNode>)>,
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
) -> Result<MessageNode, String> {
    if uses_mf2_test_resolution(selectors, locals, inputs)
        || !inputs.is_empty()
        || !locals.is_empty()
    {
        return Ok(MessageNode::Mf2Match {
            selectors: selectors.iter().map(|s| IStr::from(s.as_str())).collect(),
            inputs: inputs.clone(),
            locals: locals.clone(),
            variants: entries
                .into_iter()
                .map(|(keys, nodes)| (keys.into_iter().map(IStr::from).collect(), nodes))
                .collect(),
        });
    }
    if selectors.len() == 1 {
        return build_single_selector_match(&selectors[0], entries);
    }
    if selectors.len() == 2 {
        return build_two_selector_match(&selectors[0], &selectors[1], entries);
    }
    Err(format!(
        "Only 1-2 selectors supported, got {}",
        selectors.len()
    ))
}

fn uses_mf2_test_resolution(
    selectors: &[String],
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
) -> bool {
    let mut visiting = HashSet::new();
    selectors
        .iter()
        .any(|sel| resolves_to_test_function(sel, locals, inputs, &mut visiting))
}

fn resolves_to_test_function(
    name: &str,
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
    visiting: &mut HashSet<String>,
) -> bool {
    if !visiting.insert(name.to_string()) {
        return false;
    }
    let ok = if let Some(expr) = inputs.get(name) {
        expression_has_test_function(expr)
    } else if let Some(expr) = locals.get(name) {
        expression_has_test_function(expr)
            || expression_references_test_local(expr, locals, inputs, visiting)
    } else {
        false
    };
    visiting.remove(name);
    ok
}

fn expression_has_test_function(expr: &MessageNode) -> bool {
    match expr {
        MessageNode::Custom { format, .. } => is_test_formatter(&format.formatter),
        _ => false,
    }
}

fn expression_references_test_local(
    expr: &MessageNode,
    locals: &HashMap<String, MessageNode>,
    inputs: &HashMap<String, MessageNode>,
    visiting: &mut HashSet<String>,
) -> bool {
    match expr {
        MessageNode::Variable(name) => resolves_to_test_function(name, locals, inputs, visiting),
        MessageNode::Custom { var, .. } if !var.is_empty() => {
            resolves_to_test_function(var, locals, inputs, visiting)
        }
        _ => false,
    }
}

fn is_test_formatter(name: &str) -> bool {
    matches!(name, "test:function" | "test:select" | "test:format")
}

fn build_single_selector_match(
    var: &str,
    entries: Vec<(Vec<String>, Vec<MessageNode>)>,
) -> Result<MessageNode, String> {
    let is_plural = entries
        .iter()
        .all(|(keys, _)| keys.first().is_some_and(|k| k == "*" || is_plural_key(k)));
    if is_plural {
        let cases = entries
            .into_iter()
            .map(|(keys, nodes)| (to_plural_key(&keys[0]), nodes))
            .collect();
        return Ok(MessageNode::Plural {
            var: var.into(),
            ordinal: false,
            cases,
        });
    }
    let cases = entries
        .into_iter()
        .map(|(keys, nodes)| (IStr::from(keys[0].as_str()), nodes))
        .collect();
    Ok(MessageNode::Select {
        var: var.into(),
        cases,
    })
}

fn build_two_selector_match(
    inner_var: &str,
    outer_var: &str,
    entries: Vec<(Vec<String>, Vec<MessageNode>)>,
) -> Result<MessageNode, String> {
    let mut groups: std::collections::BTreeMap<String, Vec<(String, Vec<MessageNode>)>> =
        std::collections::BTreeMap::new();
    for (keys, nodes) in entries {
        let outer_key = if keys[1] == "*" {
            "other".to_string()
        } else {
            keys[1].clone()
        };
        let inner_key = if keys[0] == "*" {
            "*".to_string()
        } else {
            keys[0].clone()
        };
        groups
            .entry(outer_key)
            .or_default()
            .push((inner_key, nodes));
    }

    let mut outer_cases = Vec::new();
    for (outer_key, sub) in groups {
        let sub_is_plural = sub.iter().all(|(k, _)| k == "*" || is_plural_key(k));
        let inner = if sub_is_plural {
            vec![MessageNode::Plural {
                var: inner_var.into(),
                ordinal: false,
                cases: sub
                    .into_iter()
                    .map(|(k, nodes)| {
                        (
                            if k == "*" {
                                PluralCaseKey::Other
                            } else {
                                to_plural_key(&k)
                            },
                            nodes,
                        )
                    })
                    .collect(),
            }]
        } else {
            vec![MessageNode::Select {
                var: inner_var.into(),
                cases: sub
                    .into_iter()
                    .map(|(k, nodes)| {
                        (
                            if k == "*" {
                                IStr::from("other")
                            } else {
                                IStr::from(k)
                            },
                            nodes,
                        )
                    })
                    .collect(),
            }]
        };
        outer_cases.push((outer_key, inner));
    }

    let outer_is_plural = outer_cases
        .iter()
        .all(|(k, _)| k == "*" || is_plural_key(k));
    if outer_is_plural {
        let cases = outer_cases
            .into_iter()
            .map(|(k, nodes)| (to_plural_key(&k), nodes))
            .collect();
        return Ok(MessageNode::Plural {
            var: outer_var.into(),
            ordinal: false,
            cases,
        });
    }
    Ok(MessageNode::Select {
        var: outer_var.into(),
        cases: outer_cases
            .into_iter()
            .map(|(k, nodes)| (IStr::from(k), nodes))
            .collect(),
    })
}

fn to_plural_key(k: &str) -> PluralCaseKey {
    match k {
        "zero" => PluralCaseKey::Zero,
        "one" => PluralCaseKey::One,
        "two" => PluralCaseKey::Two,
        "few" => PluralCaseKey::Few,
        "many" => PluralCaseKey::Many,
        "other" | "*" => PluralCaseKey::Other,
        _ => {
            if let Some(stripped) = k.strip_prefix('=') {
                PluralCaseKey::Exact(stripped.trim().parse().unwrap_or(f64::NAN))
            } else if let Ok(val) = k.parse::<f64>() {
                PluralCaseKey::Exact(val)
            } else {
                PluralCaseKey::Other
            }
        }
    }
}

fn validate_mf2_expression_syntax(raw: &str) -> Result<(), String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("Empty expression".to_string());
    }
    validate_source_text(t)?;
    if let Some(first) = t.chars().next() {
        if is_reserved_sigil(first) && !(t.len() == 1 && (first == '+' || first == '-')) {
            return Err("Reserved expression".to_string());
        }
    }
    if t.starts_with('@') || t.starts_with(' ') {
        return Err("Invalid expression".to_string());
    }
    if t.contains('#') && t.starts_with('|') {
        return Err("Invalid literal with markup".to_string());
    }
    if t.starts_with('#') || t.starts_with('/') {
        return Ok(());
    }
    let bytes = t.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' && i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_digit() || prev == b'|' {
                return Err("Missing space before function annotation".to_string());
            }
        }
        if b == b'@' && i > 0 && !bytes[i - 1].is_ascii_whitespace() {
            return Err("Missing space before attribute".to_string());
        }
        if b == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'@' {
            return Err("Invalid attribute".to_string());
        }
        if b == b'@' {
            let after = &t[i + 1..];
            if let Some(eq) = after.find('=') {
                let val = after[eq + 1..].split_whitespace().next().unwrap_or("");
                if val.is_empty() {
                    return Err("Empty attribute value".to_string());
                }
                if val.starts_with('$') || val.starts_with('@') {
                    return Err("Invalid attribute value".to_string());
                }
            }
        }
    }

    if t == ":" || t.starts_with(":}") {
        return Err("Empty function name".to_string());
    }
    if t.starts_with(':') {
        let spec = strip_attributes(t.trim_start_matches(':'));
        validate_function_spec(spec)?;
    }
    Ok(())
}

pub fn strip_attributes(s: &str) -> &str {
    let mut end = s.len();
    while let Some(at) = s[..end].rfind('@') {
        let before = s[..at].trim_end();
        if before.is_empty() {
            break;
        }
        end = before.len();
    }
    s[..end].trim()
}

fn substitute_locals_in_nodes(
    nodes: &[MessageNode],
    locals: &HashMap<String, MessageNode>,
) -> Vec<MessageNode> {
    let mut visiting = HashSet::new();
    let mut out = Vec::new();
    for node in nodes {
        out.extend(substitute_locals_in_node(node, locals, &mut visiting));
    }
    out
}

fn substitute_locals_in_node(
    node: &MessageNode,
    locals: &HashMap<String, MessageNode>,
    visiting: &mut HashSet<String>,
) -> Vec<MessageNode> {
    match node {
        MessageNode::Variable(name) => {
            if let Some(local) = locals.get(&**name) {
                if !visiting.insert(name.to_string()) {
                    return vec![node.clone()];
                }
                let expanded = substitute_locals_in_node(local, locals, visiting);
                visiting.remove(&**name);
                expanded
            } else {
                vec![node.clone()]
            }
        }
        MessageNode::Plural {
            var,
            ordinal,
            cases,
        } => {
            let new_cases = cases
                .iter()
                .map(|(k, pat)| (k.clone(), substitute_locals_in_nodes(pat, locals)))
                .collect();
            vec![MessageNode::Plural {
                var: var.clone(),
                ordinal: *ordinal,
                cases: new_cases,
            }]
        }
        MessageNode::Select { var, cases } => {
            let new_cases = cases
                .iter()
                .map(|(k, pat)| (k.clone(), substitute_locals_in_nodes(pat, locals)))
                .collect();
            vec![MessageNode::Select {
                var: var.clone(),
                cases: new_cases,
            }]
        }
        _ => vec![node.clone()],
    }
}

#[cfg(test)]
mod mf2_match_tests {
    use crate::binary_writer::serialize_message;
    use crate::icu_parser::{MessageNode, MessageParser};

    #[test]
    fn pattern_selection_local_test_select() {
        let src = ".local $x = {1 :test:select} .match $x 1.0 {{1.0}} 1 {{1}} * {{other}}";
        let nodes = MessageParser::new(src).parse().expect("parse");
        assert!(matches!(&nodes[0], MessageNode::Mf2Match { .. }));
        let bc = serialize_message(&nodes);
        assert_eq!(bc[0], 0x0E);
        if let MessageNode::Mf2Match {
            locals, variants, ..
        } = &nodes[0]
        {
            assert!(locals.contains_key("x"));
            assert_eq!(variants.len(), 3);
        }
        let mut out = String::new();
        let fmt_result = l10n4x_core::formatter::format_message(&bc, "und", &[], &mut out);
        assert!(
            fmt_result.is_ok(),
            "format failed, bytecode={bc:?}, out={out:?}"
        );
        assert_eq!(out, "1");
    }
}
