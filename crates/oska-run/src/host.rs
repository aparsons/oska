//! The [`Host`]: the interpreter's only way to reach the OS.
//!
//! Every intrinsic that touches the outside world goes through it, so a
//! program behaves the same on [`RealHost`] and [`FakeHost`] except for
//! what the world gives back.
//!
//! To add an operation, such as reading a file: add a method to [`Host`],
//! implement it on both hosts (`FakeHost` deterministically, in memory),
//! and call it from the intrinsic that needs it (`intrinsics.rs`).

use std::io::{self, Write};

/// What a program wrote to its standard streams, as a host recorded it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// The operating system, as the interpreter sees it.
pub trait Host {
    /// Write `text` to standard output, as is.
    fn write_stdout(&mut self, text: &str) -> io::Result<()>;

    /// Hand over what the host has recorded of the program's output since
    /// the last call. A host that writes straight to the real streams
    /// records nothing.
    fn take_output(&mut self) -> Output;
}

/// The real OS: backs `oskac run` and integration tests.
#[derive(Debug, Default)]
pub struct RealHost;

impl RealHost {
    pub fn new() -> Self {
        RealHost
    }
}

impl Host for RealHost {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        io::stdout().write_all(text.as_bytes())
    }

    fn take_output(&mut self) -> Output {
        Output::default()
    }
}

/// A deterministic stand-in for the OS: it records output rather than
/// printing it. Backs this crate's tests and the language's test
/// stand-ins.
#[derive(Debug, Default)]
pub struct FakeHost {
    output: Output,
}

impl FakeHost {
    pub fn new() -> Self {
        FakeHost::default()
    }
}

impl Host for FakeHost {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        self.output.stdout.push_str(text);
        Ok(())
    }

    fn take_output(&mut self) -> Output {
        std::mem::take(&mut self.output)
    }
}
