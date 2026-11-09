//! Helpers shared by the `check_module` integration tests.
//!
//! Every test checks a small Oska snippet against the embedded prelude and
//! asserts on what an agent sees: diagnostic codes with the source text
//! they point at, the source a fix produces, and the authority report.

#![allow(dead_code)]

use oska_check::{check_module, CheckReport};
use oska_syntax::Diagnostic;

/// Check `src` against the embedded prelude.
pub fn check(src: &str) -> CheckReport {
    check_module(src)
}

/// The source text a diagnostic's span covers.
fn spanned<'a>(src: &'a str, diagnostic: &Diagnostic) -> &'a str {
    &src[diagnostic.span.start..diagnostic.span.end]
}

/// Render diagnostics as `(code, spanned text)` pairs, for assertions.
fn codes_and_text<'a>(src: &'a str, report: &CheckReport) -> Vec<(&'static str, &'a str)> {
    report
        .diagnostics
        .iter()
        .map(|d| (d.code, spanned(src, d)))
        .collect()
}

/// Assert that `src` checks with no diagnostics.
pub fn assert_ok(src: &str) {
    let report = check(src);
    assert!(
        report.diagnostics.is_empty(),
        "{src:?} should check cleanly, found {:#?}",
        report.diagnostics
    );
}

/// Assert that `src` reports exactly `expected`, in source order: each
/// entry is a diagnostic code and the source text its span covers.
pub fn assert_errors(src: &str, expected: &[(&str, &str)]) {
    let report = check(src);
    let actual = codes_and_text(src, &report);
    let expected: Vec<(&str, &str)> = expected.to_vec();
    assert_eq!(
        actual, expected,
        "diagnostics of {src:?}: {:#?}",
        report.diagnostics
    );
}

/// Assert that the first diagnostic with `code` carries a fix, and that
/// applying the fix to `src` produces `expected`.
pub fn assert_fix(src: &str, code: &str, expected: &str) {
    let report = check(src);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("{src:?} should report {code}: {:#?}", report.diagnostics));
    let fix = diagnostic
        .fix
        .as_ref()
        .unwrap_or_else(|| panic!("{code} on {src:?} should carry a fix"));
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.to_string();
    for edit in edits {
        fixed.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    assert_eq!(fixed, expected, "fix for {code} on {src:?}");
}

/// Assert the authority report for the function `name` (a method is named
/// `Type.method`): the built-in capabilities it can reach, in order, and
/// whether it is pure.
pub fn assert_authority(src: &str, name: &str, authority: &[&str], pure: bool) {
    let report = check(src);
    let function = report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("{src:?} should report a function `{name}`"));
    let actual: Vec<&str> = function.authority.iter().map(String::as_str).collect();
    assert_eq!(
        (actual.as_slice(), function.pure),
        (authority, pure),
        "authority and purity of `{name}` in {src:?}"
    );
}

/// Remove all whitespace outside JSON strings, so inline JSON can be laid
/// out for readability.
pub fn compact(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut in_str = false;
    let mut escaped = false;
    for c in json.chars() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Assert that the JSON report of `src` is exactly the inline `expected`.
pub fn assert_report_json(src: &str, expected: &str) {
    assert_eq!(
        compact(&check(src).to_json()),
        compact(expected),
        "JSON report of {src:?}"
    );
}

/// Assert that the audit sites of `src` are exactly `expected`, in source
/// order: each entry is a site's kind and the source text its span covers.
pub fn assert_audit(src: &str, expected: &[(&str, &str)]) {
    let report = check(src);
    let actual: Vec<(&str, &str)> = report
        .audit
        .iter()
        .map(|site| (site.kind.as_str(), &src[site.span.start..site.span.end]))
        .collect();
    assert_eq!(
        actual,
        expected.to_vec(),
        "audit sites of {src:?}, with diagnostics {:#?}",
        report.diagnostics
    );
}
