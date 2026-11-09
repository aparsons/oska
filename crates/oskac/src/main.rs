//! `oskac`: the Oska compiler command-line interface.

use std::process::ExitCode;

const USAGE: &str = "\
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac --version                        Print the version
    oskac --help                           Print this help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--format=json");
    let positional: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--format="))
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
