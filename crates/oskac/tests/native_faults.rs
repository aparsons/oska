//! What a native program's buffered stdout does when the program cannot
//! end normally (ticket #143): a failed flush at exit is the program's
//! `IoError`, as the interpreter's unbuffered write is, and a fault
//! signal still shows what the program printed before its report. On
//! Windows, which has no signals, a stack overflow is an exception the
//! process ends with (#179).

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{build_native, Case};
use std::process::Command;

/// `case` run three ways from the directory it was built in: on the
/// interpreter, natively under `oskac run`, and as the executable.
fn three_ways(case: &Case) -> Vec<(Command, &'static str)> {
    let (exe, dir) = build_native(case);
    let mut interpreter = Command::new(env!("CARGO_BIN_EXE_oskac"));
    interpreter
        .current_dir(&dir)
        .args(["run", "--interpret", case.target()]);
    let mut native = Command::new(env!("CARGO_BIN_EXE_oskac"));
    native.current_dir(&dir).args(["run", case.target()]);
    vec![
        (interpreter, "the interpreter"),
        (native, "`oskac run` natively"),
        (Command::new(&exe), "the executable"),
    ]
}

// ---- The flush at exit ------------------------------------------------------

/// macOS has no `/dev/full`.
#[cfg(target_os = "linux")]
#[test]
fn a_failed_final_flush_is_an_io_error_on_both_backends() {
    // One short line stays in the native buffer until exit, where the
    // write to /dev/full fails with ENOSPC. The interpreter's write fails
    // at `println`; either way the program reports it and exits 1.
    let case = Case::file(
        "fault_full_final_flush",
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hello\")?\n    \
         Ok(())\n}\n",
    );
    for (mut command, label) in three_ways(&case) {
        let full = std::fs::File::create("/dev/full").expect("/dev/full should open");
        let output = command
            .stdout(full)
            .output()
            .expect("the program should run");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "no space left on device\n",
            "{label}"
        );
        assert_eq!(output.status.code(), Some(1), "{label}");
    }
}

#[test]
fn a_closed_pipe_found_by_the_final_flush_stays_quiet() {
    // As `oskac run x.osk | head -0`: the reader goes before the program
    // prints, so the flush at exit meets EPIPE. That stays quiet, with 0
    // (#86), on both backends.
    use std::io::{Read, Write};
    use std::process::Stdio;
    let case = Case::file(
        "fault_closed_final_flush",
        "fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {\n    \
         let _ = input.read_line()\n    io.println(\"hello\")?\n    Ok(())\n}\n",
    );
    for (mut command, label) in three_ways(&case) {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the program should start");
        drop(child.stdout.take());
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(b"go\n").expect("should write stdin");
        drop(stdin);
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("should read stderr");
        let status = child.wait().expect("the program should end");
        assert_eq!((stderr.as_str(), status.code()), ("", Some(0)), "{label}");
    }
}

// ---- Fault signals ----------------------------------------------------------

/// The report the runtime writes to stderr when the process dies of the
/// fault signal `number`, `name`.
#[cfg(unix)]
fn fault_report(number: i32, name: &str) -> String {
    format!("error: the native program faulted with signal {number} ({name})\n")
}

/// `oskac run`'s report (`E0392`) of a native program killed by the
/// fault signal `number`, `name`.
#[cfg(unix)]
fn run_e0392(number: i32, name: &str) -> String {
    format!(
        "error[E0392]: the native program was killed by signal {number} ({name}): a fault in \
         the native backend, not in the program; run it with `--interpret`\n"
    )
}

/// A program that prints, then overflows the native stack: `dbg` of a
/// chain five million links deep recurses natively once per link, with no
/// limit: a fault in the native backend. Should that recursion ever be
/// bounded, this needs another way to overflow the stack.
const OVERFLOW_ITEMS: &str = "enum Chain {\n    Link(Int, Chain),\n    End,\n}\n\n\
     fn chain() -> Chain {\n    var c = Chain.End\n    var i = 0\n    \
     while i < 5000000 {\n        c = Chain.Link(i, c)\n        i = i + 1\n    }\n    c\n}\n";

/// `SIGBUS`'s number: the one fault signal whose number differs.
#[cfg(unix)]
const SIGBUS: i32 = if cfg!(target_os = "macos") { 10 } else { 7 };

/// The signal a stack overflow raises: an access to the guard region
/// below `main`'s stack, which Linux reports as `SIGSEGV` and macOS, as
/// a protection fault, as `SIGBUS`.
#[cfg(unix)]
const OVERFLOW_SIGNAL: (i32, &str) = if cfg!(target_os = "macos") {
    (SIGBUS, "SIGBUS")
} else {
    (11, "SIGSEGV")
};

/// `STATUS_STACK_OVERFLOW`: the exception a stack overflow raises on
/// Windows, and the code the process ends with.
#[cfg(windows)]
const STACK_OVERFLOW: u32 = 0xC000_00FD;

/// What the runtime writes to stderr when a stack overflow ends the
/// process: the signal's report, or on Windows the exception's.
fn overflow_report() -> String {
    #[cfg(unix)]
    let (number, name) = OVERFLOW_SIGNAL;
    #[cfg(unix)]
    return fault_report(number, name);
    #[cfg(windows)]
    format!("error: the native program faulted with exception {STACK_OVERFLOW:#010X}\n")
}

/// `oskac run`'s report (`E0392`) of a native program a stack overflow
/// ended.
fn overflow_run_e0392() -> String {
    #[cfg(unix)]
    let (number, name) = OVERFLOW_SIGNAL;
    #[cfg(unix)]
    return run_e0392(number, name);
    #[cfg(windows)]
    format!(
        "error[E0392]: the native program faulted with exception {STACK_OVERFLOW:#010X}: a fault \
         in the native backend, not in the program; run it with `--interpret`\n"
    )
}

/// How `oskac test` says a stack overflow ended a test process.
fn overflow_ending() -> String {
    #[cfg(unix)]
    let (number, name) = OVERFLOW_SIGNAL;
    #[cfg(unix)]
    return format!("was killed by signal {number} ({name})");
    #[cfg(windows)]
    format!("faulted with exception {STACK_OVERFLOW:#010X}")
}

/// Whether `status` is a process's that a stack overflow ended.
fn overflowed(status: std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(OVERFLOW_SIGNAL.0)
    }
    #[cfg(windows)]
    {
        status.code() == Some(STACK_OVERFLOW as i32)
    }
}

/// The code `oskac run` ends with after a stack overflow ended the
/// program: the shell's `128 + n` after a signal, or on Windows the
/// program's own.
fn overflow_run_code() -> i32 {
    #[cfg(unix)]
    return 128 + OVERFLOW_SIGNAL.0;
    #[cfg(windows)]
    {
        STACK_OVERFLOW as i32
    }
}

#[test]
fn an_executable_that_overflows_its_stack_shows_its_output_then_the_fault_report() {
    let src = format!(
        "{OVERFLOW_ITEMS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(\"printed before the fault\")?\n    let c = dbg(chain())\n    Ok(())\n}}\n"
    );
    let (exe, dir) = build_native(&Case::file("fault_executable_overflow", &src));
    let output = Command::new(&exe)
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "printed before the fault\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), overflow_report());
    assert!(overflowed(output.status), "{output:?}");
}

#[test]
fn output_printed_before_a_stack_overflow_shows_before_the_fault_report() {
    let src = format!(
        "{OVERFLOW_ITEMS}\nfn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println(\"printed before the fault\")?\n    let c = dbg(chain())\n    Ok(())\n}}\n"
    );
    let case = Case::file("fault_stack_overflow", &src);
    let (_, dir) = build_native(&case);
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(&dir)
        .args(["run", case.target()])
        .output()
        .expect("oskac run should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "printed before the fault\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{}{}", overflow_report(), overflow_run_e0392())
    );
    assert_eq!(output.status.code(), Some(overflow_run_code()));
}

#[test]
fn a_test_that_overflows_the_stack_is_still_reported_with_e0392() {
    let src = format!(
        "{OVERFLOW_ITEMS}\nfn main() {{}}\n\ntest \"overflows\" {{\n    let c = dbg(chain())\n}}\n"
    );
    let case = Case::file("fault_test_stack_overflow", &src);
    let (_, dir) = build_native(&case);
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .current_dir(&dir)
        .args(["test", case.target()])
        .output()
        .expect("oskac test should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "error[E0392]: the native test process {}: a fault in the native backend, not in \
             the test; run the test with `--interpret`",
            overflow_ending()
        )),
        "{stdout}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), overflow_report());
    assert_eq!(output.status.code(), Some(1));
}

/// The fault signals: each is sent to a native program that printed a
/// line, then waits in a loop. Sent from outside, it reaches the same
/// handler a fault does.
#[cfg(unix)]
const FAULT_SIGNALS: [(i32, &str); 6] = [
    (4, "SIGILL"),
    (5, "SIGTRAP"),
    (6, "SIGABRT"),
    (SIGBUS, "SIGBUS"),
    (8, "SIGFPE"),
    (11, "SIGSEGV"),
];

/// Unix only: nothing outside a Windows process can raise an exception
/// in it.
#[cfg(unix)]
#[test]
fn every_fault_signal_shows_the_output_then_a_report_and_kills_the_program() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    // The marker file says the line is printed (and still in the buffer,
    // since nothing went to stderr and stdout is a pipe).
    let case = Case::file(
        "fault_signals",
        "fn main(io: Stdout, fs: FsWrite) -> Result<(), IoError> {\n    \
         io.println(\"printed before the fault\")?\n    \
         fs.restrict_to_file(\"ready\").write(\"\")?\n    \
         var i = 0\n    while true {\n        i = i + 0\n    }\n    Ok(())\n}\n",
    );
    let (exe, dir) = build_native(&case);
    for (number, name) in FAULT_SIGNALS {
        let ready = dir.join("ready");
        let _ = std::fs::remove_file(&ready);
        let mut child = Command::new(&exe)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the program should start");
        let deadline = Instant::now() + Duration::from_secs(60);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "{name}: the program never got ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let sent = Command::new("kill")
            .args([
                "-s",
                name.trim_start_matches("SIG"),
                &child.id().to_string(),
            ])
            .status()
            .expect("kill should run");
        assert!(sent.success(), "{name}: kill failed");
        let deadline = Instant::now() + Duration::from_secs(60);
        while child.try_wait().expect("should poll the program").is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("{name}: the program did not end");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().expect("should read the output");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "printed before the fault\n",
            "{name}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            fault_report(number, name),
            "{name}"
        );
        assert_eq!(output.status.signal(), Some(number), "{name}");
    }
}
