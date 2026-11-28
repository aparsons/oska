// SPDX-License-Identifier: Apache-2.0
//! `oskac`: the Oska compiler command-line interface.

use std::process::ExitCode;

/// `print!` to stdout through [`write_stdout`].
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::write_stdout(&format!($($arg)*))
    };
}

/// `println!` to stdout through [`write_stdout`].
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::write_stdout(&format!("{}\n", format_args!($($arg)*)))
    };
}

/// Whether this `oskac` is a debug build, the only kind whose programs
/// honor the DNS test variables `OSKA_DNS_HOSTS`, `OSKA_DNS_RESOLV_CONF` and
/// `OSKA_DNS_PORT` (ADR 0026). The interpreter's host is told it, and the
/// native compiler bakes it into each executable; `oskac release` always
/// builds without it.
pub(crate) const DNS_TEST_HOOKS: bool = cfg!(debug_assertions);

mod authority_gate;
mod build;
mod cache;
mod check;
mod cli;
mod crash;
mod dispatch;
mod doc;
mod explain;
mod fence;
mod fix;
mod heap;
mod hook;
mod json;
mod lex;
mod limits;
mod native;
mod native_test;
mod new;
mod package;
mod parse;
mod places;
mod release;
mod run;
mod self_check;
mod self_cmd;
mod self_update;
mod semver;
mod sha256;
mod source;
mod stdin;
mod target;
mod test;
mod update_checker;
mod verdict;

fn main() -> ExitCode {
    crash::install();
    dispatch::dispatch(cli::CommandLine::parse(std::env::args().skip(1).collect()))
}

/// Write `text` to stdout. A reader that has gone, as in `oskac check
/// --format=json | head -1`, ends `oskac` quietly with 0, as it ends a
/// program `oskac run` runs; any other failure panics, as `print!` does.
pub(crate) fn write_stdout(text: &str) {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(err) => panic!("failed printing to stdout: {err}"),
    }
}
