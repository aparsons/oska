//! Bytes through the filesystem capabilities (#223): `read_bytes` on
//! `DirRead` and `FileRead` gives a file's bytes as they are, UTF-8 or
//! not, and `write_bytes` on `DirWrite` and `FileWrite` writes exactly the
//! bytes given, through the same confined opens as the text methods.
//! `.assume_trusted()` on bytes gives the bytes themselves. Each program
//! runs on [`FakeHost`], and natively on the same world where native
//! executables run.

mod common;

use common::{fs_program, run_differential, test_fake};
use oska_run::{FakeHost, TestStatus};

/// What the program running `body` with [`common::fs_program`]'s
/// capabilities prints on `host`, with the helpers of [`HEX`] and the
/// trusted `fn` `extra` beside it. The host is left as the program left
/// it.
fn printed_with(host: &mut FakeHost, body: &str, extra: &str) -> String {
    let src = format!("{}{HEX}{extra}", fs_program(body));
    let outcome = run_differential(host, &src, &[]);
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run: {outcome:#?}"
    );
    outcome.stdout
}

/// [`printed_with`] on a host the test does not look at afterwards.
fn printed(mut host: FakeHost, body: &str, extra: &str) -> String {
    printed_with(&mut host, body, extra)
}

/// Oska helpers that print what a byte read returned: its bytes in
/// hexadecimal, or `REFUSED: ` and its error's message.
const HEX: &str = r#"
fn hex(result: Result<Untrusted<Bytes>, IoError>) -> Untrusted<String> {
    match result {
        Ok(bytes) => bytes.to_hex()
        Err(e) => refused(e)
    }
}
"#;

/// The bytes of a file that is not UTF-8: a lone continuation byte, a
/// NUL, and an invalid lead byte.
const NOT_UTF8: &[u8] = &[0x80, 0x00, 0xff, 0x41, 0x0d, 0x0a];

#[test]
fn read_bytes_gives_a_file_that_is_not_utf8_as_it_is() {
    let host = FakeHost::new()
        .with_file_bytes("data/blob.bin", NOT_UTF8)
        .with_file_bytes("one.bin", NOT_UTF8);
    let out = printed(
        host,
        r#"    let data = read.restrict_to("data")
    io.println(hex(data.read_bytes(Path.from("blob.bin"))))?
    io.println(hex(read.restrict_to_file("one.bin").read_bytes()))?
    io.println(shown(data.read_to_string(Path.from("blob.bin"))))?"#,
        "",
    );
    assert_eq!(
        out,
        "8000ff410d0a\n8000ff410d0a\nREFUSED: stream did not contain valid UTF-8\n"
    );
}

#[test]
fn write_bytes_writes_exactly_the_bytes_given() {
    let mut host = FakeHost::new().with_dir("out");
    let out = printed_with(
        &mut host,
        r#"    let out = write.restrict_to("out")
    var b = Bytes.from_hex("8000ff410d0a").unwrap_or(Bytes.new())
    io.println(done(out.write_bytes(Path.from("blob.bin"), b)))?
    b.push(255)
    io.println(done(write.restrict_to_file("out/one.bin").write_bytes(b)))?"#,
        "",
    );
    assert_eq!(out, "done\ndone\n");
    assert_eq!(host.file_bytes("out/blob.bin"), Some(NOT_UTF8));
    assert_eq!(
        host.file_bytes("out/one.bin"),
        Some(&[0x80, 0x00, 0xff, 0x41, 0x0d, 0x0a, 0xff][..])
    );
}

#[test]
fn bytes_written_read_back_the_same() {
    let host = FakeHost::new().with_dir("out");
    let out = printed(
        host,
        r#"    let dir = fs.restrict_to("out")
    let raw = Bytes.from_hex("00fe0d0a80").unwrap_or(Bytes.new())
    io.println(done(dir.write_only().write_bytes(Path.from("x.bin"), raw)))?
    io.println(hex(dir.read_only().read_bytes(Path.from("x.bin"))))?"#,
        "",
    );
    assert_eq!(out, "done\n00fe0d0a80\n");
}

#[test]
fn read_bytes_is_refused_where_read_to_string_is() {
    let host = FakeHost::new()
        .with_file_bytes("secret.bin", NOT_UTF8)
        .with_dir("data")
        .with_symlink("data/link.bin", "secret.bin");
    let out = printed(
        host,
        r#"    let data = read.restrict_to("data")
    for raw in ["../secret.bin", "link.bin", "."] {
        io.println(hex(data.read_bytes(Path.from(raw))))?
        io.println(shown(data.read_to_string(Path.from(raw))))?
    }"#,
        "",
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 6, "{out}");
    for pair in lines.chunks(2) {
        assert!(pair[0].starts_with("REFUSED: "), "{out}");
        assert_eq!(pair[0], pair[1], "{out}");
    }
}

#[test]
fn write_bytes_is_confined_beneath_its_root() {
    let mut host = FakeHost::new().with_dir("out");
    let out = printed_with(
        &mut host,
        r#"    let out = write.restrict_to("out")
    io.println(done(out.write_bytes(Path.from("../escaped.bin"), Bytes.zeroed(1))))?"#,
        "",
    );
    assert!(out.starts_with("REFUSED: "), "{out}");
    assert_eq!(host.file_bytes("escaped.bin"), None);
}

#[test]
fn assume_trusted_gives_the_bytes_themselves() {
    let host = FakeHost::new().with_file_bytes("in.bin", NOT_UTF8);
    let out = printed(
        host,
        r#"    match read.restrict_to_file("in.bin").read_bytes() {
        Ok(raw) => io.println(trusted(raw.assume_trusted()))?
        Err(e) => io.println(refused(e))?
    }"#,
        "fn trusted(b: Bytes) -> String {\n    b.slice(1, 3).to_hex()\n}\n",
    );
    assert_eq!(out, "00ff\n");
}

#[test]
fn a_test_writes_and_reads_back_bytes_that_are_not_utf8() {
    let src = "test \"bytes\" {\n    \
               let path = Path.new(\"blob.bin\")\n    \
               let raw = Bytes.from_hex(\"8000ff0d0a\").unwrap_or(Bytes.new())\n    \
               t.assert(t.fs.restrict_to(\"/\").write_only().write_bytes(path, raw).is_ok())\n    \
               let read = t.fs.restrict_to(\"/\").read_only()\n    \
               match read.read_bytes(path) {\n        \
               Ok(back) => t.assert_eq(back.to_hex(), \"8000ff0d0a\")\n        \
               Err(_) => t.fail(\"read failed\")\n    }\n    \
               match read.read_to_string(path) {\n        \
               Ok(_) => t.fail(\"read as text\")\n        \
               Err(e) => t.assert_eq(e.message, \"stream did not contain valid UTF-8\")\n    }\n}\n";
    let report = test_fake(src);
    assert!(
        !report.tests.is_empty() && report.tests.iter().all(|t| t.status == TestStatus::Passed),
        "{src}\n{report:#?}"
    );
}
