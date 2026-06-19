//! Helpers for working with probe `checks` (filt-rs expressions).
//!
//! When a check fails it is far more useful to see *what the probe actually
//! observed* than to be told the expression didn't match. This module walks a
//! check's parsed expression tree (via filt-rs's [`ExprVisitor`]) to discover
//! which sample fields it consulted, then renders those fields and their values
//! — truncated to keep the message readable — as part of the failure message.

use filt_rs::{
    BinaryOperator, Expr, ExprVisitor, Filter, FilterValue, Function, Glob, LogicalOperator,
    UnaryOperator,
};

use crate::Sample;

/// The maximum number of fields to enumerate in a failure summary before the
/// remainder are collapsed into an "and N more" suffix.
const DEFAULT_MAX_FIELDS: usize = 6;

/// The maximum rendered length (in characters) of each field's value before it
/// is truncated with an ellipsis.
const DEFAULT_MAX_VALUE_LEN: usize = 64;

/// An [`ExprVisitor`] that collects, in order of first appearance and without
/// duplicates, the names of every sample field (property) a check references.
///
/// The visitor borrows each property name straight out of the expression tree
/// (note the shared `'a` lifetime), so collecting them allocates nothing beyond
/// the backing `Vec`. The `()` result type means each `visit_*` method records
/// into `self` and recurses rather than folding a value back up the tree.
#[derive(Default)]
struct FieldCollector<'a> {
    fields: Vec<&'a str>,
}

impl<'a> FieldCollector<'a> {
    fn record(&mut self, name: &'a str) {
        if !self.fields.contains(&name) {
            self.fields.push(name);
        }
    }
}

impl<'a> ExprVisitor<'a, ()> for FieldCollector<'a> {
    fn visit_literal(&mut self, _value: &'a FilterValue<'a>) {}

    fn visit_property(&mut self, name: &'a str) {
        self.record(name);
    }

    fn visit_function_call(&mut self, _function: &'a dyn Function, args: &'a [Expr<'a>]) {
        // A field may be passed as a function argument (e.g. `trim(http.body)`).
        for arg in args {
            self.visit_expr(arg);
        }
    }

    fn visit_binary(&mut self, left: &'a Expr<'a>, _operator: BinaryOperator, right: &'a Expr<'a>) {
        self.visit_expr(left);
        self.visit_expr(right);
    }

    fn visit_logical(
        &mut self,
        left: &'a Expr<'a>,
        _operator: LogicalOperator,
        right: &'a Expr<'a>,
    ) {
        self.visit_expr(left);
        self.visit_expr(right);
    }

    fn visit_unary(&mut self, _operator: UnaryOperator, right: &'a Expr<'a>) {
        self.visit_expr(right);
    }

    fn visit_like(&mut self, left: &'a Expr<'a>, _glob: &'a Glob) {
        self.visit_expr(left);
    }

    // `matches` (regex) support is always compiled: the workspace enables filt-rs's `regex` and
    // `visitor` features together, so the trait method and `CompiledRegex` are present.
    fn visit_matches(&mut self, left: &'a Expr<'a>, _regex: &'a filt_rs::CompiledRegex) {
        self.visit_expr(left);
    }
}

/// Returns the sample fields a check references, in order of first appearance
/// and without duplicates.
pub fn referenced_fields(check: &Filter) -> Vec<&str> {
    let mut collector = FieldCollector::default();
    check.visit(&mut collector);
    collector.fields
}

/// Renders the sample fields a check consulted and the values they held, to aid
/// debugging when the check fails — for example
/// `http.status=503, http.header.content-type="text/html…"`.
///
/// Returns an empty string when the check references no fields (e.g. a constant
/// expression), so callers can omit the "observed" clause entirely.
pub fn observed_fields(check: &Filter, sample: &Sample) -> String {
    observed_fields_with_limits(check, sample, DEFAULT_MAX_FIELDS, DEFAULT_MAX_VALUE_LEN)
}

/// [`observed_fields`] with explicit limits, exposed for testing.
fn observed_fields_with_limits(
    check: &Filter,
    sample: &Sample,
    max_fields: usize,
    max_value_len: usize,
) -> String {
    let fields = referenced_fields(check);
    if fields.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = fields
        .iter()
        .take(max_fields)
        .map(|field| {
            let value = truncate(&sample.get(*field).to_string(), max_value_len);
            format!("{field}={value}")
        })
        .collect();

    if fields.len() > max_fields {
        parts.push(format!("and {} more", fields.len() - max_fields));
    }

    parts.join(", ")
}

/// Builds the `(probe-level message, per-check detail)` pair for a failed check.
///
/// The probe-level message names the check so the top-line probe status has
/// context for which expression failed. The per-check detail deliberately omits
/// the expression — it is the key in the validations map, so repeating it would
/// be redundant — and both append the observed field values to aid debugging.
pub fn describe_failure(check: &Filter, sample: &Sample, reason: String) -> (String, String) {
    let observed = observed_fields(check, sample);
    let suffix = if observed.is_empty() {
        String::new()
    } else {
        format!(" Observed {observed}.")
    };

    let probe_message = format!("The check '{check}' {reason}.{suffix}");
    let detail = format!("{}.{suffix}", capitalise_first(&reason));
    (probe_message, detail)
}

/// Truncates `value` to at most `max_len` characters (respecting char
/// boundaries), appending an ellipsis when anything was dropped.
fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_string()
    } else {
        let kept: String = value.chars().take(max_len).collect();
        format!("{kept}…")
    }
}

/// Upper-cases the first character of `s`, leaving the rest untouched.
fn capitalise_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sample;

    fn filter(expr: &str) -> Filter {
        Filter::new(expr).expect("parse filter")
    }

    #[test]
    fn referenced_fields_are_collected_in_order_without_duplicates() {
        let check = filter(
            r#"http.status >= 200 && http.status < 300 && http.header.content-type contains "html""#,
        );
        assert_eq!(
            referenced_fields(&check),
            vec!["http.status", "http.header.content-type"]
        );
    }

    #[test]
    fn referenced_fields_includes_function_arguments() {
        let check = filter(r#"trim(http.body) == "ok""#);
        assert_eq!(referenced_fields(&check), vec!["http.body"]);
    }

    #[test]
    fn referenced_fields_is_empty_for_a_constant_expression() {
        let check = filter("true");
        assert!(referenced_fields(&check).is_empty());
    }

    #[test]
    fn referenced_fields_descends_through_unary_negation() {
        let check = filter("!http.healthy");
        assert_eq!(referenced_fields(&check), vec!["http.healthy"]);
    }

    #[test]
    fn referenced_fields_descends_through_like_globs() {
        let check = filter(r#"http.body like "*ok*""#);
        assert_eq!(referenced_fields(&check), vec!["http.body"]);
    }

    #[test]
    fn referenced_fields_descends_through_regex_matches() {
        let check = filter(r#"http.header.content-type matches r"^text/html""#);
        assert_eq!(referenced_fields(&check), vec!["http.header.content-type"]);
    }

    #[test]
    fn observed_fields_renders_referenced_values() {
        let sample = Sample::default()
            .with("http.status", 503)
            .with("http.header.content-type", "text/html");
        let check = filter(r#"http.status == 200 && http.header.content-type == "application/json""#);
        assert_eq!(
            observed_fields(&check, &sample),
            r#"http.status=503, http.header.content-type="text/html""#
        );
    }

    #[test]
    fn observed_fields_renders_null_for_missing_fields() {
        let sample = Sample::default();
        let check = filter("net.ip == \"127.0.0.1\"");
        assert_eq!(observed_fields(&check, &sample), "net.ip=null");
    }

    #[test]
    fn observed_fields_truncates_long_values() {
        let sample = Sample::default().with("http.body", "a".repeat(100).as_str());
        let check = filter(r#"http.body contains "z""#);
        let summary = observed_fields_with_limits(&check, &sample, 6, 8);
        // The value renders as a quoted string; 8 characters are kept, then an ellipsis.
        assert_eq!(summary, "http.body=\"aaaaaaa…");
    }

    #[test]
    fn observed_fields_caps_the_field_count() {
        let sample = Sample::default();
        let check = filter("a == 1 && b == 2 && c == 3");
        let summary = observed_fields_with_limits(&check, &sample, 2, 64);
        assert_eq!(summary, "a=null, b=null, and 1 more");
    }

    #[test]
    fn observed_fields_is_empty_without_referenced_fields() {
        let sample = Sample::default();
        assert_eq!(observed_fields(&filter("true"), &sample), "");
    }

    #[test]
    fn describe_failure_names_the_check_only_in_the_probe_message() {
        let sample = Sample::default().with("http.status", 503);
        let check = filter("http.status == 200");
        let (probe_message, detail) = describe_failure(&check, &sample, "did not pass".to_string());

        assert_eq!(
            probe_message,
            "The check 'http.status == 200' did not pass. Observed http.status=503."
        );
        assert_eq!(detail, "Did not pass. Observed http.status=503.");
        // The per-check detail must not repeat the expression (it is the map key).
        assert!(!detail.contains("http.status == 200"));
    }

    #[test]
    fn describe_failure_omits_the_observed_clause_without_fields() {
        let sample = Sample::default();
        let check = filter("true");
        let (probe_message, detail) = describe_failure(&check, &sample, "did not pass".to_string());
        assert_eq!(probe_message, "The check 'true' did not pass.");
        assert_eq!(detail, "Did not pass.");
    }

    #[test]
    fn capitalise_first_handles_empty_and_non_empty_input() {
        assert_eq!(capitalise_first(""), "");
        assert_eq!(capitalise_first("did not pass"), "Did not pass");
    }
}
