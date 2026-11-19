//! The step limit (spec #446, tickets #452 and #454): `oskac run
//! --max-steps=<n>` stops a program after `n` steps (ADR 0021) with
//! `P0038` and exit code 101, at the same step on the interpreter and
//! natively. The fixtures in `tests/fixtures/run_limits/steps/` each say,
//! on their first line, how many steps they take, and why. A malformed
//! value and the flag on any command but `run` are usage errors (exit
//! code 2).
//!
//! The expected output is in `tests/golden/run_limits/`. To regenerate it
//! after an intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac
//! --test run_limits_steps` and review the diff.

mod common;

use std::path::Path;

use common::portable::Scratch;
use common::run_limits::{assert_golden, oskac_in, oskac_with_stdin, program, transcript, HELLO};

/// Loops without end, and takes no other step.
const LOOPS_FOREVER: &str = "fn main() {\n    while true {}\n}\n";

#[test]
fn a_program_that_loops_without_end_stops_at_the_step_limit_with_p0038() {
    let dir = program("steps_endless", LOOPS_FOREVER);
    let output = oskac_in(
        &dir,
        &["run", "--interpret", "--max-steps=1000000", "main.osk"],
    );
    assert_eq!(output.status.code(), Some(101));
    assert!(output.stdout.is_empty());
    assert_golden(
        "steps_endless.txt",
        &String::from_utf8_lossy(&output.stderr),
    );
}

/// The step fixtures, `tests/fixtures/run_limits/steps/*.osk`: each name
/// and source, sorted by name.
fn step_fixtures() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run_limits/steps");
    let mut fixtures: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("the step fixtures")
        .map(|entry| {
            let path = entry.expect("an entry").path();
            let name = path
                .file_stem()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            let src = std::fs::read_to_string(&path).expect("a fixture");
            (name, src)
        })
        .collect();
    fixtures.sort();
    fixtures
}

/// The steps a fixture says it takes, on its first line: `// Steps: N.`
fn steps_written_in(name: &str, src: &str) -> u64 {
    src.lines()
        .next()
        .and_then(|line| line.strip_prefix("// Steps: "))
        .and_then(|rest| rest.split('.').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{name}.osk should start with `// Steps: N.`"))
}

/// Each fixture takes exactly the steps it says (ADR 0021): with that
/// many it runs to the end as it does without a limit, and with one fewer
/// it stops with `P0038` before printing.
#[test]
fn each_fixture_takes_the_steps_it_says() {
    let fixtures = step_fixtures();
    let names: Vec<&str> = fixtures.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "closure",
            "contracts",
            "eq",
            "fn_values",
            "loop",
            "main_error",
            "recursion",
            "std_module",
            "tasks",
            "trait_method",
            "try_convert"
        ]
    );
    for (name, src) in &fixtures {
        let steps = steps_written_in(name, src);
        let dir = program(&format!("steps_{name}"), src);
        let unlimited = oskac_in(&dir, &["run", "--interpret", "main.osk"]);
        let ended = unlimited.status.code();
        // It runs to its end: exit code 0, or 1 when `main` returns an
        // error, with no diagnostic or panic.
        assert!(matches!(ended, Some(0 | 1)), "{name}: {unlimited:?}");
        let stderr = String::from_utf8_lossy(&unlimited.stderr);
        assert!(!stderr.contains("error["), "{name}: {stderr}");
        let enough = format!("--max-steps={steps}");
        let output = oskac_in(&dir, &["run", "--interpret", &enough, "main.osk"]);
        assert_eq!(output.status.code(), ended, "{name} {enough}: {output:?}");
        assert_eq!(output.stdout, unlimited.stdout, "{name} {enough}");
        assert_eq!(output.stderr, unlimited.stderr, "{name} {enough}");
        let short = format!("--max-steps={}", steps - 1);
        let output = oskac_in(&dir, &["run", "--interpret", &short, "main.osk"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{name} {short}: {stderr}");
        assert!(output.stdout.is_empty(), "{name} {short}: {output:?}");
        let message = format!(
            "error[P0038]: step limit reached: the program would take more than {} step",
            steps - 1
        );
        assert!(stderr.contains(&message), "{name} {short}: {stderr}");
    }
}

#[test]
fn a_max_steps_of_zero_a_negative_number_or_no_number_is_a_usage_error() {
    let dir = program("malformed_steps", HELLO);
    let mut all = String::new();
    for value in ["0", "-1", "abc", "", "1.5", "99999999999999999999"] {
        let flag = format!("--max-steps={value}");
        let line = transcript(&dir, &["run", "--interpret", "main.osk", &flag]);
        assert!(line.contains("--- exit Some(2)"), "{line}");
        all.push_str(&line);
    }
    let line = transcript(
        &dir,
        &[
            "run",
            "--interpret",
            "main.osk",
            "--max-steps=5",
            "--max-steps=5",
        ],
    );
    assert!(line.contains("--- exit Some(2)"), "{line}");
    all.push_str(&line);
    assert_golden("malformed_steps.txt", &all);
}

#[test]
fn max_steps_on_any_command_but_run_is_a_usage_error() {
    let dir = program("other_commands_steps", HELLO);
    for command in ["check", "test", "build", "fix"] {
        let output = oskac_in(&dir, &[command, "main.osk", "--max-steps=64"]);
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("oskac "),
            "{command}: the usage, {output:?}"
        );
    }
}

#[test]
fn the_step_limit_report_is_one_json_object_with_format_json() {
    let dir = program("steps_json", LOOPS_FOREVER);
    let output = oskac_in(
        &dir,
        &[
            "run",
            "--interpret",
            "--max-steps=3",
            "--format=json",
            "main.osk",
        ],
    );
    assert_eq!(output.status.code(), Some(101));
    assert_golden(
        "steps_endless_json.txt",
        &String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn explain_p0038_explains_the_step_limit() {
    let output = oskac_in(Path::new(env!("CARGO_MANIFEST_DIR")), &["explain", "P0038"]);
    assert_eq!(output.status.code(), Some(0));
    assert_golden(
        "explain_p0038.txt",
        &String::from_utf8_lossy(&output.stdout),
    );
}

/// `oskac run -` (#447) takes the step limit too, on the interpreter and
/// natively where the host has a backend.
#[test]
fn a_program_on_standard_input_runs_under_the_step_limit() {
    let dir = Scratch::new("run-limits", "steps_stdin");
    for backend in [&["--interpret"][..], &[]] {
        let mut args = vec!["run", "-", "--max-steps=7"];
        args.extend_from_slice(backend);
        let output = oskac_with_stdin(&dir, &args, LOOPS_FOREVER);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with(
                "<stdin>:2:5: error[P0038]: step limit reached: the program would take more \
                 than 7 steps"
            ),
            "{args:?}: {stderr}"
        );
    }
}

/// The step limit's message reads right for a limit of one step.
#[test]
fn a_limit_of_one_step_is_worded_in_the_singular() {
    let dir = program("steps_one", LOOPS_FOREVER);
    let output = oskac_in(&dir, &["run", "--interpret", "--max-steps=1", "main.osk"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("would take more than 1 step, its calls"),
        "{stderr}"
    );
}

/// The `Display` of the error `main` returns is a step, taken after
/// `main` has returned: a limit it passes stops the program there, with
/// `P0038` spanning `main`'s body and the error not printed. Native code
/// counts it the same (`both_backends`).
#[test]
fn the_display_of_mains_error_is_a_step_after_main_returns() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/run_limits/steps/main_error.osk"),
    )
    .expect("the fixture");
    let dir = program("steps_main_error_display", &src);
    let output = oskac_in(&dir, &["run", "--interpret", "--max-steps=1", "main.osk"]);
    assert_eq!(output.status.code(), Some(101));
    assert!(output.stdout.is_empty());
    assert_golden(
        "steps_main_error.txt",
        &String::from_utf8_lossy(&output.stderr),
    );
}

// ---- Both backends (the differential suite) -----------------------------------

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod differential;

/// The same program under the same step limit stops at the same step,
/// with the same output, report and exit code, natively and on the
/// interpreter (ADR 0021).
#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod both_backends {
    use super::differential::{assert_same, Case};
    use super::{program, step_fixtures, steps_written_in, HELLO, LOOPS_FOREVER};

    /// Every fixture, under every limit from 1 step to the steps it
    /// takes: each limit below stops it at the same step on both
    /// backends, and its own count lets it run to its end.
    #[test]
    fn each_fixture_stops_at_the_same_step_on_both_backends() {
        for (name, src) in step_fixtures() {
            let steps = steps_written_in(&name, &src);
            for limit in 1..=steps {
                let flag = format!("--max-steps={limit}");
                let case =
                    Case::file(&format!("steps_{name}_{limit}"), &src).with_run_flags(&[&flag]);
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
        let case =
            Case::file("steps_endless", LOOPS_FOREVER).with_run_flags(&["--max-steps=1000000"]);
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
        let case = Case::file("steps_json", LOOPS_FOREVER)
            .with_run_flags(&["--max-steps=3", "--format=json"]);
        let outcome = assert_same(&case);
        assert!(
            outcome.stderr.starts_with("{\"code\":\"P0038\""),
            "{}",
            outcome.stderr
        );
    }

    /// Both limits at once: whichever the program reaches first stops it,
    /// on both backends alike.
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

    /// The build cache keeps a run with `--max-steps` apart from one
    /// without it, in either order, and from a run with another limit:
    /// without it, `oskac run` runs the executable `oskac build` writes,
    /// byte for byte; with it, another.
    #[test]
    fn a_run_with_the_step_limit_is_kept_apart_in_the_build_cache() {
        for (order, runs) in [
            ("unlimited_first", [&[][..], &["--max-steps=64"][..]]),
            ("limited_first", [&["--max-steps=64"][..], &[][..]]),
        ] {
            let dir = program(&format!("steps_cache_{order}"), HELLO);
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
                output.stdout
            };
            // The cache's entries, in `v1/entries/` (beside its counts of
            // hits and misses).
            let entries = || -> Vec<Vec<u8>> {
                let Ok(listing) = std::fs::read_dir(cache.join("v1/entries")) else {
                    return Vec::new();
                };
                listing
                    .map(|entry| std::fs::read(entry.expect("an entry").path()).expect("its bytes"))
                    .collect()
            };
            let run = |flags: &[&str]| {
                let mut args = vec!["run"];
                args.extend_from_slice(flags);
                args.push("main.osk");
                assert_eq!(with_cache(&args), b"hello\n", "{args:?}");
            };
            run(runs[0]);
            let after_first = entries().len();
            run(runs[1]);
            let after_both = entries().len();
            assert_eq!(after_both, after_first + 1, "{order}: a new entry");
            // Each run again: both hit their own entry.
            run(runs[0]);
            run(runs[1]);
            assert_eq!(entries().len(), after_both, "{order}: no new entry");
            // Another limit is another entry still.
            run(&["--max-steps=65"]);
            assert_eq!(entries().len(), after_both + 1, "{order}: another limit");
            with_cache(&["build", "main.osk", "-o", "built"]);
            let built = std::fs::read(dir.join("built")).expect("the built executable");
            let all = entries();
            let holding_built = all.iter().filter(|e| e.ends_with(&built)).count();
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
}
