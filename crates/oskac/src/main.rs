//! `oskac`: the Oska compiler command-line interface.

use std::process::ExitCode;

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
    oskac --version                        Print the version
    oskac --help                           Print this help
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // Everything after `--` belongs to the program being run.
    let program_args = match args.iter().position(|a| a == "--") {
        Some(at) => {
            let rest = args.split_off(at + 1);
            args.pop();
            rest
        }
        None => Vec::new(),
    };
    let json = args.iter().any(|a| a == "--format=json");
    let release = args.iter().any(|a| a == "--release");
    let positional: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--format=") && *a != "--release")
        .collect();

    match positional.as_slice() {
        ["--version" | "-V"] => {
            println!("oskac {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["lex", path] => lex_file(path, json),
        ["parse", path] => parse_file(path, json),
        ["check", path] => check_file(path, json, release),
        ["run", path] => run_file(path, json, &program_args),
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
fn parse_file(path: &str, json: bool) -> ExitCode {
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
fn check_file(path: &str, json: bool, release: bool) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let report = oska_check::check_module_with(&src, oska_check::CheckOptions { release });
    if json {
        print!("{}", report.to_json());
    } else {
        print_diagnostics(path, &report.diagnostics);
        for function in &report.functions {
            println!("{}", describe_authority(function));
        }
        for site in &report.audit {
            println!("{path}: {site}");
        }
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
fn run_file(path: &str, json: bool, args: &[String]) -> ExitCode {
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

/// One line of the human-readable authority report, such as
/// `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`.
fn describe_authority(function: &oska_check::FunctionAuthority) -> String {
    if function.pure {
        return format!("fn {}: pure", function.name);
    }
    let sources: Vec<String> = function
        .via
        .iter()
        .map(|via| {
            if via.capabilities.is_empty() {
                format!("cap fn ({})", via.param)
            } else {
                format!("{} ({})", via.capabilities.join(", "), via.param)
            }
        })
        .collect();
    format!("fn {}: {}", function.name, sources.join(", "))
}

fn lex_file(path: &str, json: bool) -> ExitCode {
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
