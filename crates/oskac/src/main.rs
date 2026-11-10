//! `oskac`: the Oska compiler command-line interface.

use std::process::ExitCode;

mod explain;

const USAGE: &str = "\
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check <file.osk> [--format=json] [--release]
                                           Type-check a source file and report
                                           each function's authority
    oskac run <file.osk> [--format=json] [-- args...]
                                           Check a source file and, if it has
                                           no diagnostics, run it
    oskac test <file.osk> [--format=json] [--integration]
                                           Run the test blocks in a source file
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
";

/// The flags `oskac` takes, whichever command they are given to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    /// `--format=json`: machine-readable output. Any other `--format=`
    /// means the default, human-readable output.
    json: bool,
    /// `--release`: `oskac check` rejects `dbg` and `todo`.
    release: bool,
    /// `--integration`: `oskac test` runs `#[integration]` tests too.
    integration: bool,
}

/// A command line, parsed: the command and its operands, the flags, and
/// the arguments after `--`, which belong to the program being run.
#[derive(Debug, Default, PartialEq, Eq)]
struct CommandLine {
    positional: Vec<String>,
    flags: Flags,
    program_args: Vec<String>,
}

impl CommandLine {
    /// Parse `args`, without the executable's own name. A flag may come
    /// anywhere before `--`; anything else is positional.
    fn parse(mut args: Vec<String>) -> CommandLine {
        let program_args = match args.iter().position(|a| a == "--") {
            Some(at) => {
                let rest = args.split_off(at + 1);
                args.pop();
                rest
            }
            None => Vec::new(),
        };
        let mut line = CommandLine {
            program_args,
            ..CommandLine::default()
        };
        for arg in args {
            match arg.as_str() {
                "--release" => line.flags.release = true,
                "--integration" => line.flags.integration = true,
                format if format.starts_with("--format=") => {
                    line.flags.json = format == "--format=json";
                }
                _ => line.positional.push(arg),
            }
        }
        line
    }
}

fn main() -> ExitCode {
    let line = CommandLine::parse(std::env::args().skip(1).collect());
    let flags = line.flags;
    let positional: Vec<&str> = line.positional.iter().map(String::as_str).collect();
    match positional.as_slice() {
        ["--version" | "-V"] => {
            if flags.json {
                println!(
                    "{{\"name\":\"oskac\",\"version\":\"{}\"}}",
                    env!("CARGO_PKG_VERSION")
                );
            } else {
                println!("oskac {}", env!("CARGO_PKG_VERSION"));
            }
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["lex", path] => lex_file(path, flags),
        ["parse", path] => parse_file(path, flags),
        ["check", path] => check_file(path, flags),
        ["run", path] => run_file(path, flags, &line.program_args),
        ["test", path] => test_file(path, flags),
        ["explain", code] => explain_code(code, flags),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn read_source(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(src) => Some(src),
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            None
        }
    }
}

/// `oskac parse`: print the module's syntax tree, as JSON with
/// `--format=json`. A parse error is printed as diagnostic JSON on stdout
/// (`--format=json`) or as text on stderr, with a non-zero exit code.
fn parse_file(path: &str, Flags { json, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    match oska_syntax::parse_module(&src) {
        Ok(module) if json => {
            print!("{}", oska_syntax::module_to_json(&module, path));
            ExitCode::SUCCESS
        }
        Ok(module) => {
            print!("{}", oska_syntax::module_to_tree(&module));
            ExitCode::SUCCESS
        }
        Err(err) if json => {
            println!("{}", err.to_json());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{path}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `oskac check`: print every diagnostic, each function's authority, and
/// every audit site, as
/// `{"diagnostics": [...], "functions": [...], "audit": [...]}` with
/// `--format=json`.
/// The exit code is non-zero when there are diagnostics.
fn check_file(path: &str, Flags { json, release, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let report = oska_check::check_module_with(&src, oska_check::CheckOptions { release });
    if json {
        print!("{}", report.to_json());
    } else {
        print_diagnostics(path, &report.diagnostics);
        print!("{}", report.authority_to_human(path));
    }
    if report.diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Print diagnostics to stderr, each with its fix, as `oskac check` does.
fn print_diagnostics(path: &str, diagnostics: &[oska_syntax::Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{path}: {diagnostic}");
        if let Some(fix) = &diagnostic.fix {
            eprintln!("    fix: {}", fix.message);
        }
    }
}

/// `oskac run`: check the program and, if the check reports no
/// diagnostic, run it on the real OS with `args` after the program path.
/// A refused program's diagnostics, or the panic that stopped it, go to
/// stderr as `oskac check` prints diagnostics, or one JSON object per
/// line with `--format=json`. The exit code is the program's: 101 after
/// a panic (see `docs/runtime.md`).
fn run_file(path: &str, Flags { json, .. }: Flags, args: &[String]) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let mut argv = vec![path.to_string()];
    argv.extend_from_slice(args);
    let outcome = oska_run::run(&src, &mut oska_run::RealHost::new(), &argv);
    eprint!("{}", outcome.stderr);
    // A panic is printed in the shape of a diagnostic, after the output
    // the program wrote before it.
    let mut reported = outcome.diagnostics;
    reported.extend(outcome.panic);
    if json {
        for diagnostic in &reported {
            eprintln!("{}", diagnostic.to_json());
        }
    } else {
        print_diagnostics(path, &reported);
    }
    ExitCode::from(u8::try_from(outcome.exit_code).unwrap_or(u8::MAX))
}

/// `oskac test`: run the file's `test` blocks, the `#[integration]`
/// ones too, on the real OS, with `--integration`. The report goes to
/// stdout: one line per test, the failures, and a summary, or one JSON
/// object with `--format=json`. A refused file's diagnostics go to stderr
/// as `oskac check` prints them, or into the JSON report. The exit code
/// is 1 when a test failed (see `docs/runtime.md`).
fn test_file(path: &str, flags: Flags) -> ExitCode {
    let Flags {
        json, integration, ..
    } = flags;
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let options = oska_run::TestOptions { integration };
    let report = oska_run::test(&src, &mut oska_run::RealHost::new(), options);
    if json {
        println!("{}", report.to_json());
    } else if !report.diagnostics.is_empty() {
        print_diagnostics(path, &report.diagnostics);
    } else {
        print!("{}", report.to_human(path));
    }
    ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(u8::MAX))
}

/// `oskac explain`: print the catalog entry of a diagnostic code, as JSON
/// with `--format=json`. An unknown code is an error (exit code 2) that
/// suggests the closest code, as text on stderr or as JSON on stdout.
fn explain_code(code: &str, Flags { json, .. }: Flags) -> ExitCode {
    let entries = explain::catalog();
    if let Some(entry) = explain::find(&entries, code) {
        if json {
            println!("{}", explain::to_json(entry));
        } else {
            print!("{}", explain::to_human(entry));
        }
        return ExitCode::SUCCESS;
    }
    let suggestion = explain::closest(&entries, code);
    if json {
        println!("{}", explain::unknown_to_json(code, suggestion));
    } else {
        eprintln!("{}", explain::unknown_to_human(code, suggestion));
    }
    ExitCode::from(2)
}

fn lex_file(path: &str, Flags { json, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    match oska_syntax::lex(&src) {
        Ok(tokens) => {
            for token in tokens {
                println!(
                    "{:>5}..{:<5} {:?}",
                    token.span.start, token.span.end, token.kind
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) if json => {
            println!("{}", err.to_json());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{path}: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandLine, Flags};

    fn parse(args: &[&str]) -> CommandLine {
        CommandLine::parse(args.iter().map(|a| a.to_string()).collect())
    }

    #[test]
    fn flags_come_anywhere_before_the_program_arguments() {
        let line = parse(&["--integration", "test", "f.osk", "--format=json"]);
        assert_eq!(line.positional, ["test", "f.osk"]);
        assert_eq!(
            line.flags,
            Flags {
                json: true,
                release: false,
                integration: true
            }
        );
        assert!(line.program_args.is_empty());
    }

    #[test]
    fn everything_after_a_double_dash_belongs_to_the_program() {
        let line = parse(&["run", "f.osk", "--", "--release", "x"]);
        assert_eq!(line.positional, ["run", "f.osk"]);
        assert_eq!(line.flags, Flags::default());
        assert_eq!(line.program_args, ["--release", "x"]);
    }

    #[test]
    fn another_format_means_human_output_and_an_unknown_flag_is_positional() {
        let line = parse(&["check", "f.osk", "--format=text", "--nope"]);
        assert!(!line.flags.json);
        assert_eq!(line.positional, ["check", "f.osk", "--nope"]);
    }
}
