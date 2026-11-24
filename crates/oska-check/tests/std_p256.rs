// SPDX-License-Identifier: Apache-2.0
//! `std.p256` (#242), a `std` module of Oska source over secret integers
//! that imports another (`std.ecdsa`, for its public-key check): a program
//! that imports it has both linked in and checked under the rules for user
//! code. The scalar and every result stay secret, so exposing one is the
//! caller's audit site and the module has none, its helpers stay private,
//! and every function in it is pure. Its verdicts are held by test vectors
//! that run as Oska tests (`crates/oskac/tests/fixtures/p256_rfc5903.osk`),
//! and its machine code's freedom from secret-dependent branches by
//! `oska-native`'s constant-time tests.

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions of `std.p256`, as the authority report names them.
const FUNCTIONS: [&str; 2] = ["std.p256.public_key", "std.p256.shared_secret"];

/// Every public function, called from `main` on a secret scalar and an
/// untrusted peer.
const EVERY_FUNCTION: &str = r#"use std.p256

fn main(io: Stdout, env: Env, args: Args) {
    let scalar = env.secret("P256_SCALAR").to_bytes()
    let peer: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    let same = match p256.shared_secret(scalar, peer) {
        Some(shared) => match p256.public_key(scalar) {
            Some(public) => shared.constant_time_eq_secret(public)
            None => false
        }
        None => false
    }
    let _ = io.println(if same { "same" } else { "different" })
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
        "use std.p256.{public_key, shared_secret}\n\
         fn share(scalar: Secret<Bytes>, peer: Untrusted<Bytes>) -> Option<Secret<Bytes>> {\n    \
             match public_key(scalar) {\n        \
                 Some(_) => shared_secret(scalar, peer)\n        \
                 None => None\n    }\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn share(scalar: Secret<Bytes>) -> Bool {\n    p256.public_key(scalar).is_some()\n}",
        &[("E0221", "p256")],
    );
}

#[test]
fn importing_it_does_not_bind_ecdsa() {
    // `std.p256`'s own `use std.ecdsa` is its own: the program must import
    // `std.ecdsa` to name it.
    assert_errors(
        "use std.p256\n\
         fn load(raw: Bytes) -> Bool {\n    ecdsa.PublicKey.from_bytes(ecdsa.Curve.P256, raw).is_some()\n}",
        &[("E0221", "ecdsa"), ("E0221", "ecdsa")],
    );
}

// ---- Secrets ---------------------------------------------------------------------------

#[test]
fn the_results_are_secret_and_cannot_decide_a_branch_or_leave_unexposed() {
    // A secret result is compared in constant time, not with `==`.
    let report = check(
        "use std.p256\n\
         fn same(scalar: Secret<Bytes>, peer: Bytes, expected: Bytes) -> Bool {\n    \
             match p256.shared_secret(scalar, peer) {\n        \
                 Some(shared) => shared == expected\n        \
                 None => false\n    }\n}",
    );
    assert!(
        report.diagnostics.iter().any(|d| d.code == "E0351"),
        "{:#?}",
        report.diagnostics
    );
    assert_ok(
        "use std.p256\n\
         fn same(scalar: Secret<Bytes>, peer: Bytes, expected: Bytes) -> Bool {\n    \
             match p256.shared_secret(scalar, peer) {\n        \
                 Some(shared) => shared.constant_time_eq(expected)\n        \
                 None => false\n    }\n}",
    );
}

#[test]
fn exposing_a_result_is_the_callers_audit_site() {
    let report = check(
        "use std.p256\n\
         fn share(scalar: Secret<Bytes>) -> Bytes {\n    \
             match p256.public_key(scalar) {\n        \
                 Some(point) => point.expose()\n        \
                 None => Bytes.new()\n    }\n}",
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.audit.len(), 1, "{:#?}", report.audit);
}

#[test]
fn a_scalar_that_is_not_secret_is_refused() {
    assert_errors(
        "use std.p256\n\
         fn share(scalar: Bytes) -> Option<Secret<Bytes>> {\n    p256.public_key(scalar)\n}",
        &[("E0220", "scalar")],
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.p256\n\
         fn main() {\n    let _ = p256.invert([])\n}",
        &[("E0360", "invert")],
    );
    assert_errors("use std.p256.{multiply}", &[("E0360", "multiply")]);
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
        program.functions.iter().all(|f| !f.name.contains("p256")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/p256.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.p256.").unwrap_or(name);
        let f = function(&report, name);
        assert!(f.pure, "`{name}` should be pure: {:?}", f.authority);
    }
    for f in &report.functions {
        assert!(f.pure, "`{}` should be pure: {:?}", f.name, f.authority);
    }
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}

#[test]
fn the_module_checks_cleanly_and_never_exposes_or_trusts() {
    let source = include_str!("../src/std/p256.osk");
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
            "std.p256 uses `{word}`"
        );
    }
}

// ---- Packages ---------------------------------------------------------------------------

#[test]
fn any_module_of_a_package_may_import_it() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.exchange\n\
             fn main(io: Stdout, env: Env) {\n    \
                 let _ = io.println(if exchange.ready(env.secret(\"P256_SCALAR\").to_bytes()) { \"ready\" } else { \"no\" })\n}",
        ),
        (
            "exchange",
            "use std.p256\n\
             pub(pkg) fn ready(scalar: Secret<Bytes>) -> Bool {\n    \
                 p256.public_key(scalar).is_some()\n}",
        ),
    ]);
}
