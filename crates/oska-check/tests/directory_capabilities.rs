//! Directory capabilities (#96, ADR 0008): attenuating `Fs`, `FsRead` or
//! `FsWrite` to a directory gives a `Dir`, `DirRead` or `DirWrite`, which
//! reads and writes only beneath that root and can narrow further with a
//! `Path` built from untrusted text.

mod common;

use common::{assert_authority, assert_errors, assert_ok};

// ---- Attenuating the filesystem capabilities -----------------------------------------

#[test]
fn restricting_each_filesystem_capability_gives_its_directory_capability() {
    assert_ok(
        "cap struct Roots { app: Dir, docs: DirRead, site: DirWrite }\n\
         fn f(fs: Fs, read: FsRead, write: FsWrite) -> Roots {\n    \
         Roots(app: fs.restrict_to(\"/srv/app\"), docs: read.restrict_to(\"docs\"), \
         site: write.restrict_to(\"_site\"))\n}",
    );
}

#[test]
fn an_untrusted_root_is_rejected_on_every_filesystem_capability() {
    for (cap, result) in [
        ("Fs", "Dir"),
        ("FsRead", "DirRead"),
        ("FsWrite", "DirWrite"),
    ] {
        assert_errors(
            &format!(
                "fn f(fs: {cap}, dir: Untrusted<String>) -> {result} {{\n    fs.restrict_to(dir)\n}}"
            ),
            &[("E0300", "dir")],
        );
    }
}

#[test]
fn a_dir_gives_a_read_only_and_a_write_only_directory_capability() {
    assert_ok(
        "fn reader(app: Dir) -> DirRead { app.read_only() }\n\
         fn writer(app: Dir) -> DirWrite { app.write_only() }",
    );
}

// ---- What each directory capability can do -------------------------------------------

#[test]
fn a_dir_read_reads_files_and_lists_directories() {
    assert_ok(
        "fn f(docs: DirRead) -> Result<Int, IoError> {\n    \
         let text = docs.read_to_string(Path.new(\"intro.md\"))?\n    \
         let names = docs.list_dir(Path.new(\".\"))?\n    \
         Ok(text.lines().count() + names.len())\n}",
    );
}

#[test]
fn a_dir_write_writes_files_and_creates_directories() {
    assert_ok(
        "fn f(site: DirWrite, page: Untrusted<String>) -> Result<(), IoError> {\n    \
         site.create_dir(Path.new(\"adr\"))?\n    \
         site.write(Path.new(\"adr/index.html\"), page)\n}",
    );
}

#[test]
fn a_dir_read_cannot_write_and_a_dir_write_cannot_read() {
    assert_errors(
        "fn f(docs: DirRead) -> Result<(), IoError> {\n    docs.write(Path.new(\"x\"), \"y\")\n}",
        &[("E0228", "write")],
    );
    assert_errors(
        "fn f(site: DirWrite) -> Result<Untrusted<String>, IoError> {\n    \
         site.read_to_string(Path.new(\"x\"))\n}",
        &[("E0228", "read_to_string")],
    );
}

#[test]
fn a_dir_reads_and_writes_only_through_read_only_and_write_only() {
    assert_errors(
        "fn f(app: Dir) -> Result<Untrusted<String>, IoError> {\n    \
         app.read_to_string(Path.new(\"x\"))\n}",
        &[("E0228", "read_to_string")],
    );
}

// ---- Narrowing -----------------------------------------------------------------------

#[test]
fn each_directory_capability_narrows_to_its_own_type() {
    assert_ok(
        "fn a(app: Dir) -> Dir { app.restrict_to(Path.new(\"uploads\")) }\n\
         fn b(docs: DirRead) -> DirRead { docs.restrict_to(Path.new(\"adr\")) }\n\
         fn c(site: DirWrite) -> DirWrite { site.restrict_to(Path.new(\"adr\")) }",
    );
}

#[test]
fn a_narrowing_from_an_untrusted_path_type_checks() {
    assert_ok(
        "fn f(uploads: DirRead, user: Untrusted<String>) -> Result<Untrusted<String>, IoError> {\n    \
         uploads.restrict_to(Path.from(user)).read_to_string(Path.new(\"avatar.png\"))\n}",
    );
}

#[test]
fn a_narrowing_takes_a_path_not_text() {
    assert_errors(
        "fn f(docs: DirRead) -> DirRead {\n    docs.restrict_to(\"adr\")\n}",
        &[("E0220", "\"adr\"")],
    );
}

// ---- Authority -----------------------------------------------------------------------

#[test]
fn a_directory_capability_parameter_is_the_functions_authority() {
    assert_authority(
        "fn build(docs: DirRead, site: DirWrite) -> Int { 0 }",
        "build",
        &["DirRead", "DirWrite"],
        false,
    );
}
