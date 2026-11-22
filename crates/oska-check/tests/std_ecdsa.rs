//! `std.ecdsa` (#242), a `std` module of Oska source: a program that
//! imports it has it linked in and checked under the rules for user code. A
//! key loads from untrusted bytes with no trust assertion, its point and
//! the module's helpers stay private, the module adds no audit site, and
//! every function in it is pure. Its verdicts are held by test vectors that
//! run as Oska tests (`crates/oskac/tests/fixtures/ecdsa_vectors.osk`).

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions and methods of `std.ecdsa`, as the authority
/// report names them.
const FUNCTIONS: [&str; 4] = [
    "std.ecdsa.PublicKey.from_bytes",
    "std.ecdsa.PublicKey.to_bytes",
    "std.ecdsa.verify",
    "std.ecdsa.verify_der",
];

/// Every public function, called from `main` on untrusted input.
const EVERY_FUNCTION: &str = r#"use std.ecdsa

fn main(io: Stdout, args: Args) {
    let raw: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    for curve in [ecdsa.Curve.P256, ecdsa.Curve.P384] {
        match ecdsa.PublicKey.from_bytes(curve, raw) {
            Some(key) => {
                let ok = ecdsa.verify(key, raw, raw)
                let der = ecdsa.verify_der(key, raw, raw)
                let _ = io.println(key.to_bytes().to_hex())
                let _ = io.println(if ok || der { "valid" } else { "invalid" })
            }
            None => {
                let _ = io.println("not a key")
            }
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
        "use std.ecdsa.{Curve, PublicKey, verify, verify_der}\n\
         fn check(key: Bytes, digest: Bytes, signature: Bytes) -> Bool {\n    \
             match PublicKey.from_bytes(Curve.P384, key) {\n        \
                 Some(k) => verify(k, digest, signature) || verify_der(k, digest, signature)\n        \
                 None => false\n    }\n}",
    );
}

#[test]
fn a_key_from_untrusted_bytes_is_trusted_and_encodes_to_trusted_bytes() {
    // The key and its encoding go where only trusted values may: a
    // trusted return.
    assert_ok(
        "use std.ecdsa\n\
         fn load(raw: Untrusted<Bytes>) -> Option<ecdsa.PublicKey> {\n    \
             ecdsa.PublicKey.from_bytes(ecdsa.Curve.P256, raw)\n}\n\
         fn encoding(key: ecdsa.PublicKey) -> Bytes {\n    key.to_bytes()\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn check(key: Bytes) -> Bool {\n    ecdsa.PublicKey.from_bytes(ecdsa.Curve.P256, key).is_some()\n}",
        &[("E0221", "ecdsa"), ("E0221", "ecdsa")],
    );
}

#[test]
fn it_imports_beside_sha2_which_hashes_the_message() {
    assert_ok(
        "use std.ecdsa\nuse std.sha2\n\
         fn check(key: ecdsa.PublicKey, data: Bytes, signature: Bytes) -> Bool {\n    \
             ecdsa.verify(key, sha2.sha256(data), signature)\n}",
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.ecdsa\n\
         fn main() {\n    let _ = ecdsa.limbs_to_be([1])\n}",
        &[("E0360", "limbs_to_be")],
    );
    assert_errors("use std.ecdsa.{mont_mul}", &[("E0360", "mont_mul")]);
}

#[test]
fn a_keys_point_is_private_and_a_key_cannot_be_forged() {
    let report = check(
        "use std.ecdsa\n\
         fn peek(key: ecdsa.PublicKey) -> Bytes {\n    key.encoding\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    assert!(
        report.diagnostics[0].message.contains("`std.ecdsa`"),
        "{}",
        report.diagnostics[0].message
    );
    assert!(report.diagnostics[0].fix.is_none(), "no fix edits `std`");
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn every_function_is_pure_and_the_module_has_no_audit_site() {
    // The report lists only the program's own functions: the module's are
    // the toolchain's code. So its purity is checked on the module itself,
    // as a file, where every function it declares, `pub` or not, is named.
    let program = check(EVERY_FUNCTION);
    assert!(program.diagnostics.is_empty(), "{:#?}", program.diagnostics);
    assert!(
        program.functions.iter().all(|f| !f.name.contains("ecdsa")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/ecdsa.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.ecdsa.").unwrap_or(name);
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
    let source = include_str!("../src/std/ecdsa.osk");
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
            "std.ecdsa uses `{word}`"
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
                 let _ = io.println(if keys.valid(Bytes.zeroed(65)) { \"key\" } else { \"no key\" })\n}",
        ),
        (
            "keys",
            "use std.ecdsa.{Curve, PublicKey}\n\
             pub(pkg) fn valid(raw: Untrusted<Bytes>) -> Bool {\n    \
                 PublicKey.from_bytes(Curve.P256, raw).is_some()\n}",
        ),
    ]);
}
