//! Secrets (#55): `env.secret("KEY")` is a `Secret<String>`. A secret
//! cannot be displayed, interpolated, or compared with `==`; its value
//! leaves the wrapper only through `.expose()`, an audit site.

mod common;

use common::{assert_audit, assert_errors, assert_fix, assert_ok};

#[test]
fn an_exposed_secret_is_trusted_text() {
    assert_ok(
        "fn token(env: Env) -> String {\n\
         \x20   let key: Secret<String> = env.secret(\"API_KEY\")\n\
         \x20   key.expose()\n\
         }",
    );
}

#[test]
fn every_exposure_is_an_audit_site() {
    assert_audit(
        "fn token(env: Env) -> String {\n    env.secret(\"API_KEY\").expose()\n}",
        &[("expose", "env.secret(\"API_KEY\").expose()")],
    );
}

#[test]
fn interpolating_a_secret_is_an_error() {
    assert_errors(
        "fn header(env: Env) -> String {\n\
         \x20   let key = env.secret(\"API_KEY\")\n\
         \x20   \"Bearer {key}\"\n\
         }",
        &[("E0350", "{key}")],
    );
}

#[test]
fn interpolating_a_secret_field_is_an_error() {
    assert_errors(
        "struct Config { token: Secret<String> }\n\
         fn show(c: Config) -> String {\n    \"token: {c.token}\"\n}",
        &[("E0350", "{c.token}")],
    );
}

/// `{op}` compares a secret with a request's token.
fn compare(op: &str) -> String {
    format!(
        "fn authorized(env: Env, token: Untrusted<String>) -> Bool {{\n\
         \x20   let key = env.secret(\"API_KEY\")\n\
         \x20   {op}\n\
         }}"
    )
}

#[test]
fn comparing_a_secret_in_constant_time_is_allowed() {
    assert_ok(&compare("key.constant_time_eq(token)"));
    assert_ok(&compare("!key.constant_time_eq(\"literal\")"));
}

#[test]
fn comparing_a_secret_with_eq_is_an_error() {
    assert_errors(&compare("key == token"), &[("E0351", "key == token")]);
    assert_errors(&compare("token == key"), &[("E0351", "token == key")]);
}

#[test]
fn comparing_a_secret_with_ne_is_an_error() {
    assert_errors(&compare("key != token"), &[("E0351", "key != token")]);
}

#[test]
fn the_eq_fix_compares_in_constant_time() {
    assert_fix(
        &compare("key == token"),
        "E0351",
        &compare("key.constant_time_eq(token)"),
    );
}

#[test]
fn the_ne_fix_negates_the_constant_time_comparison() {
    assert_fix(
        &compare("key != token"),
        "E0351",
        &compare("!key.constant_time_eq(token)"),
    );
}

#[test]
fn the_fix_makes_the_secret_the_receiver() {
    assert_fix(
        &compare("token == key"),
        "E0351",
        &compare("key.constant_time_eq(token)"),
    );
}

#[test]
fn the_fix_compares_two_secrets_by_exposing_the_second() {
    let src = "fn same(a: Secret<String>, b: Secret<String>) -> Bool {\n    a == b\n}";
    assert_fix(
        src,
        "E0351",
        "fn same(a: Secret<String>, b: Secret<String>) -> Bool {\n    a.constant_time_eq(b.expose())\n}",
    );
}

#[test]
fn the_fix_parenthesizes_a_compound_secret_operand() {
    assert_fix(
        "fn same(f: Bool, a: Secret<String>, b: Secret<String>, s: String) -> Bool {\n\
         \x20   if f { a } else { b } == s\n\
         }",
        "E0351",
        "fn same(f: Bool, a: Secret<String>, b: Secret<String>, s: String) -> Bool {\n\
         \x20   (if f { a } else { b }).constant_time_eq(s)\n\
         }",
    );
}

#[test]
fn the_fixed_comparisons_check() {
    assert_ok(&compare("key.constant_time_eq(token)"));
    assert_ok("fn same(a: Secret<String>, b: Secret<String>) -> Bool {\n    a.constant_time_eq(b.expose())\n}");
}

#[test]
fn a_secret_is_not_eq() {
    assert_errors(
        "test \"keys match\" {\n\
         \x20   let key: Secret<String> = todo()\n\
         \x20   t.assert_eq(key, key)\n\
         }",
        &[("E0351", "t.assert_eq(key, key)")],
    );
}

#[test]
fn a_secret_has_no_eq_method() {
    assert_errors(&compare("key.eq(key)"), &[("E0351", "eq")]);
}

#[test]
fn code_cannot_make_a_secret() {
    assert_errors(
        "fn fake() -> Secret<String> {\n    Secret(value: \"hunter2\")\n}",
        &[("E0271", "Secret")],
    );
}

#[test]
fn a_secret_is_not_display() {
    assert_errors(
        "fn show<T: Display>(value: T) -> String {\n    value.to_string()\n}\n\
         fn leak(env: Env) -> String {\n    show(env.secret(\"API_KEY\"))\n}",
        &[("E0352", "show(env.secret(\"API_KEY\"))")],
    );
}

#[test]
fn a_secret_has_no_to_string() {
    assert_errors(
        "fn leak(env: Env) -> String {\n    env.secret(\"API_KEY\").to_string()\n}",
        &[("E0352", "to_string")],
    );
}

#[test]
fn an_exposed_secret_is_display() {
    assert_ok(
        "fn show<T: Display>(value: T) -> String {\n    value.to_string()\n}\n\
         fn reveal(env: Env) -> String {\n    show(env.secret(\"API_KEY\").expose())\n}",
    );
}

#[test]
fn interpolating_an_exposed_secret_is_allowed() {
    assert_ok(
        "fn header(env: Env) -> String {\n\
         \x20   let key = env.secret(\"API_KEY\").expose()\n\
         \x20   \"Bearer {key}\"\n\
         }",
    );
}
