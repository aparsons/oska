// SPDX-License-Identifier: Apache-2.0
//! The time limit (spec #446, ticket #455): `oskac run --timeout=<ms>`
//! stops a program after that many milliseconds of wall-clock time, also
//! while it waits for input, with `P0039` and exit code 101, on the
//! interpreter and natively. Natively, `oskac` stops its own child
//! process at the deadline and prints the report itself.
//!
//! A timeout is not deterministic: these tests check that the program
//! stops, with what report, and when, never at which step. The promise
//! (`docs/runtime.md`, Run limits) is a stop within [`TOLERANCE_MS`] of the
//! deadline. A test asserts it with [`LOADED_MACHINE_MS`] more, since CI
//! runners and shared machines are loaded and a test that measures time
//! must not flake.
//!
//! The expected output is in `tests/golden/run_limits/`. To regenerate it
//! after an intended change, run `UPDATE_GOLDEN=1 cargo test -p oskac
//! --test run_limits_timeout` and review the diff.

mod common;

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::frames::without_frames;
use common::run_limits::{assert_golden, oskac_in, program, transcript, HELLO};

/// How long after its deadline a program may stop, as `docs/runtime.md`
/// documents it.
const TOLERANCE_MS: u64 = 100;

/// What a test allows past [`TOLERANCE_MS`]: the slack of a loaded
/// machine, where `oskac`'s own exit and the test's wake-up are late.
const LOADED_MACHINE_MS: u64 = 2000;

/// Loops without end, and takes no other step.
const LOOPS_FOREVER: &str = "fn main() {\n    while true {}\n}\n";

/// The backends a timeout is tested on: the interpreter, and the default,
/// native where there is a backend for the host.
const BACKENDS: [&[&str]; 2] = [&["--interpret"], &[]];

/// `args` after `run`, with the `backend` flags.
fn run_args<'a>(backend: &[&'a str], rest: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["run"];
    args.extend_from_slice(backend);
    args.extend_from_slice(rest);
    args
}

#[test]
fn a_program_that_loops_without_end_stops_at_the_deadline_with_p0039() {
    let dir = program("timeout_loop", LOOPS_FOREVER);
    for backend in BACKENDS {
        let output = oskac_in(&dir, &run_args(backend, &["--timeout=300", "main.osk"]));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(101), "{backend:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{backend:?}");
        assert!(
            stderr.contains("error[P0039]: time limit reached"),
            "{backend:?}: {stderr}"
        );
        assert_eq!(
            without_frames(&stderr).lines().count(),
            1,
            "{backend:?}: one report, {stderr}"
        );
    }
}

/// Prints the time it starts, in milliseconds since the Unix epoch, then
/// loops without end. It prints to stderr: native stdout is buffered
/// while it is not a terminal, and what a stopped program had not yet
/// written out is lost.
const PRINTS_START_THEN_LOOPS: &str = r#"fn main(err: Stderr, clock: Clock) {
    let now = clock.now_millis()
    let _ = err.println("{now}")
    while true {}
}
"#;

/// Prints the time it starts, then waits for a line of input.
const PRINTS_START_THEN_WAITS: &str = r#"fn main(io: Stdout, clock: Clock, input: Stdin) {
    let now = clock.now_millis()
    let _ = io.println("{now}")
    let _ = input.read_line()
}
"#;

/// Milliseconds since the Unix epoch, now.
fn now_millis() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970");
    u64::try_from(since.as_millis()).expect("in range")
}

/// When `oskac` was started and when it had ended, in milliseconds since
/// the Unix epoch, as the test saw them.
struct Span {
    spawned: u64,
    ended: u64,
}

/// Run `oskac <args>` in `dir` with a standard input that stays open and
/// sends nothing, and return its exit code, stdout and stderr, and when
/// it was started and had ended.
fn run_with_open_stdin(dir: &Path, args: &[&str]) -> (Option<i32>, String, String, Span) {
    let spawned = now_millis();
    let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .env_remove("OSKA_CACHE_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oskac should start");
    // Held, never written or closed, until oskac has ended.
    let stdin = child.stdin.take();
    let mut stdout = child.stdout.take().expect("a stdout");
    let mut stderr = child.stderr.take().expect("a stderr");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut out = String::new();
    let _ = stdout.read_to_string(&mut out);
    let status = child.wait().expect("oskac should end");
    let ended = now_millis();
    drop(stdin);
    let span = Span { spawned, ended };
    (status.code(), out, errors.join().expect("stderr"), span)
}

/// Assert that `oskac`, run over `span`, with a program that printed the
/// time it started on the first line of `printed`, ran no less than the
/// timeout `ms` and stopped within the tolerance after it.
fn assert_stopped_in_time(what: &str, printed: &str, span: &Span, ms: u64) {
    // The deadline starts after the test starts `oskac`, so `oskac` can
    // never end sooner than `ms` after that, however slowly the program
    // starts. (The program's own start time is no lower bound: a loaded
    // machine can take more than 50 ms between `oskac` starting the
    // deadline and the program reading the clock.) 1 ms covers the two
    // clock readings each rounding down.
    let lived = span.ended.saturating_sub(span.spawned);
    assert!(
        lived + 1 >= ms,
        "{what}: oskac ended {lived} ms after it started, before the deadline of {ms} ms"
    );
    // The program starts after the deadline does, so the time since it
    // started is at most the time since the deadline started.
    let started: u64 = printed
        .lines()
        .next()
        .and_then(|l| l.parse().ok())
        .unwrap_or_else(|| panic!("{what}: no start time in {printed:?}"));
    let ran = span.ended.saturating_sub(started);
    assert!(
        ran <= ms + TOLERANCE_MS + LOADED_MACHINE_MS,
        "{what}: stopped {ran} ms after the program started, {} ms past its deadline",
        ran.saturating_sub(ms)
    );
}

#[test]
fn a_looping_program_stops_within_the_tolerance_of_its_deadline() {
    let dir = program("timeout_in_time", PRINTS_START_THEN_LOOPS);
    for backend in BACKENDS {
        let args = run_args(backend, &["--timeout=500", "main.osk"]);
        let (code, _, stderr, span) = run_with_open_stdin(&dir, &args);
        assert_eq!(code, Some(101), "{backend:?}: {stderr}");
        assert!(stderr.contains("error[P0039]"), "{backend:?}: {stderr}");
        assert_stopped_in_time(&format!("{backend:?}"), &stderr, &span, 500);
    }
}

#[test]
fn a_program_that_waits_for_input_stops_at_the_deadline_with_p0039() {
    let dir = program("timeout_waits", PRINTS_START_THEN_WAITS);
    for backend in BACKENDS {
        let args = run_args(backend, &["--timeout=500", "main.osk"]);
        let (code, stdout, stderr, span) = run_with_open_stdin(&dir, &args);
        assert_eq!(code, Some(101), "{backend:?}: {stderr}");
        assert!(
            stderr.contains("error[P0039]: time limit reached"),
            "{backend:?}: {stderr}"
        );
        assert_eq!(
            without_frames(&stderr).lines().count(),
            1,
            "{backend:?}: one report, {stderr}"
        );
        assert_stopped_in_time(&format!("{backend:?}"), &stdout, &span, 500);
    }
}

/// Fails with an error after it has written a line.
const FAILS: &str = r#"fn main(io: Stdout) -> Result<(), String> {
    let _ = io.println("before the error")
    Err("it failed")
}
"#;

#[test]
fn a_program_that_ends_before_its_deadline_gives_its_own_output_and_exit_code() {
    let hello = program("timeout_hello", HELLO);
    let fails = program("timeout_fails", FAILS);
    for backend in BACKENDS {
        let started = Instant::now();
        let output = oskac_in(&hello, &run_args(backend, &["--timeout=60000", "main.osk"]));
        assert_eq!(output.status.code(), Some(0), "{backend:?}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
        assert!(output.stderr.is_empty(), "{backend:?}: {output:?}");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{backend:?}: it ended as it was done, not at its deadline"
        );

        // The largest timeout is a deadline too far to reach, not an
        // overflow.
        let longest = oskac_in(
            &hello,
            &run_args(backend, &["--timeout=9223372036854775807", "main.osk"]),
        );
        assert_eq!(longest.status.code(), Some(0), "{backend:?}: {longest:?}");
        assert_eq!(String::from_utf8_lossy(&longest.stdout), "hello\n");

        let limited = oskac_in(&fails, &run_args(backend, &["--timeout=60000", "main.osk"]));
        let unlimited = oskac_in(&fails, &run_args(backend, &["main.osk"]));
        assert_eq!(limited.status.code(), Some(1), "{backend:?}: {limited:?}");
        assert_eq!(limited.stdout, unlimited.stdout, "{backend:?}");
        assert_eq!(limited.stderr, unlimited.stderr, "{backend:?}");
    }
}

/// Whether `oskac run` runs a program natively on this host by default.
const NATIVE: bool = cfg!(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
));

#[test]
fn the_time_limit_report_is_the_usual_panic_report_or_one_json_object() {
    let dir = program("timeout_report", LOOPS_FOREVER);
    let mut all = String::new();
    for format in [&[][..], &["--format=json"]] {
        let mut rest = vec!["--timeout=200"];
        rest.extend_from_slice(format);
        rest.push("main.osk");
        let output = oskac_in(&dir, &run_args(&["--interpret"], &rest));
        assert_eq!(output.status.code(), Some(101), "{output:?}");
        all.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    assert_golden("timeout_interpreter.txt", &all);
}

/// Natively, `oskac` stops the program and reports the limit itself: it
/// does not know where the program was, so the report has no place.
#[test]
fn a_native_program_stopped_at_its_deadline_is_reported_by_oskac_with_101() {
    if !NATIVE {
        // No native backend for this host: `run` interprets, as tested
        // above.
        return;
    }
    let dir = program("timeout_native", LOOPS_FOREVER);
    let mut all = String::new();
    for format in [&[][..], &["--format=json"]] {
        let mut rest = vec!["--timeout=200"];
        rest.extend_from_slice(format);
        rest.push("main.osk");
        let output = oskac_in(&dir, &run_args(&[], &rest));
        assert_eq!(output.status.code(), Some(101), "never 128 + n: {output:?}");
        all.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    assert_golden("timeout_native.txt", &all);
}

#[test]
fn a_timeout_of_zero_a_negative_number_or_no_number_is_a_usage_error() {
    let dir = program("timeout_malformed", HELLO);
    let mut all = String::new();
    for value in [
        "0",
        "-1",
        "abc",
        "",
        "1.5",
        "+5",
        "1s",
        "99999999999999999999",
    ] {
        let flag = format!("--timeout={value}");
        let line = transcript(&dir, &["run", "main.osk", &flag]);
        assert!(line.contains("--- exit Some(2)"), "{line}");
        all.push_str(&line);
    }
    let line = transcript(&dir, &["run", "main.osk", "--timeout=5", "--timeout=5"]);
    assert!(line.contains("--- exit Some(2)"), "{line}");
    all.push_str(&line);
    assert_golden("malformed_timeout.txt", &all);
}

#[test]
fn a_timeout_on_any_command_but_run_is_a_usage_error() {
    let dir = program("timeout_other_commands", HELLO);
    for command in ["check", "test", "build", "fix"] {
        let output = oskac_in(&dir, &[command, "main.osk", "--timeout=1000"]);
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("oskac "),
            "{command}: the usage, {output:?}"
        );
    }
}

#[test]
fn explain_p0039_explains_the_time_limit() {
    let output = oskac_in(Path::new(env!("CARGO_MANIFEST_DIR")), &["explain", "P0039"]);
    assert_eq!(output.status.code(), Some(0));
    assert_golden(
        "explain_p0039.txt",
        &String::from_utf8_lossy(&output.stdout),
    );
}

/// Security: at the deadline `oskac` stops only its own child. Another
/// program that waits, started by this test beside it (in the same
/// process group), still runs afterwards.
#[test]
fn attack_the_timeout_stops_only_oskacs_own_child() {
    let dir = program("timeout_only_child", PRINTS_START_THEN_WAITS);
    let mut bystander = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["run", "main.osk"])
        .current_dir(&*dir)
        .env_remove("OSKA_CACHE_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bystander should start");
    for backend in BACKENDS {
        let (code, _, stderr, _) =
            run_with_open_stdin(&dir, &run_args(backend, &["--timeout=300", "main.osk"]));
        assert_eq!(code, Some(101), "{backend:?}: {stderr}");
    }
    let still_running = bystander
        .try_wait()
        .expect("the bystander's status")
        .is_none();
    // This test's own child, by its handle.
    let _ = bystander.kill();
    let _ = bystander.wait();
    assert!(still_running, "the bystander was stopped too");
}

/// Prints each number it counts to, without end: one back-edge a line.
const COUNTS: &str = r#"fn main(io: Stdout) {
    var i = 0
    while true {
        let _ = io.println("{i}")
        i = i + 1
    }
}
"#;

/// Under `--timeout` the interpreter looks at the clock every so many
/// steps; `--max-steps` with it still stops at exactly the same step,
/// with the same output, across many such looks.
#[test]
fn the_step_limit_stops_at_the_same_step_under_a_timeout() {
    let dir = program("timeout_and_steps", COUNTS);
    for steps in ["1", "1023", "1024", "1025", "5000"] {
        let flag = format!("--max-steps={steps}");
        let alone = oskac_in(&dir, &["run", "--interpret", &flag, "main.osk"]);
        let timed = oskac_in(
            &dir,
            &["run", "--interpret", &flag, "--timeout=600000", "main.osk"],
        );
        assert_eq!(alone.status.code(), Some(101), "{steps}: {alone:?}");
        assert_eq!(timed.status.code(), Some(101), "{steps}: {timed:?}");
        assert_eq!(timed.stdout, alone.stdout, "{steps}");
        assert_eq!(timed.stderr, alone.stderr, "{steps}");
        assert!(String::from_utf8_lossy(&timed.stderr).contains("error[P0038]"));
    }
}
