//! ICU MessageFormat Parsing and Representation.
//!
//! ### Supported ICU MessageFormat 2.0 (MF2) Subset:
//! - **Single or Dual Selectors**: `match` statements support 1 or 2 selector variables (e.g. `match $count $gender`).
//! - **Nested AST**: Two-variable matches compile to a nested `Select`/`Plural` tree (outer on the second variable).
//! - **Fallback Catch-all**: Wildcard `*` patterns in match bodies map to the default `other` case.
//! - **Standard Variables**: Simple braced placeholders (e.g. `{name}`) are parsed as variable node interpolation.
//!
#[derive(Debug, Clone, PartialEq)]
pub enum NumberStyle {
    Decimal,
    Percent,
    Integer,
    Currency(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    Date,
    Time,
    DateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    Conjunction,
    Disjunction,
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelTimeStyle {
    Auto,
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomFormat {
    pub formatter: String,
    pub options: std::collections::HashMap<String, String>,
}

/// Interned string used for `MessageNode` payloads. `Arc<str>` makes AST clones
/// (key-ref inlining, MF2 local substitution) share text instead of deep-copying it.
pub type IStr = std::sync::Arc<str>;

#[derive(Debug, Clone, PartialEq)]
pub enum MessageNode {
    Text(IStr),
    Variable(IStr),
    /// Unescaped variable ({- name} in ICU 1.0 syntax). Same as Variable but without HTML escaping.
    RawVariable(IStr),
    Plural {
        var: IStr,
        ordinal: bool,
        cases: PluralCases,
    },
    Select {
        var: IStr,
        cases: Vec<(IStr, Vec<MessageNode>)>,
    },
    Number {
        var: IStr,
        style: NumberStyle,
    },
    Date {
        var: IStr,
        style: DateStyle,
    },
    RelTime {
        var: IStr,
        style: RelTimeStyle,
    },
    List {
        var: IStr,
        style: ListStyle,
    },
    KeyRef(IStr),
    VariableWithDefault {
        name: IStr,
        default: IStr,
    },
    Custom {
        var: IStr,
        /// Number literal operand for MF2 expressions like `{1 :test:select}`.
        literal_operand: Option<IStr>,
        format: CustomFormat,
    },
    /// MF2 `.match` with runtime selector resolution (`:test:*` functions).
    Mf2Match {
        selectors: Vec<IStr>,
        inputs: std::collections::HashMap<String, MessageNode>,
        locals: std::collections::HashMap<String, MessageNode>,
        variants: Vec<(Vec<IStr>, Vec<MessageNode>)>,
    },
    /// MF2 markup placeholder (`{#tag}`, `{/tag}`, `{#tag/}`) — no text output.
    Markup {
        kind: MarkupKind,
        name: IStr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    Open,
    Close,
    Standalone,
}

/// Maximum plural/interval rules per message (compile-time guard, not a range-width cap).
pub const MAX_PLURAL_RULES_PER_MESSAGE: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum PluralCaseKey {
    Exact(f64),
    /// Inclusive integer range `[min, max]`; `max == i32::MAX` means open-ended (`inf`).
    Range(i32, i32),
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

/// A parsed plural block's cases: each key paired with its message pattern.
pub type PluralCases = Vec<(PluralCaseKey, Vec<MessageNode>)>;

pub struct MessageParser<'a> {
    input: &'a str,
    chars: core::iter::Peekable<core::str::Chars<'a>>,
}

impl<'a> MessageParser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.chars().peekable(),
        }
    }

    pub fn parse(mut self) -> Result<Vec<MessageNode>, String> {
        let trimmed = self.input.trim();
        crate::mf2_parser::validate_source_text(trimmed)?;
        if trimmed == "}" || trimmed == "{" {
            return Err("Unbalanced braces".to_string());
        }
        if crate::mf2_parser::is_complex_message(trimmed) {
            return crate::mf2_parser::parse_complex(trimmed);
        }
        if trimmed.starts_with("match") || trimmed.starts_with(".match") {
            let node = parse_mf2_match(trimmed)?;
            Ok(vec![node])
        } else {
            self.parse_pattern(false)
        }
    }

    fn parse_pattern(&mut self, in_brace: bool) -> Result<Vec<MessageNode>, String> {
        let mut nodes = Vec::new();
        let mut current_text = String::new();

        while let Some(&c) = self.chars.peek() {
            if c == '\\' {
                self.chars.next();
                if let Some(&next) = self.chars.peek() {
                    self.chars.next();
                    match next {
                        '\\' => current_text.push('\\'),
                        '{' => current_text.push('{'),
                        '}' => current_text.push('}'),
                        '|' => current_text.push('|'),
                        other => {
                            current_text.push('\\');
                            current_text.push(other);
                        }
                    }
                }
            } else if c == '{' {
                self.chars.next(); // consume '{'
                if !current_text.is_empty() {
                    nodes.push(MessageNode::Text(core::mem::take(&mut current_text).into()));
                }
                let node = self.parse_expression()?;
                nodes.push(node);
            } else if c == '}' && in_brace {
                break;
            } else if c == '$' {
                // Check for $t(...) cross-reference
                let saved = self.chars.clone();
                self.chars.next();
                if self.chars.peek() == Some(&'t') {
                    self.chars.next();
                    if self.chars.peek() == Some(&'(') {
                        self.chars.next();
                        if !current_text.is_empty() {
                            nodes
                                .push(MessageNode::Text(core::mem::take(&mut current_text).into()));
                        }
                        let mut key_ref = String::new();
                        for ch in self.chars.by_ref() {
                            if ch == ')' {
                                break;
                            }
                            key_ref.push(ch);
                        }
                        nodes.push(MessageNode::KeyRef(key_ref.trim().into()));
                        continue;
                    }
                }
                // Not a $t(...), restore and push as regular text
                self.chars = saved;
                self.chars.next();
                current_text.push('$');
            } else {
                self.chars.next();
                current_text.push(c);
            }
        }

        if !current_text.is_empty() {
            nodes.push(MessageNode::Text(current_text.into()));
        }

        Ok(nodes)
    }

    fn parse_expression(&mut self) -> Result<MessageNode, String> {
        let mut expr_str = String::new();
        let mut brace_count = 1;
        for c in self.chars.by_ref() {
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    break;
                }
            }
            expr_str.push(c);
        }

        if brace_count > 0 {
            return Err("Unmatched brace".to_string());
        }
        if expr_str.trim().is_empty() {
            return Err("Empty expression".to_string());
        }

        if !expr_str.contains(',') {
            return crate::mf2_parser::parse_mf2_expression(expr_str.trim(), false);
        }

        let parts: Vec<&str> = expr_str.splitn(3, ',').collect();
        if parts.len() >= 2 {
            let var_name: IStr = parts[0].trim().trim_start_matches('$').into();
            let expr_type = parts[1].trim();

            if expr_type == "plural" && parts.len() == 3 {
                let body = parts[2].trim();
                let (body, ordinal) = if let Some(rest) = body.strip_prefix("ordinal") {
                    (rest.trim().trim_start_matches(','), true)
                } else {
                    (body, false)
                };
                let cases = parse_cases(body, &var_name)?;
                return Ok(MessageNode::Plural {
                    var: var_name,
                    ordinal,
                    cases,
                });
            } else if expr_type == "select" && parts.len() == 3 {
                let body = parts[2].trim();
                let cases = parse_select_cases(body)?;
                return Ok(MessageNode::Select {
                    var: var_name,
                    cases,
                });
            } else if expr_type == "number" {
                let style = if parts.len() == 3 {
                    match parts[2].trim() {
                        "percent" => NumberStyle::Percent,
                        "integer" => NumberStyle::Integer,
                        "currency" => NumberStyle::Currency(
                            parts
                                .get(3)
                                .map(|s| s.trim().to_string())
                                .unwrap_or_default(),
                        ),
                        _ => NumberStyle::Decimal,
                    }
                } else {
                    NumberStyle::Decimal
                };
                return Ok(MessageNode::Number {
                    var: var_name,
                    style,
                });
            } else if expr_type == "date" || expr_type == "Date" {
                return Ok(MessageNode::Date {
                    var: var_name,
                    style: DateStyle::Date,
                });
            } else if expr_type == "time" || expr_type == "Time" {
                return Ok(MessageNode::Date {
                    var: var_name,
                    style: DateStyle::Time,
                });
            } else if expr_type == "datetime" || expr_type == "dateTime" || expr_type == "DateTime"
            {
                return Ok(MessageNode::Date {
                    var: var_name,
                    style: DateStyle::DateTime,
                });
            } else if expr_type == "list" {
                let style = if parts.len() == 3 {
                    match parts[2].trim() {
                        "or" => ListStyle::Disjunction,
                        "unit" => ListStyle::Unit,
                        _ => ListStyle::Conjunction,
                    }
                } else {
                    ListStyle::Conjunction
                };
                return Ok(MessageNode::List {
                    var: var_name,
                    style,
                });
            } else if expr_type == "relativedelta" || expr_type == "relativetime" {
                let style = if parts.len() == 3 {
                    match parts[2].trim() {
                        "seconds" => RelTimeStyle::Seconds,
                        "minutes" => RelTimeStyle::Minutes,
                        "hours" => RelTimeStyle::Hours,
                        "days" => RelTimeStyle::Days,
                        "weeks" => RelTimeStyle::Weeks,
                        "months" => RelTimeStyle::Months,
                        "years" => RelTimeStyle::Years,
                        _ => RelTimeStyle::Auto,
                    }
                } else {
                    RelTimeStyle::Auto
                };
                return Ok(MessageNode::RelTime {
                    var: var_name,
                    style,
                });
            } else if !expr_type.is_empty() {
                // Custom formatter (unknown function type in ICU 1.0 syntax)
                let mut options = std::collections::HashMap::new();
                if parts.len() >= 3 {
                    let body = parts[2].trim();
                    for option in body.split_whitespace() {
                        if let Some(eq_pos) = option.find('=') {
                            options.insert(
                                option[..eq_pos].trim().to_string(),
                                option[eq_pos + 1..].trim().to_string(),
                            );
                        }
                    }
                }
                return Ok(MessageNode::Custom {
                    var: var_name,
                    literal_operand: None,
                    format: CustomFormat {
                        formatter: expr_type.to_string(),
                        options,
                    },
                });
            }
        }

        let trimmed = expr_str.trim();
        // Check for unescape marker: {- name}
        if let Some(rest) = trimmed.strip_prefix('-') {
            let bare_name = rest.trim_start_matches('$').trim().to_string();
            // Check for pipe default value syntax: {- name|Guest}
            if let Some(pipe_pos) = bare_name.find('|') {
                let name = bare_name[..pipe_pos].trim().into();
                let default = bare_name[pipe_pos + 1..].trim().into();
                // For raw variables with defaults, use RawVariableWithDefault (handled via 0x0C flags & 0x01)
                // We still mark as VariableWithDefault for now — the binary writer handles flags
                return Ok(MessageNode::VariableWithDefault { name, default });
            }
            return Ok(MessageNode::RawVariable(bare_name.into()));
        }

        // Check for pipe default value syntax: {name|Guest}
        if let Some(pipe_pos) = trimmed.find('|') {
            let name = trimmed[..pipe_pos].trim_start_matches('$').trim().into();
            let default = trimmed[pipe_pos + 1..].trim().into();
            return Ok(MessageNode::VariableWithDefault { name, default });
        }

        let var_name = trimmed.trim_start_matches('$').to_string();

        // Check for MF2 inline function syntax: {$var :number} or {$var :number style=percent}
        if let Some(func_part) = var_name.split_once(':') {
            let base_var: IStr = func_part.0.trim().into();
            let func_spec = func_part.1.trim();
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
                return Ok(MessageNode::Number {
                    var: base_var,
                    style,
                });
            }
            if func_spec.starts_with(":string") || func_spec.starts_with("string") {
                let default = if let Some(start) = func_spec.find("default=\"") {
                    let after = &func_spec[start + 9..];
                    after.split('"').next().unwrap_or("").to_string()
                } else if let Some(start) = func_spec.find("default=") {
                    let after = &func_spec[start + 8..];
                    after.split_whitespace().next().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                return Ok(MessageNode::VariableWithDefault {
                    name: base_var,
                    default: default.into(),
                });
            }
            if func_spec == ":date" || func_spec == "date" || func_spec == "Date" {
                return Ok(MessageNode::Date {
                    var: base_var,
                    style: DateStyle::Date,
                });
            }
            if func_spec == ":time" || func_spec == "time" || func_spec == "Time" {
                return Ok(MessageNode::Date {
                    var: base_var,
                    style: DateStyle::Time,
                });
            }
            if func_spec == ":datetime" || func_spec == "datetime" || func_spec == "DateTime" {
                return Ok(MessageNode::Date {
                    var: base_var,
                    style: DateStyle::DateTime,
                });
            }
            if func_spec.starts_with(":list") || func_spec.starts_with("list") {
                let style = if func_spec.contains("style=or") {
                    ListStyle::Disjunction
                } else if func_spec.contains("style=unit") {
                    ListStyle::Unit
                } else {
                    ListStyle::Conjunction
                };
                return Ok(MessageNode::List {
                    var: base_var,
                    style,
                });
            }
            if func_spec.starts_with(":relativedelta")
                || func_spec.starts_with("relativedelta")
                || func_spec.starts_with(":relativetime")
                || func_spec.starts_with("relativetime")
            {
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
                return Ok(MessageNode::RelTime {
                    var: base_var,
                    style,
                });
            }
        }

        Ok(MessageNode::Variable(var_name.into()))
    }
}

/// Parses an interval plural string like `(0)[no messages];(1)[one message];(2-7)[a few messages]`
/// into standard plural cases.
///
/// Returns:
/// - `Ok(None)` if the string is not an interval pattern (does not start with `(`).
/// - `Ok(Some(cases))` on a successful parse.
/// - `Err(message)` when the input *is* an interval pattern but is malformed
///   (unbalanced brackets, unparseable range, or exceeds `MAX_PLURAL_RULES_PER_MESSAGE`).
pub fn parse_interval_plural(input: &str) -> Result<Option<PluralCases>, String> {
    let input = input.trim();
    if !input.starts_with('(') {
        return Ok(None);
    }

    let mut cases = Vec::new();
    let mut remaining = input;

    while !remaining.is_empty() {
        remaining = remaining.trim();
        if !remaining.starts_with('(') {
            break;
        }

        // Find the matching ) for the range specifier
        let close_paren = remaining
            .find(')')
            .ok_or_else(|| "Unmatched '(' in interval plural".to_string())?;
        let range_part = &remaining[0..close_paren + 1]; // e.g., "(0)" or "(2-7)" or "(7-inf)"

        // Find the matching [ for body
        let open_body = remaining[close_paren..]
            .find('[')
            .ok_or_else(|| "Missing '[' in interval plural".to_string())?;
        let body_start = close_paren + open_body + 1;
        let body_end = remaining[body_start..]
            .find(']')
            .ok_or_else(|| "Unmatched '[' in interval plural".to_string())?;
        let body_str = &remaining[body_start..body_start + body_end];
        remaining = &remaining[body_start + body_end + 1..];
        if remaining.starts_with(';') {
            remaining = &remaining[1..];
        }

        if cases.len() >= MAX_PLURAL_RULES_PER_MESSAGE {
            return Err(format!(
                "Interval plural exceeds {MAX_PLURAL_RULES_PER_MESSAGE} cases"
            ));
        }

        // Parse range: (exact), (min-max), or (min-inf)
        let inner = range_part.trim_start_matches('(').trim_end_matches(')');
        let mut parser = MessageParser::new(body_str);
        let nodes = parser
            .parse_pattern(false)
            .map_err(|e| format!("Invalid interval plural body: {e}"))?;

        if let Some(hyphen) = inner.find('-') {
            let min_str = inner[..hyphen].trim();
            let max_str = inner[hyphen + 1..].trim();
            let min: i32 = min_str
                .parse()
                .map_err(|_| format!("Invalid interval minimum: {min_str}"))?;
            let max: i32 = if max_str == "inf" || max_str == "∞" {
                i32::MAX
            } else {
                max_str
                    .parse()
                    .map_err(|_| format!("Invalid interval maximum: {max_str}"))?
            };
            if min > max {
                return Err(format!("Interval range min > max: {min} > {max}"));
            }
            cases.push((PluralCaseKey::Range(min, max), nodes));
        } else {
            let val: f64 = inner
                .parse()
                .map_err(|_| format!("Invalid interval value: {inner}"))?;
            cases.push((PluralCaseKey::Exact(val), nodes));
        }
    }

    if cases.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cases))
    }
}

/// Expands `#` to `{var_name}` (but not `\#`) in a plural case pattern string.
/// This must be called per-body with the immediately enclosing plural's variable name.
///
/// Returns a borrowed slice when the pattern contains no `#` (the common case), avoiding
/// an allocation per plural case body.
fn expand_hash_for_var<'a>(pattern: &'a str, var_name: &str) -> std::borrow::Cow<'a, str> {
    // Fast path: no '#' at all → nothing to expand, reuse the input slice verbatim.
    if !pattern.contains('#') {
        return std::borrow::Cow::Borrowed(pattern);
    }
    let mut result = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'#') {
            chars.next();
            result.push('#');
        } else if c == '#' {
            result.push('{');
            result.push_str(var_name);
            result.push('}');
        } else {
            result.push(c);
        }
    }
    std::borrow::Cow::Owned(result)
}

fn parse_cases(mut input: &str, var_name: &str) -> Result<PluralCases, String> {
    let mut cases = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.is_empty() {
            break;
        }

        let brace_idx = input
            .find('{')
            .ok_or_else(|| format!("Expected case body in: {}", input))?;
        let key_str = input[..brace_idx].trim();
        let key = if let Some(stripped) = key_str.strip_prefix('=') {
            let val = stripped
                .trim()
                .parse::<f64>()
                .map_err(|_| "Invalid exact plural value")?;
            PluralCaseKey::Exact(val)
        } else {
            match key_str {
                "zero" => PluralCaseKey::Zero,
                "one" => PluralCaseKey::One,
                "two" => PluralCaseKey::Two,
                "few" => PluralCaseKey::Few,
                "many" => PluralCaseKey::Many,
                "other" => PluralCaseKey::Other,
                _ => return Err(format!("Invalid plural key: {}", key_str)),
            }
        };

        let (end_idx, pattern_str) = extract_case_body(input, brace_idx)?;

        let expanded = expand_hash_for_var(pattern_str, var_name);
        let mut parser = MessageParser::new(expanded.as_ref());
        let pattern_nodes = parser.parse_pattern(false)?;

        cases.push((key, pattern_nodes));
        input = &input[end_idx + 1..];
    }
    Ok(cases)
}

fn parse_select_cases(mut input: &str) -> Result<Vec<(IStr, Vec<MessageNode>)>, String> {
    let mut cases = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.is_empty() {
            break;
        }

        let brace_idx = input
            .find('{')
            .ok_or_else(|| format!("Expected case body in: {}", input))?;
        let key_str: IStr = input[..brace_idx].trim().into();

        let (end_idx, pattern_str) = extract_case_body(input, brace_idx)?;

        let mut parser = MessageParser::new(pattern_str);
        let pattern_nodes = parser.parse_pattern(false)?;

        cases.push((key_str, pattern_nodes));
        input = &input[end_idx + 1..];
    }
    Ok(cases)
}

/// Extracts all unique interpolation variable names from a parsed message AST.
/// Returns a deduplicated, unsorted Vec of parameter names.
pub fn extract_params(nodes: &[MessageNode]) -> Vec<String> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out = Vec::new();
    collect_params(nodes, &mut seen, &mut out);
    out
}

fn collect_params<'a>(
    nodes: &'a [MessageNode],
    seen: &mut std::collections::HashSet<&'a str>,
    out: &mut Vec<String>,
) {
    for node in nodes {
        match node {
            MessageNode::Variable(v)
            | MessageNode::RawVariable(v)
            | MessageNode::Number { var: v, .. }
            | MessageNode::Date { var: v, .. }
            | MessageNode::RelTime { var: v, .. }
            | MessageNode::List { var: v, .. } => {
                if seen.insert(v) {
                    out.push(v.to_string());
                }
            }
            MessageNode::VariableWithDefault { name, .. } => {
                if seen.insert(name) {
                    out.push(name.to_string());
                }
            }
            MessageNode::Plural {
                var,
                ordinal: _,
                cases,
            } => {
                if seen.insert(var) {
                    out.push(var.to_string());
                }
                for (_, body) in cases {
                    collect_params(body, seen, out);
                }
            }
            MessageNode::Select { var, cases } => {
                if seen.insert(var) {
                    out.push(var.to_string());
                }
                for (_, body) in cases {
                    collect_params(body, seen, out);
                }
            }
            MessageNode::Custom { var, .. } => {
                if !var.is_empty() && seen.insert(var) {
                    out.push(var.to_string());
                }
            }
            MessageNode::Mf2Match {
                selectors,
                inputs,
                locals,
                variants,
            } => {
                for name in inputs.keys() {
                    if seen.insert(name.as_str()) {
                        out.push(name.clone());
                    }
                }
                for name in selectors {
                    if !locals.contains_key(&**name) && seen.insert(name) {
                        out.push(name.to_string());
                    }
                }
                for (_, pat) in variants {
                    collect_params(pat, seen, out);
                }
            }
            MessageNode::Text(_) | MessageNode::KeyRef(_) | MessageNode::Markup { .. } => {}
        }
    }
}

#[cfg(test)]
mod interval_plural_tests {
    use super::*;

    #[test]
    fn parse_exact_interval() {
        let cases = parse_interval_plural("(0)[none];(1)[one];(2)[two]")
            .unwrap()
            .unwrap();
        assert_eq!(cases.len(), 3);
    }

    #[test]
    fn parse_range_interval() {
        let cases = parse_interval_plural("(0)[none];(1)[one];(2-7)[few]")
            .unwrap()
            .unwrap();
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[2].0, PluralCaseKey::Range(2, 7));
    }

    #[test]
    fn parse_large_range_not_expanded() {
        let cases = parse_interval_plural("(0)[none];(4-500)[many]")
            .unwrap()
            .unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[1].0, PluralCaseKey::Range(4, 500));
    }

    #[test]
    fn parse_inf_interval() {
        let cases = parse_interval_plural("(0)[none];(1)[one];(2-7)[few];(7-inf)[many]")
            .unwrap()
            .unwrap();
        assert_eq!(cases.len(), 4);
        assert_eq!(cases[3].0, PluralCaseKey::Range(7, i32::MAX));
    }

    #[test]
    fn rejects_too_many_rules() {
        let mut input = String::new();
        for i in 0..=MAX_PLURAL_RULES_PER_MESSAGE {
            if i > 0 {
                input.push(';');
            }
            input.push_str(&format!("({i})[x]"));
        }
        // Too-many-rules is a hard error, not a silent fallthrough.
        assert!(parse_interval_plural(&input).is_err());
    }

    #[test]
    fn non_interval_returns_none() {
        // Non-interval input is Ok(None), not an error.
        assert!(parse_interval_plural("Hello {name}").unwrap().is_none());
    }
}

#[cfg(test)]
mod param_extraction_tests {
    use super::*;

    #[test]
    fn extracts_variable_params() {
        let nodes = MessageParser::new("Hello {name}, you have {count} messages")
            .parse()
            .unwrap();
        let mut params = extract_params(&nodes);
        params.sort();
        assert_eq!(params, vec!["count", "name"]);
    }

    #[test]
    fn extracts_plural_variable() {
        let nodes = MessageParser::new("{count, plural, one {# item} other {# items}}")
            .parse()
            .unwrap();
        let params = extract_params(&nodes);
        assert!(params.contains(&"count".to_string()));
    }

    #[test]
    fn extracts_select_variable() {
        let nodes = MessageParser::new("{gender, select, male {He} female {She} other {They}}")
            .parse()
            .unwrap();
        let params = extract_params(&nodes);
        assert!(params.contains(&"gender".to_string()));
    }

    #[test]
    fn no_params_for_static_text() {
        let nodes = MessageParser::new("Hello world").parse().unwrap();
        let params = extract_params(&nodes);
        assert!(params.is_empty());
    }

    #[test]
    fn deduplicates_repeated_vars() {
        let nodes = MessageParser::new("{name} and {name} again")
            .parse()
            .unwrap();
        let params = extract_params(&nodes);
        let count = params.iter().filter(|s| *s == "name").count();
        assert_eq!(count, 1, "duplicate params should be deduplicated");
    }
}

#[cfg(test)]
mod number_tests {
    use super::*;

    #[test]
    fn parses_icu1_number_function() {
        let parser = MessageParser::new("Price: {price, number}");
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(matches!(&nodes[1], MessageNode::Number { var, style }
            if &var[..] == "price" && *style == NumberStyle::Decimal));
    }

    #[test]
    fn parses_mf2_number_function() {
        let parser = MessageParser::new("{$amount :number style=percent}");
        let nodes = parser.parse().unwrap();
        assert!(matches!(&nodes[0], MessageNode::Number { var, style }
            if &var[..] == "amount" && *style == NumberStyle::Percent));
    }
}

#[cfg(test)]
mod multi_match_tests {
    use super::*;

    #[test]
    fn parse_multi_variable_match_select_first() {
        let input = r#"match $count $gender
when one masculine {1 hombre}
when one feminine  {1 mujer}
when *   *         {{count}}"#;
        let nodes = MessageParser::new(input).parse().unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MessageNode::Select { var, cases } => {
                assert_eq!(&var[..], "gender");
                assert_eq!(cases.len(), 3); // masculine, feminine, other
                for (_, pattern) in cases {
                    assert_eq!(pattern.len(), 1);
                    assert!(matches!(pattern[0], MessageNode::Plural { .. }));
                }
            }
            other => panic!("Expected Select node, got {:?}", other),
        }
    }

    #[test]
    fn parse_multi_variable_with_all_wildcards() {
        let input = r#"match $a $b
when * * {fallback}"#;
        let nodes = MessageParser::new(input).parse().unwrap();
        match &nodes[0] {
            MessageNode::Select { var, cases } => {
                assert_eq!(&var[..], "b");
                assert_eq!(cases.len(), 1);
                assert_eq!(cases[0].0, "other".into());
            }
            other => panic!("Expected Select, got {:?}", other),
        }
    }

    #[test]
    fn parse_multi_variable_rejects_three_selectors() {
        let input = r#"match $a $b $c
when x y z {value}"#;
        let result = MessageParser::new(input).parse();
        assert!(result.is_err());
    }

    #[test]
    fn parse_multi_variable_select_first_string_keys() {
        let input = r#"match $type $level
when info debug {Info Debug}
when info error {Info Error}
when warn * {Warning}
when *   * {Unknown}"#;
        let nodes = MessageParser::new(input).parse().unwrap();
        match &nodes[0] {
            MessageNode::Select { var, cases: _ } => {
                assert_eq!(
                    &var[..],
                    "level",
                    "innermost var should be the 2nd selector"
                );
            }
            other => panic!("Expected Select, got {:?}", other),
        }
    }

    #[test]
    fn single_variable_match_still_works() {
        let input = r#"match $count
when one {1 item}
when other {{count} items}"#;
        let nodes = MessageParser::new(input).parse().unwrap();
        match &nodes[0] {
            MessageNode::Plural { var, .. } => {
                assert_eq!(&var[..], "count");
            }
            other => panic!("Expected Plural, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod custom_formatter_parse_tests {
    use super::*;

    #[test]
    fn parses_unknown_function_as_custom() {
        let nodes = MessageParser::new("{name, uppercase}").parse().unwrap();
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MessageNode::Custom { var, format, .. } => {
                assert_eq!(&var[..], "name");
                assert_eq!(format.formatter, "uppercase");
                assert!(format.options.is_empty());
            }
            other => panic!("Expected Custom, got {:?}", other),
        }
    }

    #[test]
    fn parses_custom_with_options() {
        let nodes = MessageParser::new("{val, prefix, prefix=Hello_}")
            .parse()
            .unwrap();
        match &nodes[0] {
            MessageNode::Custom { var, format, .. } => {
                assert_eq!(&var[..], "val");
                assert_eq!(format.formatter, "prefix");
                assert_eq!(
                    format.options.get("prefix").map(|s| s.as_str()),
                    Some("Hello_")
                );
            }
            other => panic!("Expected Custom, got {:?}", other),
        }
    }

    #[test]
    fn custom_node_is_extracted_as_param() {
        let nodes = MessageParser::new("Hello {name, uppercase}!")
            .parse()
            .unwrap();
        let params = extract_params(&nodes);
        assert!(params.contains(&"name".to_string()));
    }

    #[test]
    fn test_hash_escape_in_plural() {
        let nodes =
            MessageParser::new("{count, plural, one {escaped \\# here} other {normal # here}}")
                .parse()
                .unwrap();
        if let MessageNode::Plural { var, cases, .. } = &nodes[0] {
            assert_eq!(&var[..], "count");
            assert_eq!(cases.len(), 2);
            assert_eq!(cases[0].0, PluralCaseKey::One);
            assert_eq!(cases[0].1, vec![MessageNode::Text("escaped # here".into())]);
            assert_eq!(cases[1].0, PluralCaseKey::Other);
            assert_eq!(
                cases[1].1,
                vec![
                    MessageNode::Text("normal ".into()),
                    MessageNode::Variable("count".into()),
                    MessageNode::Text(" here".into())
                ]
            );
        } else {
            panic!("Expected Plural");
        }
    }
}

fn extract_case_body(remaining: &str, brace_idx: usize) -> Result<(usize, &str), String> {
    let mut brace_count = 0;
    let mut end_idx = None;
    for (idx, c) in remaining[brace_idx..].char_indices() {
        if c == '{' {
            brace_count += 1;
        } else if c == '}' {
            brace_count -= 1;
            if brace_count == 0 {
                end_idx = Some(brace_idx + idx);
                break;
            }
        }
    }
    let end_idx = end_idx.ok_or("Unmatched brace in match case")?;
    let pattern_str = &remaining[brace_idx + 1..end_idx];
    Ok((end_idx, pattern_str))
}

/// True when `key` is a CLDR plural category, an `=N` exact match, or a bare number.
/// Does NOT match the MF2 catch-all `"*"` — callers that accept it must check separately.
pub(crate) fn is_plural_key(key: &str) -> bool {
    matches!(key, "zero" | "one" | "two" | "few" | "many" | "other")
        || key.starts_with('=')
        || key.parse::<f64>().is_ok()
}

fn parse_mf2_match(input: &str) -> Result<MessageNode, String> {
    let input = input.trim();
    if !input.starts_with("match") {
        return Err("Not a match statement".to_string());
    }

    let mut lines = input.lines().map(|s| s.trim()).filter(|s| !s.is_empty());
    let match_line = lines.next().ok_or("Missing match line")?;

    let vars: Vec<&str> = match_line.split_whitespace().skip(1).collect();
    if vars.is_empty() {
        return Err("Match statement must have at least one selector variable".to_string());
    }
    if vars.len() > 2 {
        return Err("Only up to 2 selector variables supported in match".to_string());
    }

    let two_vars = vars.len() == 2 && vars[1].starts_with('$');

    if !two_vars {
        // --- SINGLE VARIABLE LOGIC (unchanged from original) ---
        let var_name: IStr = vars[0].trim_start_matches('$').into();
        let ordinal = vars.get(1).is_some_and(|v| v.trim() == "selectordinal");

        let mut cases = Vec::new();
        let mut is_select = false;

        let mut remaining = input[match_line.len()..].trim();
        while !remaining.is_empty() {
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                break;
            }

            let is_when = remaining.starts_with("when");
            let is_fallback = remaining.starts_with('*');

            if !is_when && !is_fallback {
                return Err(format!(
                    "Expected 'when' or '*' in match body: {}",
                    remaining
                ));
            }

            let brace_idx = remaining
                .find('{')
                .ok_or("Expected case pattern starting with '{'")?;
            let key_part = remaining[..brace_idx].trim();

            let key_str = if is_when {
                key_part
                    .split_whitespace()
                    .nth(1)
                    .ok_or("Expected key after when")?
            } else {
                "other"
            };

            let (end_idx, pattern_str) = extract_case_body(remaining, brace_idx)?;

            let mut parser = MessageParser::new(pattern_str);
            let pattern_nodes = parser.parse_pattern(false)?;

            let plural_key = match key_str {
                "zero" => Some(PluralCaseKey::Zero),
                "one" => Some(PluralCaseKey::One),
                "two" => Some(PluralCaseKey::Two),
                "few" => Some(PluralCaseKey::Few),
                "many" => Some(PluralCaseKey::Many),
                "other" => Some(PluralCaseKey::Other),
                _ => {
                    if let Ok(val) = key_str.parse::<f64>() {
                        Some(PluralCaseKey::Exact(val))
                    } else {
                        None
                    }
                }
            };

            if plural_key.is_none() {
                is_select = true;
            }

            cases.push((IStr::from(key_str), plural_key, pattern_nodes));
            remaining = &remaining[end_idx + 1..];
        }

        if is_select {
            let select_cases = cases
                .into_iter()
                .map(|(key, _, nodes)| (key, nodes))
                .collect();
            Ok(MessageNode::Select {
                var: var_name,
                cases: select_cases,
            })
        } else {
            let plural_cases = cases
                .into_iter()
                .map(|(_, pkey, nodes)| (pkey.unwrap_or(PluralCaseKey::Other), nodes))
                .collect();
            Ok(MessageNode::Plural {
                var: var_name,
                ordinal,
                cases: plural_cases,
            })
        }
    } else {
        // --- TWO-VARIABLE LOGIC ---
        let var1: IStr = vars[0].trim_start_matches('$').into();
        let var2: IStr = vars[1].trim_start_matches('$').into();

        // Parse all case entries with compound keys
        let mut entries: Vec<(String, String, Vec<MessageNode>)> = Vec::new();

        let mut remaining = input[match_line.len()..].trim();
        while !remaining.is_empty() {
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                break;
            }

            let is_when = remaining.starts_with("when");
            if !is_when {
                return Err(format!(
                    "Expected 'when' in match body for multi-variable match: {}",
                    remaining
                ));
            }

            let brace_idx = remaining
                .find('{')
                .ok_or("Expected case pattern starting with '{'")?;
            let key_part = remaining[..brace_idx].trim();

            let keys: Vec<&str> = key_part.split_whitespace().skip(1).collect();
            if keys.len() != 2 {
                return Err(format!(
                    "Expected 2 keys after 'when' for 2-variable match, got {}",
                    keys.len()
                ));
            }

            let (end_idx, pattern_str) = extract_case_body(remaining, brace_idx)?;

            let mut parser = MessageParser::new(pattern_str);
            let pattern_nodes = parser.parse_pattern(false)?;

            entries.push((keys[0].to_string(), keys[1].to_string(), pattern_nodes));
            remaining = &remaining[end_idx + 1..];
        }

        if entries.is_empty() {
            return Err("No case entries in match body".to_string());
        }

        // Group by key2 (the second selector variable — becomes the outer node)
        let mut groups: std::collections::BTreeMap<String, Vec<(String, Vec<MessageNode>)>> =
            std::collections::BTreeMap::new();
        let mut outer_wildcards: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for (k1, k2, nodes) in entries {
            let is_wildcard = k2 == "*";
            // Map wildcard "*" to "other" for the outer key
            let outer_key = if is_wildcard { "other".to_string() } else { k2 };
            if is_wildcard {
                outer_wildcards.insert(outer_key.clone());
            }
            groups.entry(outer_key).or_default().push((k1, nodes));
        }

        // For each group, determine if inner is plural or select and build the inner node
        let mut outer_cases: Vec<(IStr, Vec<MessageNode>)> = Vec::new();

        for (outer_key, sub_entries) in &groups {
            // Determine if sub-keys are plural or select
            let sub_is_plural = sub_entries
                .iter()
                .all(|(k, _)| k == "*" || is_plural_key(k));

            let inner_pattern = if sub_is_plural {
                let plural_cases: PluralCases = sub_entries
                    .iter()
                    .map(|(k, nodes)| {
                        let plural_key = if k == "*" {
                            PluralCaseKey::Other
                        } else {
                            match k.as_str() {
                                "zero" => PluralCaseKey::Zero,
                                "one" => PluralCaseKey::One,
                                "two" => PluralCaseKey::Two,
                                "few" => PluralCaseKey::Few,
                                "many" => PluralCaseKey::Many,
                                "other" => PluralCaseKey::Other,
                                _ => {
                                    if let Some(stripped) = k.strip_prefix('=') {
                                        PluralCaseKey::Exact(
                                            stripped.trim().parse::<f64>().unwrap_or(f64::NAN),
                                        )
                                    } else if let Ok(val) = k.parse::<f64>() {
                                        PluralCaseKey::Exact(val)
                                    } else {
                                        PluralCaseKey::Other
                                    }
                                }
                            }
                        };
                        (plural_key, nodes.clone())
                    })
                    .collect();
                vec![MessageNode::Plural {
                    var: var1.clone(),
                    ordinal: false,
                    cases: plural_cases,
                }]
            } else {
                let select_cases: Vec<(IStr, Vec<MessageNode>)> = sub_entries
                    .iter()
                    .map(|(k, nodes)| {
                        let select_key = if k == "*" { "other" } else { k.as_str() };
                        (IStr::from(select_key), nodes.clone())
                    })
                    .collect();
                vec![MessageNode::Select {
                    var: var1.clone(),
                    cases: select_cases,
                }]
            };

            outer_cases.push((IStr::from(outer_key.as_str()), inner_pattern));
        }

        // Determine if the outer is select or plural (based on outer keys)
        // Keys that originated from wildcards are excluded from the plural check
        // since "*" is ambiguous — it maps to "other" for both plural and select.
        // If ALL outer keys are wildcard-originated, default to Select.
        let outer_is_plural = groups.keys().all(|k| {
            if outer_wildcards.contains(k.as_str()) {
                false // wildcard-originated "other" is ambiguous — don't assume plural
            } else {
                is_plural_key(k)
            }
        });

        if outer_is_plural {
            let plural_cases: PluralCases = outer_cases
                .into_iter()
                .map(|(k, nodes)| {
                    let plural_key = match &*k {
                        "zero" => PluralCaseKey::Zero,
                        "one" => PluralCaseKey::One,
                        "two" => PluralCaseKey::Two,
                        "few" => PluralCaseKey::Few,
                        "many" => PluralCaseKey::Many,
                        "other" => PluralCaseKey::Other,
                        _ => {
                            if let Some(stripped) = k.strip_prefix('=') {
                                PluralCaseKey::Exact(
                                    stripped.trim().parse::<f64>().unwrap_or(f64::NAN),
                                )
                            } else if let Ok(val) = k.parse::<f64>() {
                                PluralCaseKey::Exact(val)
                            } else {
                                PluralCaseKey::Other
                            }
                        }
                    };
                    (plural_key, nodes)
                })
                .collect();
            Ok(MessageNode::Plural {
                var: var2,
                ordinal: false,
                cases: plural_cases,
            })
        } else {
            Ok(MessageNode::Select {
                var: var2,
                cases: outer_cases,
            })
        }
    }
}
