//! The step limit natively (spec #446, ticket #454): `oskac run
//! --max-steps=<n>` builds the program with a step counter, which stops
//! it at the same step as the interpreter (ADR 0021), with the same
//! output, report and exit code. Without the flag the executable is the
//! one built before the counter existed, byte for byte.
//!
//! The step fixtures are `tests/fixtures/run_limits/steps/` (see
//! `run_limits_steps.rs`). The digests of their executables built without
//! the flag are in `tests/golden/run_limits/unlimited_executables.txt`,
//! recorded from the build before #454. A change to the native backend
//! or `std.rt` that changes executables on purpose regenerates it with
//! `UPDATE_GOLDEN=1 cargo test -p oskac --test run_limits_steps_native`;
//! a change that adds a run limit must leave it as it is.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod common;
mod differential;

use common::portable::Scratch;
use common::run_limits::{
    assert_golden, cache_entries, cache_files, oskac_cached, oskac_in, oskac_with_stdin, program,
    step_fixtures, steps_written_in, HELLO,
};
use differential::{assert_same, Case};

/// Loops without end, and takes no other step.
const LOOPS_FOREVER: &str = "fn main() {\n    while true {}\n}\n";

/// Every fixture, under every limit from 1 step to the steps it takes:
/// each limit below stops it at the same step on both backends, and its
/// own count lets it run to its end.
#[test]
fn each_fixture_stops_at_the_same_step_on_both_backends() {
    for (name, src) in step_fixtures() {
        let steps = steps_written_in(&name, &src);
        for limit in 1..=steps {
            let flag = format!("--max-steps={limit}");
            let case = Case::file(&format!("steps_{name}_{limit}"), &src).with_run_flags(&[&flag]);
            let outcome = assert_same(&case);
            if limit < steps {
                assert_eq!(outcome.exit_code, Some(101), "{name} {flag}: {outcome:#?}");
                assert!(
                    outcome.stderr.contains("error[P0038]"),
                    "{name} {flag}: {outcome:#?}"
                );
            } else {
                assert!(
                    matches!(outcome.exit_code, Some(0 | 1)),
                    "{name} {flag}: {outcome:#?}"
                );
            }
        }
    }
}

#[test]
fn a_program_that_loops_without_end_stops_at_the_same_step_on_both_backends() {
    let case = Case::file("steps_endless", LOOPS_FOREVER).with_run_flags(&["--max-steps=1000000"]);
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(101));
    assert!(
        outcome.stderr.starts_with("main.osk:2:5: error[P0038]: "),
        "{}",
        outcome.stderr
    );
}

#[test]
fn the_json_report_is_the_same_on_both_backends() {
    let case =
        Case::file("steps_json", LOOPS_FOREVER).with_run_flags(&["--max-steps=3", "--format=json"]);
    let outcome = assert_same(&case);
    assert!(
        outcome.stderr.starts_with("{\"code\":\"P0038\""),
        "{}",
        outcome.stderr
    );
}

/// Recurses through a function value without end: each call is a step
/// and nests one deeper. `deep(0)` is the first call; the 50,001st is
/// both the step past `--max-steps=50000` and the call past the depth
/// limit (50,000 nested calls).
const DEEP_THROUGH_A_VALUE: &str = r#"fn deep(n: Int) -> Int {
    let f = deep
    f(n + 1)
}

fn main(io: Stdout) {
    let n = deep(0)
    let _ = io.println("{n}")
}
"#;

/// A call past both the step limit and the depth limit stops with
/// `P0038` on both backends: the step comes first, as in the
/// interpreter's `enter`. One step more, and the depth limit stops it.
#[test]
fn a_call_past_the_step_and_depth_limits_stops_at_the_step_limit_on_both_backends() {
    for (limit, code) in [(50_000, "P0038"), (50_001, "P0021")] {
        let flag = format!("--max-steps={limit}");
        let case = Case::file(&format!("steps_deep_{limit}"), DEEP_THROUGH_A_VALUE)
            .with_run_flags(&[&flag]);
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(101), "{flag}");
        assert!(
            outcome
                .stderr
                .starts_with(&format!("main.osk:3:5: error[{code}]: ")),
            "{flag}: {}",
            outcome.stderr
        );
    }
}

/// Both limits at once: whichever the program reaches first stops it, on
/// both backends alike.
#[test]
fn the_step_and_output_limits_together_stop_at_the_same_point() {
    let src = "fn main(io: Stdout) {\n    var i = 0\n    while true {\n        \
               let _ = io.println(\"{i}\")\n        i = i + 1\n    }\n}\n";
    for (steps, output) in [(5, 100), (100, 5), (50, 100)] {
        let steps = format!("--max-steps={steps}");
        let output = format!("--max-output={output}");
        let case = Case::file(&format!("steps_and_output_{}", &steps[12..]), src)
            .with_run_flags(&[&steps, &output]);
        assert_eq!(assert_same(&case).exit_code, Some(101));
    }
}

/// `oskac run -` (#447) under the step limit, natively.
#[test]
fn a_program_on_standard_input_runs_under_the_step_limit_natively() {
    let dir = Scratch::new("run-limits", "steps_stdin_native");
    let output = oskac_with_stdin(&dir, &["run", "-", "--max-steps=7"], LOOPS_FOREVER);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{stderr}");
    assert!(
        stderr.starts_with(
            "<stdin>:2:5: error[P0038]: step limit reached: the program would take more than 7 \
             steps"
        ),
        "{stderr}"
    );
}

/// The build cache keeps a run with `--max-steps` apart from one without
/// it, in either order, and from a run with another limit: without it,
/// `oskac run` runs the executable `oskac build` writes, byte for byte;
/// with it, another.
#[test]
fn a_run_with_the_step_limit_is_kept_apart_in_the_build_cache() {
    for (order, runs) in [
        ("unlimited_first", [&[][..], &["--max-steps=64"][..]]),
        ("limited_first", [&["--max-steps=64"][..], &[][..]]),
    ] {
        let dir = program(&format!("steps_cache_{order}"), HELLO);
        let cache = dir.join("cache");
        let run = |flags: &[&str]| {
            let mut args = vec!["run"];
            args.extend_from_slice(flags);
            args.push("main.osk");
            let output = oskac_cached(&dir, &cache, &args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert_eq!(output.stdout, b"hello\n", "{args:?}");
        };
        run(runs[0]);
        let after_first = cache_entries(&dir, &cache);
        run(runs[1]);
        let after_both = cache_entries(&dir, &cache);
        assert_eq!(after_both, after_first + 1, "{order}: a new entry");
        // Each run again: both hit their own entry.
        run(runs[0]);
        run(runs[1]);
        assert_eq!(
            cache_entries(&dir, &cache),
            after_both,
            "{order}: no new entry"
        );
        // Another limit is another entry still.
        run(&["--max-steps=65"]);
        assert_eq!(
            cache_entries(&dir, &cache),
            after_both + 1,
            "{order}: another limit"
        );
        let output = oskac_cached(&dir, &cache, &["build", "main.osk", "-o", "built"]);
        assert!(output.status.success(), "{output:?}");
        let built = std::fs::read(dir.join("built")).expect("the built executable");
        let holding_built = cache_files(&cache)
            .iter()
            .filter(|e| e.ends_with(&built))
            .count();
        assert_eq!(
            holding_built, 2,
            "{order}: only the unlimited run's and the build's"
        );
    }
}

/// Without `--max-steps`, each fixture's executable is the one `oskac
/// build` writes, and it runs as the interpreter does.
#[test]
fn without_the_flag_each_fixture_runs_as_oskac_build_writes_it() {
    for (name, src) in step_fixtures() {
        let outcome = assert_same(&Case::file(&format!("steps_unlimited_{name}"), &src));
        assert!(
            matches!(outcome.exit_code, Some(0 | 1)),
            "{name}: {outcome:#?}"
        );
    }
}

/// FNV-1a, 64 bits: a digest of an executable's bytes, enough to tell a
/// changed executable from the same one.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Without `--max-steps` the counter is not built in at all: every
/// fixture, between them every kind of step (calls, methods, closures,
/// function values, a user `Eq`, `?`, contracts, tasks, and each loop),
/// builds for every target to exactly the executable it built to before
/// #454 (the golden digests were recorded from that build).
#[test]
fn without_the_flag_every_executable_is_the_one_built_before_the_step_counter() {
    let mut digests = String::new();
    for (name, src) in step_fixtures() {
        let dir = program(&format!("steps_bytes_{name}"), &src);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("the output directory");
        let output = oskac_in(
            &dir,
            &[
                "build",
                "main.osk",
                "--target",
                "all",
                "-o",
                "out",
                "--no-cache",
            ],
        );
        assert!(output.status.success(), "{name}: {output:?}");
        let mut files: Vec<_> = std::fs::read_dir(&out)
            .expect("the executables")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        files.sort();
        for path in files {
            let bytes = std::fs::read(&path).expect("an executable");
            let file = path.file_name().expect("a name").to_string_lossy();
            digests.push_str(&format!(
                "{name} {file} {} {:016x}\n",
                bytes.len(),
                digest(&bytes)
            ));
        }
    }
    assert_golden("unlimited_executables.txt", &digests);
}
