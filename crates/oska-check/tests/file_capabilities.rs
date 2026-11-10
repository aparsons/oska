//! File capabilities (#97, ADR 0008): `restrict_to_file` on `FsRead` and
//! `FsWrite` gives a `FileRead` or `FileWrite` for one file, and a
//! directory capability hands out one for a file beneath it, from a
//! `Path` that may be untrusted.

mod common;

use common::{assert_authority, assert_errors, assert_ok};

// ---- Attenuating to one file ---------------------------------------------------------

#[test]
fn restricting_to_a_file_gives_a_file_capability() {
    assert_ok(
        "cap struct Files { readme: FileRead, out: FileWrite }\n\
         fn f(read: FsRead, write: FsWrite) -> Files {\n    \
         Files(readme: read.restrict_to_file(\"README.md\"), \
         out: write.restrict_to_file(\"out.txt\"))\n}",
    );
}

#[test]
fn an_untrusted_file_root_is_rejected() {
    assert_errors(
        "fn f(read: FsRead, name: Untrusted<String>) -> FileRead {\n    \
         read.restrict_to_file(name)\n}",
        &[("E0300", "name")],
    );
    assert_errors(
        "fn f(write: FsWrite, name: Untrusted<String>) -> FileWrite {\n    \
         write.restrict_to_file(name)\n}",
        &[("E0300", "name")],
    );
}

#[test]
fn a_file_root_is_text_not_a_path() {
    assert_errors(
        "fn f(read: FsRead) -> FileRead {\n    read.restrict_to_file(Path.new(\"a\"))\n}",
        &[("E0220", "Path.new(\"a\")")],
    );
}

#[test]
fn the_whole_filesystem_has_no_file_capability_to_grant() {
    assert_errors(
        "fn f(fs: Fs) -> FileRead {\n    fs.restrict_to_file(\"a\")\n}",
        &[("E0228", "restrict_to_file")],
    );
}

// ---- Handing out a file beneath a directory ------------------------------------------

#[test]
fn a_directory_capability_hands_out_a_file_capability() {
    assert_ok(
        "fn a(docs: DirRead) -> FileRead { docs.restrict_to_file(Path.new(\"intro.md\")) }\n\
         fn b(site: DirWrite) -> FileWrite { site.restrict_to_file(Path.new(\"index.html\")) }",
    );
}

#[test]
fn a_file_beneath_a_directory_may_be_named_by_untrusted_text() {
    assert_ok(
        "fn f(uploads: DirRead, user: Untrusted<String>) -> Result<Untrusted<String>, IoError> {\n    \
         uploads.restrict_to_file(Path.from(user)).read_to_string()\n}",
    );
}

#[test]
fn a_dir_has_no_file_capability_until_it_is_read_or_write_only() {
    assert_errors(
        "fn f(app: Dir) -> FileRead {\n    app.restrict_to_file(Path.new(\"a\"))\n}",
        &[("E0228", "restrict_to_file")],
    );
    assert_ok("fn f(app: Dir) -> FileRead { app.read_only().restrict_to_file(Path.new(\"a\")) }");
}

#[test]
fn a_directory_capability_gives_a_file_capability_of_its_own_kind() {
    assert_errors(
        "fn f(docs: DirRead) -> FileWrite {\n    docs.restrict_to_file(Path.new(\"a\"))\n}",
        &[("E0220", "docs.restrict_to_file(Path.new(\"a\"))")],
    );
}

// ---- What each file capability can do ------------------------------------------------

#[test]
fn a_file_read_reads_with_no_path() {
    assert_ok(
        "fn f(readme: FileRead) -> Result<Int, IoError> {\n    \
         let text = readme.read_to_string()?\n    Ok(text.lines().count())\n}",
    );
}

#[test]
fn a_file_write_writes_with_no_path() {
    assert_ok(
        "fn f(out: FileWrite, text: Untrusted<String>) -> Result<(), IoError> {\n    \
         out.write(text)\n}",
    );
}

#[test]
fn a_file_read_cannot_write_and_a_file_write_cannot_read() {
    assert_errors(
        "fn f(readme: FileRead) -> Result<(), IoError> {\n    readme.write(\"x\")\n}",
        &[("E0228", "write")],
    );
    assert_errors(
        "fn f(out: FileWrite) -> Result<Untrusted<String>, IoError> {\n    out.read_to_string()\n}",
        &[("E0228", "read_to_string")],
    );
}

#[test]
fn a_file_capability_takes_no_path() {
    assert_errors(
        "fn f(readme: FileRead) -> Result<Untrusted<String>, IoError> {\n    \
         readme.read_to_string(Path.new(\"a\"))\n}",
        &[("E0224", "readme.read_to_string(Path.new(\"a\"))")],
    );
}

#[test]
fn a_file_capability_cannot_be_narrowed_further() {
    assert_errors(
        "fn f(readme: FileRead) -> FileRead {\n    readme.restrict_to_file(Path.new(\"a\"))\n}",
        &[("E0228", "restrict_to_file")],
    );
}

// ---- Authority -----------------------------------------------------------------------

#[test]
fn a_file_capability_parameter_is_the_functions_authority() {
    assert_authority(
        "fn build(readme: FileRead, out: FileWrite) -> Int { 0 }",
        "build",
        &["FileRead", "FileWrite"],
        false,
    );
}
