// SPDX-License-Identifier: Apache-2.0
//! `std.ed25519` (#233) as programs run: Oska source linked into the
//! program, with `std.sha2`, which it imports, and run as Oska by both
//! backends. RFC 8032's vectors and the edge cases are a test-bearing
//! fixture (`fixtures/ed25519_rfc8032.osk`), which the native test corpus
//! runs on both backends; this file runs whole programs through the
//! differential harness, from a single file and from a package's module,
//! and every Wycheproof vector (`vectors/ed25519_wycheproof.osk`)
//! natively, as the interpreter takes minutes over them.

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

/// RFC 8032's TEST 2 (section 7.1): a key, a one-byte message and its
/// signature, in hexadecimal.
const KEY: &str = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
const MESSAGE: &str = "72";
const SIGNATURE: &str = "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
                         085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00";

/// Every public function, on a key, message and signature read from
/// stdin, one hexadecimal line each, so all three are untrusted.
const EVERY_FUNCTION: &str = r#"use std.ed25519

fn line(input: Stdin) -> Result<Untrusted<Bytes>, IoError> {
    let text = match input.read_line()? {
        Some(line) => line.trim()
        None => ""
    }
    Ok(match Bytes.from_hex(text) {
        Some(bytes) => bytes
        None => Bytes.new()
    })
}

fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let raw_key = line(input)?
    let message = line(input)?
    let signature = line(input)?
    match ed25519.PublicKey.from_bytes(raw_key) {
        Some(key) => {
            io.println(key.to_bytes().to_hex())?
            io.println(if ed25519.verify(key, message, signature) { "valid" } else { "invalid" })?
            var other = message
            other.push(0)
            io.println(if ed25519.verify(key, other, signature) { "valid" } else { "invalid" })
        }
        None => io.println("not a key")
    }
}
"#;

#[test]
fn every_function_gives_the_same_verdicts_on_both_backends() {
    let stdin = format!("{KEY}\n{MESSAGE}\n{SIGNATURE}\n");
    let outcome =
        assert_same(&Case::file("ed25519_every_function", EVERY_FUNCTION).with_stdin(&stdin));
    assert_eq!(
        outcome.stdout,
        format!("{KEY}\nvalid\ninvalid\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_key_that_is_not_a_point_gives_the_same_on_both_backends() {
    // y = 2 has no x on the curve.
    let stdin = format!(
        "0200000000000000000000000000000000000000000000000000000000000000\n{MESSAGE}\n{SIGNATURE}\n"
    );
    let outcome =
        assert_same(&Case::file("ed25519_not_a_point", EVERY_FUNCTION).with_stdin(&stdin));
    assert_eq!(outcome.stdout, "not a key\n", "{outcome:#?}");
}

#[test]
fn a_package_module_verifies_on_both_backends() {
    let main = format!(
        "use pkg.check\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(if check.valid(\"{KEY}\", \"{MESSAGE}\", \"{SIGNATURE}\") {{ \"valid\" }} else {{ \"invalid\" }})\n}}\n"
    );
    let case = Case::package(
        "ed25519_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"verifier\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", &main),
            (
                "src/check.osk",
                "use std.ed25519.{PublicKey, verify}\n\n\
                 fn hex(text: String) -> Bytes {\n    \
                 Bytes.from_hex(text).unwrap_or(Bytes.new())\n}\n\n\
                 pub(pkg) fn valid(key: String, message: String, signature: String) -> Bool {\n    \
                 match PublicKey.from_bytes(hex(key)) {\n        \
                 Some(k) => verify(k, hex(message), hex(signature))\n        \
                 None => false\n    }\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, "valid\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// Every Wycheproof Ed25519 vector, natively: `oskac test` on the file
/// passes every test.
#[test]
fn every_wycheproof_vector_natively() {
    let src = include_str!("vectors/ed25519_wycheproof.osk");
    let dir = Scratch::new("oska-ed25519", "wycheproof");
    std::fs::write(dir.join("wycheproof.osk"), src).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "wycheproof.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("14 tests: 14 passed, 0 failed"), "{stdout}");
}

/// A program using `std.ed25519` builds for every target with
/// `--target all`.
#[test]
fn a_verifier_builds_for_every_target() {
    let dir = Scratch::new("oska-ed25519", "all");
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
