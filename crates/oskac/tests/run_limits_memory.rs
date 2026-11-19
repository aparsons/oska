//! The memory run limit (spec #446, ticket #453): `oskac run
//! --max-memory=<bytes>` stops a program whose values take more than that
//! many bytes, net of what it frees, with `P0040` and exit code 101, on
//! the interpreter and natively. It is a resource bound, not a parity
//! promise: each backend counts with its own allocator, so the two may
//! stop at different points, and the tests check only that each stops.
//!
//! The expected output is in `tests/golden/run_limits/`. To regenerate it
//! after an intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac
//! --test run_limits_memory` and review the diff.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::memory::oskac_within;
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

/// A scratch directory holding `main.osk` with `src`.
fn program(name: &str, src: &str) -> Scratch {
    let dir = Scratch::new("run-limits-memory", name);
    std::fs::write(dir.join("main.osk"), src).expect("should write main.osk");
    dir
}

/// The backends: the interpreter, and natively where the host has a
/// backend (elsewhere `oskac run` interprets, which runs it twice).
const BACKENDS: [&[&str]; 2] = [&["--interpret"], &[]];

/// 64 MiB, the limit the ticket names.
const LIMIT: &str = "--max-memory=67108864";

/// Grows a list without end.
const GROWS: &str = r#"fn main() {
    var items: List<String> = []
    var i = 0
    while true {
        items.push("item {i}")
        i = i + 1
    }
}
"#;

/// Makes and lets go of texts, lists and closures, 200 MB of them in
/// all, holding a few KiB at a time; then prints `done`.
const CHURNS: &str = r#"fn main(io: Stdout) {
    var i = 0
    while i < 100000 {
        let line = "x".repeat(1000)
        var items: List<String> = []
        items.push(line)
        items.push("{line}{i}")
        let count = |extra: Int| items.len() + extra
        i = i + count(0) - 1
    }
    let _ = io.println("done {i}")
}
"#;

#[test]
fn a_max_memory_of_zero_a_negative_number_or_no_number_is_a_usage_error() {
    let dir = program("malformed", GROWS);
    for value in ["0", "-1", "abc", "", "1.5", "+5", "99999999999999999999"] {
        let flag = format!("--max-memory={value}");
        let output = oskac_in(&dir, &["run", "main.osk", &flag]);
        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("error: --max-memory takes a whole number of bytes above 0, not `{value}`\n")
        );
        assert!(output.stdout.is_empty());
    }
    let output = oskac_in(
        &dir,
        &["run", "main.osk", "--max-memory=8", "--max-memory=8"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: --max-memory is given more than once\n"
    );
}

#[test]
fn max_memory_on_any_command_but_run_is_a_usage_error() {
    let dir = program("other_commands", GROWS);
    for command in ["check", "test", "build", "fix"] {
        let output = oskac_in(&dir, &[command, "main.osk", LIMIT]);
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("\nUSAGE:\n"),
            "{command}: {output:?}"
        );
    }
}

/// The address space (Linux) or resident memory (macOS) a run under a
/// 64 MiB limit stays within: the interpreter's thread reserves a 256 MiB
/// stack, and `oskac` itself and a native program's stack need some
/// more, but a program that ran on past the limit would pass it.
fn bound_mib(backend: &[&str]) -> u64 {
    if backend.is_empty() {
        256
    } else {
        512
    }
}

#[test]
fn a_list_that_grows_without_end_stops_near_the_limit_with_p0040() {
    let dir = program("grows", GROWS);
    for backend in BACKENDS {
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.extend([LIMIT, "main.osk"]);
        let output = oskac_within(&dir, &args, bound_mib(backend));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
        assert!(output.stdout.is_empty());
        assert!(
            stderr.contains(
                "error[P0040]: memory limit reached: the program's values take more than \
                 67108864 bytes (`--max-memory=67108864`)"
            ),
            "{backend:?}: {stderr}"
        );
        assert!(stderr.starts_with("main.osk:"), "{backend:?}: {stderr}");
    }
}

/// The bound above means something: without the limit, the same run does
/// not stay within it.
#[cfg(target_os = "linux")]
#[test]
fn without_the_limit_the_list_grows_past_the_bound() {
    let dir = program("grows_unlimited", GROWS);
    for backend in BACKENDS {
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.push("main.osk");
        let output = oskac_within(&dir, &args, bound_mib(backend));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(output.status.code(), Some(0), "{backend:?}");
        assert!(!stderr.contains("P0040"), "{backend:?}: {stderr}");
    }
}

#[test]
fn a_program_that_allocates_and_frees_under_the_limit_runs_to_the_end() {
    let dir = program("churns", CHURNS);
    for backend in BACKENDS {
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        // 1 MiB: far less than the 200 MB the program makes in all.
        args.extend(["--max-memory=1048576", "main.osk"]);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{backend:?}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "done 100000\n");
    }
}

#[test]
fn the_memory_limit_report_is_one_json_object_with_format_json() {
    let dir = program("grows_json", GROWS);
    for backend in BACKENDS {
        let mut args = vec!["run"];
        args.extend_from_slice(backend);
        args.extend(["--max-memory=1048576", "--format=json", "main.osk"]);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{backend:?}: {stderr}");
        let report = common::Json::parse(stderr.trim_end());
        assert_eq!(
            report.get("code"),
            Some(&common::Json::Str("P0040".to_string()))
        );
        assert_eq!(
            report.get("path"),
            Some(&common::Json::Str("main.osk".to_string()))
        );
    }
}

#[test]
fn a_program_on_standard_input_runs_under_the_memory_limit() {
    use std::io::Write;
    let dir = Scratch::new("run-limits-memory", "stdin");
    for backend in BACKENDS {
        let mut args = vec!["run", "-", "--max-memory=1048576"];
        args.extend_from_slice(backend);
        let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(&args)
            .current_dir(&*dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("oskac should start");
        child
            .stdin
            .take()
            .expect("a stdin")
            .write_all(GROWS.as_bytes())
            .expect("should write the program");
        let output = child.wait_with_output().expect("oskac should finish");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with("<stdin>:") && stderr.contains("error[P0040]"),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn explain_p0040_explains_the_memory_limit() {
    let output = oskac_in(Path::new(env!("CARGO_MANIFEST_DIR")), &["explain", "P0040"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("the two may stop at different points"),
        "{text}"
    );
    assert_golden("explain_p0040.txt", &text);
}

// ---- Both backends (the differential suite) -----------------------------------

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod differential;

/// Each backend stops with `P0040` and exit code 101, where it may; a
/// program under the limit gives the same output on both; and the limit
/// changes nothing for a run without it.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod both_backends {
    use super::differential::{assert_same, run, Case};
    use super::{program, CHURNS, GROWS};

    #[test]
    fn each_backend_stops_a_growing_list_with_p0040() {
        let case = Case::file("memory_grows", GROWS).with_run_flags(&["--max-memory=4194304"]);
        let (interpreted, native) = run(&case);
        for (backend, outcome) in [("interpreter", interpreted), ("native", native)] {
            assert_eq!(outcome.exit_code, Some(101), "{backend}: {outcome:#?}");
            assert!(
                outcome.stderr.starts_with("main.osk:")
                    && outcome
                        .stderr
                        .contains("error[P0040]: memory limit reached")
                    && outcome.stderr.lines().count() == 1,
                "{backend}: {outcome:#?}"
            );
        }
    }

    #[test]
    fn a_program_under_the_memory_limit_runs_the_same_on_both_backends() {
        let case = Case::file("memory_churns", CHURNS).with_run_flags(&["--max-memory=1048576"]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.stdout, "done 100000\n");
    }

    /// Without the flag the executable `oskac run` runs is the one `oskac
    /// build` writes, byte for byte, and a memory-limited run builds
    /// another, kept apart in the build cache.
    #[test]
    fn a_memory_limited_run_builds_another_executable() {
        let dir = program("memory_cache", CHURNS);
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
        with_cache(&["build", "main.osk", "-o", "built"]);
        let built = std::fs::read(dir.join("built")).expect("the built executable");
        let holding_built = |all: &[Vec<u8>]| all.iter().filter(|e| e.ends_with(&built)).count();
        with_cache(&["run", "main.osk"]);
        let after_run = entries();
        assert_eq!(
            holding_built(&after_run),
            2,
            "`oskac run` runs what `oskac build` writes"
        );
        with_cache(&["run", "--max-memory=1048576", "main.osk"]);
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
}
