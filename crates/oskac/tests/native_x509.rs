// SPDX-License-Identifier: Apache-2.0
//! `std.x509` (#243) as programs run: Oska source linked into the program,
//! with the `std` modules it imports, and run as Oska by both backends.
//! The crafted chains are in two test files made by
//! `scripts/x509-roots/chains.py`: `fixtures/x509_cases.osk`, the cases that
//! need almost no signature check, which the test corpus runs on both
//! backends, and `vectors/x509_chains.osk`, every chain, which checks about
//! forty signatures and so runs natively here (the debug interpreter takes
//! over a minute on it). This file also runs whole programs through the differential harness, from a single file and
//! from a package's module, and every real root of Mozilla's store
//! (`vectors/x509_mozilla_roots.osk`) natively.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::process::Command;

use common::portable::Scratch;
use differential::{assert_same, Case};

const CHAINS: &str = include_str!("vectors/x509_chains.osk");

/// The hexadecimal certificate that `fn <name>() -> String` of the chains
/// fixture returns.
fn cert(name: &str) -> &'static str {
    let head = format!("fn {name}() -> String {{\n    \"");
    let start = CHAINS
        .find(&head)
        .unwrap_or_else(|| panic!("the fixture has `{name}`"))
        + head.len();
    let end = CHAINS[start..].find('"').expect("a closing quote") + start;
    &CHAINS[start..end]
}

/// A program that reads a root, a server certificate, one other
/// certificate and a host from stdin, one line each (the root is trusted
/// by the program's own assertion, which `oskac audit` would list), and
/// prints what every function of the module says about them.
const EVERY_FUNCTION: &str = r#"use std.x509

fn line(input: Stdin) -> Result<Untrusted<String>, IoError> {
    match input.read_line()? {
        Some(text) => Ok(text.trim())
        None => Ok("")
    }
}

fn bytes_of(text: Untrusted<String>) -> Untrusted<Bytes> {
    match Bytes.from_hex(text) {
        Some(bytes) => bytes
        None => Bytes.new()
    }
}

fn describe(label: String, result: Result<List<x509.Certificate>, x509.X509Error>) -> String {
    match result {
        Ok(path) => {
            let count = path.len()
            "{label} ok {count}"
        }
        Err(e) => "{label}: {e}"
    }
}

fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let root_hex = line(input)?
    let leaf_hex = line(input)?
    let other_hex = line(input)?
    let host = line(input)?.assume_trusted()
    let root = bytes_of(root_hex).assume_trusted()
    let store = match x509.TrustConfig.only_roots([]).with_root_der(root) {
        Ok(s) => s
        Err(_) => x509.TrustConfig.only_roots([])
    }
    io.println(store.len().to_string())?
    let leaf = match x509.Certificate.parse(bytes_of(leaf_hex)) {
        Ok(c) => c
        Err(e) => {
            return io.println("leaf: {e}")
        }
    }
    let other = match x509.Certificate.parse(bytes_of(other_hex)) {
        Ok(c) => [c]
        Err(_) => []
    }
    io.println(if leaf.is_ca() { "ca" } else { "leaf" })?
    io.println(leaf.not_before().to_string())?
    io.println(leaf.not_after().to_string())?
    var names: List<String> = []
    for name in leaf.dns_names() {
        names.push(name.assume_trusted())
    }
    io.println(",".join(names))?
    io.println(if leaf.matches_host(host) { "host matches" } else { "host differs" })?
    io.println(if leaf.public_key().is_some() { "key" } else { "no key" })?
    io.println(leaf.subject().to_hex())?
    io.println(leaf.issuer().to_hex())?
    io.println(leaf.serial().to_hex())?
    io.println(if store.contains(leaf) { "in store" } else { "not in store" })?
    io.println(if leaf.signed_by(leaf).is_ok() { "self-signed" } else { "not self-signed" })?
    io.println(describe("server", x509.verify_server(store, leaf, other, host, 1750000000)))?
    io.println(describe("chain", x509.verify_chain(store, leaf, other, 1750000000)))?
    io.println(describe("late", x509.verify_chain(store, leaf, other, 2100000000)))
}
"#;

fn stdin(root: &str, leaf: &str, other: &str, host: &str) -> String {
    format!("{root}\n{leaf}\n{other}\n{host}\n")
}

#[test]
fn a_valid_chain_gives_the_same_verdicts_on_both_backends() {
    let input = stdin(
        cert("root_a"),
        cert("leaf_a"),
        cert("int_a"),
        "example.test",
    );
    let outcome = assert_same(&Case::file("x509_valid_chain", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines[0], "1", "{outcome:#?}");
    assert_eq!(lines[1], "leaf", "{outcome:#?}");
    assert_eq!(lines[2], "1704067200", "{outcome:#?}");
    assert_eq!(lines[3], "2019686400", "{outcome:#?}");
    assert_eq!(
        lines[4],
        "example.test,www.example.test,*.wild.test,*.test,w*.example.test,*.sub.example.test",
        "{outcome:#?}"
    );
    assert_eq!(lines[5], "host matches", "{outcome:#?}");
    assert_eq!(lines[6], "key", "{outcome:#?}");
    assert_eq!(lines[10], "not in store", "{outcome:#?}");
    assert_eq!(lines[11], "not self-signed", "{outcome:#?}");
    assert_eq!(lines[12], "server ok 2", "{outcome:#?}");
    assert_eq!(lines[13], "chain ok 2", "{outcome:#?}");
    // Past the certificates' expiry (2034).
    assert_eq!(lines[14], "late: a certificate has expired", "{outcome:#?}");
}

#[test]
fn a_store_with_the_root_verifies_and_the_clock_matters_on_both_backends() {
    // The root is the program's store, and the intermediate is sent along.
    let input = stdin(cert("root_a"), cert("int_a"), cert("int_a"), "example.test");
    let outcome = assert_same(&Case::file("x509_ca_as_leaf", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(outcome.stdout.contains("\nca\n"), "{outcome:#?}");
    // A certificate that is the root: it is in the store and self-signed.
    let input = stdin(cert("self_signed"), cert("self_signed"), "", "self.test");
    let outcome = assert_same(&Case::file("x509_self_signed", EVERY_FUNCTION).with_stdin(&input));
    assert!(
        outcome.stdout.contains("\nin store\nself-signed\n"),
        "{outcome:#?}"
    );
    assert!(
        outcome
            .stdout
            .contains("\nserver ok 1\nchain ok 1\nlate: a certificate has expired\n"),
        "{outcome:#?}"
    );
}

#[test]
fn the_clock_decides_expiry_on_both_backends() {
    let input = stdin(
        cert("root_a"),
        cert("leaf_a"),
        cert("int_a"),
        "example.test",
    );
    let outcome = assert_same(&Case::file("x509_late", EVERY_FUNCTION).with_stdin(&input));
    assert!(
        outcome
            .stdout
            .ends_with("server ok 2\nchain ok 2\nlate: a certificate has expired\n"),
        "{outcome:#?}"
    );
    // A root sent as the server's certificate is in the store and self-signed,
    // but its key usage is for signing certificates, not a server's.
    let input = stdin(cert("root_a"), cert("root_a"), "", "example.test");
    let outcome =
        assert_same(&Case::file("x509_root_as_server", EVERY_FUNCTION).with_stdin(&input));
    assert!(
        outcome.stdout.contains("\nin store\nself-signed\n"),
        "{outcome:#?}"
    );
    assert!(
        outcome
            .stdout
            .contains("\nserver: the certificate is not for this host name\n"),
        "{outcome:#?}"
    );
    assert!(
        outcome
            .stdout
            .contains("\nchain: the certificate's key usage does not allow its use\n"),
        "{outcome:#?}"
    );
}

#[test]
fn a_malformed_certificate_is_refused_on_both_backends() {
    let good = cert("leaf_a");
    let input = stdin(cert("root_a"), &format!("{good}00"), "", "example.test");
    let outcome = assert_same(&Case::file("x509_trailing", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout, "1\nleaf: the certificate is malformed\n",
        "{outcome:#?}"
    );
    let input = stdin(cert("root_a"), "", "", "example.test");
    let outcome = assert_same(&Case::file("x509_empty", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout, "1\nleaf: the certificate is malformed\n",
        "{outcome:#?}"
    );
}

#[test]
fn a_package_module_verifies_on_both_backends() {
    let main = format!(
        "use pkg.trust\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(trust.verdict(\"{}\", \"{}\", \"{}\"))\n}}\n",
        cert("root_a"),
        cert("leaf_a"),
        cert("int_a")
    );
    let case = Case::package(
        "x509_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"verifier\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", &main),
            (
                "src/trust.osk",
                "use std.x509.{Certificate, TrustConfig, verify_server}\n\n\
                 fn hex(text: String) -> Bytes {\n    \
                 Bytes.from_hex(text).unwrap_or(Bytes.new())\n}\n\n\
                 fn check(store: TrustConfig, l: Certificate, o: Certificate) -> String {\n    \
                 match verify_server(store, l, [o], \"example.test\", 1750000000) {\n        \
                 Ok(_) => \"valid\"\n        Err(_) => \"invalid\"\n    }\n}\n\n\
                 pub(pkg) fn verdict(root: String, leaf: String, other: String) -> String {\n    \
                 let store = match TrustConfig.only_roots([]).with_root_der(hex(root)) {\n        \
                 Ok(s) => s\n        Err(_) => TrustConfig.only_roots([])\n    }\n    \
                 match Certificate.parse(hex(leaf)) {\n        \
                 Ok(l) => match Certificate.parse(hex(other)) {\n            \
                 Ok(o) => check(store, l, o)\n            Err(_) => \"malformed\"\n        }\n        \
                 Err(_) => \"malformed\"\n    }\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, "valid\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// Every crafted chain in `vectors/x509_chains.osk`, natively: `oskac test`
/// on the file passes every test.
#[test]
fn every_crafted_chain_natively() {
    let dir = Scratch::new("oska-x509", "chains");
    std::fs::write(dir.join("chains.osk"), CHAINS).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "chains.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("41 tests: 41 passed, 0 failed"), "{stdout}");
}

/// Every real root of Mozilla's store in `vectors/x509_mozilla_roots.osk`,
/// natively: `oskac test` on the file passes every test.
#[test]
fn every_real_root_natively() {
    let src = include_str!("vectors/x509_mozilla_roots.osk");
    let dir = Scratch::new("oska-x509", "roots");
    std::fs::write(dir.join("roots.osk"), src).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "roots.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("2 tests: 2 passed, 0 failed"), "{stdout}");
}

/// A program using `std.x509` builds for every target with
/// `--target all`.
#[test]
fn a_verifier_builds_for_every_target() {
    let dir = Scratch::new("oska-x509", "all");
    std::fs::write(dir.join("verify.osk"), EVERY_FUNCTION).expect("write the program");
    std::fs::create_dir(dir.join("out")).expect("make the output directory");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "verify.osk", "--target", "all", "-o", "out"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    assert!(output.status.success(), "{output:?}");
    let count = std::fs::read_dir(dir.join("out"))
        .expect("the output directory")
        .count();
    assert_eq!(count, 5, "one executable per target");
}
