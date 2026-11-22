//! `std.sha2` (#231), the first `std` module of Oska source: a program
//! that imports it, directly or from any module of a package, has it
//! linked in and checked under the rules for user code. Its digests are
//! trusted `Bytes`, its secret form keeps secrecy, its private helpers
//! stay private, it adds no audit site, and every function in it is pure.
//! A cycle of imports between `std` modules is refused (`link.rs`'s own
//! tests, as no real `std` modules form one).

mod common;

use common::{
    assert_errors, assert_modules_errors, assert_modules_ok, assert_ok, check, check_modules,
};
use oska_check::CheckReport;

/// The public functions and methods of `std.sha2`, as the authority report
/// names them.
const FUNCTIONS: [&str; 13] = [
    "std.sha2.sha256",
    "std.sha2.sha512",
    "std.sha2.sha384",
    "std.sha2.sha256_secret",
    "std.sha2.Sha256.new",
    "std.sha2.Sha256.update",
    "std.sha2.Sha256.finish",
    "std.sha2.Sha512.new",
    "std.sha2.Sha512.update",
    "std.sha2.Sha512.finish",
    "std.sha2.Sha256Secret.new",
    "std.sha2.Sha256Secret.update",
    "std.sha2.Sha256Secret.finish",
];

/// Every public function, called from `main`.
const EVERY_FUNCTION: &str = r#"use std.sha2

fn main(io: Stdout, args: Args) {
    let raw: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    let a = sha2.sha256(raw)
    let b = sha2.sha512("abc".to_bytes())
    var h = sha2.Sha256.new()
    h.update(raw)
    var g = sha2.Sha512.new()
    g.update(raw)
    var s = sha2.Sha256Secret.new()
    s.update(Secret.new(a))
    let key = sha2.sha256_secret(Secret.new(b))
    let same = key.constant_time_eq_secret(s.finish())
    let _ = io.println(if same { "same" } else { "different" })
    let _ = io.println(h.finish().to_hex())
    let _ = io.println(g.finish().to_hex())
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
        "use std.sha2.{sha256, Sha512}\n\
         fn digest(data: Bytes) -> Bytes {\n    \
             var h = Sha512.new()\n    h.update(data)\n    \
             sha256(h.finish())\n}",
    );
}

#[test]
fn untrusted_bytes_hash_to_a_trusted_digest() {
    // The digest goes where only trusted bytes may: a trusted return.
    assert_ok(
        "use std.sha2\n\
         fn digest(raw: Untrusted<Bytes>) -> Bytes {\n    sha2.sha256(raw)\n}\n\
         fn digest512(raw: Untrusted<Bytes>) -> String {\n    sha2.sha512(raw).to_hex()\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn digest(data: Bytes) -> Bytes {\n    sha2.sha256(data)\n}",
        &[("E0221", "sha2")],
    );
}

#[test]
fn a_brace_group_name_the_module_does_not_declare_is_an_error() {
    assert_errors("use std.sha2.{sha265}", &[("E0312", "sha265")]);
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.sha2\n\
         fn main() {\n    let _ = sha2.padding(0, 64)\n}",
        &[("E0360", "padding")],
    );
    assert_errors("use std.sha2.{padding}", &[("E0360", "padding")]);
}

#[test]
fn a_hashers_state_is_private_and_it_cannot_be_forged() {
    let report = check(
        "use std.sha2\n\
         fn peek(h: sha2.Sha256) -> Int {\n    h.length\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    let message = &report.diagnostics[0].message;
    assert!(message.contains("`std.sha2`"), "{message}");
    assert!(
        report.diagnostics[0].fix.is_none(),
        "no fix edits `std`: {:#?}",
        report.diagnostics
    );
}

// ---- Secrets --------------------------------------------------------------------------

#[test]
fn a_secret_digest_stays_secret() {
    assert_errors(
        "use std.sha2\n\
         fn leak(key: Secret<Bytes>) -> Bytes {\n    sha2.sha256_secret(key)\n}",
        &[("E0220", "sha2.sha256_secret(key)")],
    );
}

#[test]
fn comparing_a_secret_digest_with_eq_is_refused() {
    let report = check(
        "use std.sha2\n\
         fn same(key: Secret<Bytes>, tag: Bytes) -> Bool {\n    \
             sha2.sha256_secret(key) == Secret.new(tag)\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0351"], "{:#?}", report.diagnostics);
}

#[test]
fn printing_a_secret_digest_is_refused() {
    let report = check(
        "use std.sha2\n\
         fn show(io: Stdout, key: Secret<Bytes>) {\n    \
             let _ = io.println(sha2.sha256_secret(key).to_string())\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0352"], "{:#?}", report.diagnostics);
}

#[test]
fn interpolating_a_secret_digest_is_refused() {
    let report = check(
        "use std.sha2\n\
         fn show(io: Stdout, key: Secret<Bytes>) {\n    \
             let tag = sha2.sha256_secret(key)\n    \
             let _ = io.println(\"{tag}\")\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0350"], "{:#?}", report.diagnostics);
}

#[test]
fn a_secret_digest_cannot_decide_a_branch() {
    let report = check(
        "use std.sha2\n\
         fn check(io: Stdout, key: Secret<Bytes>) {\n    \
             let tag = sha2.sha256_secret(key)\n    \
             if tag.at(0).ct_eq(Secret.new(0)) {\n        let _ = io.println(\"zero\")\n    }\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0353"], "{:#?}", report.diagnostics);
}

#[test]
fn exposing_a_secret_digest_is_the_callers_audit_site() {
    let src = "use std.sha2\n\
               fn tag(key: Secret<Bytes>) -> Bytes {\n    \
                   sha2.sha256_secret(key).expose()\n}";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let sites: Vec<&str> = report
        .audit
        .iter()
        .map(|site| &src[site.span.start..site.span.end])
        .collect();
    assert_eq!(sites, ["sha2.sha256_secret(key).expose()"]);
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
        program.functions.iter().all(|f| !f.name.contains("sha2")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/sha2.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.sha2.").unwrap_or(name);
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
    let source = include_str!("../src/std/sha2.osk");
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
            "std.sha2 calls `{word}`"
        );
    }
}

// ---- Packages ---------------------------------------------------------------------------

#[test]
fn any_module_of_a_package_may_import_it() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.digest\nuse std.sha2\n\
             fn main(io: Stdout) {\n    \
                 let _ = io.println(digest.hex(\"abc\".to_bytes()))\n    \
                 let _ = io.println(sha2.sha512(Bytes.new()).to_hex())\n}",
        ),
        (
            "digest",
            "use std.sha2.{sha256}\n\
             pub(pkg) fn hex(data: Bytes) -> String {\n    sha256(data).to_hex()\n}",
        ),
    ]);
}

#[test]
fn a_module_sees_it_only_through_its_own_use() {
    assert_modules_errors(
        &[
            (
                "main",
                "use pkg.digest\nuse std.sha2\n\
                 fn main(io: Stdout) {\n    \
                     let _ = io.println(digest.hex(sha2.sha256(Bytes.new())))\n}",
            ),
            (
                "digest",
                "pub(pkg) fn hex(data: Bytes) -> String {\n    sha2.sha512(data).to_hex()\n}",
            ),
        ],
        &[("src/digest.osk", "E0221", "sha2")],
    );
}

#[test]
fn a_package_names_its_functions_by_key_and_std_ones_by_their_module() {
    let report = check_modules(&[
        (
            "main",
            "use std.sha2\n\
             fn hex(data: Bytes) -> String {\n    sha2.sha256(data).to_hex()\n}\n\
             fn main(io: Stdout) {\n    let _ = io.println(hex(Bytes.new()))\n}",
        ),
        ("util", "pub(pkg) fn one() -> Int {\n    1\n}"),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(function(&report, "pkg.main.hex").pure);
    // The module's own functions are the toolchain's, not the program's.
    assert!(
        report.functions.iter().all(|f| !f.name.starts_with("std.")),
        "{:?}",
        report.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}
