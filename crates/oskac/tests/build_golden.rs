//! Seam 2 of the native backend (spec #125, ticket #128): `oskac build`
//! on every host, with golden output.
//!
//! Each case writes its files into a fresh directory, runs the real
//! binary there, and compares the exit code, stdout, stderr and the files
//! the build left with `tests/golden/build/<case>.txt`. To regenerate the
//! golden files after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test build_golden` and review
//! the diff. What the executables do is the differential harness's
//! business (`native_tracer.rs`); here only their headers are read.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")\n}\n";

const MANIFEST: &str = "[package]\nname = \"greeter\"\nversion = \"0.1.0\"\n";

/// A new, empty directory for `case`, holding `files`.
fn workspace(case: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build_golden")
        .join(case);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir
}

/// Every file under `dir`, by relative path.
fn files_in(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("should list the directory") {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files_in(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name);
        }
    }
}

/// Run `oskac args` in `cwd` (relative to the case's directory) and
/// compare with the golden file `case`.
fn assert_golden_in(case: &str, files: &[(&str, &str)], cwd: &str, args: &[&str]) -> PathBuf {
    let dir = workspace(case, files);
    let mut before = BTreeSet::new();
    files_in(&dir, "", &mut before);
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir.join(cwd))
        .output()
        .expect("oskac should run");
    let mut after = BTreeSet::new();
    files_in(&dir, "", &mut after);
    let written: Vec<&String> = after.difference(&before).collect();
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- written\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        written.iter().map(|w| format!("{w}\n")).collect::<String>(),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/build")
        .join(format!("{case}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
    dir
}

fn assert_golden(case: &str, files: &[(&str, &str)], args: &[&str]) -> PathBuf {
    assert_golden_in(case, files, "", args)
}

fn package(main: &str) -> Vec<(&'static str, String)> {
    vec![
        ("greeter/oska.toml", MANIFEST.to_string()),
        ("greeter/src/main.osk", main.to_string()),
    ]
}

fn borrowed<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files.iter().map(|(p, t)| (*p, t.as_str())).collect()
}

// ---- Output paths --------------------------------------------------------------

#[test]
fn a_file_builds_to_its_stem_in_the_current_directory() {
    assert_golden(
        "file_default_output",
        &[("src/hello.osk", HELLO)],
        &["build", "src/hello.osk"],
    );
}

#[test]
fn dash_o_names_the_output() {
    assert_golden(
        "file_dash_o",
        &[("hello.osk", HELLO), ("bin/.keep", "")],
        &["build", "hello.osk", "-o", "bin/greet"],
    );
}

#[test]
fn json_reports_the_output() {
    assert_golden(
        "file_json",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "--format=json"],
    );
}

#[test]
fn a_package_builds_to_its_directory_name() {
    let files = [
        ("pkgs/greeter/oska.toml", MANIFEST),
        ("pkgs/greeter/src/main.osk", HELLO),
    ];
    assert_golden("package_by_path", &files, &["build", "pkgs/greeter"]);
}

#[test]
fn a_package_named_from_its_parent_cannot_overwrite_its_own_directory() {
    let files = package(HELLO);
    assert_golden(
        "package_output_is_its_directory",
        &borrowed(&files),
        &["build", "greeter"],
    );
}

#[test]
fn no_path_builds_the_package_in_the_current_directory() {
    let files = package(HELLO);
    assert_golden_in("package_no_path", &borrowed(&files), "greeter", &["build"]);
}

#[test]
fn a_file_inside_a_package_is_built_alone_with_a_hint() {
    let files = package(HELLO);
    assert_golden(
        "package_file",
        &borrowed(&files),
        &["build", "greeter/src/main.osk", "-o", "alone"],
    );
}

#[test]
fn a_library_is_refused() {
    assert_golden(
        "library",
        &[
            ("lib/oska.toml", MANIFEST),
            ("lib/src/util.osk", "pub fn one() -> Int {\n    1\n}\n"),
        ],
        &["build", "lib"],
    );
}

// ---- Refusals ------------------------------------------------------------------

const MISTYPED: &str =
    "fn main(io: Stdout) -> Result<(), IoError> {\n    io.printline(\"hi\")\n}\n";

#[test]
fn checker_diagnostics_refuse_the_build() {
    assert_golden(
        "checker_refusal",
        &[("bad.osk", MISTYPED)],
        &["build", "bad.osk"],
    );
    assert_golden(
        "checker_refusal_json",
        &[("bad.osk", MISTYPED)],
        &["build", "bad.osk", "--format=json"],
    );
}

/// An integer literal outside its type is the checker's error, so the
/// build refuses it exactly as `oskac run` does: no backend ever sees one.
#[test]
fn an_out_of_range_literal_refuses_the_build_as_it_refuses_a_run() {
    let src = "fn main(io: Stdout) {\n    let small: U8 = 256\n    let neg: U16 = -1\n    \
               let big: I8 = -129\n}\n";
    assert_golden(
        "literal_out_of_range",
        &[("range.osk", src)],
        &["build", "range.osk"],
    );
    assert_golden(
        "literal_out_of_range_run",
        &[("range.osk", src)],
        &["run", "range.osk"],
    );
}

#[test]
fn checker_diagnostics_in_a_package_refuse_the_build() {
    let files = package(MISTYPED);
    assert_golden(
        "package_checker_refusal",
        &borrowed(&files),
        &["build", "greeter"],
    );
}

#[test]
fn naming_a_std_rt_intrinsic_is_refused_by_the_checker() {
    // Written here, not as a fixture file: the intrinsic guard refuses any
    // `.osk` file outside std.rt that names one.
    let intrinsic = ["rt", "sys", "exit_group"].join("_");
    let src = format!("fn main() {{\n    let _ = {intrinsic}(3)\n}}\n");
    assert_golden(
        "intrinsic_refusal",
        &[("raw.osk", &src)],
        &["build", "raw.osk"],
    );
}

#[test]
fn a_module_with_no_runtime_is_refused_as_the_interpreter_refuses_it() {
    assert_golden(
        "no_runtime_module",
        &[("sql.osk", "use std.sql\n\nfn main() {}\n")],
        &["build", "sql.osk"],
    );
}

/// A program with a construct the backend cannot compile yet: a `scope`.
const UNSUPPORTED: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
                           let n = scope |s| { 3 }\n    io.println(\"three\")\n}\n";

#[test]
fn a_construct_the_backend_cannot_compile_yet_is_refused() {
    assert_golden(
        "unsupported",
        &[("sum.osk", UNSUPPORTED)],
        &["build", "sum.osk"],
    );
    assert_golden(
        "unsupported_json",
        &[("sum.osk", UNSUPPORTED)],
        &["build", "sum.osk", "--format=json"],
    );
}

#[test]
fn an_unsupported_construct_in_a_package_module_is_located_in_its_file() {
    let files = [
        ("greeter/oska.toml", MANIFEST),
        (
            "greeter/src/main.osk",
            "use pkg.sum\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    sum.check(io)\n}\n",
        ),
        (
            "greeter/src/sum.osk",
            "pub fn check(io: Stdout) -> Result<(), IoError> {\n    \
             let n = scope |s| { 3 }\n    io.println(\"three\")\n}\n",
        ),
    ];
    assert_golden("package_unsupported", &files, &["build", "greeter"]);
}

#[test]
fn an_unwritable_output_is_refused() {
    assert_golden(
        "unwritable",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "-o", "missing/dir/hello"],
    );
    assert_golden(
        "unwritable_json",
        &[("hello.osk", HELLO)],
        &[
            "build",
            "hello.osk",
            "-o",
            "missing/dir/hello",
            "--format=json",
        ],
    );
}

// ---- The executable ------------------------------------------------------------

/// A field of `bytes`, little-endian, at `at`.
fn field(bytes: &[u8], at: usize, size: usize) -> u64 {
    bytes[at..at + size]
        .iter()
        .rev()
        .fold(0, |acc, &b| acc << 8 | u64::from(b))
}

#[test]
fn the_executable_is_a_static_position_independent_elf_with_no_executable_data() {
    let dir = assert_golden(
        "file_dash_o_elf",
        &[("hello.osk", HELLO)],
        &["build", "hello.osk", "-o", "hello"],
    );
    let elf = std::fs::read(dir.join("hello")).expect("the executable should exist");
    assert_eq!(&elf[..4], b"\x7fELF");
    assert_eq!(elf[4], 2, "64-bit");
    assert_eq!(elf[5], 1, "little-endian");
    assert_eq!(field(&elf, 16, 2), 3, "ET_DYN: position-independent");
    assert_eq!(field(&elf, 18, 2), 183, "EM_AARCH64");
    assert_eq!(
        field(&elf, 40, 8),
        0,
        "no section headers, so no relocation sections"
    );
    let phoff = field(&elf, 32, 8) as usize;
    let count = field(&elf, 56, 2) as usize;
    let mut segments = Vec::new();
    for i in 0..count {
        let header = phoff + 56 * i;
        segments.push((field(&elf, header, 4), field(&elf, header + 4, 4)));
    }
    const LOAD: u64 = 1;
    const GNU_STACK: u64 = 0x6474_e551;
    // Flags: 4 read, 2 write, 1 execute.
    assert_eq!(
        segments,
        [(LOAD, 4), (LOAD, 5), (LOAD, 6), (GNU_STACK, 6)],
        "rodata R, text R+X, data RW, a non-executable stack, and nothing \
         else: no PT_INTERP (3) and no PT_DYNAMIC (2)"
    );
    let entry = field(&elf, 24, 8);
    let text = phoff + 56;
    let (text_start, text_size) = (field(&elf, text + 16, 8), field(&elf, text + 40, 8));
    assert!(
        (text_start..text_start + text_size).contains(&entry),
        "the entry point is in the code"
    );
}

#[test]
fn the_same_program_builds_to_the_same_bytes_wherever_it_is_built_from() {
    let dir = workspace(
        "reproducible",
        &[("a/hello.osk", HELLO), ("b/c/hello.osk", HELLO)],
    );
    // The path a program is named by is part of it (a panic reports it),
    // so both builds name it alike, from different directories.
    for cwd in ["a", "b/c"] {
        let status = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "hello.osk", "-o", "out"])
            .current_dir(dir.join(cwd))
            .status()
            .expect("oskac should run");
        assert!(status.success());
    }
    let a = std::fs::read(dir.join("a/out")).expect("the first build");
    let b = std::fs::read(dir.join("b/c/out")).expect("the second build");
    assert!(a == b, "two builds of one program differ");
}
