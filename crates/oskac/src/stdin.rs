// SPDX-License-Identifier: Apache-2.0
//! A bare `-` as the path: `oskac check -` and `oskac run -` read one
//! module from standard input, as an embedder pipes a snippet, and report
//! it as the same bytes in a file named `<stdin>`. A program run from
//! standard input gets a closed standard input of its own. `main`
//! refuses `-` for every other command.

use std::io::Read;
use std::process::ExitCode;

use crate::cli::Flags;
use crate::{authority_gate, cache};

/// The operand that means standard input.
pub const DASH: &str = "-";

/// The path a module from standard input is reported under, and its
/// program's `args.get(0)`.
pub const PATH: &str = "<stdin>";

/// `oskac check -`: check the module on standard input, with what to do
/// with a baseline or a grant (`gate`).
pub fn check(flags: Flags, gate: Option<authority_gate::Gate>) -> ExitCode {
    let store = cache::for_command(flags.no_cache);
    let Some(src) = read() else {
        return ExitCode::FAILURE;
    };
    crate::check::check_source(PATH, &src, flags, gate, store.as_ref())
}

/// `oskac run -`: check the module on standard input and, within its
/// grant (`run`), run it with `args` after `<stdin>`, a closed standard
/// input of its own, and the run limits.
pub fn run(flags: Flags, run: crate::run::RunOptions, args: &[String]) -> ExitCode {
    let store = cache::for_command(flags.no_cache);
    let Some(src) = read() else {
        return ExitCode::FAILURE;
    };
    crate::run::run_source(PATH, &src, flags, run, args, store.as_ref(), true)
}

/// All of standard input, as UTF-8 text. Text that cannot be read is the
/// error a file that cannot be read gives.
fn read() -> Option<String> {
    let mut src = String::new();
    match std::io::stdin().lock().read_to_string(&mut src) {
        Ok(_) => Some(src),
        Err(err) => {
            eprintln!("error: cannot read {PATH}: {err}");
            None
        }
    }
}
