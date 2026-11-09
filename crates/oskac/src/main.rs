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
    oskac --version                        Print the version
    oskac --help                           Print this help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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

/// `oskac check`: print every diagnostic and each function's authority,
/// as `{"diagnostics": [...], "functions": [...]}` with `--format=json`.
/// The exit code is non-zero when there are diagnostics.
fn check_file(path: &str, json: bool, release: bool) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let report = oska_check::check_module_with(&src, oska_check::CheckOptions { release });
    if json {
        print!("{}", report.to_json());
    } else {
        for diagnostic in &report.diagnostics {
            eprintln!("{path}: {diagnostic}");
            if let Some(fix) = &diagnostic.fix {
                eprintln!("    fix: {}", fix.message);
            }
        }
        for function in &report.functions {
            println!("{}", describe_authority(function));
        }
    }
    if report.diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
