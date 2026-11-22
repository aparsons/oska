//! `std.sha2` (#231) as programs run: the first `std` module of Oska
//! source, linked into the program and run as Oska by both backends, with
//! no `E0371`. Its test vectors are test-bearing fixtures
//! (`fixtures/sha2_fips.osk` and `fixtures/sha2_cavp.osk`), which the
//! native test corpus runs on both backends; this file runs whole programs
//! through the differential harness, from a single file and from a
//! package's module, and FIPS 180-4's one-million-`a` example natively.

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

/// The digests of "abc" (FIPS 180-4's example): SHA-256, then SHA-512.
const ABC256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const ABC512: &str = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
                      2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";

/// Every public function on "abc", read from stdin, so the digests are of
/// untrusted bytes.
const EVERY_FUNCTION: &str = r#"use std.sha2

fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let raw = match input.read_line()? {
        Some(line) => line.trim().to_bytes()
        None => Bytes.new()
    }
    io.println(sha2.sha256(raw).to_hex())?
    io.println(sha2.sha512(raw).to_hex())?
    var h = sha2.Sha256.new()
    h.update(raw)
    io.println(h.finish().to_hex())?
    var g = sha2.Sha512.new()
    g.update(raw)
    io.println(g.finish().to_hex())?
    let key = Secret.new(sha2.sha256(raw))
    let digest = sha2.sha256_secret(Secret.new("abc".to_bytes()))
    var s = sha2.Sha256Secret.new()
    s.update(Secret.new("abc".to_bytes()))
    let same = digest.constant_time_eq_secret(key) && s.finish().constant_time_eq_secret(key)
    io.println(if same { "secret digests match" } else { "secret digests differ" })
}
"#;

#[test]
fn every_function_gives_the_same_digests_on_both_backends() {
    let outcome =
        assert_same(&Case::file("sha2_every_function", EVERY_FUNCTION).with_stdin("abc\n"));
    assert_eq!(
        outcome.stdout,
        format!("{ABC256}\n{ABC512}\n{ABC256}\n{ABC512}\nsecret digests match\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_package_module_hashes_on_both_backends() {
    let case = Case::package(
        "sha2_package",
        &[
            (
                "oska.toml",
                "[package]\nname = \"digests\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/main.osk",
                "use pkg.digest\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    \
                 io.println(digest.hex(\"abc\"))\n}\n",
            ),
            (
                "src/digest.osk",
                "use std.sha2.{sha256}\n\n\
                 pub(pkg) fn hex(text: String) -> String {\n    \
                 sha256(text.to_bytes()).to_hex()\n}\n",
            ),
        ],
    );
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, format!("{ABC256}\n"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// FIPS 180-4's third example for each function: one million `a`, fed a
/// thousand bytes at a time. It runs natively only: the interpreter takes
/// over a minute on it, and the same code runs on both backends in every
/// other vector.
#[test]
fn one_million_a_natively() {
    let src = r#"use std.sha2

fn main(io: Stdout) -> Result<(), IoError> {
    let block = "a".repeat(1000).to_bytes()
    var h = sha2.Sha256.new()
    var g = sha2.Sha512.new()
    var s = sha2.Sha256Secret.new()
    for _ in 0..1000 {
        h.update(block)
        g.update(block)
        s.update(Secret.new(block))
    }
    io.println(h.finish().to_hex())?
    io.println(g.finish().to_hex())?
    let want = Secret.new(h.finish())
    io.println(if s.finish().constant_time_eq_secret(want) { "secret matches" } else { "secret differs" })
}
"#;
    let dir = Scratch::new("oska-sha2", "million");
    std::fs::write(dir.join("million.osk"), src).expect("write the program");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["run", "million.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout,
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0\n\
         e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
         de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b\n\
         secret matches\n"
    );
}

/// A program using `sha256_secret` builds for every target with
/// `--target all`: the secret-marking invariant holds on each, with no
/// internal error (`E0398`), and two builds on this host write the same
/// bytes. That every host writes the same bytes is CI's
/// `cross-build-compare`, whose corpus holds `fixtures/sha2_fips.osk`.
#[test]
fn the_secret_hash_builds_for_every_target_the_same_twice() {
    let src = r#"use std.sha2

fn main(io: Stdout) -> Result<(), IoError> {
    let key = Secret.new("key material".to_bytes())
    let tag = sha2.sha256_secret(key)
    let same = tag.constant_time_eq_secret(sha2.sha256_secret(key))
    io.println(if same { "same" } else { "different" })
}
"#;
    let dir = Scratch::new("oska-sha2", "all");
    std::fs::write(dir.join("tag.osk"), src).expect("write the program");
    let mut builds = Vec::new();
    for out in ["first", "second"] {
        std::fs::create_dir(dir.join(out)).expect("make the output directory");
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "tag.osk", "--target", "all", "-o", out])
            // A cache of each build's own, so the second is built afresh.
            .env("OSKA_CACHE_DIR", dir.join(format!("cache-{out}")))
            .current_dir(&dir)
            .output()
            .expect("oskac runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{output:?}");
        assert!(!stderr.contains("E0398"), "{stderr}");
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir.join(out))
            .expect("the output directory")
            .map(|e| {
                let path = e.expect("an entry").path();
                let name = path
                    .file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned();
                (name, std::fs::read(&path).expect("an executable"))
            })
            .collect();
        files.sort();
        builds.push(files);
    }
    let names: Vec<&str> = builds[0].iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names.len(), 5, "one executable per target: {names:?}");
    assert!(builds[0] == builds[1], "two builds differ: {names:?}");
}
