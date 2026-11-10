//! The contract step of ADR 0008 (#99): `Fs`, `FsRead` and `FsWrite` only
//! attenuate, so a path-taking call on one gets `E0273`, and `main` cannot
//! declare a directory or file capability (`E0217`).

mod common;

use common::{assert_authority, assert_errors, assert_ok};

// ---- Path-taking calls on Fs, FsRead and FsWrite --------------------------------------

#[test]
fn reading_through_fsread_is_refused_with_the_attenuation_fix() {
    assert_errors(
        "fn f(fs: FsRead, path: Path) -> Result<Untrusted<String>, IoError> {\n    \
         fs.read_to_string(path)\n}",
        &[("E0273", "fs.read_to_string(path)")],
    );
}

#[test]
fn every_removed_method_is_refused_on_every_type() {
    for (ty, call) in [
        ("FsRead", "fs.read_to_string(p)"),
        ("FsRead", "fs.list_dir(p)"),
        ("FsWrite", "fs.write(p, \"x\")"),
        ("FsWrite", "fs.create_dir(p)"),
        ("Fs", "fs.read_to_string(p)"),
        ("Fs", "fs.list_dir(p)"),
        ("Fs", "fs.write(p, \"x\")"),
        ("Fs", "fs.create_dir(p)"),
    ] {
        let src = format!("fn f(fs: {ty}, p: Path) {{\n    let _ = {call}\n}}");
        assert_errors(&src, &[("E0273", call)]);
    }
}

#[test]
fn the_message_explains_attenuation_and_shows_the_fix_shape() {
    let report =
        common::check("fn f(fs: FsRead, path: Path) {\n    let _ = fs.read_to_string(path)\n}");
    let message = &report.diagnostics[0].message;
    assert!(message.contains("`FsRead`"), "{message}");
    assert!(message.contains("read_to_string"), "{message}");
    assert!(
        message.contains("fs.restrict_to(\"<root>\").read_to_string(path)"),
        "{message}"
    );
    assert!(
        report.diagnostics[0].fix.is_none(),
        "the root is the programmer's to choose"
    );
}

#[test]
fn the_fix_shape_for_a_write_attenuates_to_a_write_directory() {
    let report = common::check("fn f(out: FsWrite, p: Path) {\n    let _ = out.create_dir(p)\n}");
    let message = &report.diagnostics[0].message;
    assert!(
        message.contains("out.restrict_to(\"<root>\").create_dir(path)"),
        "{message}"
    );
}

#[test]
fn the_arguments_of_a_refused_call_are_still_checked() {
    assert_errors(
        "fn f(fs: FsRead) {\n    let _ = fs.read_to_string(missing)\n}",
        &[
            ("E0273", "fs.read_to_string(missing)"),
            ("E0221", "missing"),
        ],
    );
}

#[test]
fn attenuating_first_is_accepted() {
    assert_ok(
        "fn f(fs: FsRead, path: Path) -> Result<Untrusted<String>, IoError> {\n    \
         fs.restrict_to(\"/srv\").read_to_string(path)\n}",
    );
}

#[test]
fn other_unknown_methods_keep_their_ordinary_error() {
    assert_errors(
        "fn f(fs: FsRead) {\n    let _ = fs.delete(Path.new(\"a\"))\n}",
        &[("E0228", "delete")],
    );
}

#[test]
fn a_user_type_with_these_method_names_is_not_affected() {
    assert_ok(
        "struct Store { n: Int }\n\
         impl Store {\n    pub fn write(self, p: Path) -> Int { self.n }\n}\n\
         fn f(s: Store) -> Int { s.write(Path.new(\"a\")) }",
    );
}

// ---- main cannot declare a confined capability ---------------------------------------

#[test]
fn main_cannot_declare_a_directory_or_file_capability() {
    for ty in ["Dir", "DirRead", "DirWrite", "FileRead", "FileWrite"] {
        let src = format!("fn main(c: {ty}) {{}}");
        assert_errors(&src, &[("E0217", ty)]);
    }
}

#[test]
fn main_may_still_declare_the_fs_family() {
    assert_ok("fn main(a: Fs, b: FsRead, c: FsWrite) {}");
}

#[test]
fn a_confined_capability_is_fine_as_any_other_parameter() {
    assert_authority(
        "fn f(c: DirRead, g: FileWrite) {}",
        "f",
        &["DirRead", "FileWrite"],
        false,
    );
}
