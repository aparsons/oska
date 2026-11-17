//! `oska-eval`: the evaluation harness for AI agents writing Oska. It is
//! run by the maintainer, locally, and is never released or installed.
//!
//! Each subcommand reads files and writes JSON to standard output. The exit
//! code is 0 on success, 1 when a check failed, and 2 on bad input.

mod grader;
mod json;
mod task;
mod validate;
mod workspace;

use std::process::ExitCode;

const USAGE: &str = "\
oska-eval - the evaluation harness for AI agents writing Oska

USAGE:
    oska-eval validate --oskac <path> <tasks-dir>
        Check every task in <tasks-dir> (the repo's is eval/tasks): its files
        and, for an active task, its hidden checks against its reference
        solution (which must pass them all) and its insecure solution (which
        must pass the functional checks and fail a security check), run with
        the oskac at <path>.

EXIT CODES:
    0  success    1  a check failed    2  bad input
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["validate", "--oskac", oskac, tasks] => validate::run(oskac, tasks),
        ["--help" | "-h" | "help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
