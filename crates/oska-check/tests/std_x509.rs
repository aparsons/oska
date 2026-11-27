// SPDX-License-Identifier: Apache-2.0
//! `std.x509` (#243), a `std` module of Oska source that imports others
//! (`std.sha2`, `std.ecdsa`, `std.rsa`, `std.ed25519` and the generated
//! `std.x509_roots`): a program that imports it has them linked in and
//! checked under the rules for user code. A certificate parses from
//! untrusted bytes with no trust assertion, its fields and the module's
//! helpers stay private, the module adds no audit site, and every function
//! in it is pure. Its verdicts are held by test chains that run as Oska
//! tests (`crates/oskac/tests/fixtures/x509_cases.osk` on both backends and
//! `crates/oskac/tests/vectors/x509_chains.osk` natively) and by every root
//! of Mozilla's store (`crates/oskac/tests/vectors/x509_mozilla_roots.osk`).

mod common;

use common::{assert_errors, assert_ok, check};
use oska_check::CheckReport;

/// The public functions and methods of `std.x509`, as the authority report
/// names them.
const FUNCTIONS: [&str; 26] = [
    "std.x509.Certificate.parse",
    "std.x509.Certificate.to_der",
    "std.x509.Certificate.subject",
    "std.x509.Certificate.issuer",
    "std.x509.Certificate.serial",
    "std.x509.Certificate.authority_key_id",
    "std.x509.Certificate.ari_id",
    "std.x509.Certificate.not_before",
    "std.x509.Certificate.not_after",
    "std.x509.Certificate.signed_by",
    "std.x509.Certificate.is_ca",
    "std.x509.Certificate.dns_names",
    "std.x509.Certificate.public_key",
    "std.x509.Certificate.matches_host",
    "std.x509.TrustConfig.default",
    "std.x509.TrustConfig.with_roots",
    "std.x509.TrustConfig.only_roots",
    "std.x509.TrustConfig.with_root_der",
    "std.x509.TrustConfig.with_root_pem",
    "std.x509.TrustConfig.with_root_distrusting_after",
    "std.x509.TrustConfig.contains",
    "std.x509.TrustConfig.len",
    "std.x509.parse_der",
    "std.x509.parse_pem",
    "std.x509.verify_chain",
    "std.x509.verify_server",
];

/// Every public function, called from `main` on untrusted input.
const EVERY_FUNCTION: &str = r#"use std.x509

fn main(io: Stdout, args: Args) {
    let raw: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    match x509.Certificate.parse(raw) {
        Ok(cert) => {
            let store = x509.TrustConfig.default()
            let _ = io.println(store.len().to_string())
            let _ = io.println(if cert.is_ca() { "ca" } else { "leaf" })
            let _ = io.println(cert.not_before().to_string())
            let _ = io.println(cert.not_after().to_string())
            let _ = io.println(cert.dns_names().len().to_string())
            let _ = io.println(if cert.matches_host("example.test") { "match" } else { "no match" })
            let _ = io.println(if cert.public_key().is_some() { "key" } else { "no key" })
            let _ = io.println(cert.subject().len().to_string())
            let _ = io.println(cert.issuer().len().to_string())
            let _ = io.println(cert.serial().len().to_string())
            let _ = io.println(if cert.authority_key_id().is_ok() { "aki" } else { "no aki" })
            let _ = io.println(if cert.ari_id().is_ok() { "ari" } else { "no ari" })
            let _ = io.println(cert.to_der().len().to_string())
            let _ = io.println(if store.contains(cert) { "root" } else { "not a root" })
            let _ = io.println(if cert.signed_by(cert).is_ok() { "self-signed" } else { "not self-signed" })
            match x509.verify_server(store, cert, [], "example.test", 1750000000) {
                Ok(path) => {
                    let _ = io.println(path.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match x509.verify_chain(store, cert, [cert], 1750000000) {
                Ok(path) => {
                    let _ = io.println(path.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match store.with_root_der(Bytes.new()) {
                Ok(s) => {
                    let _ = io.println(s.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match store.with_root_distrusting_after(Bytes.new(), 0) {
                Ok(s) => {
                    let _ = io.println(s.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match x509.parse_der(raw) {
                Ok(c) => {
                    let _ = io.println(store.with_roots([c]).len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match x509.parse_pem("") {
                Ok(list) => {
                    let _ = io.println(list.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
            match x509.TrustConfig.only_roots([cert]).with_root_pem("") {
                Ok(s) => {
                    let _ = io.println(s.len().to_string())
                }
                Err(e) => {
                    let _ = io.println(e.to_string())
                }
            }
        }
        Err(e) => {
            let _ = io.println(e.to_string())
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
        "use std.x509.{Certificate, TrustConfig, X509Error, verify_server}\n\
         fn check(der: Untrusted<Bytes>, now: Int) -> Result<Int, X509Error> {\n    \
             let leaf = Certificate.parse(der)?\n    \
             let chain = verify_server(TrustConfig.default(), leaf, [], \"example.test\", now)?\n    \
             Ok(chain.len())\n}",
    );
}

#[test]
fn a_certificate_from_untrusted_bytes_is_trusted() {
    // The certificate goes where only trusted values may: a trusted return.
    assert_ok(
        "use std.x509\n\
         fn load(der: Untrusted<Bytes>) -> Option<x509.Certificate> {\n    \
             match x509.Certificate.parse(der) {\n        \
                 Ok(c) => Some(c)\n        \
                 Err(_) => None\n    }\n}",
    );
}

#[test]
fn the_host_name_is_trusted_text() {
    // Which host a certificate is checked for is the program's choice: an
    // untrusted name needs the caller's own trust assertion.
    assert_errors(
        "use std.x509\n\
         fn matches(cert: x509.Certificate, host: Untrusted<String>) -> Bool {\n    \
             cert.matches_host(host)\n}",
        &[("E0300", "host")],
    );
}

#[test]
fn names_read_from_a_certificate_are_untrusted_text() {
    // A name a certificate claims is not a name the program chose.
    assert_errors(
        "use std.x509\n\
         fn first(cert: x509.Certificate) -> Bool {\n    \
             let names = cert.dns_names()\n    \
             cert.matches_host(names.at(0))\n}",
        &[("E0300", "names.at(0)")],
    );
}

#[test]
fn extra_roots_are_trusted_bytes_and_text() {
    // Which roots to trust is a structural position, as which key to trust is for `std.sshsig`.
    assert_errors(
        "use std.x509\n\
         fn add(store: x509.TrustConfig, der: Untrusted<Bytes>) -> Bool {\n    \
             store.with_root_der(der).is_ok()\n}",
        &[("E0300", "der")],
    );
    assert_errors(
        "use std.x509\n\
         fn add(store: x509.TrustConfig, pem: Untrusted<String>) -> Bool {\n    \
             store.with_root_pem(pem).is_ok()\n}",
        &[("E0300", "pem")],
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn check(der: Bytes) -> Bool {\n    x509.Certificate.parse(der).is_ok()\n}",
        &[("E0221", "x509")],
    );
}

#[test]
fn importing_it_does_not_bind_its_own_imports() {
    assert_errors(
        "use std.x509\n\
         fn digest(data: Bytes) -> Bytes {\n    sha2.sha256(data)\n}",
        &[("E0221", "sha2")],
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.x509\n\
         fn main() {\n    let _ = x509.days_from_civil(1970, 1, 1)\n}",
        &[("E0360", "days_from_civil")],
    );
    assert_errors("use std.x509.{read_tlv}", &[("E0360", "read_tlv")]);
}

#[test]
fn a_certificates_fields_are_private_and_a_certificate_cannot_be_forged() {
    let report = check(
        "use std.x509\n\
         fn peek(cert: x509.Certificate) -> Bytes {\n    cert.tbs\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    assert!(
        report.diagnostics[0].message.contains("`std.x509`"),
        "{}",
        report.diagnostics[0].message
    );
    assert!(report.diagnostics[0].fix.is_none(), "no fix edits `std`");
}

#[test]
fn a_root_store_cannot_be_forged() {
    // A store with a root of the caller's making would bypass `with_root_der`.
    let report = check(
        "use std.x509\n\
         fn forge() -> x509.TrustConfig {\n    x509.TrustConfig(anchors: [])\n}",
    );
    assert!(!report.diagnostics.is_empty(), "a store has private fields");
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
        program.functions.iter().all(|f| !f.name.contains("x509")),
        "the report lists only the program's own functions"
    );
    assert!(program.audit.is_empty(), "{:#?}", program.audit);
    assert!(!function(&program, "main").pure);
    let report = check(include_str!("../src/std/x509.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let name = name.strip_prefix("std.x509.").unwrap_or(name);
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
    for source in [
        include_str!("../src/std/x509.osk"),
        include_str!("../src/std/x509_roots.osk"),
    ] {
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
                "std.x509 uses `{word}`"
            );
        }
    }
}

// ---- The pinned roots ---------------------------------------------------------------

#[test]
fn the_roots_module_is_generated_from_a_named_certdata() {
    let source = include_str!("../src/std/x509_roots.osk");
    assert!(
        source
            .lines()
            .nth(1)
            .is_some_and(|l| l.starts_with("// GENERATED by scripts/x509-roots/generate.py")),
        "the module says how it was made"
    );
    let header = source.lines().nth(2).expect("a source line");
    assert!(
        header.contains("NSS release NSS_") && header.contains("SHA-256"),
        "{header}"
    );
    let roots = source
        .lines()
        .filter(|l| l.starts_with("        Root(name: "))
        .count();
    assert!(
        roots > 100,
        "Mozilla trusts well over 100 roots, found {roots}"
    );
    assert!(
        source.contains("pub fn nss_release() -> String {\n    \"NSS_"),
        "the module names the NSS release it came from"
    );
    let count_line = source.lines().nth(3).expect("a count line");
    assert!(
        count_line.starts_with(&format!("// {roots} roots ")),
        "{count_line}"
    );
}

// ---- Packages ---------------------------------------------------------------------------

#[test]
fn any_module_of_a_package_may_import_it() {
    common::assert_modules_ok(&[
        (
            "main",
            "use pkg.certs\n\
             fn main(io: Stdout) {\n    \
                 let _ = io.println(if certs.parses(Bytes.zeroed(4)) { \"cert\" } else { \"no cert\" })\n}",
        ),
        (
            "certs",
            "use std.x509.{Certificate}\n\
             pub(pkg) fn parses(raw: Untrusted<Bytes>) -> Bool {\n    \
                 Certificate.parse(raw).is_ok()\n}",
        ),
    ]);
}
