// SPDX-License-Identifier: Apache-2.0
//! `std.p256` (#242) as programs run: Oska source over secret integers,
//! linked into the program with `std.ecdsa`, which it imports, and run as
//! Oska by both backends. RFC 5903's vectors and the edge cases are a
//! test-bearing fixture (`fixtures/p256_rfc5903.osk`), which the native
//! test corpus runs on both backends; this file runs whole programs through
//! the differential harness, from a single file and from a package's
//! module, and every Wycheproof ECDH vector
//! (`vectors/p256_ecdh_wycheproof.osk`) natively. That the machine code has
//! no branch or address that depends on the scalar is held by
//! `oska-native`'s constant-time tests.

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

/// RFC 5903 section 8.1: the initiator's private scalar and public point,
/// the responder's public point, and the shared secret, in hexadecimal.
const SCALAR: &str = "C88F01F510D9AC3F70A292DAA2316DE544E9AAB8AFE84049C62A9C57862D1433";
const PEER: &str = "04D12DFB5289C8D4F81208B70270398C342296970A0BCCB74C736FC7554494BF6356FBF3CA366CC23E8157854C13C58D6AAC23F046ADA30F8353E74F33039872AB";
const PUBLIC: &str = "04DAD0B65394221CF9B051E1FECA5787D098DFE637FC90B9EF945D0C3772581180\
                      5271A0461CDB8252D61F1C456FA3E59AB1F45B33ACCF5F58389E0577B8990BB3";
const SHARED: &str = "D6840F6B42F6EDAFD13116E0E12565202FEF8E9ECE7DCE03812464D04B9442DE";

/// Every public function, on a scalar and a peer's point read from stdin,
/// one hexadecimal line each. The scalar is made secret (and vouched for,
/// which the program's audit lists) and the peer is untrusted; the program
/// exposes the results to print them.
const EVERY_FUNCTION: &str = r#"use std.p256

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
    let scalar: Secret<Bytes> = Secret.new(line(input)?.assume_trusted())
    let peer = line(input)?
    match p256.public_key(scalar) {
        Some(point) => io.println(point.expose().to_hex())?
        None => io.println("no public key")?
    }
    match p256.shared_secret(scalar, peer) {
        Some(shared) => io.println(shared.expose().to_hex())
        None => io.println("no shared secret")
    }
}
"#;

#[test]
fn rfc_5903_gives_the_same_shared_secret_on_both_backends() {
    let input = format!("{SCALAR}\n{PEER}\n");
    let outcome =
        assert_same(&Case::file("p256_every_function", EVERY_FUNCTION).with_stdin(&input));
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{outcome:#?}");
    assert_eq!(lines[0], PUBLIC.to_lowercase(), "{outcome:#?}");
    assert_eq!(lines[1], SHARED.to_lowercase(), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_peer_that_is_not_a_point_gives_the_same_on_both_backends() {
    // The responder's point with its y changed is not on the curve.
    let off_curve = format!("{}00", &PEER[..PEER.len() - 2]);
    let input = format!("{SCALAR}\n{off_curve}\n");
    let outcome = assert_same(&Case::file("p256_off_curve", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout,
        format!("{}\nno shared secret\n", PUBLIC.to_lowercase()),
        "{outcome:#?}"
    );
}

#[test]
fn a_scalar_of_the_wrong_length_gives_the_same_on_both_backends() {
    let input = format!("{}\n{PEER}\n", &SCALAR[2..]);
    let outcome = assert_same(&Case::file("p256_short_scalar", EVERY_FUNCTION).with_stdin(&input));
    assert_eq!(
        outcome.stdout, "no public key\nno shared secret\n",
        "{outcome:#?}"
    );
}

#[test]
fn a_package_module_agrees_on_both_backends() {
    let main = format!(
        "use pkg.exchange\n\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(exchange.shared(\"{SCALAR}\", \"{PEER}\"))\n}}\n"
    );
    let case = Case::package(
        "p256_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"exchange\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", &main),
            (
                "src/exchange.osk",
                "use std.p256.{shared_secret}\n\n\
                 fn hex(text: String) -> Bytes {\n    \
                 Bytes.from_hex(text).unwrap_or(Bytes.new())\n}\n\n\
                 pub(pkg) fn shared(scalar: String, peer: String) -> String {\n    \
                 match shared_secret(Secret.new(hex(scalar)), hex(peer)) {\n        \
                 Some(secret) => secret.expose().to_hex()\n        \
                 None => \"none\"\n    }\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        format!("{}\n", SHARED.to_lowercase()),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// Every Wycheproof vector in `vectors/p256_ecdh_wycheproof.osk`, natively: `oskac test`
/// on the file passes every test.
#[test]
fn every_wycheproof_vector_natively() {
    let src = include_str!("vectors/p256_ecdh_wycheproof.osk");
    let dir = Scratch::new("oska-p256", "wycheproof");
    std::fs::write(dir.join("wycheproof.osk"), src).expect("write the tests");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "wycheproof.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("12 tests: 12 passed, 0 failed"), "{stdout}");
}

/// A program using `std.p256` builds for every target with
/// `--target all`.
#[test]
fn a_verifier_builds_for_every_target() {
    let dir = Scratch::new("oska-p256", "all");
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
