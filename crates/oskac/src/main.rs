//! `oskac`: the Oska compiler command-line interface.

use oska_syntax::Diagnostic;
use std::process::ExitCode;

mod explain;
mod package;

const USAGE: &str = "\
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check [path] [--format=json] [--release]
                                           Type-check a source file, or a
                                           package directory (the current one
                                           with no path), and report each
                                           function's authority
    oskac run [path] [--format=json] [-- args...]
                                           Check a source file or package and,
                                           if it has no diagnostics, run it
    oskac test [path] [--format=json] [--integration]
                                           Run the test blocks in a source file
                                           or package
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
        ["check", rest @ ..] if rest.len() <= 1 => check_target(rest.first(), flags),
        ["run", rest @ ..] if rest.len() <= 1 => {
            run_target(rest.first(), flags, &line.program_args)
        }
        ["test", rest @ ..] if rest.len() <= 1 => test_target(rest.first(), flags),
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

/// `oskac check`: print every diagnostic, each function's authority with
/// its attenuations, and every audit site, as `{"diagnostics": [...],
/// "functions": [...], "audit": [...], "attenuations": [...]}` with
/// `--format=json`.
/// The exit code is non-zero when there are diagnostics.
fn check_file(path: &str, Flags { json, release, .. }: Flags) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let report = oska_check::check_file(path, &src, oska_check::CheckOptions { release });
    print_report(&report, json, path)
}

/// Print a check report as `oskac check` does, and return its exit code.
/// `path` names the unlocated entries' file in the human report.
fn print_report(report: &oska_check::CheckReport, json: bool, path: &str) -> ExitCode {
    if json {
        print!("{}", report.to_json());
    } else {
        print_diagnostics(&report.diagnostics);
        print!("{}", report.authority_to_human(path));
    }
    if report.diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Print diagnostics to stderr, each after its location,
/// `path:line:col`, and with its fix, as `oskac check` does.
fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        match &diagnostic.location {
            Some(location) => eprintln!("{location}: {diagnostic}"),
            None => eprintln!("{diagnostic}"),
        }
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
    let locate = in_file(path, &src);
    finish_run(outcome, json, &locate)
}

/// A locator for diagnostics whose spans are in `src`, the text of the
/// file at `path`.
fn in_file<'a>(path: &'a str, src: &'a str) -> impl Fn(Diagnostic) -> Diagnostic + 'a {
    move |d| match d.location {
        Some(_) => d,
        None => d.locate(path, src),
    }
}

/// Print how a run ended and return the program's exit code. `locate`
/// names the file of a panic or a refusal, which `--format=json` prints
/// as `path` and the human output as `path:line:col`.
fn finish_run(
    outcome: oska_run::RunOutcome,
    json: bool,
    locate: &dyn Fn(Diagnostic) -> Diagnostic,
) -> ExitCode {
    eprint!("{}", outcome.stderr);
    // A panic is printed in the shape of a diagnostic, after the output
    // the program wrote before it.
    let mut reported = outcome.diagnostics;
    reported.extend(outcome.panic);
    let reported: Vec<Diagnostic> = reported.into_iter().map(locate).collect();
    if json {
        for diagnostic in &reported {
            eprintln!("{}", diagnostic.to_json());
        }
    } else {
        print_diagnostics(&reported);
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
    let locate = in_file(path, &src);
    finish_test(report, json, &locate)
}

/// Print a test report and return its exit code. `locate` names the file
/// of each failure and refusal, as for [`finish_run`].
fn finish_test(
    mut report: oska_run::TestReport,
    json: bool,
    locate: &dyn Fn(Diagnostic) -> Diagnostic,
) -> ExitCode {
    for test in &mut report.tests {
        test.failure = test.failure.take().map(locate);
    }
    report.diagnostics = report.diagnostics.drain(..).map(locate).collect();
    if json {
        println!("{}", report.to_json());
    } else if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics);
    } else {
        print!("{}", report.to_human(""));
    }
    ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(u8::MAX))
}

// ---- Packages ------------------------------------------------------------------

/// `oskac check [path]`: a package directory (the current one with no
/// path), or a file, which keeps its single-file behaviour.
fn check_target(path: Option<&&str>, flags: Flags) -> ExitCode {
    match package::target(path.copied()) {
        package::Target::File(file) => {
            let code = check_file(&file, flags);
            hint_package(&file, "check");
            code
        }
        package::Target::Dir(dir) => {
            let Some((package, refused)) = load_package(&dir, flags.json) else {
                return ExitCode::FAILURE;
            };
            let options = oska_check::CheckOptions {
                release: flags.release,
            };
            let report = checked(&package, refused, options);
            print_report(&report, flags.json, &dir)
        }
    }
}

/// `oskac run [path]`: a package directory, or a file. A library, a
/// package without `src/main.osk`, is refused.
fn run_target(path: Option<&&str>, flags: Flags, args: &[String]) -> ExitCode {
    let json = flags.json;
    match package::target(path.copied()) {
        package::Target::File(file) => {
            let code = run_file(&file, flags, args);
            hint_package(&file, "run");
            code
        }
        package::Target::Dir(dir) => {
            let Some((package, refused)) = load_package(&dir, json) else {
                return ExitCode::FAILURE;
            };
            if package.is_library() {
                return refuse(vec![oska_check::run_of_library(&dir)], json);
            }
            let report = checked(&package, refused, oska_check::CheckOptions::default());
            let Some(program) = &report.program else {
                return refuse_unchecked(report, json);
            };
            let mut argv = vec![dir.clone()];
            argv.extend_from_slice(args);
            let outcome = oska_run::run_program(program, &mut oska_run::RealHost::new(), &argv);
            finish_run(outcome, json, &|d| {
                oska_check::locate_in_package(&package, d)
            })
        }
    }
}

/// `oskac test [path]`: a package directory, or a file.
fn test_target(path: Option<&&str>, flags: Flags) -> ExitCode {
    let json = flags.json;
    match package::target(path.copied()) {
        package::Target::File(file) => {
            let code = test_file(&file, flags);
            hint_package(&file, "test");
            code
        }
        package::Target::Dir(dir) => {
            let Some((package, refused)) = load_package(&dir, json) else {
                return ExitCode::FAILURE;
            };
            let report = checked(&package, refused, oska_check::CheckOptions::default());
            let Some(program) = &report.program else {
                return refuse_unchecked(report, json);
            };
            let options = oska_run::TestOptions {
                integration: flags.integration,
            };
            let tests = oska_run::test_program(program, &mut oska_run::RealHost::new(), options);
            finish_test(tests, json, &|d| oska_check::locate_in_package(&package, d))
        }
    }
}

/// Read the package in `dir`. `None` after printing why it is not one.
fn load_package(dir: &str, json: bool) -> Option<(oska_check::Package, Vec<Diagnostic>)> {
    match package::load(dir) {
        package::Loaded::Package { package, refused } => Some((package, refused)),
        package::Loaded::NotAPackage(refusal) => {
            refuse(vec![refusal], json);
            None
        }
        package::Loaded::Unreadable(message) => {
            eprintln!("error: {message}");
            None
        }
    }
}

/// Check `package`; the `refused` symlinks come first, and a package that
/// had any has no program to run.
fn checked(
    package: &oska_check::Package,
    refused: Vec<Diagnostic>,
    options: oska_check::CheckOptions,
) -> oska_check::CheckReport {
    let mut report = oska_check::check_package(package, options);
    if !refused.is_empty() {
        report.diagnostics.splice(0..0, refused);
        report.program = None;
    }
    report
}

/// Print `diagnostics` as a refusal: as a report's diagnostics with
/// `--format=json`, or as `oskac check` prints them. Exit code 1.
fn refuse(diagnostics: Vec<Diagnostic>, json: bool) -> ExitCode {
    if json {
        let report = oska_check::CheckReport {
            diagnostics,
            ..oska_check::CheckReport::default()
        };
        print!("{}", report.to_json());
    } else {
        print_diagnostics(&diagnostics);
    }
    ExitCode::FAILURE
}

/// `report` has no program, so it has diagnostics, unless the package has
/// no module at all: print them.
fn refuse_unchecked(report: oska_check::CheckReport, json: bool) -> ExitCode {
    if report.diagnostics.is_empty() {
        eprintln!("error: the package has no modules under `src/`");
        return ExitCode::FAILURE;
    }
    if json {
        for diagnostic in &report.diagnostics {
            eprintln!("{}", diagnostic.to_json());
        }
    } else {
        print_diagnostics(&report.diagnostics);
    }
    ExitCode::FAILURE
}

/// After `oskac <command> <file>`, a file that is part of a package gets
/// a hint to run the command on the package: the file was checked alone.
fn hint_package(file: &str, command: &str) {
    if let Some(dir) = package::package_containing(file) {
        eprintln!(
            "hint: `{file}` is part of the package in `{dir}` and was handled alone; \
             run `oskac {command} {dir}` to {command} the package"
        );
    }
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
