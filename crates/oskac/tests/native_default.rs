//! Native by default (spec #125, ticket #141; x86_64, #165; macOS, #172
//! and #173): on aarch64 and x86_64 Linux and macOS, `oskac run` and
//! `oskac test` compile the program natively for the host into a private
//! temporary directory and run it there, and `--interpret` selects the
//! interpreter; on x86_64 Windows too (#179). Every other host keeps the
//! interpreter.
//!
//! The parity gate (spec #155, ticket #181): a program the native backend
//! cannot compile yet (`E0390`) is an error for `run` and `test`, as for
//! `build`, on every native host, with no run on the interpreter unless
//! `--interpret` asks for one.
//!
//! The golden cases (`tests/golden/native_default/<case>.txt`) are the
//! same on every native host. Elsewhere the interpreter runs, and must
//! print exactly what the native run prints; only the gate's cases are
//! for native hosts alone. To regenerate them after an intended change, run
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test native_default` and review
//! the diff. The tests for the native hosts below show that the default
//! really is native, through what only the native backend does: it writes
//! its executable into the temporary directory, which the interpreter
//! never touches.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Whether this host runs programs natively by default.
const NATIVE_HOST: bool = cfg!(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
));

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
/// A `Bytes` program (ticket #222): what it prints, `dbg` of a `Bytes`,
/// and the report of a read past its end (`P0042`).
const BYTES: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
                     var b = \"Oska\".to_bytes()\n    \
                     b.push_u16_be(258)\n    \
                     io.println(b.to_hex())?\n    \
                     let _ = dbg(b.slice(1, 3))\n    \
                     let n = b.read_u32_le(4)\n    \
                     io.println(\"unreached\")\n}\n";

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

/// On Windows, a native run's `args.get(0)` is its temporary executable,
/// not the path the user named, as Rust cannot set it there (#475): the
/// first line `[...\program.exe]` becomes `[<path>]`, the path after
/// `run`, so the rest of the arguments still meet the golden.
fn windows_argv0(stdout: &str, args: &[&str]) -> String {
    let named = args.iter().skip_while(|a| **a != "run").nth(1);
    match (cfg!(windows), named, stdout.split_once('\n')) {
        (true, Some(named), Some((first, rest)))
            if first.starts_with('[') && first.ends_with("\\program.exe]") =>
        {
            format!("[{named}]\n{rest}")
        }
        _ => stdout.to_string(),
    }
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
        .env("TMP", dir.join("tmp"))
        .env("TEMP", dir.join("tmp"))
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
        common::golden::mask_durations(&windows_argv0(
            &String::from_utf8_lossy(&output.stdout),
            args
        )),
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
fn a_bytes_program_prints_and_panics_as_the_interpreter_does() {
    let files = [("bytes.osk", BYTES)];
    assert_golden("run_bytes", &files, &["run", "bytes.osk"]);
    assert_golden(
        "run_bytes_interpret",
        &files,
        &["run", "--interpret", "bytes.osk"],
    );
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
fn a_program_the_native_backend_refuses_is_an_error() {
    if !NATIVE_HOST {
        return;
    }
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
fn a_package_the_native_backend_refuses_is_an_error() {
    if !NATIVE_HOST {
        return;
    }
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
    let spawn = [("spawn.osk", "fn main(spawn: Spawn) {}\n")];
    assert_golden("run_ungranted", &spawn, &["run", "spawn.osk"]);
    // The network runs on the interpreter only (ADR 0023): the native
    // backend refuses it, an error on every platform, and `--interpret`
    // runs it.
    let net = [(
        "net.osk",
        "fn main(net: Net) {\n    let _ = net.restrict_to(\"example.com\", 443)\n}\n",
    )];
    assert_golden("run_net_refused_natively", &net, &["run", "net.osk"]);
    assert_golden(
        "run_net_interpret",
        &net,
        &["run", "net.osk", "--interpret"],
    );
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
fn tests_the_native_backend_refuses_are_an_error_and_interpret_runs_them() {
    let src = "test \"refused\" {\n    let d = dbg(t.fs.restrict_to(\"/\"))\n    \
               t.assert(true)\n}\n";
    // `--interpret` runs them on every host; the gate refuses them on the
    // native ones.
    assert_golden(
        "test_unsupported_interpret",
        &[("unsupported.osk", src)],
        &["test", "unsupported.osk", "--interpret"],
    );
    if NATIVE_HOST {
        assert_golden(
            "test_unsupported",
            &[("unsupported.osk", src)],
            &["test", "unsupported.osk"],
        );
    }
}

// ---- aarch64 and x86_64 Linux and macOS: the default is native --------------

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
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

/// A recursion 40,000 calls deep, within the language's limit (50,000),
/// which native code reaches. How deep the interpreter gets depends on
/// the build profile (its frames are larger unoptimised), so nothing here
/// relies on it failing.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
const DEEP: &str = "fn down(n: Int) -> Int {\n    if n == 0 {\n        return 0\n    }\n    \
                    down(n - 1) + 1\n}\n\n\
                    fn main(io: Stdout) -> Result<(), IoError> {\n    \
                    let d = down(40000)\n    io.println(\"{d}\")\n}\n\n\
                    test \"deep\" {\n    t.assert_eq(down(40000), 40000)\n}\n";

/// A small program that prints a line.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
const GREETS: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n";

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[test]
fn run_and_test_are_native_by_default_and_interpret_selects_the_interpreter() {
    let dir = workspace("deep", &[("deep.osk", DEEP)]);
    let run = oskac(&dir, &["run", "deep.osk"]);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "40000\n");
    assert_eq!(run.status.code(), Some(0));
    let test = oskac(&dir, &["test", "deep.osk"]);
    assert_eq!(test.status.code(), Some(0), "{test:?}");
    // The selection, independent of how deep an interpreter can recurse in
    // this build profile: a native run needs a temporary directory it can
    // write its executable to, and `--interpret` does not.
    let src = format!("{GREETS}\ntest \"passes\" {{\n    t.assert(true)\n}}\n");
    let dir = workspace("selection", &[("greets.osk", &src)]);
    let missing = dir.join("missing");
    let in_missing = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(&dir)
            .env("TMPDIR", &missing)
            .output()
            .expect("oskac should run")
    };
    for command in ["run", "test"] {
        let native = in_missing(&[command, "greets.osk"]);
        assert_eq!(
            native.status.code(),
            Some(1),
            "{command} is native: {native:?}"
        );
        assert!(
            String::from_utf8_lossy(&native.stderr).contains("E0391"),
            "{command} is native: {native:?}"
        );
        let interpreted = in_missing(&[command, "greets.osk", "--interpret"]);
        assert_eq!(
            interpreted.status.code(),
            Some(0),
            "--interpret runs the interpreter: {interpreted:?}"
        );
        assert!(
            !String::from_utf8_lossy(&interpreted.stderr).contains("E0391"),
            "--interpret runs the interpreter: {interpreted:?}"
        );
    }
    assert_eq!(
        String::from_utf8_lossy(&in_missing(&["run", "greets.osk", "--interpret"]).stdout),
        "hi\n"
    );
    let dir = workspace("deep", &[("deep.osk", DEEP)]);
    let mut left = BTreeSet::new();
    entries(&dir.join("tmp"), "", &mut left);
    assert!(
        left.is_empty(),
        "the temporary directory is left empty: {left:?}"
    );
}
