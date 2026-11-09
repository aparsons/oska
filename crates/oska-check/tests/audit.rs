//! Trust assertions and the audit array (#53): a validator turns untrusted
//! text into trusted text with `.assume_trusted()`, and every such call is
//! an audit site in the report.

mod common;

use common::{assert_audit, assert_ok, assert_report_json};

/// The validator from the design notes.
const EMAIL: &str = "enum ParseError { Invalid }\n\
     struct Email { raw: String }\n\
     impl Email {\n\
     \x20   pub fn parse(s: Untrusted<String>) -> Result<Email, ParseError> {\n\
     \x20       if !s.contains(\"@\") { return Err(ParseError.Invalid) }\n\
     \x20       Ok(Email(raw: s.assume_trusted()))\n\
     \x20   }\n\
     }";

#[test]
fn a_validator_checks() {
    assert_ok(EMAIL);
}

#[test]
fn a_validator_trust_assertion_is_an_audit_site() {
    assert_audit(EMAIL, &[("assume_trusted", "s.assume_trusted()")]);
}

#[test]
fn every_trust_assertion_is_listed_in_source_order() {
    assert_audit(
        "fn pick(a: Untrusted<String>, b: Untrusted<String>) -> String {\n\
         \x20   let first = b.assume_trusted()\n\
         \x20   let check = |s: Untrusted<String>| s.assume_trusted()\n\
         \x20   if first.len() > 0 { check(a) } else { first }\n\
         }",
        &[
            ("assume_trusted", "b.assume_trusted()"),
            ("assume_trusted", "s.assume_trusted()"),
        ],
    );
}

#[test]
fn a_user_method_named_assume_trusted_is_not_an_audit_site() {
    assert_audit(
        "struct Token { raw: String }\n\
         impl Token {\n\
         \x20   pub fn assume_trusted(self) -> String { self.raw }\n\
         }\n\
         fn f(t: Token) -> String {\n    t.assume_trusted()\n}",
        &[],
    );
}

#[test]
fn the_report_lists_audit_sites_with_their_kind_and_span() {
    assert_report_json(
        "fn f(s: Untrusted<String>) -> String {\n    s.assume_trusted()\n}",
        r#"{
            "diagnostics": [],
            "functions": [{"name":"f","authority":[],"via":[],"pure":true}],
            "audit": [{"kind":"assume_trusted","span":{"start":43,"end":61}}]
        }"#,
    );
}

#[test]
fn the_audit_array_is_present_and_empty_without_audit_sites() {
    assert_report_json(
        "fn f() -> Int {\n    1\n}",
        r#"{
            "diagnostics": [],
            "functions": [{"name":"f","authority":[],"via":[],"pure":true}],
            "audit": []
        }"#,
    );
}
