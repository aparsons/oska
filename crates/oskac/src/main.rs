//! `oskac`: the Oska compiler command-line interface.

use std::process::ExitCode;

const USAGE: &str = "\
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
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
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn lex_file(path: &str, json: bool) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
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
