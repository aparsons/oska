//! `std.hmac` (#232): HMAC-SHA-256 in Oska, linked into a program that
//! imports it and checked under the rules for user code. It uses
//! `std.sha2`'s public API only. Its tag is a secret: comparing it with
//! `==`, branching on it or printing it is refused with the secret codes,
//! and exposing it is the caller's audit site. Its verdict is a public
//! `Bool`. Every function in it is pure, and the module has no audit site.

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions of `std.hmac`, as the authority report names them.
const FUNCTIONS: [&str; 2] = ["std.hmac.sha256", "std.hmac.verify_sha256"];

/// Every public function, called from `main` with a key from the
/// environment and a message and tag from the command line.
const EVERY_FUNCTION: &str = r#"use std.hmac

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("HMAC_KEY").to_bytes()
    let message: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    let tag: Untrusted<Bytes> = match args.get(2) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    let mine = hmac.sha256(key, message)
    let same = mine.constant_time_eq(tag) && hmac.verify_sha256(key, message, tag)
    let _ = io.println(if same { "valid" } else { "invalid" })
}
"#;

fn function<'r>(report: &'r CheckReport, name: &str) -> &'r oska_check::FunctionAuthority {
    report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();
            panic!("the report should name `{name}`: {names:?}")
        })
}

/// The codes of `src`'s diagnostics, in order.
fn codes(src: &str) -> Vec<&'static str> {
    check(src).diagnostics.iter().map(|d| d.code).collect()
}

// ---- Importing it ------------------------------------------------------------------

#[test]
fn a_program_calling_every_function_checks_and_lowers() {
    let report = check(EVERY_FUNCTION);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.program.is_some(), "a clean program lowers");
}

#[test]
fn a_brace_group_imports_its_items_by_name() {
    assert_ok(
        "use std.hmac.{sha256, verify_sha256}\n\
         fn check(key: Secret<Bytes>, message: Bytes, tag: Untrusted<Bytes>) -> Bool {\n    \
             sha256(key, message).constant_time_eq(tag) && verify_sha256(key, message, tag)\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn tag(key: Secret<Bytes>) -> Secret<Bytes> {\n    hmac.sha256(key, Bytes.new())\n}",
        &[("E0221", "hmac")],
    );
}

#[test]
fn its_helpers_are_private() {
    assert_errors("use std.hmac.{key_block}", &[("E0206", "key_block")]);
}

#[test]
fn any_module_of_a_package_may_import_it() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.auth\n\
             fn main(io: Stdout, env: Env) {\n    \
                 let ok = auth.check(env.secret(\"K\").to_bytes(), \"m\".to_bytes())\n    \
                 let _ = io.println(if ok { \"ok\" } else { \"no\" })\n}",
        ),
        (
            "auth",
            "use std.hmac\n\
             pub(pkg) fn check(key: Secret<Bytes>, tag: Untrusted<Bytes>) -> Bool {\n    \
                 hmac.verify_sha256(key, \"message\".to_bytes(), tag)\n}",
        ),
    ]);
}

// ---- Secrets: misuse of a tag --------------------------------------------------------

#[test]
fn a_tag_is_secret() {
    assert_errors(
        "use std.hmac\n\
         fn leak(key: Secret<Bytes>) -> Bytes {\n    hmac.sha256(key, Bytes.new())\n}",
        &[("E0220", "hmac.sha256(key, Bytes.new())")],
    );
}

#[test]
fn comparing_a_tag_with_eq_is_refused_with_its_fix() {
    let report = check(
        "use std.hmac\n\
         fn same(key: Secret<Bytes>, tag: Secret<Bytes>) -> Bool {\n    \
             hmac.sha256(key, Bytes.new()) == tag\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0351"], "{:#?}", report.diagnostics);
    let fix = report.diagnostics[0].fix.as_ref().expect("a fix");
    let replaced: Vec<&str> = fix.edits.iter().map(|e| e.replacement.as_str()).collect();
    assert_eq!(
        replaced,
        ["hmac.sha256(key, Bytes.new()).constant_time_eq_secret(tag)"],
        "{fix:#?}"
    );
}

#[test]
fn a_tag_cannot_decide_a_branch() {
    assert_eq!(
        codes(
            "use std.hmac\n\
             fn check(io: Stdout, key: Secret<Bytes>) {\n    \
                 let tag = hmac.sha256(key, Bytes.new())\n    \
                 if tag.at(0).ct_eq(Secret.new(0)) {\n        let _ = io.println(\"zero\")\n    }\n}",
        ),
        ["E0353"]
    );
}

#[test]
fn printing_a_tag_is_refused() {
    assert_eq!(
        codes(
            "use std.hmac\n\
             fn show(io: Stdout, key: Secret<Bytes>) {\n    \
                 let _ = io.println(hmac.sha256(key, Bytes.new()).to_string())\n}",
        ),
        ["E0352"]
    );
}

#[test]
fn a_secret_message_is_not_accepted() {
    // The message is public in this spec; a secret one is a type error.
    assert_eq!(
        codes(
            "use std.hmac\n\
             fn tag(key: Secret<Bytes>) -> Secret<Bytes> {\n    hmac.sha256(key, key)\n}",
        ),
        ["E0220"]
    );
}

#[test]
fn exposing_a_tag_is_the_callers_audit_site() {
    let src = "use std.hmac\n\
               fn tag(env: Env) -> Bytes {\n    \
                   hmac.sha256(env.secret(\"K\").to_bytes(), Bytes.new()).expose()\n}";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let sites: Vec<&str> = report
        .audit
        .iter()
        .map(|site| &src[site.span.start..site.span.end])
        .collect();
    assert_eq!(
        sites,
        ["hmac.sha256(env.secret(\"K\").to_bytes(), Bytes.new()).expose()"]
    );
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn every_function_is_pure_and_the_module_has_no_audit_site() {
    let report = check(EVERY_FUNCTION);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let f = function(&report, name);
        assert!(f.pure, "`{name}` should be pure: {:?}", f.authority);
    }
    // `std.sha2`, which it uses, is linked in too.
    assert!(function(&report, "std.sha2.Sha256Secret.finish").pure);
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}

#[test]
fn the_module_checks_cleanly_under_the_rules_for_user_code() {
    let source = include_str!("../src/std/hmac.osk");
    let report = check(source);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
    let code: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    for word in [".expose()", ".assume_trusted()"] {
        assert!(
            !code.iter().any(|line| line.contains(word)),
            "std.hmac calls `{word}`"
        );
    }
}
