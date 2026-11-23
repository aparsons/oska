// SPDX-License-Identifier: Apache-2.0
//! `std.hmac` (#232) as programs run: Oska source that uses `std.sha2`,
//! linked into the program and run as Oska by both backends, with no
//! `E0371`. Its test vectors are test-bearing fixtures
//! (`fixtures/hmac_rfc4231.osk` and `fixtures/hmac_wycheproof.osk`), which
//! the native test corpus runs on both backends; this file runs whole
//! programs through the differential harness, with a key from the
//! environment made into bytes by `Secret<String>.to_bytes`, and builds
//! one for every target.

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

/// HMAC-SHA-256 of "The quick brown fox jumps over the lazy dog" under
/// the key "key", as Python's `hmac` module gives it.
const FOX: &str = "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8";

/// A key from the environment, a message from stdin and a tag from the
/// command line: the tag, exposed, and both verdicts on the tag given.
const AUTHENTICATE: &str = r#"use std.hmac

fn main(io: Stdout, env: Env, input: Stdin, args: Args) -> Result<(), IoError> {
    let key = env.secret("HMAC_KEY").to_bytes()
    let message = match input.read_line()? {
        Some(line) => line.trim().to_bytes()
        None => Bytes.new()
    }
    let given = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let tag = hmac.sha256(key, message)
    io.println(tag.expose().to_hex())?
    io.println(if hmac.verify_sha256(key, message, given) { "valid" } else { "invalid" })?
    let truncated = given.slice(0, 16)
    io.println(if tag.slice(0, 16).constant_time_eq(truncated) { "prefix valid" } else { "prefix invalid" })
}
"#;

fn authenticate(name: &str, tag: &str) -> Case {
    Case::file(name, AUTHENTICATE)
        .with_env("HMAC_KEY", "key")
        .with_stdin("The quick brown fox jumps over the lazy dog\n")
        .with_args(&[tag])
}

#[test]
fn a_tag_and_its_verdict_are_the_same_on_both_backends() {
    let outcome = assert_same(&authenticate("hmac_valid", FOX));
    assert_eq!(
        outcome.stdout,
        format!("{FOX}\nvalid\nprefix valid\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_changed_tag_is_refused_on_both_backends() {
    // The last bit flipped: the prefix still matches, the whole does not.
    let changed = format!("{}9", &FOX[..63]);
    let outcome = assert_same(&authenticate("hmac_changed", &changed));
    assert_eq!(
        outcome.stdout,
        format!("{FOX}\ninvalid\nprefix valid\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn a_package_module_authenticates_on_both_backends() {
    let case = Case::package(
        "hmac_package",
        &[
            ("oska.toml", "[package]\nname = \"auth\"\nversion = \"0.1.0\"\n"),
            (
                "src/main.osk",
                "use pkg.auth\n\nfn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
                 io.println(auth.tag(env.secret(\"HMAC_KEY\").to_bytes()))\n}\n",
            ),
            (
                "src/auth.osk",
                "use std.hmac.{sha256}\n\n\
                 pub(pkg) fn tag(key: Secret<Bytes>) -> String {\n    \
                 sha256(key, \"The quick brown fox jumps over the lazy dog\".to_bytes()).expose().to_hex()\n}\n",
            ),
        ],
    )
    .with_env("HMAC_KEY", "key");
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, format!("{FOX}\n"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

/// A program using `sha2.sha256_secret` and `hmac.verify_sha256` builds
/// for every target with `--target all`: the secret-marking invariant
/// holds on each, with no internal error (`E0398`), and two builds on
/// this host write the same bytes. That every host writes the same bytes
/// is CI's `cross-build-compare`, whose corpus holds
/// `fixtures/hmac_rfc4231.osk`.
#[test]
fn hmac_builds_for_every_target_the_same_twice() {
    let src = r#"use std.hmac
use std.sha2

fn main(io: Stdout, env: Env, args: Args) -> Result<(), IoError> {
    let key = sha2.sha256_secret(env.secret("HMAC_KEY").to_bytes())
    let tag = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let valid = hmac.verify_sha256(key, "message".to_bytes(), tag)
    io.println(if valid { "valid" } else { "invalid" })
}
"#;
    let dir = Scratch::new("oska-hmac", "all");
    std::fs::write(dir.join("verify.osk"), src).expect("write the program");
    let mut builds = Vec::new();
    for out in ["first", "second"] {
        std::fs::create_dir(dir.join(out)).expect("make the output directory");
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "verify.osk", "--target", "all", "-o", out])
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
