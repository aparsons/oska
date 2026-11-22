//! The key-exchange and symmetric primitives of TLS 1.3 (#241): `std.x25519`,
//! `std.hkdf`, `std.sha384`, `std.chacha20poly1305` and `std.aes128gcm`.
//! Each is Oska source linked into a program that imports it and checked
//! under the rules for user code. A secret leaves one as a `Secret<Bytes>`,
//! which `==`, a branch or printing refuse with the secret codes and which
//! the caller exposes in its own code; a verdict leaves as a public `Bool`
//! or an `Option`. Every function in them is pure, and none has an audit
//! site: no `expose()` and no `.assume_trusted()`.

mod common;

use common::{assert_errors, assert_ok, check};

/// Each module's path, the name it is called by, and its source.
const MODULES: [(&str, &str, &str); 5] = [
    (
        "std.x25519",
        "x25519",
        include_str!("../src/std/x25519.osk"),
    ),
    ("std.hkdf", "hkdf", include_str!("../src/std/hkdf.osk")),
    (
        "std.sha384",
        "sha384",
        include_str!("../src/std/sha384.osk"),
    ),
    (
        "std.chacha20poly1305",
        "chacha20poly1305",
        include_str!("../src/std/chacha20poly1305.osk"),
    ),
    (
        "std.aes128gcm",
        "aes128gcm",
        include_str!("../src/std/aes128gcm.osk"),
    ),
];

/// Every public function, called from `main` with a key from the
/// environment and a message from the command line.
const EVERY_FUNCTION: &str = r#"use std.x25519
use std.hkdf
use std.sha384
use std.chacha20poly1305
use std.aes128gcm

fn main(io: Stdout, env: Env, args: Args) {
    let secret = env.secret("KEY").to_bytes()
    let peer: Untrusted<Bytes> = match args.get(1) {
        Some(text) => text.to_bytes()
        None => Bytes.new()
    }
    let public = x25519.public_key(secret)
    let shared = x25519.scalarmult(secret, peer)
    let prk = hkdf.extract(secret, secret)
    let okm = hkdf.expand(prk, peer, 42)
    let digest = sha384.sha384(peer)
    var hasher = sha384.Sha384.new()
    hasher.update(peer)
    let same = hasher.finish() == digest
    let chacha = chacha20poly1305.seal(secret, peer, peer, secret)
    let chacha_open = chacha20poly1305.open(secret, peer, peer, peer)
    let aes = aes128gcm.seal(secret, peer, peer, secret)
    let aes_open = aes128gcm.open(secret, peer, peer, peer)
    let _ = io.println(if same && public.is_some() && shared.is_some() && okm.is_some() { "ok" } else { "no" })
    let _ = io.println(if chacha.is_some() && chacha_open.is_some() && aes.is_some() && aes_open.is_some() { "ok" } else { "no" })
}
"#;

// ---- Importing them --------------------------------------------------------------

#[test]
fn a_program_calling_every_function_checks_and_lowers() {
    let report = check(EVERY_FUNCTION);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.program.is_some(), "a clean program lowers");
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}

#[test]
fn a_brace_group_imports_items_by_name() {
    assert_ok(
        "use std.hkdf.{extract, expand}\n\
         use std.sha384.{sha384, Sha384}\n\
         fn derive(salt: Secret<Bytes>, ikm: Secret<Bytes>) -> Option<Secret<Bytes>> {\n    \
             expand(extract(salt, ikm), Bytes.new(), 32)\n}\n\
         fn digest(data: Untrusted<Bytes>) -> Bytes {\n    \
             var h = Sha384.new()\n    h.update(data)\n    h.finish()\n}",
    );
}

#[test]
fn without_its_use_each_module_is_unknown() {
    for (_, name, _) in MODULES {
        let src = format!("fn f() {{\n    let _ = {name}.nothing()\n}}");
        let report = check(&src);
        let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, ["E0221"], "{name}: {:#?}", report.diagnostics);
    }
}

#[test]
fn the_helpers_of_each_module_are_private() {
    assert_errors("use std.x25519.{mul}", &[("E0360", "mul")]);
    assert_errors("use std.hkdf.{mac}", &[("E0360", "mac")]);
    assert_errors("use std.sha384.{compress512}", &[("E0360", "compress512")]);
    assert_errors(
        "use std.chacha20poly1305.{poly1305}",
        &[("E0360", "poly1305")],
    );
    assert_errors("use std.aes128gcm.{ghash}", &[("E0360", "ghash")]);
}

// ---- Secrets: misuse of what they give ------------------------------------------

#[test]
fn a_key_a_tag_and_a_sealed_message_are_secret() {
    let cases = [
        (
            "use std.x25519\nfn leak(k: Secret<Bytes>, u: Bytes) -> Option<Bytes> {\n    x25519.scalarmult(k, u)\n}",
            "x25519.scalarmult(k, u)",
        ),
        (
            "use std.hkdf\nfn leak(k: Secret<Bytes>) -> Bytes {\n    hkdf.extract(k, k)\n}",
            "hkdf.extract(k, k)",
        ),
        (
            "use std.chacha20poly1305\nfn leak(k: Secret<Bytes>, n: Bytes) -> Option<Bytes> {\n    chacha20poly1305.seal(k, n, n, k)\n}",
            "chacha20poly1305.seal(k, n, n, k)",
        ),
        (
            "use std.aes128gcm\nfn leak(k: Secret<Bytes>, n: Bytes) -> Option<Bytes> {\n    aes128gcm.seal(k, n, n, k)\n}",
            "aes128gcm.seal(k, n, n, k)",
        ),
    ];
    for (src, spanned) in cases {
        assert_errors(src, &[("E0220", spanned)]);
    }
}

#[test]
fn a_secret_key_is_required_where_one_is_taken() {
    // A key as plain bytes is a type error: it would be public.
    assert_errors(
        "use std.chacha20poly1305\nfn f(k: Bytes, n: Bytes) -> Option<Secret<Bytes>> {\n    chacha20poly1305.seal(k, n, n, Secret.new(n))\n}",
        &[("E0220", "k")],
    );
    assert_errors(
        "use std.x25519\nfn f(k: Bytes, u: Bytes) -> Option<Secret<Bytes>> {\n    x25519.scalarmult(k, u)\n}",
        &[("E0220", "k")],
    );
}

#[test]
fn a_secret_cannot_be_the_public_nonce_or_data() {
    assert_errors(
        "use std.aes128gcm\nfn f(k: Secret<Bytes>) -> Option<Secret<Bytes>> {\n    aes128gcm.seal(k, k, k, k)\n}",
        &[("E0220", "k"), ("E0220", "k")],
    );
}

#[test]
fn comparing_a_result_with_eq_is_refused() {
    let report = check(
        "use std.hkdf\n\
         fn same(k: Secret<Bytes>, other: Secret<Bytes>) -> Bool {\n    \
             hkdf.extract(k, k) == other\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0351"], "{:#?}", report.diagnostics);
}

#[test]
fn a_secret_result_cannot_decide_a_branch_or_be_printed() {
    let report = check(
        "use std.hkdf\n\
         fn branch(io: Stdout, k: Secret<Bytes>) {\n    \
             let prk = hkdf.extract(k, k)\n    \
             if prk.at(0).ct_eq(Secret.new(0)) {\n        let _ = io.println(\"zero\")\n    }\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0353"], "{:#?}", report.diagnostics);
    let report = check(
        "use std.hkdf\n\
         fn show(io: Stdout, k: Secret<Bytes>) {\n    \
             let _ = io.println(hkdf.extract(k, k).to_string())\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0352"], "{:#?}", report.diagnostics);
}

#[test]
fn exposing_a_result_is_the_callers_audit_site() {
    let src = "use std.chacha20poly1305\n\
               fn wire(k: Secret<Bytes>, n: Bytes) -> Bytes {\n    \
                   match chacha20poly1305.seal(k, n, n, Secret.new(n)) {\n        \
                       Some(sealed) => sealed.expose()\n        None => Bytes.new()\n    }\n}";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let sites: Vec<&str> = report
        .audit
        .iter()
        .map(|site| &src[site.span.start..site.span.end])
        .collect();
    assert_eq!(sites, ["sealed.expose()"]);
}

#[test]
fn a_digest_is_trusted_bytes_and_a_verdict_is_a_public_option() {
    // SHA-384 hashes public data into trusted `Bytes`, which is compared
    // with `==` and used as a structural value without an assertion.
    assert_ok(
        "use std.sha384\n\
         fn same(a: Untrusted<Bytes>, b: Untrusted<Bytes>) -> Bool {\n    \
             sha384.sha384(a) == sha384.sha384(b)\n}",
    );
    // `open` decides on a public verdict, which a branch may use.
    assert_ok(
        "use std.aes128gcm\n\
         fn accepts(k: Secret<Bytes>, n: Bytes, sealed: Untrusted<Bytes>) -> Bool {\n    \
             match aes128gcm.open(k, n, n, sealed) {\n        Some(_) => true\n        None => false\n    }\n}",
    );
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn every_module_checks_cleanly_has_no_audit_site_and_is_pure() {
    // A program that imports a module is checked with it linked in, and the
    // report lists only the program's own functions: the module's are the
    // toolchain's code. So purity is checked on each module itself, as a
    // file, where every function it declares, `pub` or not, is named.
    let program = check(EVERY_FUNCTION);
    assert!(
        program
            .functions
            .iter()
            .all(|f| MODULES.iter().all(|(_, name, _)| !f.name.contains(name))),
        "the report lists only the program's own functions"
    );
    for (path, _, source) in MODULES {
        let report = check(source);
        assert!(
            report.diagnostics.is_empty(),
            "{path}: {:#?}",
            report.diagnostics
        );
        assert!(report.audit.is_empty(), "{path}: {:#?}", report.audit);
        assert!(!report.functions.is_empty(), "{path} declares functions");
        for f in &report.functions {
            assert!(
                f.pure,
                "`{path}`: `{}` should be pure: {:?}",
                f.name, f.authority
            );
        }
        let code: Vec<&str> = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        for word in [".expose()", ".assume_trusted()"] {
            assert!(
                !code.iter().any(|line| line.contains(word)),
                "{path} calls `{word}`"
            );
        }
    }
}

#[test]
fn each_module_names_the_pure_functions_it_offers() {
    let offered = [
        ("std.x25519", vec!["public_key", "scalarmult"]),
        ("std.hkdf", vec!["extract", "expand"]),
        ("std.sha384", vec!["sha384"]),
        ("std.chacha20poly1305", vec!["seal", "open"]),
        ("std.aes128gcm", vec!["seal", "open"]),
    ];
    for ((path, _, source), (same, names)) in MODULES.iter().zip(offered) {
        assert_eq!(*path, same);
        let report = check(source);
        for name in names {
            let f = report
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{path}: the report should name `{name}`"));
            assert!(f.pure, "{path}: `{name}` should be pure: {:?}", f.authority);
        }
    }
}
