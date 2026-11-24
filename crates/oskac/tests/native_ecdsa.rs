// SPDX-License-Identifier: Apache-2.0
//! `std.ecdsa` (#242) as programs run: Oska source linked into the program
//! and run as Oska by both backends. RFC 6979's vectors and the edge cases
//! are a test-bearing fixture (`fixtures/ecdsa_vectors.osk`), which the
//! native test corpus runs on both backends; this file runs whole programs
//! through the differential harness, from a single file and from a
//! package's module, and every Wycheproof ECDSA vector
//! (`vectors/ecdsa_wycheproof.osk`) natively, as the interpreter takes
//! minutes over them.

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

/// RFC 6979 appendix A.2.5 (P-256 with SHA-256) and A.2.6 (P-384 with
/// SHA-384): a key, the digest of "sample" and its signature, as `r || s`
/// and in DER, in hexadecimal.
const KEY_256: &str = "0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299";
const DIGEST_256: &str = "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf";
const RAW_256: &str = "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8";
const DER_256: &str = "3046022100efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716022100f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8";
const KEY_384: &str = "04ec3a4e415b4e19a4568618029f427fa5da9a8bc4ae92e02e06aae5286b300c64def8f0ea9055866064a254515480bc138015d9b72d7d57244ea8ef9ac0c621896708a59367f9dfb9f54ca84b3f1c9db1288b231c3ae0d4fe7344fd2533264720";
const DIGEST_384: &str = "9a9083505bc92276aec4be312696ef7bf3bf603f4bbd381196a029f340585312313bca4a9b5b890efee42c77b1ee25fe";
const RAW_384: &str = "94edbb92a5ecb8aad4736e56c691916b3f88140666ce9fa73d64c4ea95ad133c81a648152e44acf96e36dd1e80fabe4699ef4aeb15f178cea1fe40db2603138f130e740a19624526203b6351d0a3a94fa329c145786e679e7b82c71a38628ac8";
const DER_384: &str = "306602310094edbb92a5ecb8aad4736e56c691916b3f88140666ce9fa73d64c4ea95ad133c81a648152e44acf96e36dd1e80fabe4602310099ef4aeb15f178cea1fe40db2603138f130e740a19624526203b6351d0a3a94fa329c145786e679e7b82c71a38628ac8";

/// Every public function, on a curve name, key, digest and the two
/// forms of a signature read from stdin, one line each, so all are
/// untrusted.
const EVERY_FUNCTION: &str = r#"use std.ecdsa

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
    let curve = if line(input)?.len() == 1 { ecdsa.Curve.P256 } else { ecdsa.Curve.P384 }
    let raw_key = line(input)?
    let digest = line(input)?
    let raw = line(input)?
    let der = line(input)?
    match ecdsa.PublicKey.from_bytes(curve, raw_key) {
        Some(key) => {
            io.println(key.to_bytes().to_hex())?
            io.println(if ecdsa.verify(key, digest, raw) { "valid" } else { "invalid" })?
            io.println(if ecdsa.verify_der(key, digest, der) { "valid" } else { "invalid" })?
            io.println(if ecdsa.verify(key, digest, der) { "valid" } else { "invalid" })
        }
        None => io.println("not a key")
    }
}
"#;

fn stdin(curve: &str, key: &str, digest: &str, raw: &str, der: &str) -> String {
    format!("{curve}\n{key}\n{digest}\n{raw}\n{der}\n")
}

#[test]
fn p256_gives_the_same_verdicts_on_both_backends() {
    let input = stdin("01", KEY_256, DIGEST_256, RAW_256, DER_256);
    let outcome = assert_same(&Case::file("ecdsa_p256", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout,
        format!("{KEY_256}\nvalid\nvalid\ninvalid\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn p384_gives_the_same_verdicts_on_both_backends() {
    let input = stdin("0000", KEY_384, DIGEST_384, RAW_384, DER_384);
    let outcome = assert_same(&Case::file("ecdsa_p384", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout,
        format!("{KEY_384}\nvalid\nvalid\ninvalid\n"),
        "{outcome:#?}"
    );
}

#[test]
fn a_signature_for_another_digest_is_refused_on_both_backends() {
    let other = format!("00{}", &DIGEST_256[2..]);
    let input = stdin("01", KEY_256, &other, RAW_256, DER_256);
    let outcome = assert_same(&Case::file("ecdsa_other_digest", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout,
        format!("{KEY_256}\ninvalid\ninvalid\ninvalid\n"),
        "{outcome:#?}"
    );
}

#[test]
fn a_key_that_is_not_a_point_gives_the_same_on_both_backends() {
    // The base point with its y changed is not on the curve.
    let off_curve = format!("{}00", &KEY_256[..KEY_256.len() - 2]);
    let input = stdin("01", &off_curve, DIGEST_256, RAW_256, DER_256);
    let outcome = assert_same(&Case::file("ecdsa_not_a_point", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(outcome.stdout, "not a key\n", "{outcome:#?}");
}

#[test]
fn a_package_module_verifies_on_both_backends() {
    let main = format!(
        "use pkg.check\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(if check.valid(\"{KEY_256}\", \"{DIGEST_256}\", \"{DER_256}\") {{ \"valid\" }} else {{ \"invalid\" }})\n}}\n"
    );
    let case = Case::package(
        "ecdsa_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"verifier\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", &main),
            (
                "src/check.osk",
                "use std.ecdsa.{Curve, PublicKey, verify_der}\n\n\
                 fn hex(text: String) -> Bytes {\n    \
                 Bytes.from_hex(text).unwrap_or(Bytes.new())\n}\n\n\
                 pub(pkg) fn valid(key: String, digest: String, signature: String) -> Bool {\n    \
                 match PublicKey.from_bytes(Curve.P256, hex(key)) {\n        \
                 Some(k) => verify_der(k, hex(digest), hex(signature))\n        \
                 None => false\n    }\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, "valid\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// Every Wycheproof vector in `vectors/ecdsa_wycheproof.osk`, natively: `oskac test`
/// on the file passes every test.
#[test]
fn every_wycheproof_vector_natively() {
    let src = include_str!("vectors/ecdsa_wycheproof.osk");
    let dir = Scratch::new("oska-ecdsa", "wycheproof");
    std::fs::write(dir.join("wycheproof.osk"), src).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "wycheproof.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("55 tests: 55 passed, 0 failed"), "{stdout}");
}

/// A program using `std.ecdsa` builds for every target with
/// `--target all`.
#[test]
fn a_verifier_builds_for_every_target() {
    let dir = Scratch::new("oska-ecdsa", "all");
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
