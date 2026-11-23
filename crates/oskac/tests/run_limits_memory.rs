// SPDX-License-Identifier: Apache-2.0
//! The memory run limit (spec #446, ticket #453): `oskac run
//! --max-memory=<bytes>` stops a program before its values take more than
//! that many bytes, net of those it let go of, with `P0040` and exit code
//! 101, on the interpreter and natively. It is a resource bound, not a
//! parity promise: each backend counts in its own way, so the two
//! may stop at different points, and the tests check only that each
//! stops.
//!
//! The expected output is in `tests/golden/run_limits/`. To regenerate it
//! after an intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac
//! --test run_limits_memory` and review the diff.

mod common;

use std::path::Path;

use common::frames::without_frames;
use common::run_limits::{assert_golden, oskac_in, oskac_with_stdin, program};
use common::Json;

/// The backends: the interpreter, and natively where the host has a
/// backend (elsewhere `oskac run` interprets, which runs it twice).
const BACKENDS: [&[&str]; 2] = [&["--interpret"], &[]];

/// Grows a list of texts without end.
const GROWS: &str = r#"fn main() {
    var items: List<String> = []
    var i = 0
    while true {
        items.push("item {i}")
        i = i + 1
    }
}
"#;

/// Grows a list held in a struct's field without end, writing the struct
/// as it goes.
const GROWS_IN_A_FIELD: &str = r#"struct Bag {
    items: List<String>,
    count: Int
}

fn main() {
    var bag = Bag(items: [], count: 0)
    while true {
        bag.items.push("item {bag.count}")
        bag.count = bag.count + 1
    }
}
"#;

/// Doubles a page of `std.html` markup without end.
const HTML_DOUBLES: &str = r#"use std.html

fn main() {
    var page = html.text("<p>grows</p>")
    while true {
        page = page.concat(page)
    }
}
"#;

/// Keeps `KEPT` small texts, near an 8 MiB limit on the interpreter, then
/// makes and lets go of two million more, one at a time.
fn near_the_limit(kept: u32) -> String {
    format!(
        r#"fn main(io: Stdout) {{
    var kept: List<String> = []
    var i = 0
    while i < {kept} {{
        kept.push("kept {{i}}")
        i = i + 1
    }}
    var j = 0
    while j < 2000000 {{
        let passing = "passing {{j}}"
        j = j + 1
    }}
    let _ = io.println("done {{j}}")
}}
"#
    )
}

/// Makes one text of 200 MB at once.
const ONE_HUGE_TEXT: &str = r#"fn main(io: Stdout) {
    let huge = "x".repeat(200000000)
    let n = huge.len()
    let _ = io.println("made {n}")
}
"#;

/// Makes and lets go of texts, lists, structs and closures, 200 MB of
/// them in all, holding a few KiB at a time; then prints `done`.
const CHURNS: &str = r#"struct Bag {
    items: List<String>,
    count: Int
}

fn main(io: Stdout) {
    var bag = Bag(items: [], count: 0)
    var i = 0
    while i < 100000 {
        let line = "x".repeat(1000)
        var items: List<String> = []
        items.push(line)
        items.push("{line}{i}")
        let count = |extra: Int| items.len() + extra
        bag.items.push("item {i}")
        let copy = bag
        bag.count = copy.count + count(0) - 1
        if bag.items.len() > 50 {
            bag = Bag(items: [], count: bag.count)
        }
        i = i + 1
    }
    let n = bag.count
    let _ = io.println("done {n}")
}
"#;

/// The report of a run stopped by a 64 MiB limit.
const REPORT_64_MIB: &str = "error[P0040]: memory limit reached: the program's values take \
     more than 67108864 bytes (`--max-memory=67108864`)";

#[test]
fn a_max_memory_of_zero_a_negative_number_or_no_number_is_a_usage_error() {
    let dir = program("memory_malformed", GROWS);
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
    let dir = program("memory_other_commands", GROWS);
    for command in ["check", "test", "build", "fix"] {
        let output = oskac_in(&dir, &[command, "main.osk", "--max-memory=67108864"]);
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("\nUSAGE:\n"),
            "{command}: {output:?}"
        );
    }
}

/// On every platform: a list that grows without end, directly or in a
/// struct's field, stops with `P0040` on both backends. (Where the
/// platform lets a test measure it, `both_backends` also shows the run
/// stays near the limit; Windows has no such measure here.)
#[test]
fn a_list_that_grows_without_end_stops_with_p0040() {
    for (name, src) in [
        ("memory_grows", GROWS),
        ("memory_grows_in_a_field", GROWS_IN_A_FIELD),
    ] {
        let dir = program(name, src);
        for backend in BACKENDS {
            let mut args = vec!["run", "--no-cache", "--max-memory=67108864", "main.osk"];
            args.extend_from_slice(backend);
            let output = oskac_in(&dir, &args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(101),
                "{name} {backend:?}: {stderr}"
            );
            assert!(output.stdout.is_empty());
            assert!(
                stderr.starts_with("main.osk:") && stderr.contains(REPORT_64_MIB),
                "{name} {backend:?}: {stderr}"
            );
            assert_eq!(
                without_frames(&stderr).lines().count(),
                1,
                "{name} {backend:?}: {stderr}"
            );
        }
    }
}

/// The interpreter checks a value before it makes it: one text over the
/// limit is never built, and the report spans the call that would have
/// made it. Natively the runtime checks each block before it allocates
/// it.
#[test]
fn a_text_over_the_limit_is_never_made() {
    let dir = program("memory_one_huge_text", ONE_HUGE_TEXT);
    let output = oskac_in(
        &dir,
        &["run", "--interpret", "--max-memory=67108864", "main.osk"],
    );
    assert_eq!(output.status.code(), Some(101));
    assert!(output.stdout.is_empty());
    assert_golden(
        "memory_one_huge_text.txt",
        &String::from_utf8_lossy(&output.stderr),
    );
    let output = oskac_in(
        &dir,
        &["run", "--no-cache", "--max-memory=67108864", "main.osk"],
    );
    assert_eq!(output.status.code(), Some(101));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains(REPORT_64_MIB));
}

#[test]
fn markup_that_doubles_without_end_stops_with_p0040() {
    let dir = program("memory_html_doubles", HTML_DOUBLES);
    for backend in BACKENDS {
        let mut args = vec!["run", "--no-cache", "--max-memory=8388608", "main.osk"];
        args.extend_from_slice(backend);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
        assert!(
            stderr.starts_with("main.osk:") && stderr.contains("error[P0040]"),
            "{backend:?}: {stderr}"
        );
    }
}

/// A program that holds nearly all of its limit in many small values
/// while it makes and lets go of many more is not slowed down by the
/// meter's sweeps: on the interpreter, a run within an eighth of the
/// limit stops with `P0040` at once, and one with more room runs to the
/// end; natively each runs to the end or stops. Either way, promptly.
#[test]
fn a_program_near_the_limit_finishes_or_stops_promptly() {
    for (kept, interpreted) in [(40000, Some(0)), (46000, Some(101))] {
        let dir = program(&format!("memory_near_{kept}"), &near_the_limit(kept));
        for backend in BACKENDS {
            let mut args = vec!["run", "--no-cache", "--max-memory=8388608", "main.osk"];
            args.extend_from_slice(backend);
            let started = std::time::Instant::now();
            let output = oskac_in(&dir, &args);
            let took = started.elapsed();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code();
            if backend == ["--interpret"] {
                assert_eq!(code, interpreted, "{kept} {backend:?}: {stderr}");
            } else {
                assert!(
                    matches!(code, Some(0 | 101)),
                    "{kept} {backend:?}: {stderr}"
                );
            }
            if code == Some(0) {
                assert_eq!(String::from_utf8_lossy(&output.stdout), "done 2000000\n");
            } else {
                assert!(
                    stderr.contains("error[P0040]"),
                    "{kept} {backend:?}: {stderr}"
                );
            }
            assert!(
                took < std::time::Duration::from_secs(60),
                "{kept} {backend:?} took {took:?}"
            );
        }
    }
}

#[test]
fn a_program_that_allocates_and_frees_under_the_limit_runs_to_the_end() {
    let dir = program("memory_churns", CHURNS);
    for backend in BACKENDS {
        // 1 MiB: far less than the 200 MB the program makes in all.
        let mut args = vec!["run", "--no-cache", "--max-memory=1048576", "main.osk"];
        args.extend_from_slice(backend);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{backend:?}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "done 100000\n");
    }
}

/// Grows a `Bytes` to 3 MiB a byte at a time, in place, and makes and
/// lets go of many slices of it: within an 8 MiB limit on both backends.
const BYTES_FIT: &str = r#"fn main(io: Stdout) {
    var b = Bytes.new()
    var i = 0
    while i < 3145728 {
        b.push(1)
        i = i + 1
    }
    var total = 0
    var j = 0
    while j < 2000 {
        total = total + b.slice(j, j + 100000).len()
        j = j + 1
    }
    let n = b.len()
    let _ = io.println("{n} {total}")
}
"#;

/// Appends a chunk to a `Bytes` without end.
const BYTES_GROW: &str = r#"fn main() {
    let chunk = Bytes.zeroed(4096)
    var b = Bytes.new()
    while true {
        b.append(chunk)
    }
}
"#;

/// Keeps changed copies of a shared 3 MiB `Bytes`: each change copies it.
const BYTES_COPIES: &str = r#"fn main() {
    let shared = Bytes.zeroed(3145728)
    var copies: List<Bytes> = []
    while true {
        var copy = shared
        copy.set(0, 1)
        copies.push(copy)
    }
}
"#;

/// `Bytes` under `--max-memory` (ticket #222): a buffer that grows in
/// place within the limit runs to the end; one that grows past it by
/// appends, or by changed copies of a shared buffer, stops with `P0040`
/// on both backends. On the interpreter the report spans the call that
/// would have grown or copied it.
#[test]
fn bytes_are_counted_against_the_memory_limit() {
    let fits = program("memory_bytes_fit", BYTES_FIT);
    let grows = program("memory_bytes_grow", BYTES_GROW);
    let copies = program("memory_bytes_copies", BYTES_COPIES);
    for backend in BACKENDS {
        let run = |dir: &Path| {
            let mut args = vec!["run", "--no-cache", "--max-memory=8388608", "main.osk"];
            args.extend_from_slice(backend);
            let output = oskac_in(dir, &args);
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            (output.status.code(), output.stdout, stderr)
        };
        let (code, stdout, stderr) = run(&fits);
        assert_eq!(code, Some(0), "{backend:?}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&stdout), "3145728 200000000\n");
        for (dir, line) in [(&grows, "main.osk:5:9: "), (&copies, "main.osk:6:9: ")] {
            let (code, stdout, stderr) = run(dir);
            assert_eq!(code, Some(101), "{backend:?}: {stderr}");
            assert!(stdout.is_empty(), "{backend:?}: {stderr}");
            assert!(stderr.contains("error[P0040]"), "{backend:?}: {stderr}");
            if backend == ["--interpret"] {
                assert!(stderr.starts_with(line), "{line} {backend:?}: {stderr}");
            }
        }
    }
}

#[test]
fn the_memory_limit_report_is_one_json_object_with_format_json() {
    let dir = program("memory_grows_json", GROWS);
    for backend in BACKENDS {
        let mut args = vec![
            "run",
            "--no-cache",
            "--max-memory=1048576",
            "--format=json",
            "main.osk",
        ];
        args.extend_from_slice(backend);
        let output = oskac_in(&dir, &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{backend:?}: {stderr}");
        let report = Json::parse(stderr.trim_end());
        assert_eq!(report.get("code"), Some(&Json::Str("P0040".to_string())));
        assert_eq!(report.get("path"), Some(&Json::Str("main.osk".to_string())));
    }
}

#[test]
fn a_program_on_standard_input_runs_under_the_memory_limit() {
    let dir = program("memory_stdin", "");
    for backend in BACKENDS {
        let mut args = vec!["run", "-", "--no-cache", "--max-memory=1048576"];
        args.extend_from_slice(backend);
        let output = oskac_with_stdin(&dir, &args, GROWS);
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

/// Where the host has a native backend (Linux and macOS): each backend
/// stops with `P0040` and exit code 101, where it may, within a measured
/// bound; a program under the limit gives the same output on both; and
/// the limit changes nothing for a run without it.
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod both_backends {
    use std::path::Path;

    use super::differential::{assert_same, run, within_memory, Case};
    use super::{program, CHURNS, GROWS, REPORT_64_MIB};

    /// The memory (Linux: address space; macOS: peak resident memory) a
    /// run under a 64 MiB limit stays within, which a program that ran on
    /// past the limit would pass. Natively 192 MiB: the executable maps
    /// about 60 MiB before the program starts (its stack, reserved for
    /// the deepest calls), then holds at most the 64 MiB limit, plus the
    /// mapping of the last large block it freed, which the runtime keeps
    /// for reuse, uncounted. On the interpreter 480 MiB: its thread
    /// reserves a 256 MiB stack and `oskac` itself about 80 MiB before
    /// the program starts, and growing a list copies it, so the old items
    /// and the new are both held for a moment.
    fn bound_mib(backend: &[&str]) -> u64 {
        if backend.is_empty() {
            192
        } else {
            480
        }
    }

    fn oskac_within(dir: &Path, args: &[&str], mib: u64) -> std::process::Output {
        within_memory(Path::new(env!("CARGO_BIN_EXE_oskac")), args, dir, mib)
    }

    #[test]
    fn a_list_that_grows_without_end_stops_near_the_limit() {
        let dir = program("memory_grows_bounded", GROWS);
        for backend in super::BACKENDS {
            let mut args = vec!["run", "--no-cache", "--max-memory=67108864", "main.osk"];
            args.extend_from_slice(backend);
            let output = oskac_within(&dir, &args, bound_mib(backend));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
            assert!(stderr.contains(REPORT_64_MIB), "{backend:?}: {stderr}");
        }
    }

    /// The bound means something: without the limit, the same run does
    /// not stay within it. (Linux only: macOS measures after the run, which
    /// here would not end.)
    #[cfg(target_os = "linux")]
    #[test]
    fn without_the_limit_the_list_grows_past_the_bound() {
        let dir = program("memory_grows_unbounded", GROWS);
        for backend in super::BACKENDS {
            let mut args = vec!["run", "--no-cache", "main.osk"];
            args.extend_from_slice(backend);
            let output = oskac_within(&dir, &args, bound_mib(backend));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_ne!(output.status.code(), Some(0), "{backend:?}");
            assert!(!stderr.contains("P0040"), "{backend:?}: {stderr}");
        }
    }

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
                    && crate::common::frames::without_frames(&outcome.stderr)
                        .lines()
                        .count()
                        == 1,
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
