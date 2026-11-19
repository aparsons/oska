//! Running a child process for a test with a wall-clock limit, so that a
//! hung process fails the test instead of hanging it: the native
//! executables of the differential suite (`native.rs`) and the Lean
//! reference (`lean_reference.rs`) both run through [`run_bounded`].

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How a process that ended in time ended.
#[derive(Debug)]
pub struct Finished {
    pub stdout: String,
    pub stderr: String,
    /// The exit code; `None` when a signal ended it.
    pub code: Option<i32>,
}

/// The process ran past its limit and was killed.
#[derive(Debug)]
pub struct TimedOut(pub Duration);

/// Run `command` with `stdin`, and wait for it, at most `limit`: its
/// stdout, stderr and exit code, or [`TimedOut`] after killing it. A
/// command that cannot start fails the test.
pub fn run_bounded(
    mut command: Command,
    stdin: &str,
    limit: Duration,
) -> Result<Finished, TimedOut> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap_or_else(|e| {
        panic!(
            "{} should start: {e}",
            command.get_program().to_string_lossy()
        )
    });
    let mut input = child.stdin.take().expect("a stdin pipe");
    let stdin = stdin.to_string();
    // A program that never reads its stdin may exit before it is written.
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(stdin.as_bytes());
    });
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = read(Box::new(child.stdout.take().expect("a stdout pipe")));
    let stderr = read(Box::new(child.stderr.take().expect("a stderr pipe")));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("should wait for the process") {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TimedOut(limit));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let _ = writer.join();
    let stdout = stdout.join().expect("the stdout reader");
    let stderr = stderr.join().expect("the stderr reader");
    Ok(Finished {
        stdout,
        stderr,
        code: status.code(),
    })
}
