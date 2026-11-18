//! `oska-eval`: the evaluation harness for AI agents writing Oska. It is
//! run by the maintainer, locally, and is never released or installed.
//!
//! Each subcommand reads files and writes JSON to standard output. The exit
//! code is 0 on success, 1 when a check failed, and 2 on bad input.

mod grade;
mod grader;
mod json;
mod prepare;
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

    oska-eval prepare <tasks-dir> <task-id> <run-dir>
        Make a fresh run directory for an active task: workspace/ (the
        starting package) and prompt.md.
        <run-dir> must be new, and outside the repo and the task set.

    oska-eval grade --oskac <path> <tasks-dir> <task-id> <run-dir>
        Grade a finished run's workspace/ through the oskac at <path>: the
        hidden checks, the release check, the program's output, and authority
        against the task's expected authority. Nothing changes in the run.

EXIT CODES:
    0  success    1  a check failed    2  bad input
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["validate", "--oskac", oskac, tasks] => validate::run(oskac, tasks),
        ["prepare", tasks, id, run_dir] => prepare::run(tasks, id, run_dir),
        ["grade", "--oskac", oskac, tasks, id, run_dir] => grade::run(oskac, tasks, id, run_dir),
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
