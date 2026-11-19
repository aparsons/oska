//! A native program killed by a signal (spec #153, runtime parity):
//! `oskac run` reports a signal that only a fault raises as the native
//! backend's fault, `E0392`, on aarch64 and x86_64 Linux and on Apple
//! silicon macOS alike, and ends as the program did on any other.

#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

#[cfg(target_os = "linux")]
mod common;
mod differential;

#[cfg(target_os = "linux")]
use common::procfs::children;
use differential::{build_native, Case};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

/// The processes whose parent is `pid`. macOS has no `/proc`, so there
/// `pgrep` lists them.
#[cfg(target_os = "macos")]
fn children(pid: u32) -> Vec<u32> {
    let listed = Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .expect("pgrep should run");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// `SIGBUS`'s number on this host: 10 on macOS, 7 on Linux.
const SIGBUS: i32 = if cfg!(target_os = "macos") { 10 } else { 7 };

#[test]
fn a_native_program_killed_by_a_fault_signal_is_reported_as_e0392() {
    // `oskac run` runs the program as a child process where it runs
    // natively by default, and the child is killed while it waits on
    // stdin. Only a signal that only a fault raises is reported as one
    // of the native backend's; any other ends it as any process's ends.
    let case = Case::file(
        "process_fault",
        "fn main(io: Stdout, input: Stdin) {\n    let _ = io.println(\"ready\")\n    \
         let _ = input.read_line()\n}\n",
    );
    let (_, dir) = build_native(&case);
    for (signal, number, fault) in [
        ("SEGV", 11, true),
        ("BUS", SIGBUS, true),
        ("TERM", 15, false),
    ] {
        let mut oskac = Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["run", "main.osk"])
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("oskac should start");
        let mut stdout = BufReader::new(oskac.stdout.take().expect("stdout"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("a line");
        assert_eq!(line, "ready\n");
        let program = children(oskac.id());
        if program.is_empty() {
            drop(oskac.stdin.take());
            let _ = oskac.wait();
            eprintln!("skipped: `oskac run` runs on the interpreter on this host");
            return;
        }
        assert_eq!(program.len(), 1, "{program:?}");
        let killed = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(program[0].to_string())
            .status()
            .expect("kill should run");
        assert!(killed.success());
        let mut stderr = String::new();
        oskac
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("its stderr");
        let status = oskac.wait().expect("oskac should end");
        assert_eq!(status.code(), Some(128 + number), "SIG{signal}: {stderr}");
        // A fault signal reaches the runtime's fault handler first, which
        // reports it and raises it again (#143).
        let expected = if fault {
            format!(
                "error: the native program faulted with signal {number} (SIG{signal})\n\
                 error[E0392]: the native program was killed by signal {number} (SIG{signal}): \
                 a fault in the native backend, not in the program; run it with `--interpret`\n"
            )
        } else {
            String::new()
        };
        assert_eq!(stderr, expected, "SIG{signal}");
    }
}
