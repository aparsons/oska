// SPDX-License-Identifier: Apache-2.0
//! Bytes through the confined file capabilities (#223), run on both
//! backends through the differential harness, on every platform that
//! runs native executables: `read_bytes` gives a file that is not UTF-8
//! exactly, `write_bytes` writes exactly the bytes given with no newline
//! translation, and both are confined as the text methods are.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, Case, Outcome};
use std::path::Path;

/// The bytes of `root/blob.bin`: not UTF-8, with a NUL, a carriage return
/// and a line feed.
const BLOB: &[u8] = &[0x80, 0x00, 0xff, 0x41, 0x0d, 0x0a, 0xc3];

/// The helpers every program has: what a read or a write gave, as a line.
const SHOWN: &str = r#"
fn hex(result: Result<Untrusted<Bytes>, IoError>) -> Untrusted<String> {
    match result {
        Ok(bytes) => bytes.to_hex()
        Err(e) => refused(e)
    }
}

fn shown(result: Result<Untrusted<String>, IoError>) -> Untrusted<String> {
    match result {
        Ok(text) => text
        Err(e) => refused(e)
    }
}

fn done(result: Result<(), IoError>) -> Untrusted<String> {
    match result {
        Ok(_) => "done"
        Err(e) => refused(e)
    }
}

fn refused(e: IoError) -> Untrusted<String> {
    let message = e.message
    "REFUSED: {message}"
}
"#;

/// Plant `root/blob.bin` and the directory `root/out`.
fn plant(dir: &Path) {
    std::fs::create_dir_all(dir.join("root").join("out")).expect("should make root/out");
    std::fs::write(dir.join("root").join("blob.bin"), BLOB).expect("should write the blob");
}

/// A program whose `main` has `io` and `fs` and runs `body`, on a tree
/// with `root/blob.bin`, `root/out/` and `outside/secret.txt`.
fn case(name: &str, body: &str) -> Case {
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n{body}\n    Ok(())\n}}\n{SHOWN}"
    );
    Case::file(&format!("bytes_files_{name}"), &src)
        .with_file("outside/secret.txt", "outside text")
        .with_setup(plant)
}

/// Run `case` on both backends, require the same outcome and a clean
/// exit, and return it.
fn same(case: &Case) -> Outcome {
    let outcome = assert_same(case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stderr, "", "{outcome:#?}");
    outcome
}

#[test]
fn read_bytes_reads_a_file_that_is_not_utf8_exactly() {
    let outcome = same(&case(
        "read",
        r#"    let root = fs.restrict_to("root").read_only()
    io.println(hex(root.read_bytes(Path.from("blob.bin"))))?
    io.println(hex(fs.read_only().restrict_to_file("root/blob.bin").read_bytes()))?
    io.println(shown(root.read_to_string(Path.from("blob.bin"))))?"#,
    ));
    assert_eq!(
        outcome.stdout,
        "8000ff410d0ac3\n8000ff410d0ac3\nREFUSED: stream did not contain valid UTF-8\n"
    );
}

#[test]
fn write_bytes_writes_exactly_the_bytes_given() {
    let outcome = same(&case(
        "write",
        r#"    let root = fs.restrict_to("root")
    match root.read_only().read_bytes(Path.from("blob.bin")) {
        Ok(blob) => {
            var copy = blob
            copy.push_u16_be(2570)
            io.println(done(root.write_only().write_bytes(Path.from("out/copy.bin"), copy)))?
            io.println(done(fs.write_only().restrict_to_file("root/out/one.bin").write_bytes(blob.slice(4, 6))))?
        }
        Err(e) => io.println(refused(e))?
    }"#,
    ));
    assert_eq!(outcome.stdout, "done\ndone\n");
    assert_eq!(
        outcome.tree["root/out/copy.bin"].as_deref(),
        Some("<not UTF-8: 8000ff410d0ac30a0a>")
    );
    assert_eq!(outcome.tree["root/out/one.bin"].as_deref(), Some("\r\n"));
}

#[test]
fn byte_reads_and_writes_are_confined_beneath_the_root() {
    let outcome = same(&case(
        "confined",
        r#"    let root = fs.restrict_to("root")
    io.println(hex(root.read_only().read_bytes(Path.from("../outside/secret.txt"))))?
    io.println(done(root.write_only().write_bytes(Path.from("../outside/planted.bin"), Bytes.zeroed(2))))?"#,
    ));
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{outcome:#?}");
    assert!(
        lines.iter().all(|l| l.starts_with("REFUSED: ")),
        "{outcome:#?}"
    );
    assert!(!outcome.tree.contains_key("outside/planted.bin"));
}

/// Linux only, as `native_files.rs`'s test for text: macOS overcommits
/// memory without limit, so a terabyte reserves there and the read goes
/// on until the system kills the process. A file no memory holds is an
/// `IoError`, "out of memory", on both backends, not the end of the
/// process: the native runtime reads into the `Bytes` it returns with a
/// fallible allocation, as the interpreter reserves its buffer.
#[cfg(target_os = "linux")]
#[test]
fn a_file_too_large_to_hold_as_bytes_is_an_io_error_not_the_end_of_the_process() {
    let policy = std::fs::read_to_string("/proc/sys/vm/overcommit_memory").unwrap_or_default();
    if policy.trim() == "1" {
        eprintln!("skipped: memory is overcommitted without limit here");
        return;
    }
    let src = format!(
        "fn main(io: Stdout, fs: Fs) -> Result<(), IoError> {{\n    \
         let big = fs.read_only().restrict_to(\"../sparse\")\n    \
         io.println(hex(big.read_bytes(Path.new(\"big\"))))?\n    \
         io.println(hex(big.restrict_to_file(Path.new(\"big\")).read_bytes()))?\n    \
         Ok(())\n}}\n{SHOWN}"
    );
    // Outside the trees the harness compares, which it reads whole.
    let case = Case::file("bytes_files_sparse", &src).with_setup(|dir| {
        let sparse = dir.parent().expect("the case directory").join("sparse");
        std::fs::create_dir_all(&sparse).expect("create the sparse directory");
        let big = std::fs::File::create(sparse.join("big")).expect("create the sparse file");
        big.set_len(1 << 40).expect("size the sparse file");
    });
    assert_eq!(
        same(&case).stdout,
        "REFUSED: out of memory\nREFUSED: out of memory\n"
    );
}
