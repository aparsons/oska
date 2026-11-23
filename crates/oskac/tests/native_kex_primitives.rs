//! The key-exchange and symmetric primitives of TLS 1.3 (#241) as programs
//! run: `std.x25519`, `std.hkdf`, `std.sha384`, `std.chacha20poly1305` and
//! `std.aes128gcm`, Oska source linked into the program and run as Oska by
//! both backends, with no `E0371`. Their test vectors are test-bearing
//! fixtures (`fixtures/x25519_*.osk`, `hkdf_*.osk`, `sha384_fips.osk`,
//! `chacha20poly1305_*.osk` and `aes128gcm_*.osk`), which the native test
//! corpus runs on both backends; this file runs whole programs through the
//! differential harness, from a single file and from a package's module,
//! builds one for every target, and runs natively what the interpreter is
//! too slow for: FIPS 180-4's one-million-`a` example, every Wycheproof
//! X25519 vector (`native_only/x25519_wycheproof_full.osk`; the fixture
//! the corpus runs on both backends holds every sixth) and RFC 7748's
//! 1,000-step ladder (`native_only/x25519_ladder.osk`).

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

/// A 32-byte key, as the text of the secret `KEY` in the environment.
const KEY: &str = "0123456789abcdef0123456789abcdef";

/// The public key of `KEY`, as X25519 computes it, and the peer's, with the
/// secret they share (recorded from OpenSSL).
const PUBLIC: &str = "86517bc0c39fe876b7646cb5e34ee03daf71ccbad67450c16a970ad8cbc03e7e";
const PEER: &str = "dbfca07744d1aa20b5b1e6ebe9568ccf15309ea27d666a3df07c09a680938512";
const SHARED: &str = "dfd3aa8a3164bcfe2fde3bc45d792c442bd95c080b28974fc59f8d70a676c107";

/// Every public function on a message from stdin, which is untrusted, and
/// a key from the environment: what each gives, then that each `open`
/// returns the message and refuses a changed one.
const EVERY_FUNCTION: &str = r#"use std.x25519
use std.hkdf
use std.sha384
use std.chacha20poly1305
use std.aes128gcm

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

fn opened(sealed: Option<Secret<Bytes>>, message: Untrusted<Bytes>) -> String {
    match sealed {
        Some(plain) => if plain.constant_time_eq(message) { "round trip" } else { "wrong message" }
        None => "refused"
    }
}

fn main(io: Stdout, env: Env, input: Stdin) -> Result<(), IoError> {
    let key = env.secret("KEY").to_bytes()
    let message = match input.read_line()? {
        Some(line) => line.trim().to_bytes()
        None => Bytes.new()
    }
    let peer = hex("dbfca07744d1aa20b5b1e6ebe9568ccf15309ea27d666a3df07c09a680938512")
    match x25519.public_key(key) {
        Some(public) => io.println(public.expose().to_hex())?
        None => io.println("no public key")?
    }
    match x25519.scalarmult(key, peer) {
        Some(shared) => io.println(shared.expose().to_hex())?
        None => io.println("no shared secret")?
    }
    let prk = hkdf.extract(Secret.new("salt".to_bytes()), key)
    match hkdf.expand(prk, "info".to_bytes(), 40) {
        Some(okm) => io.println(okm.expose().to_hex())?
        None => io.println("no output")?
    }
    io.println(sha384.sha384(message).to_hex())?
    var hasher = sha384.Sha384.new()
    hasher.update(message.slice(0, 5))
    hasher.update(message.slice(5, message.len()))
    io.println(if hasher.finish() == sha384.sha384(message) { "pieces match" } else { "pieces differ" })?
    let nonce = hex("000102030405060708090a0b")
    let aad = "header".to_bytes()
    match chacha20poly1305.seal(key, nonce, aad, Secret.new("attack at dawn".to_bytes())) {
        Some(sealed) => {
            io.println(sealed.expose().to_hex())?
            io.println(opened(chacha20poly1305.open(key, nonce, aad, sealed.expose()), message))?
            var forged = sealed.expose()
            forged.set(0, forged.at(0) ^ 1)
            io.println(opened(chacha20poly1305.open(key, nonce, aad, forged), message))?
        }
        None => io.println("no sealed message")?
    }
    let short = key.slice(0, 16)
    match aes128gcm.seal(short, nonce, aad, Secret.new("attack at dawn".to_bytes())) {
        Some(sealed) => {
            io.println(sealed.expose().to_hex())?
            io.println(opened(aes128gcm.open(short, nonce, aad, sealed.expose()), message))?
            var forged = sealed.expose()
            forged.set(forged.len() - 1, forged.at(forged.len() - 1) ^ 1)
            io.println(opened(aes128gcm.open(short, nonce, aad, forged), message))?
        }
        None => io.println("no sealed message")?
    }
    Ok(())
}
"#;

const OKM: &str =
    "28fca426041dba05582c710bd1c002af9bad4f4dac95d0306938649ae096b466aa8613c13356e431";
const SHA384: &str = "62a3ba26b1247ae8d3e0ec5759604688f0d33c528dc21ab52676685f1f006562d74cb75be8eac73857801db51e685f95";
const CHACHA: &str = "994b20590428dfcae3329dea42e650e5bf959dc23a419f9dfe17c918c48f";
const AES: &str = "9c4bb07c7d1f1b579c06637bb7f52d1fbbf65a8a49eebfe28cd19b57ef55";

#[test]
fn every_function_gives_the_same_answers_on_both_backends() {
    let case = Case::file("kex_every_function", EVERY_FUNCTION)
        .with_env("KEY", KEY)
        .with_stdin("attack at dawn\n");
    let outcome = assert_same(&case);
    assert_eq!(
        outcome.stdout,
        format!(
            "{PUBLIC}\n{SHARED}\n{OKM}\n{SHA384}\npieces match\n{CHACHA}\nround trip\nrefused\n{AES}\nround trip\nrefused\n"
        ),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(PEER.len() == 64, "the peer's key is 32 bytes");
}

#[test]
fn a_package_module_seals_on_both_backends() {
    let case = Case::package(
        "kex_package",
        &[
            ("oska.toml", "[package]\nname = \"vault\"\nversion = \"0.1.0\"\n"),
            (
                "src/main.osk",
                "use pkg.vault\n\nfn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
                 io.println(vault.seal_hex(env.secret(\"KEY\").to_bytes()))\n}\n",
            ),
            (
                "src/vault.osk",
                "use std.chacha20poly1305.{seal}\n\n\
                 pub(pkg) fn seal_hex(key: Secret<Bytes>) -> String {\n    \
                 let nonce = Bytes.from_hex(\"000102030405060708090a0b\").unwrap_or(Bytes.new())\n    \
                 match seal(key, nonce, \"header\".to_bytes(), Secret.new(\"attack at dawn\".to_bytes())) {\n        \
                 Some(sealed) => sealed.expose().to_hex()\n        None => \"refused\"\n    }\n}\n",
            ),
        ],
    )
    .with_env("KEY", KEY);
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, format!("{CHACHA}\n"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// A program using each secret module builds for every target with
/// `--target all`: the secret-marking invariant holds on each, with no
/// internal error (`E0398`), and two builds on this host write the same
/// bytes. That every host writes the same bytes is CI's
/// `cross-build-compare`, whose corpus holds the fixtures of the five
/// modules.
#[test]
fn the_primitives_build_for_every_target_the_same_twice() {
    let src = r#"use std.x25519
use std.hkdf
use std.chacha20poly1305
use std.aes128gcm

fn main(io: Stdout, env: Env, args: Args) -> Result<(), IoError> {
    let key = env.secret("KEY").to_bytes()
    let peer = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let nonce = Bytes.zeroed(12)
    let prk = match x25519.scalarmult(key, peer) {
        Some(shared) => hkdf.extract(Secret.new("salt".to_bytes()), shared)
        None => key
    }
    let sealed = match chacha20poly1305.seal(prk, nonce, peer, Secret.new("message".to_bytes())) {
        Some(out) => out
        None => key
    }
    let ok = match aes128gcm.open(prk.slice(0, 16), nonce, peer, peer) {
        Some(_) => true
        None => false
    }
    io.println(if ok || sealed.len() == 0 { "valid" } else { "invalid" })
}
"#;
    let dir = Scratch::new("oska-kex", "all");
    std::fs::write(dir.join("kex.osk"), src).expect("write the program");
    let mut builds = Vec::new();
    for out in ["first", "second"] {
        std::fs::create_dir(dir.join(out)).expect("make the output directory");
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "kex.osk", "--target", "all", "-o", out])
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

/// FIPS 180-4's third example for SHA-384: one million `a`, fed a thousand
/// bytes at a time. It runs natively only: the interpreter takes over a
/// minute on it, and the same code runs on both backends in every other
/// vector.
#[test]
fn one_million_a_natively() {
    let src = r#"use std.sha384

fn main(io: Stdout) -> Result<(), IoError> {
    let block = "a".repeat(1000).to_bytes()
    var h = sha384.Sha384.new()
    for _ in 0..1000 {
        h.update(block)
    }
    io.println(h.finish().to_hex())
}
"#;
    let dir = Scratch::new("oska-sha384", "million");
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
        "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985\n"
    );
}

/// The test blocks of a file under `tests/native_only`, run natively by
/// `oskac test`: all must pass, and at least `least` must run.
fn native_only_tests_pass(file: &str, least: usize) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/native_only")
        .join(file);
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("test")
        .arg(&path)
        .output()
        .expect("oskac runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{file}: {output:?}");
    assert!(stdout.contains("0 failed"), "{file}: {stdout}");
    let passed = stdout.matches("... ok").count();
    assert!(passed >= least, "{file}: {passed} tests passed: {stdout}");
}

/// Every one of Wycheproof's 518 X25519 vectors, which the interpreter
/// takes about a second each to run.
#[test]
fn every_wycheproof_x25519_vector_passes_natively() {
    native_only_tests_pass("x25519_wycheproof_full.osk", 13);
}

/// RFC 7748's 1,000 iterations of the ladder, which end at the published
/// value.
#[test]
fn the_rfc_7748_ladder_of_1000_steps_passes_natively() {
    native_only_tests_pass("x25519_ladder.osk", 1);
}
