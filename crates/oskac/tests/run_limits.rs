//! Run limits (spec #446, ticket #450): `oskac run --max-output=<bytes>`
//! stops a program before it writes more than that many bytes to standard
//! output and standard error together, with `P0041` and exit code 101, on
//! the interpreter and natively. A malformed limit, and a limit on any
//! command but `run`, is a usage error (exit code 2).
//!
//! The expected output is in `tests/golden/run_limits/`. To regenerate it
//! after an intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac
//! --test run_limits` and review the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::portable::Scratch;

fn oskac_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env_remove("OSKA_CACHE_DIR")
        .output()
        .expect("oskac should run")
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/run_limits")
        .join(name)
}

/// Assert that `actual` is the content of the golden file `name`.
fn assert_golden(name: &str, actual: &str) {
    let golden = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().expect("a parent")).expect("the golden directory");
        std::fs::write(&golden, actual).expect("should write the golden file");
    }
    let expected = std::fs::read_to_string(&golden)
        .unwrap_or_else(|e| panic!("golden file {} should exist: {e}", golden.display()));
    assert!(
        actual == expected,
        "output differs from {}; rerun with UPDATE_GOLDEN=1 if the change is intended\n{actual}",
        golden.display()
    );
}

/// What `oskac <args>` printed and its exit code, as one text: the
/// command line, then stdout, stderr and the exit code.
fn transcript(dir: &Path, args: &[&str]) -> String {
    let output = oskac_in(dir, args);
    format!(
        "$ oskac {}\n--- stdout\n{}--- stderr\n{}--- exit {:?}\n\n",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        output.status.code()
    )
}

/// A scratch directory holding `main.osk` with `src`.
fn program(name: &str, src: &str) -> Scratch {
    let dir = Scratch::new("run-limits", name);
    std::fs::write(dir.join("main.osk"), src).expect("should write main.osk");
    dir
}

const HELLO: &str = "fn main(io: Stdout) {\n    let _ = io.println(\"hello\")\n}\n";

#[test]
fn a_max_output_of_zero_a_negative_number_or_no_number_is_a_usage_error() {
    let dir = program("malformed", HELLO);
    let mut all = String::new();
    for value in [
        "0",
        "-1",
        "abc",
        "",
        "1.5",
        "+5",
        " 5",
        "1e3",
        "99999999999999999999",
    ] {
        let flag = format!("--max-output={value}");
        let line = transcript(&dir, &["run", "main.osk", &flag]);
        assert!(line.contains("--- exit Some(2)"), "{line}");
        all.push_str(&line);
    }
    assert_golden("malformed.txt", &all);
}

#[test]
fn max_output_given_twice_is_a_usage_error() {
    let dir = program("twice", HELLO);
    let output = oskac_in(
        &dir,
        &["run", "main.osk", "--max-output=8", "--max-output=8"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: --max-output is given more than once\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn max_output_on_any_command_but_run_is_a_usage_error() {
    let dir = program("other_commands", HELLO);
    let mut all = String::new();
    for command in ["check", "test", "build", "fix"] {
        let line = transcript(&dir, &[command, "main.osk", "--max-output=64"]);
        assert!(line.contains("--- exit Some(2)"), "{line}");
        all.push_str(&line);
    }
    assert_golden("other_commands.txt", &all);
    // Nothing was built or fixed.
    let mut left: Vec<String> = std::fs::read_dir(&*dir)
        .expect("the directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(left, ["main.osk"]);
}

/// Writes numbered lines to stdout without end.
const ENDLESS: &str = r#"fn main(io: Stdout) {
    var i = 0
    while true {
        let _ = io.println("line {i}")
        i = i + 1
    }
}
"#;

/// Writes one line to stdout and one to stderr, without end.
const BOTH_STREAMS: &str = r#"fn main(out: Stdout, err: Stderr) {
    var i = 0
    while true {
        let _ = out.println("out {i}")
        let _ = err.println("err {i}")
        i = i + 1
    }
}
"#;

/// Writes lines that grow by one byte each, to stdout and stderr in
/// turn, then a `dbg` line, then fails with an error message: every way a
/// program writes.
const EVERY_WRITE: &str = r#"enum Failed {
    Lost
}

impl Display for Failed {
    fn to_string(self) -> String {
        "the last words of a program that failed"
    }
}

fn main(out: Stdout, err: Stderr) -> Result<(), Failed> {
    var line = "x"
    var i = 0
    while i < 12 {
        let _ = if i % 2 == 0 { out.println(line) } else { err.println(line) }
        line = "{line}x"
        i = i + 1
    }
    let _ = dbg(i)
    Err(Failed.Lost)
}
"#;

#[test]
fn a_program_that_writes_without_end_stops_at_the_limit_with_p0041() {
    let dir = program("endless", ENDLESS);
    let output = oskac_in(
        &dir,
        &["run", "--interpret", "--max-output=1048576", "main.osk"],
    );
    assert_eq!(output.status.code(), Some(101));
    assert!(output.stdout.len() <= 1_048_576);
    // 98,890 bytes of lines up to `line 9999`, then 86,335 lines of 11
    // bytes: the next one, `line 96335\n`, would pass the limit.
    assert!(output.stdout.ends_with(b"line 96334\n"));
    assert_eq!(output.stdout.len(), 1_048_575);
    assert_golden("endless.txt", &String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_limit_report_is_one_json_object_with_format_json() {
    let dir = program("endless_json", ENDLESS);
    let output = oskac_in(
        &dir,
        &[
            "run",
            "--interpret",
            "--max-output=20",
            "--format=json",
            "main.osk",
        ],
    );
    assert_eq!(output.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "line 0\nline 1\n");
    assert_golden("endless_json.txt", &String::from_utf8_lossy(&output.stderr));
}

#[test]
fn bytes_on_standard_error_count_toward_the_same_limit() {
    let dir = program("both_streams", BOTH_STREAMS);
    let output = oskac_in(&dir, &["run", "--interpret", "--max-output=30", "main.osk"]);
    assert_eq!(output.status.code(), Some(101));
    // `out 0\n` and `err 0\n` are 6 bytes each: 5 of them fit in 30.
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "out 0\nout 1\nout 2\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("err 0\nerr 1\nmain.osk:5:17: error[P0041]: "),
        "{stderr}"
    );
}

/// Writes a line, then asks for a secret whose name is 100,000 bytes of
/// program-made text that the environment does not set: the `P0020`
/// report would quote it all.
const HUGE_SECRET: &str = r#"fn main(out: Stdout, env: Env) {
    let _ = out.println("before")
    let name = "zé".repeat(50000)
    let secret = env.secret(name)
}
"#;

/// The program's bytes in `stderr` before the panic report, which
/// starts with `main.osk:`, and the report.
fn split_report(stderr: &str) -> (&str, &str) {
    match stderr.find("main.osk:") {
        Some(at) => (&stderr[..at], &stderr[at..]),
        None => (stderr, ""),
    }
}

/// Attack: whatever the limit, no byte past it reaches either stream,
/// whether the program writes with `println` on stdout or stderr, `dbg`,
/// or `main`'s error. Only the panic report follows. `backend` is
/// `--interpret` or nothing.
fn attack_every_write(name: &str, backend: &[&str]) {
    let dir = program(name, EVERY_WRITE);
    // All the program writes: 90 bytes of lines, a 9-byte `dbg` line and
    // a 40-byte error line.
    for limit in 1..=140u64 {
        let flag = format!("--max-output={limit}");
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.extend([flag.as_str(), "main.osk"]);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let (written, report) = split_report(&stderr);
        let total = output.stdout.len() as u64 + written.len() as u64;
        assert!(total <= limit, "limit {limit}: {total} bytes written");
        if limit < 139 {
            assert_eq!(output.status.code(), Some(101), "limit {limit}: {stderr}");
            assert!(report.contains("error[P0041]"), "limit {limit}: {stderr}");
            assert_eq!(report.lines().count(), 1, "limit {limit}: {stderr}");
        } else {
            assert_eq!(output.status.code(), Some(1), "limit {limit}: {stderr}");
            assert_eq!(total, 139, "limit {limit}");
        }
    }
}

/// Attack: a panic report that quotes text the program made (`P0020`
/// names the secret) carries no more of it than the limit has left,
/// cut where a character ends.
fn attack_huge_secret(name: &str, backend: &[&str]) {
    let dir = program(name, HUGE_SECRET);
    for limit in [7u64, 8, 9, 10, 11, 12, 20, 64, 1000] {
        let flag = format!("--max-output={limit}");
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.extend([flag.as_str(), "main.osk"]);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8(output.stderr).expect("the report is whole UTF-8");
        assert_eq!(output.status.code(), Some(101), "limit {limit}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
        let (written, report) = split_report(&stderr);
        assert_eq!(written, "", "limit {limit}");
        assert!(report.contains("error[P0020]"), "limit {limit}: {report}");
        let quoted = report.bytes().filter(|&b| b == b'z' || b >= 0x80).count() as u64;
        let left = limit - 7;
        assert!(
            quoted <= left,
            "limit {limit}: the report quotes {quoted} bytes of the name, {left} were left"
        );
        // As much of the name as fits, whole characters only.
        assert!(left - quoted < 3, "limit {limit}: {report}");
    }
}

#[test]
fn attack_no_byte_past_the_limit_reaches_either_stream() {
    attack_every_write("attack", &["--interpret"]);
}

#[test]
fn attack_a_panic_report_quotes_no_program_text_past_the_limit() {
    attack_huge_secret("attack_secret", &["--interpret"]);
}

#[test]
fn without_a_limit_a_panic_report_quotes_the_whole_name() {
    let dir = program("whole_secret", HUGE_SECRET);
    let output = oskac_in(&dir, &["run", "--interpret", "main.osk"]);
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.bytes().filter(|&b| b == b'z').count(), 50000);
}

#[test]
fn without_a_limit_a_run_is_unchanged() {
    let dir = program("unlimited", EVERY_WRITE);
    for backend in [&["--interpret"][..], &[]] {
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.push("main.osk");
        let output = oskac_in(&dir, &args);
        assert_eq!(output.status.code(), Some(1));
        assert_golden(
            "unlimited.txt",
            &format!(
                "{}--- stderr\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        );
    }
}

#[test]
fn explain_p0041_explains_the_output_limit() {
    let output = oskac_in(Path::new(env!("CARGO_MANIFEST_DIR")), &["explain", "P0041"]);
    assert_eq!(output.status.code(), Some(0));
    assert_golden(
        "explain_p0041.txt",
        &String::from_utf8_lossy(&output.stdout),
    );
}

// ---- Both backends (the differential suite) -----------------------------------

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod differential;

/// The same program under the same limit stops at the same write, with
/// the same output, report and exit code, natively and on the
/// interpreter.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod both_backends {
    use super::differential::{assert_same, Case};
    use super::{attack_every_write, attack_huge_secret, BOTH_STREAMS, ENDLESS, EVERY_WRITE};
    use super::{program, HELLO, HUGE_SECRET};

    #[test]
    fn attack_no_byte_past_the_limit_reaches_either_stream_natively() {
        attack_every_write("attack_native", &[]);
    }

    #[test]
    fn attack_a_panic_report_quotes_no_program_text_past_the_limit_natively() {
        attack_huge_secret("attack_secret_native", &[]);
    }

    #[test]
    fn a_cut_secret_name_is_the_same_on_both_backends() {
        for limit in ["--max-output=9", "--max-output=10", "--max-output=30"] {
            let name = format!("limit_secret_{}", &limit[13..]);
            let case = Case::file(&name, HUGE_SECRET).with_run_flags(&[limit]);
            assert_eq!(assert_same(&case).exit_code, Some(101));
        }
    }

    /// Without a limit, the executable `oskac run` runs is the one `oskac
    /// build` writes, byte for byte: both are kept in the build cache,
    /// and one entry holds exactly the built file. With a limit it is
    /// another executable.
    #[test]
    fn without_a_limit_oskac_run_runs_the_executable_oskac_build_writes() {
        let dir = program("same_executable", HELLO);
        let cache = dir.join("cache");
        let with_cache = |args: &[&str]| {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
                .args(args)
                .current_dir(&*dir)
                .env("OSKA_CACHE_DIR", &cache)
                .env_remove("OSKA_CACHE")
                .output()
                .expect("oskac should run");
            assert!(output.status.success(), "{args:?}: {output:?}");
        };
        with_cache(&["build", "main.osk", "-o", "built"]);
        let built = std::fs::read(dir.join("built")).expect("the built executable");
        let entries = || -> Vec<Vec<u8>> {
            let mut found = Vec::new();
            let mut stack = vec![cache.clone()];
            while let Some(at) = stack.pop() {
                for entry in std::fs::read_dir(&at).expect("a cache folder") {
                    let path = entry.expect("an entry").path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        found.push(std::fs::read(&path).expect("an entry's bytes"));
                    }
                }
            }
            found
        };
        let holding_built = |all: &[Vec<u8>]| all.iter().filter(|e| e.ends_with(&built)).count();
        let before = entries();
        assert_eq!(
            holding_built(&before),
            1,
            "`oskac build` keeps its executable"
        );
        with_cache(&["run", "main.osk"]);
        let after_run = entries();
        assert_eq!(
            holding_built(&after_run),
            2,
            "`oskac run` kept an executable byte-identical to `oskac build`'s"
        );
        with_cache(&["run", "--max-output=64", "main.osk"]);
        let after_limited = entries();
        assert_eq!(
            after_limited.len(),
            after_run.len() + 1,
            "a limited run is kept apart"
        );
        assert_eq!(
            holding_built(&after_limited),
            2,
            "and is another executable"
        );
    }

    /// Writes through a closure, which a function takes as a `cap fn`,
    /// and with `dbg`, without end.
    const THROUGH_A_CLOSURE: &str = r#"fn twice(f: cap fn(String) -> Result<(), IoError>, text: String) {
    let _ = f(text)
    let _ = f(text)
}

fn main(io: Stdout) {
    while true {
        let shout = |t: String| io.println("{t}!")
        twice(shout, "loud")
        let _ = dbg(1)
    }
}
"#;

    #[test]
    fn a_program_that_writes_without_end_stops_at_one_mebibyte_on_both_backends() {
        let case = Case::file("limit_endless", ENDLESS).with_run_flags(&["--max-output=1048576"]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(101));
        assert_eq!(outcome.stdout.len(), 1_048_575);
        assert!(outcome.stdout.ends_with("line 96334\n"));
        assert!(
            outcome.stderr.starts_with("main.osk:4:17: error[P0041]: "),
            "{}",
            outcome.stderr
        );
    }

    #[test]
    fn stderr_counts_toward_the_limit_on_both_backends() {
        let case =
            Case::file("limit_both_streams", BOTH_STREAMS).with_run_flags(&["--max-output=30"]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.stdout, "out 0\nout 1\nout 2\n");
        assert_eq!(outcome.exit_code, Some(101));
    }

    #[test]
    fn every_write_stops_at_the_same_byte_on_both_backends() {
        for limit in [1, 2, 5, 20, 89, 90, 98, 99, 100, 138, 139] {
            let flag = format!("--max-output={limit}");
            let case = Case::file(&format!("limit_every_write_{limit}"), EVERY_WRITE)
                .with_run_flags(&[&flag]);
            let outcome = assert_same(&case);
            let expected = if limit < 139 { 101 } else { 1 };
            assert_eq!(
                outcome.exit_code,
                Some(expected),
                "limit {limit}: {outcome:#?}"
            );
        }
    }

    #[test]
    fn a_write_through_a_closure_or_dbg_stops_at_the_same_byte_on_both_backends() {
        let case =
            Case::file("limit_closure", THROUGH_A_CLOSURE).with_run_flags(&["--max-output=40"]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.stdout, "loud!\nloud!\nloud!\nloud!\n");
        assert_eq!(outcome.exit_code, Some(101));
    }

    #[test]
    fn the_json_report_is_the_same_on_both_backends() {
        let case =
            Case::file("limit_json", ENDLESS).with_run_flags(&["--max-output=20", "--format=json"]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.stdout, "line 0\nline 1\n");
        assert!(
            outcome.stderr.starts_with("{\"code\":\"P0041\""),
            "{}",
            outcome.stderr
        );
    }

    #[test]
    fn without_a_limit_the_program_runs_as_oskac_build_writes_it() {
        // No run flags: the executable `oskac build` writes, against the
        // interpreter.
        let outcome = assert_same(&Case::file("limit_none", EVERY_WRITE));
        assert_eq!(outcome.exit_code, Some(1));
    }
}
