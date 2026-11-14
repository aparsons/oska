//! `oskac`: the Oska compiler command-line interface.

use oska_syntax::{Applicability, Diagnostic, Span};
use std::process::ExitCode;

/// `print!` to stdout through [`write_stdout`].
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::write_stdout(&format!($($arg)*))
    };
}

/// `println!` to stdout through [`write_stdout`].
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::write_stdout(&format!("{}\n", format_args!($($arg)*)))
    };
}

mod build;
mod explain;
mod hook;
mod native;
mod native_test;
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
    oskac run [path] [--format=json] [--interpret] [-- args...]
                                           Check a source file or package and,
                                           if it has no diagnostics, run it:
                                           natively on aarch64 Linux, on the
                                           interpreter elsewhere or with
                                           --interpret
    oskac test [path] [--format=json] [--integration] [--interpret]
                                           Run the test blocks in a source file
                                           or package, natively or interpreted
                                           as `run` does
    oskac build [path] [-o out] [--format=json]
                                           Check a source file or package and,
                                           if it has no diagnostics, compile it
                                           to an aarch64 Linux executable
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac hook claude --event=post-edit    Check what Claude Code just edited,
                                           reading its hook input on standard
                                           input; exit 2 hands diagnostics back
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
    /// `--interpret`: `oskac run` and `oskac test` use the interpreter
    /// on a host with a native backend too (`native.rs`).
    interpret: bool,
}

/// A command line, parsed: the command and its operands, the flags, the
/// output path `-o` gives, and the arguments after `--`, which belong to
/// the program being run.
#[derive(Debug, Default, PartialEq, Eq)]
struct CommandLine {
    positional: Vec<String>,
    flags: Flags,
    output: Option<String>,
    /// The name `--event=` gives, for `oskac hook`.
    event: Option<String>,
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
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-o" => match args.next() {
                    Some(output) => line.output = Some(output),
                    // `-o` with no path: the usage error says what is wrong.
                    None => line.positional.push(arg),
                },
                "--release" => line.flags.release = true,
                "--integration" => line.flags.integration = true,
                "--interpret" => line.flags.interpret = true,
                event if event.starts_with("--event=") => {
                    line.event = Some(event["--event=".len()..].to_string());
                }
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
                outln!(
                    "{{\"name\":\"oskac\",\"version\":\"{}\"}}",
                    env!("CARGO_PKG_VERSION")
                );
            } else {
                outln!("oskac {}", env!("CARGO_PKG_VERSION"));
            }
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            out!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["lex", path] => lex_file(path, flags),
        ["parse", path] => parse_file(path, flags),
        ["check", rest @ ..] if rest.len() <= 1 => {
            on_target(Command::Check, rest.first(), flags, &line.program_args)
        }
        ["run", rest @ ..] if rest.len() <= 1 => {
            on_target(Command::Run, rest.first(), flags, &line.program_args)
        }
        ["test", rest @ ..] if rest.len() <= 1 => {
            on_target(Command::Test, rest.first(), flags, &line.program_args)
        }
        ["build", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            let command = Command::Build(line.output.as_deref());
            on_target(command, rest.first(), flags, &[])
        }
        ["explain", code] => explain_code(code, flags),
        ["hook", "claude"] if line.program_args.is_empty() => hook::run(line.event.as_deref()),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Write `text` to stdout. A reader that has gone, as in `oskac check
/// --format=json | head -1`, ends `oskac` quietly with 0, as it ends a
/// program `oskac run` runs; any other failure panics, as `print!` does.
pub(crate) fn write_stdout(text: &str) {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(err) => panic!("failed printing to stdout: {err}"),
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
            out!("{}", oska_syntax::module_to_json(&module, path));
            ExitCode::SUCCESS
        }
        Ok(module) => {
            out!("{}", oska_syntax::module_to_tree(&module));
            ExitCode::SUCCESS
        }
        Err(err) if json => {
            outln!("{}", err.locate(path, &src).to_json());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{path}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The commands that take a source file or a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command<'a> {
    Check,
    Run,
    Test,
    /// `oskac build`, with the output path `-o` gives.
    Build(Option<&'a str>),
}

impl Command<'_> {
    fn name(self) -> &'static str {
        match self {
            Command::Check => "check",
            Command::Run => "run",
            Command::Test => "test",
            Command::Build(_) => "build",
        }
    }
}

/// How a command's output names the place of what it reports: the
/// file's path and text for a single file; for a package, its
/// [`oska_check::Locator`], and the package directory as given, which
/// the human output's paths start from (JSON keeps them package-relative).
enum Places<'p> {
    /// What is reported is located already, or has no place.
    Located,
    File {
        path: &'p str,
        src: &'p str,
    },
    Package {
        locator: oska_check::Locator<'p>,
        dir: &'p str,
        json: bool,
    },
}

impl Places<'_> {
    /// `diagnostic`, whose span is an offset into the program, named by
    /// its file and line.
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        match self {
            Places::Located => diagnostic,
            Places::File { path, src } => match diagnostic.location {
                Some(_) => diagnostic,
                None => diagnostic.locate(path, src),
            },
            Places::Package { locator, dir, json } => {
                let located = locator.locate(diagnostic);
                if *json {
                    located
                } else {
                    from_here(dir, located)
                }
            }
        }
    }

    /// The module of the test whose item is at `span` in the program.
    fn module_of(&self, span: Span) -> Option<String> {
        match self {
            Places::Located | Places::File { .. } => None,
            Places::Package { locator, .. } => locator.module_of(span).map(str::to_string),
        }
    }
}

/// `diagnostic`, located in a package in `dir`, with its path written
/// from the current directory.
fn from_here(dir: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    if let Some(location) = &mut diagnostic.location {
        location.path = package::from_here(dir, &location.path);
    }
    diagnostic
}

/// `report`, of the package in `dir`, with every path written from the
/// current directory, for the human output.
fn report_from_here(dir: &str, mut report: oska_check::CheckReport) -> oska_check::CheckReport {
    report.diagnostics = report
        .diagnostics
        .into_iter()
        .map(|d| from_here(dir, d))
        .collect();
    for site in &mut report.audit {
        if let Some(location) = &mut site.location {
            location.path = package::from_here(dir, &location.path);
        }
    }
    for attenuation in &mut report.attenuations {
        if let Some(location) = &mut attenuation.location {
            location.path = package::from_here(dir, &location.path);
        }
    }
    report
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
        out!("{}", report.to_json());
    } else {
        print_diagnostics(&report.diagnostics);
        out!("{}", report.authority_to_human(path));
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
            match fix.applicability {
                Applicability::Machine => eprintln!("    fix: {}", fix.message),
                other => eprintln!("    fix ({}): {}", other.name(), fix.message),
            }
        }
    }
}

/// `oskac run`: check the program and, if the check reports no
/// diagnostic, run it on the real OS with `args` after the program path.
/// A refused program's diagnostics, or the panic that stopped it, go to
/// stderr as `oskac check` prints diagnostics, or one JSON object per
/// line with `--format=json`. The exit code is the program's: 101 after
/// a panic (see `docs/runtime.md`).
fn run_file(path: &str, flags: Flags, args: &[String]) -> ExitCode {
    let json = flags.json;
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let places = Places::File { path, src: &src };
    if native::runs_natively(flags) {
        let checked = oska_check::check_module(&src);
        if let Some(program) = checked
            .program
            .as_ref()
            .filter(|_| checked.diagnostics.is_empty())
        {
            match native::run(
                program,
                &places,
                native::Invocation {
                    argv0: path,
                    args,
                    json,
                },
            ) {
                native::Run::Ran(code) => return code,
                native::Run::Refused(refusal) => {
                    native::note_fallback(refusal, oska_run::refusal(program), &places, json)
                }
            }
        }
    }
    let mut argv = vec![path.to_string()];
    argv.extend_from_slice(args);
    let outcome = oska_run::run(&src, &mut oska_run::RealHost::new(), &argv);
    finish_run(outcome, json, &places)
}

/// Print how a run ended and return the program's exit code. `places`
/// names the file of a panic or a refusal, which `--format=json` prints
/// as `path` and the human output as `path:line:col`.
fn finish_run(outcome: oska_run::RunOutcome, json: bool, places: &Places) -> ExitCode {
    eprint!("{}", outcome.stderr);
    // A panic is printed in the shape of a diagnostic, after the output
    // the program wrote before it.
    let mut reported = outcome.diagnostics;
    reported.extend(outcome.panic);
    let reported: Vec<Diagnostic> = reported.into_iter().map(|d| places.locate(d)).collect();
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
    let places = Places::File { path, src: &src };
    if native::runs_natively(flags) {
        let checked = oska_check::check_module(&src);
        if let Some(program) = checked
            .program
            .as_ref()
            .filter(|_| checked.diagnostics.is_empty())
        {
            match native_test::test_program(program, options, json) {
                native_test::Tested::Report(report) => return finish_test(report, json, &places),
                native_test::Tested::Ended(code) => return code,
                native_test::Tested::Refused(refusal) => {
                    native::note_fallback(refusal, oska_run::test_refusal(program), &places, json)
                }
            }
        }
    }
    let report = oska_run::test(&src, &mut oska_run::RealHost::new(), options);
    finish_test(report, json, &places)
}

/// Print a test report and return its exit code. `places` names the file
/// of each failure and refusal, as for [`finish_run`], and each test's
/// module in a package.
fn finish_test(mut report: oska_run::TestReport, json: bool, places: &Places) -> ExitCode {
    for test in &mut report.tests {
        test.module = places.module_of(test.span);
        test.failure = test.failure.take().map(|d| places.locate(d));
    }
    report.diagnostics = report
        .diagnostics
        .drain(..)
        .map(|d| places.locate(d))
        .collect();
    if json {
        outln!("{}", report.to_json());
    } else if !report.diagnostics.is_empty() {
        print_diagnostics(&report.diagnostics);
    } else {
        out!("{}", report.to_human(""));
    }
    ExitCode::from(u8::try_from(report.exit_code()).unwrap_or(u8::MAX))
}

// ---- Packages ------------------------------------------------------------------

/// `oskac check`, `run` or `test` on `path`: a package directory (the
/// current one with no path), or a file, which keeps its single-file
/// behaviour, with a hint when it is part of a package.
fn on_target(command: Command, path: Option<&&str>, flags: Flags, args: &[String]) -> ExitCode {
    match package::target(path.copied()) {
        package::Target::File(file) => {
            let code = match command {
                Command::Check => check_file(&file, flags),
                Command::Run => run_file(&file, flags, args),
                Command::Test => test_file(&file, flags),
                Command::Build(output) => build::file(&file, flags, output),
            };
            hint_package(&file, command.name());
            code
        }
        package::Target::Dir(dir) => on_package(command, &dir, flags, args),
    }
}

/// `command` on the package in `dir`. `oskac run` refuses a library, a
/// package without `src/main.osk`.
fn on_package(command: Command, dir: &str, flags: Flags, args: &[String]) -> ExitCode {
    let json = flags.json;
    let (package, refused) = match package::load(dir, command.name()) {
        package::Loaded::Package { package, refused } => (package, refused),
        package::Loaded::Refused(diagnostics) => {
            return refuse(command, diagnostics, json, dir);
        }
        package::Loaded::Unreadable(message) => {
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };
    if matches!(command, Command::Run | Command::Build(_)) && package.is_library() {
        return refuse(command, vec![oska_check::run_of_library(dir)], json, dir);
    }
    let options = oska_check::CheckOptions {
        release: command == Command::Check && flags.release,
    };
    let mut report = oska_check::check_package(&package, options);
    if !refused.is_empty() {
        report.diagnostics.splice(0..0, refused);
        report.program = None;
    }
    if command == Command::Check {
        let report = if json {
            report
        } else {
            report_from_here(dir, report)
        };
        return print_report(&report, json, dir);
    }
    let Some(program) = &report.program else {
        return refuse(command, report.diagnostics, json, dir);
    };
    let places = Places::Package {
        locator: oska_check::Locator::new(&package),
        dir,
        json,
    };
    if let Command::Build(output) = command {
        return build::package(program, &places, dir, output, json);
    }
    let mut host = oska_run::RealHost::new();
    if command == Command::Run {
        if native::runs_natively(flags) {
            match native::run(
                program,
                &places,
                native::Invocation {
                    argv0: dir,
                    args,
                    json,
                },
            ) {
                native::Run::Ran(code) => return code,
                native::Run::Refused(refusal) => {
                    native::note_fallback(refusal, oska_run::refusal(program), &places, json)
                }
            }
        }
        let mut argv = vec![dir.to_string()];
        argv.extend_from_slice(args);
        let outcome = oska_run::run_program(program, &mut host, &argv);
        finish_run(outcome, json, &places)
    } else {
        let options = oska_run::TestOptions {
            integration: flags.integration,
        };
        if native::runs_natively(flags) {
            match native_test::test_program(program, options, json) {
                native_test::Tested::Report(tests) => return finish_test(tests, json, &places),
                native_test::Tested::Ended(code) => return code,
                native_test::Tested::Refused(refusal) => {
                    native::note_fallback(refusal, oska_run::test_refusal(program), &places, json)
                }
            }
        }
        let tests = oska_run::test_program(program, &mut host, options);
        finish_test(tests, json, &places)
    }
}

/// Refuse `command` on the package in `dir` with `diagnostics`, already
/// located, printed as the command prints a refused file's: as a check
/// report, as a run's refusal, or as a test report's diagnostics. Their
/// paths are written from the current directory in the human output.
fn refuse(command: Command, diagnostics: Vec<Diagnostic>, json: bool, dir: &str) -> ExitCode {
    let diagnostics: Vec<Diagnostic> = if json {
        diagnostics
    } else {
        diagnostics.into_iter().map(|d| from_here(dir, d)).collect()
    };
    let places = Places::Located;
    match command {
        Command::Check => {
            let report = oska_check::CheckReport {
                diagnostics,
                ..oska_check::CheckReport::default()
            };
            print_report(&report, json, dir)
        }
        Command::Run => {
            let outcome = oska_run::RunOutcome {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                panic: None,
                diagnostics,
            };
            finish_run(outcome, json, &places)
        }
        Command::Test => {
            let report = oska_run::TestReport {
                tests: Vec::new(),
                diagnostics,
            };
            finish_test(report, json, &places)
        }
        Command::Build(_) => build::refuse(&diagnostics, json, ExitCode::FAILURE),
    }
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
            outln!("{}", explain::to_json(entry));
        } else {
            out!("{}", explain::to_human(entry));
        }
        return ExitCode::SUCCESS;
    }
    let suggestion = explain::closest(&entries, code);
    if json {
        outln!("{}", explain::unknown_to_json(code, suggestion));
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
                outln!(
                    "{:>5}..{:<5} {:?}",
                    token.span.start,
                    token.span.end,
                    token.kind
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) if json => {
            outln!("{}", err.locate(path, &src).to_json());
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
                integration: true,
                interpret: false,
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
    fn dash_o_takes_the_next_argument_as_the_output_path() {
        let line = parse(&["build", "-o", "out/app", "f.osk"]);
        assert_eq!(line.positional, ["build", "f.osk"]);
        assert_eq!(line.output.as_deref(), Some("out/app"));
        // With nothing after it, `-o` is left positional: a usage error.
        let line = parse(&["build", "f.osk", "-o"]);
        assert_eq!(line.positional, ["build", "f.osk", "-o"]);
        assert_eq!(line.output, None);
    }

    #[test]
    fn another_format_means_human_output_and_an_unknown_flag_is_positional() {
        let line = parse(&["check", "f.osk", "--format=text", "--nope"]);
        assert!(!line.flags.json);
        assert_eq!(line.positional, ["check", "f.osk", "--nope"]);
    }
}
