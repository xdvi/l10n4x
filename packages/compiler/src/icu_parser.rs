//! ICU MessageFormat Parsing and Representation.
//!
//! ### Supported ICU MessageFormat 2.0 (MF2) Subset:
//! - **Single Selector**: Only a single variable selector is supported in `match` statements (e.g. `match $count`).
//! - **Fallback Catch-all**: Wildcard `*` patterns in match bodies map to the default `other` case.
//! - **Standard Variables**: Simple braced placeholders (e.g. `{name}`) are parsed as variable node interpolation.
//!
//! ### Unsupported MF2 Features:
//! - **Multiple Selectors**: Multiple selector keys in a single match block are not supported.
//! - **Local Declarations**: inline variable binding via `local` statements is not supported.
//! - **Formatting Functions**: registry operations/functions like `.number` or `.datetime` are not implemented.

#[derive(Debug, Clone, PartialEq)]
pub enum MessageNode {
    Text(String),
    Variable(String),
    Plural {
        var: String,
        cases: Vec<(PluralCaseKey, Vec<MessageNode>)>,
    },
    Select {
        var: String,
        cases: Vec<(String, Vec<MessageNode>)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluralCaseKey {
    Exact(f64),
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

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
        if trimmed.starts_with("match") {
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
            if c == '{' {
                self.chars.next(); // consume '{'
                if !current_text.is_empty() {
                    nodes.push(MessageNode::Text(core::mem::take(&mut current_text)));
                }
                let node = self.parse_expression()?;
                nodes.push(node);
            } else if c == '}' && in_brace {
                break;
            } else {
                self.chars.next();
                current_text.push(c);
            }
        }

        if !current_text.is_empty() {
            nodes.push(MessageNode::Text(current_text));
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

        let parts: Vec<&str> = expr_str.splitn(3, ',').collect();
        if parts.len() == 3 {
            let var_name = parts[0].trim().trim_start_matches('$').to_string();
            let expr_type = parts[1].trim();
            let body = parts[2].trim();

            if expr_type == "plural" {
                let cases = parse_cases(body, &var_name)?;
                return Ok(MessageNode::Plural {
                    var: var_name,
                    cases,
                });
            } else if expr_type == "select" {
                let cases = parse_select_cases(body)?;
                return Ok(MessageNode::Select {
                    var: var_name,
                    cases,
                });
            }
        }

        let var_name = expr_str.trim().trim_start_matches('$').to_string();
        Ok(MessageNode::Variable(var_name))
    }
}

fn parse_cases(
    mut input: &str,
    var_name: &str,
) -> Result<Vec<(PluralCaseKey, Vec<MessageNode>)>, String> {
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

        let mut brace_count = 0;
        let mut end_idx = None;
        for (idx, c) in input[brace_idx..].char_indices() {
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

        let end_idx = end_idx.ok_or("Unmatched brace in case pattern")?;
        let pattern_str = &input[brace_idx + 1..end_idx];

        let mut preprocessed = String::new();
        let mut chars = pattern_str.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some('#') = chars.peek() {
                    chars.next();
                    preprocessed.push('#');
                } else {
                    preprocessed.push('\\');
                }
            } else if c == '#' {
                preprocessed.push('{');
                preprocessed.push_str(var_name);
                preprocessed.push('}');
            } else {
                preprocessed.push(c);
            }
        }

        let mut parser = MessageParser::new(&preprocessed);
        let pattern_nodes = parser.parse_pattern(false)?;

        cases.push((key, pattern_nodes));
        input = &input[end_idx + 1..];
    }
    Ok(cases)
}

fn parse_select_cases(mut input: &str) -> Result<Vec<(String, Vec<MessageNode>)>, String> {
    let mut cases = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.is_empty() {
            break;
        }

        let brace_idx = input
            .find('{')
            .ok_or_else(|| format!("Expected case body in: {}", input))?;
        let key_str = input[..brace_idx].trim().to_string();

        let mut brace_count = 0;
        let mut end_idx = None;
        for (idx, c) in input[brace_idx..].char_indices() {
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

        let end_idx = end_idx.ok_or("Unmatched brace in case pattern")?;
        let pattern_str = &input[brace_idx + 1..end_idx];

        let mut parser = MessageParser::new(pattern_str);
        let pattern_nodes = parser.parse_pattern(false)?;

        cases.push((key_str, pattern_nodes));
        input = &input[end_idx + 1..];
    }
    Ok(cases)
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
    let var_name = vars[0].trim_start_matches('$').to_string();

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

        cases.push((key_str.to_string(), plural_key, pattern_nodes));
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
            cases: plural_cases,
        })
    }
}
