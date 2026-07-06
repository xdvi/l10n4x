//! ICU MessageFormat 2.0 conformance test functions (`:test:*`).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;

use crate::float_math;

/// Resolved value of a `:test:function`, `:test:select`, or `:test:format` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct TestFunctionValue {
    /// Numeric input operand.
    pub input: f64,
    /// Decimal places for formatting / matching (0 or 1).
    pub decimal_places: u8,
    /// Selector resolution should fail (bad-selector).
    pub fails_select: bool,
    /// Formatting should fail (bad-option).
    pub fails_format: bool,
}

impl TestFunctionValue {
    /// Fallback value used when operand resolution fails.
    pub fn bad_operand_fallback() -> Self {
        Self {
            input: 0.0,
            decimal_places: 0,
            fails_select: true,
            fails_format: false,
        }
    }

    /// Fallback value used when option resolution fails.
    pub fn bad_option_fallback() -> Self {
        Self {
            input: 0.0,
            decimal_places: 0,
            fails_select: true,
            fails_format: true,
        }
    }
}

/// Parse `k=v,k=v` option strings from bytecode.
pub fn parse_options(options_str: &str) -> BTreeMap<String, String> {
    let mut opts = BTreeMap::new();
    if options_str.is_empty() {
        return opts;
    }
    for pair in options_str.split(',') {
        if let Some(eq_pos) = pair.find('=') {
            opts.insert(
                pair[..eq_pos].trim().to_string(),
                pair[eq_pos + 1..].trim().to_string(),
            );
        }
    }
    opts
}

fn parse_number_operand(raw: &str) -> Option<f64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn apply_decimal_places_option(
    mut value: TestFunctionValue,
    options: &BTreeMap<String, String>,
    resolved: &BTreeMap<String, TestFunctionValue>,
) -> TestFunctionValue {
    let Some(raw) = options.get("decimalPlaces") else {
        return value;
    };
    if let Some(places) = resolve_option_u8(raw, resolved) {
        value.decimal_places = places;
        return value;
    }
    TestFunctionValue::bad_option_fallback()
}

fn apply_fails_option(
    mut value: TestFunctionValue,
    options: &BTreeMap<String, String>,
) -> TestFunctionValue {
    let Some(raw) = options.get("fails") else {
        return value;
    };
    match raw.as_str() {
        "never" => value,
        "select" => {
            value.fails_select = true;
            value
        }
        "format" => {
            value.fails_format = true;
            value
        }
        "always" => {
            value.fails_select = true;
            value.fails_format = true;
            value
        }
        _ => TestFunctionValue::bad_option_fallback(),
    }
}

fn resolve_option_u8(raw: &str, resolved: &BTreeMap<String, TestFunctionValue>) -> Option<u8> {
    if let Some(v) = parse_number_operand(raw) {
        let i = float_math::trunc(v) as i64;
        if (0..=1).contains(&i) {
            return Some(i as u8);
        }
        return None;
    }
    if let Some(rv) = resolved.get(raw.trim_start_matches('$')) {
        let i = float_math::trunc(rv.input) as i64;
        if (0..=1).contains(&i) {
            return Some(i as u8);
        }
    }
    None
}

fn finalize_test_formatter(formatter: &str, mut value: TestFunctionValue) -> TestFunctionValue {
    if formatter == "test:format" {
        value.fails_select = true;
    }
    value
}

/// Resolve a `:test:*` function from a numeric operand string.
pub fn resolve_test_function_from_operand(
    operand: &str,
    formatter: &str,
    options: &BTreeMap<String, String>,
    resolved: &BTreeMap<String, TestFunctionValue>,
) -> TestFunctionValue {
    if !matches!(formatter, "test:function" | "test:select" | "test:format") {
        return TestFunctionValue::bad_operand_fallback();
    }

    let Some(input) = parse_number_operand(operand) else {
        return TestFunctionValue::bad_operand_fallback();
    };

    let mut value = TestFunctionValue {
        input,
        decimal_places: 0,
        fails_select: false,
        fails_format: false,
    };
    value = apply_decimal_places_option(value, options, resolved);
    if value.fails_select && value.input == 0.0 && value.decimal_places == 0 && value.fails_format {
        return value;
    }
    value = apply_fails_option(value, options);
    if value.fails_select && value.input == 0.0 && value.fails_format {
        return value;
    }
    finalize_test_formatter(formatter, value)
}

/// Resolve a chained `:test:*` expression from an existing resolved value.
pub fn resolve_test_function_from_value(
    source: &TestFunctionValue,
    formatter: &str,
    options: &BTreeMap<String, String>,
    resolved: &BTreeMap<String, TestFunctionValue>,
) -> TestFunctionValue {
    if !matches!(formatter, "test:function" | "test:select" | "test:format") {
        return TestFunctionValue::bad_operand_fallback();
    }
    let mut value = source.clone();
    value = apply_decimal_places_option(value, options, resolved);
    if value.fails_select && value.input == 0.0 && value.fails_format {
        return value;
    }
    value = apply_fails_option(value, options);
    if value.fails_select && value.input == 0.0 && value.fails_format {
        return value;
    }
    finalize_test_formatter(formatter, value)
}

/// Match(`rv`, `key`) for `:test:select` / `:test:function` selectors.
pub fn test_select_match(rv: &TestFunctionValue, key: &str) -> Option<bool> {
    if rv.fails_select {
        return None;
    }
    if key == "*" {
        return Some(true);
    }
    if (rv.input - 1.0).abs() < 1e-9 && rv.decimal_places == 1 {
        return Some(key == "1.0" || key == "1");
    }
    if (rv.input - 1.0).abs() < 1e-9 && rv.decimal_places == 0 {
        return Some(key == "1");
    }
    Some(false)
}

/// Format a `:test:*` value as a decimal string.
pub fn format_test_function(rv: &TestFunctionValue) -> Option<String> {
    if rv.fails_format {
        return None;
    }
    let mut out = String::new();
    let abs = rv.input.abs();
    if rv.input < 0.0 {
        out.push('-');
    }
    let int_part = float_math::floor(abs) as i64;
    out.push_str(&int_part.to_string());
    if rv.decimal_places == 1 {
        out.push('.');
        let frac = float_math::floor((abs - float_math::floor(abs)) * 10.0) as i64;
        out.push(char::from(b'0' + frac as u8));
    }
    Some(out)
}

/// Serialized MF2 declaration expression (from opcode `0x0E`), borrowing its
/// strings directly from the message bytecode — decoded on every render, so
/// owned `String`s here would mean four allocations per declaration per call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeclExpr<'a> {
    /// Variable operand (`$x`), empty for literals or standalone functions.
    pub var: &'a str,
    /// Number literal operand, empty when using `var`.
    pub literal: &'a str,
    /// Function name (e.g. `test:select`), empty for bare variable refs.
    pub formatter: &'a str,
    /// Comma-separated `k=v` options string.
    pub options: &'a str,
}

impl DeclExpr<'_> {
    /// True when this expression applies a `:test:*` function.
    pub fn has_test_function(&self) -> bool {
        matches!(
            self.formatter,
            "test:function" | "test:select" | "test:format"
        )
    }

    /// True when this is a bare variable reference (`{$x}`).
    pub fn is_var_ref(&self) -> bool {
        !self.var.is_empty()
            && self.literal.is_empty()
            && self.formatter.is_empty()
            && self.options.is_empty()
    }
}

/// Resolved selector state for MF2 `.match` (test functions or string keys).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectorState {
    test: Option<TestFunctionValue>,
    string: Option<String>,
}

impl SelectorState {
    /// Build selector state from a resolved `:test:*` value.
    pub fn from_test(value: TestFunctionValue) -> Self {
        Self {
            test: Some(value),
            string: None,
        }
    }

    /// Build selector state from a string key (`:string` selectors).
    pub fn from_string(value: String) -> Self {
        Self {
            test: None,
            string: Some(value),
        }
    }
}

/// Resolve MF2 declaration expressions for selector matching.
pub fn resolve_selector_states<'a>(
    inputs: &[(&'a str, &DeclExpr<'a>)],
    locals: &[(&'a str, &DeclExpr<'a>)],
    params: &[(&str, &str)],
) -> BTreeMap<&'a str, SelectorState> {
    let mut resolved: BTreeMap<&str, SelectorState> = BTreeMap::new();

    for (name, expr) in inputs {
        let value = resolve_decl_to_state(expr, params, &resolved);
        resolved.insert(name, value);
    }

    for (name, expr) in locals {
        let value = resolve_decl_to_state(expr, params, &resolved);
        resolved.insert(name, value);
    }

    resolved
}

fn resolve_decl_to_state(
    expr: &DeclExpr<'_>,
    params: &[(&str, &str)],
    resolved: &BTreeMap<&str, SelectorState>,
) -> SelectorState {
    if expr.is_var_ref() {
        return resolved.get(expr.var).cloned().unwrap_or_else(|| {
            SelectorState::from_test(TestFunctionValue::bad_operand_fallback())
        });
    }

    if expr.formatter == "string" {
        if let Some((_, param_val)) = params.iter().find(|(k, _)| *k == expr.var) {
            return SelectorState::from_string(param_val.to_string());
        }
        if !expr.var.is_empty() {
            return SelectorState::from_string(expr.var.to_string());
        }
        if !expr.literal.is_empty() {
            return SelectorState::from_string(expr.literal.to_string());
        }
    }

    if expr.has_test_function() {
        let test_map: BTreeMap<String, TestFunctionValue> = resolved
            .iter()
            .filter_map(|(k, s)| s.test.clone().map(|t| (k.to_string(), t)))
            .collect();
        let opts = parse_options(expr.options);
        if !expr.var.is_empty() {
            if let Some(src) = test_map.get(expr.var) {
                return SelectorState::from_test(resolve_test_function_from_value(
                    src,
                    expr.formatter,
                    &opts,
                    &test_map,
                ));
            }
            if let Some((_, param_val)) = params.iter().find(|(k, _)| *k == expr.var) {
                return SelectorState::from_test(resolve_test_function_from_operand(
                    param_val,
                    expr.formatter,
                    &opts,
                    &test_map,
                ));
            }
            return SelectorState::from_test(TestFunctionValue::bad_operand_fallback());
        }
        if !expr.literal.is_empty() {
            return SelectorState::from_test(resolve_test_function_from_operand(
                expr.literal,
                expr.formatter,
                &opts,
                &test_map,
            ));
        }
        return SelectorState::from_test(TestFunctionValue::bad_operand_fallback());
    }

    SelectorState::from_test(TestFunctionValue::bad_operand_fallback())
}

fn quoted_variant_key(key: &str) -> Option<&str> {
    key.strip_prefix('|')
        .and_then(|rest| rest.strip_suffix('|'))
}

fn key_matches_state(state: &SelectorState, key: &str) -> bool {
    if let Some(literal) = quoted_variant_key(key) {
        if let Some(s) = &state.string {
            return literal == s;
        }
        return false;
    }
    if key == "*" {
        return true;
    }
    if let Some(s) = &state.string {
        return key == s;
    }
    if let Some(rv) = &state.test {
        return test_select_match(rv, key) == Some(true);
    }
    false
}

fn variant_matches(selector_values: &[SelectorState], keys: &[&str]) -> bool {
    for (i, key) in keys.iter().enumerate() {
        if i >= selector_values.len() {
            return false;
        }
        if !key_matches_state(&selector_values[i], key) {
            return false;
        }
    }
    true
}

/// Priority score for tie-breaking among multiple matching variants (BetterThan).
fn variant_priority(selector_values: &[SelectorState], keys: &[&str]) -> i32 {
    let mut score = 0i32;
    for (i, key) in keys.iter().enumerate() {
        if i >= selector_values.len() {
            break;
        }
        let rv = &selector_values[i];
        let match_key = quoted_variant_key(key).unwrap_or(key);
        if match_key == "*" {
            continue;
        }
        if let Some(test) = &rv.test {
            if better_than_key(test, match_key) {
                score += 100;
            } else if test_select_match(test, match_key) == Some(true) {
                score += 10;
            }
        } else if rv.string.as_deref() == Some(match_key) {
            score += 10;
        }
    }
    score
}

/// BetterThan(`rv`, `key1`, `key2`) — `key1 == '1.0'` is preferred when both match.
fn better_than_key(rv: &TestFunctionValue, key: &str) -> bool {
    key == "1.0" && (rv.input - 1.0).abs() < 1e-9 && rv.decimal_places == 1
}

/// Select the best matching variant for MF2 pattern selection.
pub fn select_mf2_variant(
    selector_values: &[SelectorState],
    variants: &[(alloc::vec::Vec<&str>, usize, usize)],
) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, i32)> = None;
    for (keys, pat_pos, pat_len) in variants {
        if !variant_matches(selector_values, keys) {
            continue;
        }
        let priority = variant_priority(selector_values, keys);
        if best.as_ref().is_none_or(|(_, _, p)| priority > *p) {
            best = Some((*pat_pos, *pat_len, priority));
        }
    }
    best.map(|(pos, len, _)| (pos, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_match_decimal_places() {
        let rv = TestFunctionValue {
            input: 1.0,
            decimal_places: 0,
            fails_select: false,
            fails_format: false,
        };
        assert_eq!(test_select_match(&rv, "1"), Some(true));
        assert_eq!(test_select_match(&rv, "1.0"), Some(false));

        let rv = TestFunctionValue {
            input: 1.0,
            decimal_places: 1,
            fails_select: false,
            fails_format: false,
        };
        assert_eq!(test_select_match(&rv, "1"), Some(true));
        assert_eq!(test_select_match(&rv, "1.0"), Some(true));
    }

    #[test]
    fn select_variant_for_test_select_one() {
        let rv = SelectorState::from_test(TestFunctionValue {
            input: 1.0,
            decimal_places: 0,
            fails_select: false,
            fails_format: false,
        });
        let variants = [
            (alloc::vec!["1.0"], 0usize, 3usize),
            (alloc::vec!["1"], 10, 1),
            (alloc::vec!["*"], 20, 5),
        ];
        let sel = select_mf2_variant(&[rv], &variants);
        assert_eq!(sel, Some((10, 1)));
    }

    #[test]
    fn format_test_function_basic() {
        let rv = TestFunctionValue {
            input: 1.0,
            decimal_places: 1,
            fails_select: false,
            fails_format: false,
        };
        assert_eq!(format_test_function(&rv).as_deref(), Some("1.0"));
    }
}
