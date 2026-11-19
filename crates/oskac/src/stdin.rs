//! A bare `-` as the path: `oskac check -` and `oskac run -` read one
//! module from standard input, as an embedder pipes a snippet, and report
//! it as the same bytes in a file named `<stdin>`. A program run from
//! standard input gets a closed standard input of its own.

use std::io::Read;
use std::process::ExitCode;

use crate::{cache, Command, Flags};

/// The operand that means standard input.
pub const DASH: &str = "-";

/// The path a module from standard input is reported under, and its
/// program's `args.get(0)`.
pub const PATH: &str = "<stdin>";

/// `command` on the module on standard input: `check` or `run`. `main`
/// refuses `-` for every other command before it gets here.
pub fn command(
    command: Command,
    flags: Flags,
    args: &[String],
    store: Option<&cache::Store>,
) -> ExitCode {
    let Some(src) = read() else {
        return ExitCode::FAILURE;
    };
    match command {
        Command::Check(gate) => crate::check_source(PATH, &src, flags, gate, store),
        Command::Run => crate::run_source(
            PATH,
            &src,
            flags,
            args,
            store,
            oska_run::RealHost::without_stdin(),
        ),
        Command::Test | Command::Build(_) => {
            eprint!("{}", crate::USAGE);
            ExitCode::from(2)
        }
    }
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
