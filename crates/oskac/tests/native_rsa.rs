// SPDX-License-Identifier: Apache-2.0
//! `std.rsa` (#242) as programs run: Oska source linked into the program,
//! with `std.sha2`, which it imports, and run as Oska by both backends. The
//! edge cases are a test-bearing fixture (`fixtures/rsa_vectors.osk`),
//! which the native test corpus runs on both backends; this file runs
//! whole programs through the differential harness, from a single file
//! and from a package's module, and every Wycheproof RSA vector
//! (`vectors/rsa_wycheproof.osk`) natively, as the interpreter takes
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

/// A 2048-bit key, and its PKCS#1 v1.5 and PSS signatures of the message
/// "sample" with SHA-256 (made with OpenSSL), in hexadecimal.
const MODULUS: &str = "d8d6a066f65c2302961f1c01627dcfc318910f468f53a3e69db0978ac50854394d1bc0e3b9691b7fde86a2d086606a8a78e1919bc238f6f2e7b6571c795dcc3f28f1f96c2a9539214e2a04a0bfaa5f10f8ee2bf32a131f9aadfe57a3f5867d3d45c3231df8db0ca9051a99aa6fd348f1723cb11d57846ed50a10b0bf39d6a667469ed6529467ac1a32b4e60641218dce363077f9b8d07516922385d8fb6dbd2fdbdb24214b692f1ddddeb22e25c9a38f887757212dc9beea221db42a132c55e8a786f7009cd8033638c32bf66da3600e4932bf09f67044a06a88b84b652fe2a493836e88aa81371ddaf99d3e7fae1384d84061e3c541b7586e885673471927f9";
const EXPONENT: &str = "010001";
const DIGEST: &str = "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf";
const PKCS1: &str = "447cf5c0ba1c1b9af2a04f6ff2f0ee0b4792595a9295919a25f409fb07774357deaa292f1778824abb6f336dbb90834313d89ab4cbc4fa2bf77c006172c6254564538d8e889d8b248ab710b9fb3e76b58b3b0a3ebc0ec58d068bfda0a9a5c8ac664837c4ce4935268eb20f31ffa79611d23da4a294f0b8e228f5a3f3510631c780645a1181199f3256f0ec72674ece8be85ed0c7666e8be87cab7b6fb362da4c8be3d7f9884ba94ca68b893a93a6dd5dd42f8dd0138c05af94309fe403d38e7e4dc18a938d72d6f8997709909d144517edcee7907cec94f5b6afa60c916916709e20d86f700f884fe72a8116f190e8faa7f8a6dad8c3be89831750bcce1b898a";
const PSS: &str = "4f688e418eb4307e243ffacaa033c08afb60911d8291ebbecd5cae8ae3c51f8772a19fea198029e67c9011197f637eb9191ad0e4fbd307ea152628dd0e147aaeed7a4e666c18f7c5ae3e3b52fe006f3e783c21029eb915ce53615529745cf25ea7a5322da9756c101fd52e30cb9eae214fd9505f81da0ef7437d6d9ded9efd732ef5d89f10670723323f0582f115eeea6321e83ccb35aaff49e3fe1f49cc1d3b168a7ab7a7c5317ce7f30ea804297dfbefdb3422c14f06018dfe1bd7b2a3ec3b0559cee0ac1a827f57903eb84d0c49e094fe37302929d0e3e596cf1d07e4c9c449220e5a5934f9b59941d0321ff53f82ad09c3d24e4cd0ce6f34fe3ccbd1e2f3";

/// Every public function, on a modulus, exponent, digest and the two
/// signatures read from stdin, one line each, so all are untrusted.
const EVERY_FUNCTION: &str = r#"use std.rsa

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
    let modulus = line(input)?
    let exponent = line(input)?
    let digest = line(input)?
    let pkcs1 = line(input)?
    let pss = line(input)?
    match rsa.PublicKey.from_components(modulus, exponent) {
        Some(key) => {
            io.println(key.bits().to_string())?
            let hash = rsa.HashAlgorithm.Sha256
            io.println(if rsa.verify_pkcs1(key, hash, digest, pkcs1) { "valid" } else { "invalid" })?
            io.println(if rsa.verify_pss(key, hash, digest, pss) { "valid" } else { "invalid" })?
            io.println(if rsa.verify_pss(key, hash, digest, pkcs1) { "valid" } else { "invalid" })
        }
        None => io.println("not a key")
    }
}
"#;

fn stdin(modulus: &str, exponent: &str, digest: &str) -> String {
    format!("{modulus}\n{exponent}\n{digest}\n{PKCS1}\n{PSS}\n")
}

#[test]
fn every_function_gives_the_same_verdicts_on_both_backends() {
    let input = stdin(MODULUS, EXPONENT, DIGEST);
    let outcome = assert_same(&Case::file("rsa_every_function", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout, "2048\nvalid\nvalid\ninvalid\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_signature_for_another_digest_is_refused_on_both_backends() {
    let other = format!("00{}", &DIGEST[2..]);
    let input = stdin(MODULUS, EXPONENT, &other);
    let outcome = assert_same(&Case::file("rsa_other_digest", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout, "2048\ninvalid\ninvalid\ninvalid\n",
        "{outcome:#?}"
    );
}

#[test]
fn a_key_that_is_too_small_or_even_gives_the_same_on_both_backends() {
    // 1024 bits: the modulus' top half.
    let small = &MODULUS[..256];
    let input = stdin(small, EXPONENT, DIGEST);
    let outcome = assert_same(&Case::file("rsa_small_key", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(outcome.stdout, "not a key\n", "{outcome:#?}");
    // Even.
    let even = format!("{}fe", &MODULUS[..MODULUS.len() - 2]);
    let input = stdin(&even, EXPONENT, DIGEST);
    let outcome = assert_same(&Case::file("rsa_even_key", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(outcome.stdout, "not a key\n", "{outcome:#?}");
}

#[test]
fn a_package_module_verifies_on_both_backends() {
    let main = format!(
        "use pkg.check\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(if check.valid(\"{MODULUS}\", \"{EXPONENT}\", \"{DIGEST}\", \"{PSS}\") {{ \"valid\" }} else {{ \"invalid\" }})\n}}\n"
    );
    let case = Case::package(
        "rsa_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"verifier\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", &main),
            (
                "src/check.osk",
                "use std.rsa.{HashAlgorithm, PublicKey, verify_pss}\n\n\
                 fn hex(text: String) -> Bytes {\n    \
                 Bytes.from_hex(text).unwrap_or(Bytes.new())\n}\n\n\
                 pub(pkg) fn valid(n: String, e: String, digest: String, signature: String) -> Bool {\n    \
                 match PublicKey.from_components(hex(n), hex(e)) {\n        \
                 Some(k) => verify_pss(k, HashAlgorithm.Sha256, hex(digest), hex(signature))\n        \
                 None => false\n    }\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, "valid\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// Every Wycheproof vector in `vectors/rsa_wycheproof.osk`, natively: `oskac test`
/// on the file passes every test.
#[test]
fn every_wycheproof_vector_natively() {
    let src = include_str!("vectors/rsa_wycheproof.osk");
    let dir = Scratch::new("oska-rsa", "wycheproof");
    std::fs::write(dir.join("wycheproof.osk"), src).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "wycheproof.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("45 tests: 45 passed, 0 failed"), "{stdout}");
}

/// A program using `std.rsa` builds for every target with
/// `--target all`.
#[test]
fn a_verifier_builds_for_every_target() {
    let dir = Scratch::new("oska-rsa", "all");
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
