//! `std.sshsig` (#234), a `std` module of Oska source that imports two
//! others (`std.sha2` and `std.ed25519`): a program that imports it has
//! all three linked in and checked under the rules for user code. The key
//! line and the namespace are trusted text only (`E0300`), a signature and
//! a message may be untrusted, the module adds no audit site, and every
//! function in it is pure. Its verdicts are held by signatures that run
//! as Oska on both backends (`crates/oskac/tests/native_sshsig.rs`).

mod common;

use common::{assert_errors, assert_modules_ok, assert_ok, check};
use oska_check::CheckReport;

/// The public functions and methods of `std.sshsig`, as the authority
/// report names them.
const FUNCTIONS: [&str; 10] = [
    "std.sshsig.PublicKey.parse",
    "std.sshsig.PublicKey.comment",
    "std.sshsig.PublicKey.fingerprint",
    "std.sshsig.PublicKey.ed25519",
    "std.sshsig.Signature.parse",
    "std.sshsig.Signature.public_key",
    "std.sshsig.Signature.namespace",
    "std.sshsig.Signature.hash_algorithm",
    "std.sshsig.verify",
    "<std.sshsig.SshsigError as Display>.to_string",
];

/// Every public function, called from `main`: a pinned key and
/// namespace, and a signature and message from outside.
const EVERY_FUNCTION: &str = r#"use std.sshsig

fn main(io: Stdout, args: Args) {
    let signature: Untrusted<String> = match args.get(1) {
        Some(text) => text
        None => ""
    }
    let message = signature.to_bytes()
    match sshsig.PublicKey.parse("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK1mgi1PCfr+Xc7xB/1TjH8haBMmfNwJO0VSKdmFyPWL oska-release") {
        Ok(key) => {
            let _ = io.println(key.fingerprint())
            let _ = io.println(key.comment())
            let _ = io.println(key.ed25519().to_bytes().to_hex())
            match sshsig.Signature.parse(signature) {
                Ok(parsed) => {
                    let _ = io.println(parsed.namespace())
                    let _ = io.println(parsed.hash_algorithm())
                    let _ = io.println(parsed.public_key().fingerprint())
                }
                Err(error) => {
                    let _ = io.println(error.to_string())
                }
            }
            match sshsig.verify(signature, message, "oska-release", key) {
                Ok(_) => {
                    let _ = io.println("valid")
                }
                Err(error) => {
                    let _ = io.println(error.to_string())
                }
            }
        }
        Err(error) => {
            let _ = io.println(error.to_string())
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
        "use std.sshsig.{PublicKey, SshsigError, verify}\n\
         fn check(key: PublicKey, signature: Untrusted<String>, message: Untrusted<Bytes>) -> Result<(), SshsigError> {\n    \
             verify(signature, message, \"oska-release\", key)\n}",
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn check() -> Bool {\n    sshsig.PublicKey.parse(\"x\").is_ok()\n}",
        &[("E0221", "sshsig")],
    );
}

#[test]
fn importing_it_does_not_bind_the_modules_it_uses() {
    assert_errors(
        "use std.sshsig\n\
         fn digest(data: Bytes) -> Bytes {\n    sha2.sha512(data)\n}",
        &[("E0221", "sha2")],
    );
}

#[test]
fn it_imports_beside_sha2_and_ed25519() {
    assert_ok(
        "use std.sshsig\nuse std.sha2\nuse std.ed25519\n\
         fn same(key: sshsig.PublicKey, other: ed25519.PublicKey) -> Bool {\n    \
             sha2.sha256(key.ed25519().to_bytes()) == sha2.sha256(other.to_bytes())\n}",
    );
}

#[test]
fn a_signature_and_a_message_may_be_untrusted_and_the_error_is_a_match_away() {
    assert_ok(
        "use std.sshsig\n\
         fn why(signature: Untrusted<String>, message: Untrusted<Bytes>, key: sshsig.PublicKey) -> String {\n    \
             match sshsig.verify(signature, message, \"oska-index\", key) {\n        \
                 Ok(_) => \"valid\"\n        \
                 Err(sshsig.SshsigError.WrongKey) => \"another key\"\n        \
                 Err(error) => error.to_string()\n    }\n}",
    );
}

// ---- Trusted text only ----------------------------------------------------------------

#[test]
fn a_key_line_must_be_trusted_text() {
    assert_errors(
        "use std.sshsig\n\
         fn load(line: Untrusted<String>) -> Bool {\n    sshsig.PublicKey.parse(line).is_ok()\n}",
        &[("E0300", "line")],
    );
}

#[test]
fn a_namespace_must_be_trusted_text() {
    assert_errors(
        "use std.sshsig\n\
         fn check(signature: Untrusted<String>, message: Untrusted<Bytes>, namespace: Untrusted<String>, key: sshsig.PublicKey) -> Bool {\n    \
             sshsig.verify(signature, message, namespace, key).is_ok()\n}",
        &[("E0300", "namespace")],
    );
}

#[test]
fn a_key_line_vouched_for_is_the_callers_own_audit_site() {
    let report = check(
        "use std.sshsig\n\
         fn load(line: Untrusted<String>) -> Bool {\n    \
             sshsig.PublicKey.parse(line.assume_trusted()).is_ok()\n}",
    );
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.audit.len(), 1, "{:#?}", report.audit);
    assert!(
        !report.audit[0].function.starts_with("std."),
        "the audit site is the caller's: {:#?}",
        report.audit
    );
}

#[test]
fn the_names_a_signature_claims_are_untrusted() {
    assert_errors(
        "use std.sshsig\n\
         fn claimed(signature: Untrusted<String>) -> String {\n    \
             match sshsig.Signature.parse(signature) {\n        \
                 Ok(parsed) => parsed.namespace()\n        \
                 Err(_) => \"\"\n    }\n}",
        &[("E0300", "parsed.namespace()")],
    );
}

// ---- Privacy ------------------------------------------------------------------------

#[test]
fn a_private_helper_cannot_be_used() {
    assert_errors(
        "use std.sshsig\n\
         fn main() {\n    let _ = sshsig.decode_base64(Bytes.new())\n}",
        &[("E0206", "decode_base64")],
    );
    assert_errors("use std.sshsig.{read_string}", &[("E0206", "read_string")]);
}

#[test]
fn a_key_and_a_signature_cannot_be_forged_or_opened() {
    let report = check(
        "use std.sshsig\n\
         fn peek(key: sshsig.PublicKey) -> Bytes {\n    key.blob\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0206"], "{:#?}", report.diagnostics);
    assert!(
        report.diagnostics[0].message.contains("`std.sshsig`"),
        "{}",
        report.diagnostics[0].message
    );
    assert!(report.diagnostics[0].fix.is_none(), "no fix edits `std`");
    let report = check(
        "use std.sshsig\n\
         fn peek(signature: sshsig.Signature) -> Bytes {\n    signature.signature\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0206"], "{:#?}", report.diagnostics);
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
    // So is everything it reaches, `std.sha2` and `std.ed25519` included.
    for f in report
        .functions
        .iter()
        .filter(|f| f.name.starts_with("std."))
    {
        assert!(f.pure, "`{}` should be pure: {:?}", f.name, f.authority);
    }
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
    assert!(!function(&report, "main").pure);
}

#[test]
fn the_module_checks_cleanly_under_the_rules_for_user_code() {
    let source = include_str!("../src/std/sshsig.osk");
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
            "std.sshsig uses `{word}`"
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
                 let _ = io.println(keys.fingerprint())\n}",
        ),
        (
            "keys",
            "use std.sshsig.{PublicKey}\n\
             pub(pkg) fn fingerprint() -> String {\n    \
                 match PublicKey.parse(\"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK1mgi1PCfr+Xc7xB/1TjH8haBMmfNwJO0VSKdmFyPWL\") {\n        \
                     Ok(key) => key.fingerprint()\n        \
                     Err(_) => \"\"\n    }\n}",
        ),
    ]);
}
