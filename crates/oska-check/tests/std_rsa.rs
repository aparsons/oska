// SPDX-License-Identifier: Apache-2.0
//! `std.rsa` (#242), a `std` module of Oska source that imports another
//! (`std.sha2`): a program that imports it has both linked in and checked
//! under the rules for user code. A key loads from untrusted bytes with no
//! trust assertion, its fields and the module's helpers stay private, the
//! module adds no audit site, and every function in it is pure. Its
//! verdicts are held by test vectors that run as Oska tests
//! (`crates/oskac/tests/fixtures/rsa_vectors.osk`).

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions and methods of `std.rsa`, as the authority report
/// names them.
const FUNCTIONS: [&str; 4] = [
    "std.rsa.PublicKey.from_components",
    "std.rsa.PublicKey.bits",
    "std.rsa.verify_pkcs1",
    "std.rsa.verify_pss",
];

/// Every public function, called from `main` on untrusted input.
const EVERY_FUNCTION: &str = r#"use std.rsa

fn main(io: Stdout, args: Args) {
    let raw: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    match rsa.PublicKey.from_components(raw, raw) {
        Some(key) => {
            let digest = Bytes.zeroed(32)
            let a = rsa.verify_pkcs1(key, rsa.HashAlgorithm.Sha256, digest, raw)
            let b = rsa.verify_pss(key, rsa.HashAlgorithm.Sha512, raw, raw)
            let _ = io.println(key.bits().to_string())
            let _ = io.println(if a || b { "valid" } else { "invalid" })
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
        "use std.rsa.{HashAlgorithm, PublicKey, verify_pkcs1, verify_pss}\n\
         fn check(n: Bytes, e: Bytes, digest: Bytes, signature: Bytes) -> Bool {\n    \
             match PublicKey.from_components(n, e) {\n        \
                 Some(k) => verify_pkcs1(k, HashAlgorithm.Sha384, digest, signature) || verify_pss(k, HashAlgorithm.Sha384, digest, signature)\n        \
                 None => false\n    }\n}",
    );
}

#[test]
fn a_key_from_untrusted_bytes_is_trusted() {
    // The key goes where only trusted values may: a trusted return.
    assert_ok(
        "use std.rsa\n\
         fn load(n: Untrusted<Bytes>, e: Untrusted<Bytes>) -> Option<rsa.PublicKey> {\n    \
             rsa.PublicKey.from_components(n, e)\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn check(n: Bytes) -> Bool {\n    rsa.PublicKey.from_components(n, n).is_some()\n}",
        &[("E0221", "rsa")],
    );
}

#[test]
fn importing_it_does_not_bind_sha2() {
    // `std.rsa`'s own `use std.sha2` is its own: the program must import
    // `std.sha2` to name it.
    assert_errors(
        "use std.rsa\n\
         fn digest(data: Bytes) -> Bytes {\n    sha2.sha256(data)\n}",
        &[("E0221", "sha2")],
    );
}

#[test]
fn it_imports_beside_sha2_which_hashes_the_message() {
    assert_ok(
        "use std.rsa\nuse std.sha2\n\
         fn check(key: rsa.PublicKey, data: Bytes, signature: Bytes) -> Bool {\n    \
             rsa.verify_pss(key, rsa.HashAlgorithm.Sha384, sha2.sha384(data), signature)\n}",
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.rsa\n\
         fn main() {\n    let _ = rsa.mgf1(rsa.HashAlgorithm.Sha256, Bytes.new(), 1)\n}",
        &[("E0360", "mgf1")],
    );
    assert_errors("use std.rsa.{make_field}", &[("E0360", "make_field")]);
}

#[test]
fn a_keys_fields_are_private_and_a_key_cannot_be_forged() {
    let report = check(
        "use std.rsa\n\
         fn peek(key: rsa.PublicKey) -> Int {\n    key.size\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    assert!(
        report.diagnostics[0].message.contains("`std.rsa`"),
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
        program.functions.iter().all(|f| !f.name.contains("rsa")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/rsa.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.rsa.").unwrap_or(name);
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
    let source = include_str!("../src/std/rsa.osk");
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
            "std.rsa uses `{word}`"
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
                 let _ = io.println(if keys.valid(Bytes.zeroed(256)) { \"key\" } else { \"no key\" })\n}",
        ),
        (
            "keys",
            "use std.rsa.{PublicKey}\n\
             pub(pkg) fn valid(raw: Untrusted<Bytes>) -> Bool {\n    \
                 PublicKey.from_components(raw, raw).is_some()\n}",
        ),
    ]);
}
