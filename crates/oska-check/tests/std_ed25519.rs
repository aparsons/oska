//! `std.ed25519` (#233), a `std` module of Oska source that imports
//! another (`std.sha2`): a program that imports it has both linked in and
//! checked under the rules for user code. A key loads from untrusted
//! bytes with no trust assertion, its point and the module's helpers stay
//! private, the module adds no audit site, and every function in it is
//! pure. Its verdicts are held by test vectors that run as Oska tests
//! (`crates/oskac/tests/fixtures/ed25519_rfc8032.osk`).

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions and methods of `std.ed25519`, as the authority
/// report names them.
const FUNCTIONS: [&str; 3] = [
    "std.ed25519.PublicKey.from_bytes",
    "std.ed25519.PublicKey.to_bytes",
    "std.ed25519.verify",
];

/// Every public function, called from `main` on untrusted input.
const EVERY_FUNCTION: &str = r#"use std.ed25519

fn main(io: Stdout, args: Args) {
    let raw: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    match ed25519.PublicKey.from_bytes(raw) {
        Some(key) => {
            let ok = ed25519.verify(key, raw, raw)
            let _ = io.println(key.to_bytes().to_hex())
            let _ = io.println(if ok { "valid" } else { "invalid" })
        }
        None => {
            let _ = io.println("not a key")
        }
    }
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
        "use std.ed25519.{PublicKey, verify}\n\
         fn check(key: Bytes, message: Bytes, signature: Bytes) -> Bool {\n    \
             match PublicKey.from_bytes(key) {\n        \
                 Some(k) => verify(k, message, signature)\n        \
                 None => false\n    }\n}",
    );
}

#[test]
fn a_key_from_untrusted_bytes_is_trusted_and_encodes_to_trusted_bytes() {
    // The key and its encoding go where only trusted values may: a
    // trusted return.
    assert_ok(
        "use std.ed25519\n\
         fn load(raw: Untrusted<Bytes>) -> Option<ed25519.PublicKey> {\n    \
             ed25519.PublicKey.from_bytes(raw)\n}\n\
         fn encoding(key: ed25519.PublicKey) -> Bytes {\n    key.to_bytes()\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn check(key: Bytes) -> Bool {\n    ed25519.PublicKey.from_bytes(key).is_some()\n}",
        &[("E0221", "ed25519")],
    );
}

#[test]
fn importing_it_does_not_bind_sha2() {
    // `std.ed25519`'s own `use std.sha2` is its own: the program must
    // import `std.sha2` to name it.
    assert_errors(
        "use std.ed25519\n\
         fn digest(data: Bytes) -> Bytes {\n    sha2.sha512(data)\n}",
        &[("E0221", "sha2")],
    );
}

#[test]
fn it_imports_beside_sha2() {
    assert_ok(
        "use std.ed25519\nuse std.sha2\n\
         fn check(key: ed25519.PublicKey, data: Bytes, signature: Bytes) -> Bool {\n    \
             ed25519.verify(key, sha2.sha512(data), signature)\n}",
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.ed25519\n\
         fn main() {\n    let _ = ed25519.reduce_mod_order(Bytes.zeroed(64))\n}",
        &[("E0360", "reduce_mod_order")],
    );
    assert_errors(
        "use std.ed25519.{decode_point}",
        &[("E0360", "decode_point")],
    );
}

#[test]
fn a_keys_point_is_private_and_a_key_cannot_be_forged() {
    let report = check(
        "use std.ed25519\n\
         fn peek(key: ed25519.PublicKey) -> Bytes {\n    key.encoding\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    assert!(
        report.diagnostics[0].message.contains("`std.ed25519`"),
        "{}",
        report.diagnostics[0].message
    );
    assert!(report.diagnostics[0].fix.is_none(), "no fix edits `std`");
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn every_function_is_pure_and_the_module_has_no_audit_site() {
    // A program that imports the module is checked with it linked in, and
    // the report lists only the program's own functions: the module's are
    // the toolchain's code. So its purity is checked on the module itself,
    // as a file, where every function it declares, `pub` or not, is named.
    let program = check(EVERY_FUNCTION);
    assert!(program.diagnostics.is_empty(), "{:#?}", program.diagnostics);
    assert!(
        program
            .functions
            .iter()
            .all(|f| !f.name.contains("ed25519")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/ed25519.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.ed25519.").unwrap_or(name);
        let f = function(&report, name);
        assert!(f.pure, "`{name}` should be pure: {:?}", f.authority);
    }
    for f in &report.functions {
        assert!(f.pure, "`{}` should be pure: {:?}", f.name, f.authority);
    }
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}

#[test]
fn the_module_checks_cleanly_under_the_rules_for_user_code() {
    let source = include_str!("../src/std/ed25519.osk");
    let report = check(source);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
    let code: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    for word in [".expose()", ".assume_trusted()", "Secret"] {
        assert!(
            !code.iter().any(|line| line.contains(word)),
            "std.ed25519 uses `{word}`"
        );
    }
}

// ---- Packages ---------------------------------------------------------------------------

#[test]
fn any_module_of_a_package_may_import_it() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.keys\n\
             fn main(io: Stdout) {\n    \
                 let _ = io.println(if keys.valid(Bytes.zeroed(32)) { \"key\" } else { \"no key\" })\n}",
        ),
        (
            "keys",
            "use std.ed25519.{PublicKey}\n\
             pub(pkg) fn valid(raw: Untrusted<Bytes>) -> Bool {\n    \
                 PublicKey.from_bytes(raw).is_some()\n}",
        ),
    ]);
}
