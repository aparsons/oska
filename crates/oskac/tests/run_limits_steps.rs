//! The step limit (spec #446, ticket #452): `oskac run --interpret
//! --max-steps=<n>` stops a program after `n` steps (ADR 0021) with
//! `P0038` and exit code 101. The fixtures in
//! `tests/fixtures/run_limits/steps/` each say, on their first line, how
//! many steps they take, and why. A malformed value, the flag on any
//! command but `run`, and the flag on a native run are usage errors (exit
//! code 2): native code does not count steps yet (#454).
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
            "error[P0038]: step limit reached: the program would take more than {} steps",
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

/// Native code does not count steps yet (#454): where `oskac run` would
/// run the program natively, `--max-steps` without `--interpret` is a
/// usage error that says to add it, and nothing runs. Where it runs the
/// interpreter (Windows), the flag works.
#[test]
fn max_steps_on_a_native_run_is_a_usage_error() {
    let dir = program("steps_native", LOOPS_FOREVER);
    let output = oskac_in(&dir, &["run", "--max-steps=10", "main.osk"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let native = cfg!(all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ));
    if native {
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert_eq!(
            stderr,
            "error: --max-steps works only on the interpreter for now: add --interpret to \
             run the program there\n"
        );
    } else {
        assert_eq!(output.status.code(), Some(101), "{stderr}");
        assert!(stderr.contains("error[P0038]"), "{stderr}");
    }
    assert!(output.stdout.is_empty());
}

/// `oskac run -` (#447) takes the step limit too.
#[test]
fn a_program_on_standard_input_runs_under_the_step_limit() {
    let dir = Scratch::new("run-limits", "steps_stdin");
    let args = ["run", "-", "--interpret", "--max-steps=7"];
    let output = oskac_with_stdin(&dir, &args, LOOPS_FOREVER);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{stderr}");
    assert!(
        stderr
            .starts_with("<stdin>:2:5: error[P0038]: step limit reached: the program would take more than 7 steps"),
        "{stderr}"
    );
}

/// The `Display` of the error `main` returns is a step, taken after
/// `main` has returned: a limit it passes stops the program there, with
/// `P0038` spanning `main`'s body and the error not printed. Native code
/// counts it the same (#454).
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
