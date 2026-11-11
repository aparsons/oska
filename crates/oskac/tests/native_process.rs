//! Native process capabilities (ticket #136), run on both backends
//! through the differential harness (seam 1): `Stdout`, `Stderr`,
//! `Stdin`, `Args`, `Env` and `Env.secret`, `Clock`, `Random`, `dbg` and
//! `todo`. The clock and the generator are compared on properties that
//! do not depend on the time or the seed.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, build_native, Case};
use std::process::Command;

/// Run `case` on both backends, require the same outcome, and that it
/// printed `stdout`, wrote `stderr` and exited with `code`.
fn assert_outcome(case: &Case, stdout: &str, stderr: &str, code: i32) {
    let outcome = assert_same(case);
    assert_eq!(outcome.stdout, stdout, "{outcome:#?}");
    assert_eq!(outcome.stderr, stderr, "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(code), "{outcome:#?}");
}

// ---- Standard streams -------------------------------------------------------

#[test]
fn stdout_and_stderr_lines_are_written_as_the_interpreter_writes_them() {
    let case = Case::file(
        "process_streams",
        r#"fn main(out: Stdout, err: Stderr) {
    let _ = out.println("one")
    let _ = err.println("to stderr")
    let _ = out.println("")
    let _ = out.println("héllo 日本語")
    let _ = err.println("")
    let _ = err.println("two\nlines")
}
"#,
    );
    assert_outcome(
        &case,
        "one\n\nhéllo 日本語\n",
        "to stderr\n\ntwo\nlines\n",
        0,
    );
}

#[test]
fn a_lot_of_output_goes_through_the_buffer_in_order() {
    let case = Case::file(
        "process_buffer",
        r#"fn main(io: Stdout) {
    var i = 0
    while i < 30000 {
        let _ = io.println("line {i} of the output")
        i = i + 1
    }
    let big = "x".repeat(200000)
    let _ = io.println(big)
    let _ = io.println("after")
}
"#,
    );
    let outcome = assert_same(&case);
    assert!(outcome.stdout.starts_with("line 0 of the output\n"));
    assert!(outcome.stdout.ends_with("\nafter\n"));
    assert!(outcome.stdout.len() > 200_000 + 30000 * 20);
    assert_eq!(outcome.exit_code, Some(0));
}

/// Standard output and error into one pipe, which is what a terminal
/// shows: the buffered output must come out before a stderr line.
fn interleaved(case: &Case) -> String {
    let (exe, dir) = build_native(case);
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{} 2>&1", exe.display()))
        .current_dir(dir)
        .env_clear()
        .output()
        .expect("the program should run");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn stdout_is_flushed_before_a_stderr_write() {
    let case = Case::file(
        "process_interleaved",
        r#"fn main(out: Stdout, err: Stderr) {
    let _ = out.println("a")
    let _ = err.println("b")
    let _ = out.println("c")
    let _ = out.println("d")
    let _ = err.println("e")
    let _ = out.println("f")
}
"#,
    );
    assert_eq!(interleaved(&case), "a\nb\nc\nd\ne\nf\n");
}

#[test]
fn stdout_is_flushed_before_a_panic_report_and_a_failed_main() {
    let panics = Case::file(
        "process_panic_flush",
        r#"fn main(out: Stdout) {
    let _ = out.println("before")
    let zero = 0
    let quotient = 1 / zero
    let _ = out.println("{quotient}")
}
"#,
    );
    let shown = interleaved(&panics);
    assert!(
        shown.starts_with("before\n") && shown.contains("error[P0004]"),
        "{shown}"
    );
    let failing = Case::file(
        "process_failed_main",
        r#"fn main(out: Stdout) -> Result<(), String> {
    let _ = out.println("before")
    Err("it failed")
}
"#,
    );
    assert_eq!(interleaved(&failing), "before\nit failed\n");
    assert_outcome(&failing, "before\n", "it failed\n", 1);
}

// ---- Standard input ---------------------------------------------------------

const ECHO: &str = r#"fn main(io: Stdout, input: Stdin) {
    var n = 0
    var more = true
    while more {
        match input.read_line() {
            Ok(Some(line)) => {
                n = n + 1
                let _ = io.println(line)
            }
            Ok(None) => {
                more = false
            }
            Err(e) => {
                more = false
                let _ = io.println("error")
            }
        }
    }
    let _ = io.println("{n} lines")
}
"#;

#[test]
fn stdin_lines_are_read_as_the_interpreter_reads_them() {
    for (name, stdin) in [
        ("process_stdin_plain", "one\ntwo\nthree\n"),
        ("process_stdin_no_final_newline", "one\ntwo"),
        ("process_stdin_empty", ""),
        ("process_stdin_blank_lines", "\n\na\n\n"),
        ("process_stdin_crlf", "a\r\nb\r\n\r\nc"),
        ("process_stdin_lone_cr", "a\rb\nc\r"),
        ("process_stdin_unicode", "héllo\n日本語\n😀\n"),
        ("process_stdin_only_newline", "\n"),
    ] {
        let case = Case::file(name, ECHO).with_stdin(stdin);
        let outcome = assert_same(&case);
        assert_eq!(outcome.exit_code, Some(0), "{name}: {outcome:#?}");
    }
}

#[test]
fn stdin_lines_longer_than_the_buffer_and_many_of_them() {
    let long = "z".repeat(150_000);
    let many: String = (0..20_000).map(|i| format!("row {i}\n")).collect();
    for (name, stdin) in [
        ("process_stdin_long", format!("{long}\nshort\n{long}")),
        ("process_stdin_many", many),
    ] {
        let case = Case::file(name, ECHO).with_stdin(&stdin);
        let outcome = assert_same(&case);
        assert!(outcome.stdout.ends_with(" lines\n"), "{name}");
        assert_eq!(outcome.exit_code, Some(0), "{name}");
    }
}

// ---- Arguments --------------------------------------------------------------

const SHOW_ARGS: &str = r#"fn main(io: Stdout, args: Args) {
    var i = 1
    var more = true
    while more {
        match args.get(i) {
            Some(arg) => {
                let _ = io.println("[{i}] ")
                let _ = io.println(arg)
                i = i + 1
            }
            None => {
                more = false
            }
        }
    }
    let _ = io.println("end at {i}")
    match args.get(0) {
        Some(_) => {
            let _ = io.println("program path present")
        }
        None => {
            let _ = io.println("no program path")
        }
    }
    match args.get(-1) {
        Some(_) => {
            let _ = io.println("negative index found")
        }
        None => {
            let _ = io.println("negative index missing")
        }
    }
}
"#;

#[test]
fn arguments_after_the_program_path_arrive_unchanged() {
    for (name, args) in [
        ("process_args_none", vec![]),
        (
            "process_args_some",
            vec!["a", "b c", "", "--flag=1", "-x", "日本語", "--"],
        ),
    ] {
        let case = Case::file(name, SHOW_ARGS).with_args(&args);
        let outcome = assert_same(&case);
        assert!(outcome.stdout.contains("program path present"), "{name}");
        assert_eq!(outcome.exit_code, Some(0), "{name}");
    }
}

// ---- Environment ------------------------------------------------------------

#[test]
fn environment_variables_are_read_only_when_the_program_asks() {
    let src = r#"fn show(io: Stdout, env: Env, name: String) {
    match env.get(name) {
        Some(value) => {
            let _ = io.println("{name} is set to:")
            let _ = io.println(value)
        }
        None => {
            let _ = io.println("{name} is not set")
        }
    }
}

fn main(io: Stdout, env: Env) {
    show(io, env, "GREETING")
    show(io, env, "EMPTY")
    show(io, env, "GREET")
    show(io, env, "GREETINGS")
    show(io, env, "MISSING")
    show(io, env, "")
    show(io, env, "A=B")
    show(io, env, "WITH_EQUALS")
    show(io, env, "UNICODE")
}
"#;
    let case = Case::file("process_env", src)
        .with_env("GREETING", "hello world")
        .with_env("EMPTY", "")
        .with_env("GREETINGS", "plural")
        .with_env("WITH_EQUALS", "a=b=c")
        .with_env("UNICODE", "日本語 é")
        .with_env("UNUSED_SECRET_TOKEN", "do not read me");
    let outcome = assert_same(&case);
    assert!(outcome
        .stdout
        .contains("GREETING is set to:\nhello world\n"));
    assert!(outcome.stdout.contains("EMPTY is set to:\n\n"));
    assert!(outcome.stdout.contains("GREET is not set"));
    assert!(outcome.stdout.contains("WITH_EQUALS is set to:\na=b=c\n"));
    assert!(!outcome.stdout.contains("do not read me"));
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn a_program_without_env_reads_nothing_from_the_environment() {
    let case = Case::file(
        "process_no_env",
        "fn main(io: Stdout) {\n    let _ = io.println(\"hi\")\n}\n",
    )
    .with_env("OSKA_TEST", "1");
    assert_outcome(&case, "hi\n", "", 0);
}

#[test]
fn secrets_are_read_exposed_and_compared() {
    let src = r#"fn main(io: Stdout, env: Env) {
    let token = env.secret("TOKEN")
    let other = env.secret("OTHER")
    let same = env.secret("SAME")
    let _ = io.println(token.expose())
    let _ = if token.constant_time_eq("s3cret") { io.println("eq") } else { io.println("ne") }
    let _ = if token.constant_time_eq("s3cre") { io.println("eq") } else { io.println("ne") }
    let _ = if token.constant_time_eq("s3crex") { io.println("eq") } else { io.println("ne") }
    let _ = if token.constant_time_eq_secret(other) { io.println("eq") } else { io.println("ne") }
    let _ = if token.constant_time_eq_secret(same) { io.println("eq") } else { io.println("ne") }
}
"#;
    let case = Case::file("process_secret", src)
        .with_env("TOKEN", "s3cret")
        .with_env("OTHER", "other")
        .with_env("SAME", "s3cret");
    assert_outcome(&case, "s3cret\neq\nne\nne\nne\neq\n", "", 0);
}

#[test]
fn a_missing_secret_panics_with_p0020() {
    let src = r#"fn main(io: Stdout, env: Env) {
    let _ = io.println("start")
    let token = env.secret("NO_SUCH_TOKEN")
    let _ = io.println(token.expose())
}
"#;
    let outcome = assert_same(&Case::file("process_missing_secret", src));
    assert_eq!(outcome.stdout, "start\n");
    assert!(
        outcome.stderr.contains(
            "error[P0020]: the secret `NO_SUCH_TOKEN` is not set in the environment (bytes"
        ),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101));
}

// ---- Clock and randomness ---------------------------------------------------

#[test]
fn the_clock_tells_the_time_and_does_not_run_backwards() {
    let src = r#"fn main(io: Stdout, clock: Clock) {
    let a = clock.now_millis()
    let b = clock.now_millis()
    let _ = io.println(if a > 1600000000000 { "after 2020" } else { "too early" })
    let _ = io.println(if a < 4000000000000 { "before 2096" } else { "too late" })
    let _ = io.println(if b >= a { "monotone" } else { "backwards" })
}
"#;
    assert_outcome(
        &Case::file("process_clock", src),
        "after 2020\nbefore 2096\nmonotone\n",
        "",
        0,
    );
}

#[test]
fn random_draws_are_in_range_and_vary() {
    let src = r#"fn main(io: Stdout, random: Random) {
    let first = random.next_int()
    var in_range = true
    var different = 0
    var i = 0
    while i < 200 {
        let n = random.next_int()
        if n < 0 {
            in_range = false
        }
        if n != first {
            different = different + 1
        }
        i = i + 1
    }
    let _ = io.println(if in_range { "in range" } else { "negative" })
    let _ = io.println(if different > 190 { "varied" } else { "constant" })
    let _ = io.println(if first >= 0 { "first ok" } else { "first negative" })
}
"#;
    assert_outcome(
        &Case::file("process_random", src),
        "in range\nvaried\nfirst ok\n",
        "",
        0,
    );
}

#[test]
fn random_is_seeded_differently_by_each_run() {
    let case = Case::file(
        "process_random_seed",
        "fn main(io: Stdout, random: Random) {\n    let n = random.next_int()\n    let _ = io.println(\"{n}\")\n}\n",
    );
    let (exe, dir) = build_native(&case);
    let draw = || {
        let output = Command::new(&exe)
            .current_dir(&dir)
            .output()
            .expect("the program should run");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let draws: Vec<String> = (0..4).map(|_| draw()).collect();
    assert!(
        draws.iter().any(|d| *d != draws[0]),
        "four runs drew the same number: {draws:?}"
    );
}

/// The interpreter's SplitMix64, to check the native generator against.
fn splitmix_after(seed: u64, draws: usize) -> u64 {
    let mut state = seed;
    let mut last = 0;
    for _ in 0..draws {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        last = z ^ (z >> 31);
    }
    last >> 1
}

#[test]
fn random_is_splitmix64_from_some_seed() {
    // Two draws expose the state: the second follows the first by one
    // step. Recover the seed by search is not possible, so check the
    // relation instead: running the generator from the first draw's own
    // inverse is not needed, only that each output is a SplitMix64 output
    // of consecutive states, which the draws' low bits must show: the
    // outputs of one run must equal the algorithm's from the seed that
    // `getrandom` gave. The native test prints the first three draws and
    // the check below recomputes them from every state the first draw
    // could come from (the state is the seed plus the increment, so a
    // seed is recovered by inverting the mixer).
    let case = Case::file(
        "process_random_algorithm",
        "fn main(io: Stdout, random: Random) {\n    let a = random.next_int()\n    let b = random.next_int()\n    let _ = io.println(\"{a} {b}\")\n}\n",
    );
    let (exe, dir) = build_native(&case);
    let output = Command::new(&exe)
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let draws: Vec<u64> = text
        .split_whitespace()
        .map(|n| n.parse().expect("a number"))
        .collect();
    assert_eq!(draws.len(), 2);
    // Invert the output function of the first draw, up to its dropped low
    // bit (two candidates), to get the first state, then step it.
    let mut found = false;
    for low in 0..2u64 {
        let out = (draws[0] << 1) | low;
        let state = invert_mix(out);
        let seed = state.wrapping_sub(0x9E37_79B9_7F4A_7C15);
        if splitmix_after(seed, 2) == draws[1] {
            found = true;
        }
    }
    assert!(found, "the draws {draws:?} are not SplitMix64's");
}

/// The state a SplitMix64 output came from: the finaliser's inverse.
fn invert_mix(out: u64) -> u64 {
    fn unxorshift(x: u64, shift: u32) -> u64 {
        let mut result = x;
        let mut done = shift;
        while done < 64 {
            result = x ^ (result >> shift);
            done += shift;
        }
        result
    }
    // The multiplicative inverses mod 2^64 of the two constants.
    fn inverse(a: u64) -> u64 {
        let mut x = a;
        for _ in 0..6 {
            x = x.wrapping_mul(2u64.wrapping_sub(a.wrapping_mul(x)));
        }
        x
    }
    let mut z = unxorshift(out, 31);
    z = z.wrapping_mul(inverse(0x94D0_49BB_1331_11EB));
    z = unxorshift(z, 27);
    z = z.wrapping_mul(inverse(0xBF58_476D_1CE4_E5B9));
    unxorshift(z, 30)
}

// ---- dbg and todo -----------------------------------------------------------

#[test]
fn dbg_shows_a_value_on_stderr_and_returns_it() {
    let src = r#"struct Point { x: Int, y: Int }

struct Label { text: String, at: Point, ok: Bool }

struct Empty {}

enum Shape {
    Circle(Int),
    Rect { w: Int, h: Int },
    Dot,
}

enum Plain {
    A,
    B,
}

enum Tree {
    Leaf,
    Node(Tree, Int, Tree),
}

fn main(io: Stdout) {
    let n = dbg(42)
    let neg = dbg(-7)
    let s = dbg("hi \"there\"\n\tback\\slash")
    let b = dbg(true)
    let u = dbg(())
    let p = dbg(Point(x: 1, y: -2))
    let l = dbg(Label(text: "é日", at: p, ok: false))
    let e = dbg(Empty())
    let shapes = dbg(Shape.Circle(3))
    let rect = dbg(Shape.Rect(w: 4, h: 5))
    let dot = dbg(Shape.Dot)
    let plain = dbg(Plain.B)
    let some = dbg(Some(5))
    let none: Option<Int> = dbg(None)
    let ok: Result<Int, String> = dbg(Ok(1))
    let err: Result<Int, String> = dbg(Err("bad"))
    let tree = dbg(Tree.Node(Tree.Node(Tree.Leaf, 1, Tree.Leaf), 2, Tree.Leaf))
    let c = dbg("a\0b\r")
    let _ = io.println("{n} {neg}")
    let _ = io.println(s)
}
"#;
    let outcome = assert_same(&Case::file("process_dbg", src));
    assert!(
        outcome.stderr.starts_with("[dbg] 42\n[dbg] -7\n"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn dbg_redacts_a_secret() {
    let src = r#"fn main(io: Stdout, env: Env) {
    let token = env.secret("TOKEN")
    let shown = dbg(token)
    let _ = io.println(shown.expose())
}
"#;
    let case = Case::file("process_dbg_secret", src).with_env("TOKEN", "hunter2");
    let outcome = assert_same(&case);
    assert_eq!(outcome.stderr, "[dbg] <redacted>\n");
    assert_eq!(outcome.stdout, "hunter2\n");
}

#[test]
fn dbg_output_follows_what_stdout_printed_before_it() {
    let case = Case::file(
        "process_dbg_order",
        "fn main(io: Stdout) {\n    let _ = io.println(\"first\")\n    let n = dbg(1)\n    let _ = io.println(\"second {n}\")\n}\n",
    );
    assert_eq!(interleaved(&case), "first\n[dbg] 1\nsecond 1\n");
}

#[test]
fn todo_panics_with_p0003() {
    let src = r#"fn later(n: Int) -> String {
    if n > 0 {
        return "written"
    }
    todo()
}

fn main(io: Stdout) {
    let _ = io.println(later(1))
    let _ = io.println(later(0))
    let _ = io.println("not reached")
}
"#;
    let outcome = assert_same(&Case::file("process_todo", src));
    assert_eq!(outcome.stdout, "written\n");
    assert!(
        outcome
            .stderr
            .contains("error[P0003]: reached `todo()`: this code is not written yet (bytes"),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101));
}

// ---- What stays unsupported ---------------------------------------------------

/// `oskac build` of `src`: its exit code and stderr.
fn build_refused(name: &str, src: &str) -> (Option<i32>, String) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("native_process_refusals")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    std::fs::write(dir.join("main.osk"), src).expect("the source");
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["build", "main.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn what_needs_other_tickets_is_refused_cleanly() {
    let (name, src, what) = (
        "main_net",
        "fn main(net: Net) {\n}\n",
        "a `main` given `Net`",
    );
    let (code, stderr) = build_refused(name, src);
    assert_eq!(code, Some(2), "{name}: {stderr}");
    assert!(stderr.contains("error[E0390]"), "{name}: {stderr}");
    assert!(stderr.contains(what), "{name}: {stderr}");
}

// ---- Bytes that are not UTF-8 ---------------------------------------------------

/// What one run with raw bytes did.
type Ran = (Vec<u8>, Vec<u8>, Option<i32>);

/// Run `case` on both backends with `stdin` bytes, `env` and `args` that
/// need not be UTF-8, and require the same stdout, stderr and exit code.
fn assert_same_bytes(
    case: &Case,
    stdin: &[u8],
    env: &[(&str, &[u8])],
) -> (String, String, Option<i32>) {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Stdio;
    let (exe, dir) = build_native(case);
    let run = |mut command: Command| -> Ran {
        command
            .current_dir(&dir)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            command.env(name, std::ffi::OsStr::from_bytes(value));
        }
        let mut child = command.spawn().expect("the program should start");
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(stdin);
        }
        let output = child.wait_with_output().expect("the program should end");
        (output.stdout, output.stderr, output.status.code())
    };
    let native = run(Command::new(&exe));
    let mut interpreter = Command::new(env!("CARGO_BIN_EXE_oskac"));
    interpreter.args(["run", "main.osk"]);
    let interpreted = run(interpreter);
    assert_eq!(native, interpreted, "the backends differ");
    (
        String::from_utf8_lossy(&native.0).into_owned(),
        String::from_utf8_lossy(&native.1).into_owned(),
        native.2,
    )
}

#[test]
fn stdin_that_is_not_utf8_fails_the_read_as_the_interpreter_does() {
    let case = Case::file("process_bad_stdin", ECHO);
    for (name, bytes) in [
        ("lone continuation", &b"ok\n\x80\nafter\n"[..]),
        ("truncated sequence", b"a\n\xE6\x97\nafter\n"),
        ("overlong", b"\xC0\xAF\nafter\n"),
        ("surrogate", b"\xED\xA0\x80\nafter\n"),
        ("above U+10FFFF", b"\xF4\x90\x80\x80\nafter\n"),
        ("bad last line", b"fine\n\xFF"),
        ("lead at the end", b"fine\n\xE6"),
        ("valid", "日本\n😀\n".as_bytes()),
        ("edges", b"\xF4\x8F\xBF\xBF\n\xED\x9F\xBF\n\xEF\xBF\xBF\n"),
    ] {
        let (stdout, _, code) = assert_same_bytes(&case, bytes, &[]);
        assert_eq!(code, Some(0), "{name}: {stdout}");
    }
}

#[test]
fn environment_values_that_are_not_utf8_are_not_set() {
    let src = r#"fn main(io: Stdout, env: Env) {
    match env.get("BAD") {
        Some(_) => {
            let _ = io.println("BAD is set")
        }
        None => {
            let _ = io.println("BAD is not set")
        }
    }
    match env.get("GOOD") {
        Some(value) => {
            let _ = io.println(value)
        }
        None => {
            let _ = io.println("GOOD is not set")
        }
    }
    let secret = env.secret("BAD")
    let _ = io.println(secret.expose())
}
"#;
    let case = Case::file("process_bad_env", src);
    let (stdout, stderr, code) =
        assert_same_bytes(&case, b"", &[("BAD", b"a\xFFb"), ("GOOD", "é".as_bytes())]);
    assert_eq!(stdout, "BAD is not set\né\n");
    assert!(stderr.contains("error[P0020]"), "{stderr}");
    assert_eq!(code, Some(101));
}

#[test]
fn an_argument_that_is_not_utf8_ends_the_program_with_101_on_both_backends() {
    use std::os::unix::ffi::OsStrExt;
    let case = Case::file(
        "process_bad_args",
        "fn main(io: Stdout) {\n    let _ = io.println(\"hi\")\n}\n",
    );
    let (native_exe, dir) = build_native(&case);
    let arg = std::ffi::OsStr::from_bytes(b"a\xFFb");
    let native = Command::new(native_exe)
        .arg(arg)
        .current_dir(&dir)
        .output()
        .expect("runs");
    let interpreted = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["run", "main.osk", "--"])
        .arg(arg)
        .current_dir(&dir)
        .output()
        .expect("runs");
    assert_eq!(native.status.code(), Some(101));
    assert_eq!(interpreted.status.code(), Some(101));
    assert!(native.stdout.is_empty() && interpreted.stdout.is_empty());
}

// ---- dbg escaping, for every character --------------------------------------------

/// The table in `std.rt` lists exactly the characters Rust's `{:?}`
/// writes as `\u{..}`.
#[test]
fn the_dbg_escape_table_is_rusts_for_every_character() {
    let source = oska_check::STD_RT_SOURCE;
    let head = "fn escaped_table() -> String {\n    \"";
    let start = source.find(head).expect("the table") + head.len();
    let table = &source[start..start + source[start..].find('"').expect("its end")];
    assert_eq!(table.len() % 12, 0);
    let hex = |from: usize| u32::from_str_radix(&table[from..from + 6], 16).expect("hex digits");
    let ranges: Vec<(u32, u32)> = (0..table.len() / 12)
        .map(|r| (hex(r * 12), hex(r * 12 + 6)))
        .collect();
    assert!(ranges.windows(2).all(|w| w[0].1 < w[1].0), "sorted, apart");
    for cp in 0..=0x10FFFFu32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let escaped = format!("{:?}", c.to_string())[1..].starts_with("\\u{");
        let listed = ranges.iter().any(|&(a, b)| a <= cp && cp <= b);
        assert_eq!(listed, escaped, "U+{cp:X}");
    }
}

#[test]
fn dbg_escapes_invisible_and_format_characters_as_the_interpreter_does() {
    // Zero-width and format characters, combining marks (which extend the
    // grapheme before them), separators, private use, controls, the
    // noncharacters, quotes and ordinary text.
    let src = "fn main(io: Stdout) {\n    \
        let a = dbg(\"zero\u{200B}width\u{200D}joiner\u{FEFF}bom\")\n    \
        let b = dbg(\"e\u{301} a\u{300}\u{20DD} \u{0915}\u{094D}\")\n    \
        let c = dbg(\"sep\u{2028}\u{2029}\u{00A0}\u{3000}\u{00AD}\")\n    \
        let d = dbg(\"\u{E000}\u{F8FF}\u{FFFF}\u{FDD0}\u{10FFFF}\u{E0001}\")\n    \
        let e = dbg(\"\u{7F}\u{80}\u{85}\u{9F}\u{A1}\u{1F600}\u{1F3FD}\")\n    \
        let f = dbg(\"it's \\\"q\\\" \\\\ \\t\\n\\r\\0 end\")\n    \
        let _ = io.println(a)\n    \
        let _ = io.println(b)\n    \
        let _ = io.println(c)\n    \
        let _ = io.println(d)\n    \
        let _ = io.println(e)\n    \
        let _ = io.println(f)\n}\n";
    let outcome = assert_same(&Case::file("process_dbg_unicode", src));
    assert!(outcome.stderr.contains("\\u{200b}"), "{outcome:#?}");
    assert!(outcome.stderr.contains("\\u{301}"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0));
}
