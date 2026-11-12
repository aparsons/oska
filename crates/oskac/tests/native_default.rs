//! Native by default (spec #125, ticket #141): on aarch64 Linux,
//! `oskac run` and `oskac test` compile the program natively into a
//! private temporary directory and run it there, and `--interpret`
//! selects the interpreter. Every other host keeps the interpreter.
//!
//! The golden cases (`tests/golden/native_default/<case>.txt`) are the
//! same on every host: on aarch64 Linux they show the native run, and
//! elsewhere the interpreter's fallback, which must print exactly the
//! same. To regenerate them after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test native_default` and review
//! the diff. The aarch64-only tests below show that the default really is
//! native, through what only the native backend can do in a debug build.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Whether this host runs programs natively by default.
const NATIVE_HOST: bool = cfg!(all(target_arch = "aarch64", target_os = "linux"));

const MANIFEST: &str = "[package]\nname = \"greeter\"\nversion = \"0.1.0\"\n";

/// A program that prints its arguments, each on its own line, after the
/// path it was run as.
const ARGS: &str = "fn main(io: Stdout, args: Args) -> Result<(), IoError> {\n    \
                    var i = 0\n    while true {\n        \
                    match args.get(i) {\n            \
                    Some(arg) => {\n                \
                    let text = arg.assume_trusted()\n                \
                    io.println(\"[{text}]\")?\n            }\n            \
                    None => {\n                break\n            }\n        }\n        i = i + 1\n    }\n    Ok(())\n}\n";

/// A program that panics after printing a line to each stream.
const PANICS: &str = "fn main(io: Stdout, err: Stderr) -> Result<(), IoError> {\n    \
                      io.println(\"before\")?\n    err.println(\"warned\")?\n    \
                      let zero = 0\n    let x = 1 / zero\n    io.println(\"after\")\n}\n";

/// A program the native backend refuses (`E0390`): `dbg` of a directory
/// capability, whose interpreter text holds an OS handle.
const UNSUPPORTED: &str = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {\n    \
                           let d = dbg(fs.restrict_to(\"/\"))\n    io.println(\"ran\")\n}\n";

/// A program with tests: one passes, one fails.
const TESTED: &str = "fn double(n: Int) -> Int {\n    n * 2\n}\n\n\
                      test \"doubles\" {\n    t.assert_eq(double(2), 4)\n}\n\n\
                      test \"wrongly\" {\n    t.assert_eq(double(2), 5)\n}\n";

/// A new, empty directory for `case`, holding `files`.
fn workspace(case: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("native_default")
        .join(case);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tmp")).expect("should create the case directory");
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a directory");
        std::fs::write(&file, text).expect("should write the file");
    }
    dir
}

/// Every entry under `dir`, by relative path, directories with a `/`.
fn entries(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("should list the directory") {
        let entry = entry.expect("a directory entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            out.insert(format!("{name}/"));
            entries(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name);
        }
    }
}

/// Run `oskac args` in the case's directory, with its own `tmp/` as the
/// temporary directory.
fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env("TMPDIR", dir.join("tmp"))
        .output()
        .expect("oskac should run")
}

/// Run `oskac args` among `files` and compare with the golden file
/// `case`: exit code, stdout, stderr, and every entry the run left behind,
/// in the temporary directory too, which a run must leave empty.
fn assert_golden(case: &str, files: &[(&str, &str)], args: &[&str]) {
    let dir = workspace(case, files);
    let mut before = BTreeSet::new();
    entries(&dir, "", &mut before);
    let output = oskac(&dir, args);
    let mut after = BTreeSet::new();
    entries(&dir, "", &mut after);
    let written: Vec<&String> = after.symmetric_difference(&before).collect();
    let actual = format!(
        "$ oskac {}\nexit: {}\n--- stdout\n{}--- stderr\n{}--- written\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string()),
        String::from_utf8_lossy(&output.stdout),
        without_handles(&String::from_utf8_lossy(&output.stderr)),
        written.iter().map(|w| format!("{w}\n")).collect::<String>(),
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/native_default")
        .join(format!("{case}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("a golden directory");
        std::fs::write(&golden, &actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("{} should exist: {e}\n{actual}", golden.display()));
    // The one difference between hosts: only a host with a native backend
    // can refuse a program natively, and notes that it runs it on the
    // interpreter instead. (Regenerate the goldens on aarch64 Linux.)
    let expected = if NATIVE_HOST {
        expected
    } else {
        expected
            .lines()
            .filter(|line| !line.contains("running on the interpreter: "))
            .map(|line| format!("{line}\n"))
            .collect()
    };
    assert!(
        actual == expected,
        "{} differs; rerun with UPDATE_GOLDEN=1 if the change is intended\n--- actual\n{actual}\n--- expected\n{expected}",
        golden.display()
    );
}

/// `text` with what each `dbg` of a directory capability shows of its
/// root handle written as `..`: the path as the host resolves it (Windows
/// canonicalizes `/` to a drive) and the OS handle, whose number depends
/// on what `oskac` inherited, and which Windows does not keep.
fn without_handles(text: &str) -> String {
    const HANDLE: &str = "RootHandle {";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(HANDLE) {
        out.push_str(&rest[..at + HANDLE.len()]);
        let after = &rest[at + HANDLE.len()..];
        // The `}` that closes this one.
        let mut depth = 1;
        let end = after
            .char_indices()
            .find(|&(_, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map_or(after.len(), |(i, _)| i);
        out.push_str(" .. ");
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

// ---- Goldens: the same on every host ------------------------------------------

#[test]
fn arguments_after_a_double_dash_reach_the_program_unchanged() {
    let args = [
        "run",
        "args.osk",
        "--",
        "one",
        "--format=json",
        "--",
        "-o",
        "",
        "two words",
    ];
    assert_golden("run_args", &[("args.osk", ARGS)], &args);
    let mut interpreted = args.to_vec();
    interpreted.insert(2, "--interpret");
    assert_golden("run_args_interpret", &[("args.osk", ARGS)], &interpreted);
}

#[test]
fn a_package_is_run_as_its_directory() {
    let files = [
        ("greeter/oska.toml", MANIFEST),
        ("greeter/src/main.osk", ARGS),
    ];
    assert_golden("run_package", &files, &["run", "greeter", "--", "x"]);
}

#[test]
fn a_panic_is_reported_as_the_interpreter_reports_it() {
    let files = [("panics.osk", PANICS)];
    assert_golden("run_panic", &files, &["run", "panics.osk"]);
    assert_golden(
        "run_panic_json",
        &files,
        &["run", "panics.osk", "--format=json"],
    );
    assert_golden(
        "run_panic_interpret",
        &files,
        &["run", "--interpret", "panics.osk"],
    );
}

#[test]
fn a_program_the_native_backend_refuses_runs_on_the_interpreter() {
    let files = [("unsupported.osk", UNSUPPORTED)];
    assert_golden("run_unsupported", &files, &["run", "unsupported.osk"]);
    assert_golden(
        "run_unsupported_json",
        &files,
        &["run", "unsupported.osk", "--format=json"],
    );
    assert_golden(
        "run_unsupported_interpret",
        &files,
        &["run", "unsupported.osk", "--interpret"],
    );
}

#[test]
fn a_package_the_native_backend_refuses_runs_on_the_interpreter() {
    let files = [
        ("app/oska.toml", MANIFEST),
        ("app/src/main.osk", UNSUPPORTED),
    ];
    assert_golden("run_unsupported_package", &files, &["run", "app"]);
}

#[test]
fn a_module_with_no_runtime_is_refused_as_the_interpreter_refuses_it() {
    let files = [("sql.osk", "use std.sql\n\nfn main() {}\n")];
    assert_golden("run_no_runtime", &files, &["run", "sql.osk"]);
    // Refused natively (`E0390`) and by the interpreter (`E0370`): only
    // the interpreter's refusal, with no note that it runs.
    let net = [("net.osk", "fn main(net: Net) {}\n")];
    assert_golden("run_ungranted", &net, &["run", "net.osk"]);
    assert_golden("test_no_runtime", &files, &["test", "sql.osk"]);
}

#[test]
fn tests_report_the_same_on_either_backend() {
    let files = [("tested.osk", TESTED)];
    assert_golden("test", &files, &["test", "tested.osk"]);
    assert_golden(
        "test_json",
        &files,
        &["test", "tested.osk", "--format=json"],
    );
    assert_golden(
        "test_interpret",
        &files,
        &["test", "tested.osk", "--interpret"],
    );
}

#[test]
fn tests_the_native_backend_refuses_run_on_the_interpreter() {
    let src = "test \"refused\" {\n    let d = dbg(t.fs.restrict_to(\"/\"))\n    \
               t.assert(true)\n}\n";
    assert_golden(
        "test_unsupported",
        &[("unsupported.osk", src)],
        &["test", "unsupported.osk"],
    );
}

// ---- aarch64 Linux: the default is native --------------------------------------

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
#[test]
fn run_and_test_report_a_missing_temporary_directory_alike() {
    let src = format!(
        "fn main(io: Stdout) -> Result<(), IoError> {{\n    io.println(\"hi\")\n}}\n\n{TESTED}"
    );
    let dir = workspace("no_tmpdir", &[("tested.osk", &src)]);
    let missing = dir.join("missing");
    let expected = format!(
        "error[E0391]: cannot write the native executable in the temporary directory \
         `{}`: No such file or directory (os error 2); set `TMPDIR` to a directory you can \
         run programs from, or use `--interpret`\n",
        missing.display()
    );
    for command in ["run", "test"] {
        let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args([command, "tested.osk"])
            .current_dir(&dir)
            .env("TMPDIR", &missing)
            .output()
            .expect("oskac should run");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            expected,
            "{command}"
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "", "{command}");
        assert_eq!(output.status.code(), Some(1), "{command}");
    }
}

/// A recursion 40,000 calls deep: within the language's limit (50,000),
/// which native code reaches, but beyond what a debug build of the
/// interpreter reaches on its own stack before it panics with `P0021`.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const DEEP: &str = "fn down(n: Int) -> Int {\n    if n == 0 {\n        return 0\n    }\n    \
                    down(n - 1) + 1\n}\n\n\
                    fn main(io: Stdout) -> Result<(), IoError> {\n    \
                    let d = down(40000)\n    io.println(\"{d}\")\n}\n\n\
                    test \"deep\" {\n    t.assert_eq(down(40000), 40000)\n}\n";

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
#[test]
fn run_and_test_are_native_by_default_and_interpret_selects_the_interpreter() {
    let dir = workspace("deep", &[("deep.osk", DEEP)]);
    let run = oskac(&dir, &["run", "deep.osk"]);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "40000\n");
    assert_eq!(run.status.code(), Some(0));
    let test = oskac(&dir, &["test", "deep.osk"]);
    assert_eq!(test.status.code(), Some(0), "{test:?}");
    if cfg!(debug_assertions) {
        for command in ["run", "test"] {
            let interpreted = oskac(&dir, &[command, "deep.osk", "--interpret"]);
            assert!(
                String::from_utf8_lossy(&interpreted.stderr).contains("P0021")
                    || String::from_utf8_lossy(&interpreted.stdout).contains("P0021"),
                "the debug interpreter cannot go this deep: {interpreted:?}"
            );
        }
    }
    let mut left = BTreeSet::new();
    entries(&dir.join("tmp"), "", &mut left);
    assert!(
        left.is_empty(),
        "the temporary directory is left empty: {left:?}"
    );
}
