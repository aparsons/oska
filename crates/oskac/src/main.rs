// SPDX-License-Identifier: Apache-2.0
//! `oskac`: the Oska compiler command-line interface.

use authority_gate::Gate;
use oska_syntax::{Applicability, Diagnostic, Frame, Span};
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

/// Whether this `oskac` is a debug build, the only kind whose programs
/// honor the DNS test variables `OSKA_DNS_HOSTS`, `OSKA_DNS_RESOLV_CONF` and
/// `OSKA_DNS_PORT` (ADR 0026). The interpreter's host is told it, and the
/// native compiler bakes it into each executable; `oskac release` always
/// builds without it.
pub(crate) const DNS_TEST_HOOKS: bool = cfg!(debug_assertions);

mod authority_gate;
mod build;
mod cache;
mod crash;
mod doc;
mod explain;
mod fence;
mod fix;
mod heap;
mod hook;
mod json;
mod limits;
mod native;
mod native_test;
mod new;
mod package;
mod release;
mod self_check;
mod self_cmd;
mod semver;
mod sha256;
mod stdin;
mod update_checker;
mod verdict;

const USAGE: &str = "\
oskac - the Oska compiler

USAGE:
    oskac lex <file.osk> [--format=json]   Print the tokens in a source file
    oskac parse <file.osk> [--format=json] Print the syntax tree of a source file
    oskac check [path | -] [--format=json] [--release] [--no-cache]
                [--baseline=<file> | --update-baseline=<file> | --grant=<file>]
                                           Type-check a source file, a package
                                           directory (the current one with no
                                           path), or one module on standard
                                           input (-), and report each
                                           function's authority; compare it
                                           with a saved baseline or an
                                           authority grant (exit code 3 when
                                           it goes past), or write a baseline
    oskac run [path | -] [--format=json] [--interpret] [--no-cache]
              [--grant=<file>] [--max-steps=<n>] [--timeout=<ms>]
              [--max-output=<bytes>] [--max-memory=<bytes>]
              [-- args...]
                                           Check a source file, package or
                                           module on standard input (-) and,
                                           if it has no diagnostics and stays
                                           within the --grant, run it:
                                           natively on aarch64 and x86_64
                                           Linux, on the interpreter elsewhere
                                           or with --interpret; stop it with
                                           P0038 after the steps --max-steps
                                           gives, with P0039 after the
                                           milliseconds --timeout gives, or
                                           with P0041 before it writes more
                                           than the bytes --max-output gives
                                           to stdout and stderr together,
                                           and with P0040 once its values
                                           take more than --max-memory bytes
    oskac test [path] [--format=json] [--integration] [--interpret] [--no-cache]
               [--filter <substring>] [--update]
                                           Run the test blocks in a source file
                                           or package, natively or interpreted
                                           as `run` does; with --filter, only
                                           those whose names contain the
                                           substring; with --update, write
                                           new and changed snapshots
    oskac build [path] [-o out] [--target <platform>|all] [--format=json] [--no-cache]
                                           Check a source file or package and,
                                           if it has no diagnostics, compile it
                                           to an executable for the platform
                                           (by default the host's when it has a
                                           backend, else linux-aarch64), or for
                                           every platform with a backend into
                                           the -o directory
    oskac release [path] --version <X.Y.Z> --key <ssh-key>
                  [-o dir] [--format=json] [--no-cache]
                                           Build a source file or package for
                                           every platform into one directory
                                           (by default <name>-v<X.Y.Z>), with
                                           a SHA256SUMS signed by the SSH key
    oskac fix [path] [--dry-run] [--placeholders] [--release] [--format=json]
                                           Apply the machine fixes to a source
                                           file or package, checking again
                                           after each (as the release check
                                           does, with --release), and write
                                           each changed file once
    oskac explain <code> [--format=json]   Explain a diagnostic code, offline
    oskac doc [module | item] [--format=json]
                                           Print the API reference of the
                                           prelude and the std modules,
                                           offline, or of one module
                                           (std.sql) or item (List.push)
    oskac new <name> [--lib] [--no-agent-files] [--format=json]
                                           Lay out a new package in the
                                           directory <name>: oska.toml,
                                           src/main.osk (src/lib.osk with
                                           --lib), and the agent-setup
                                           AGENTS.md and CLAUDE.md
    oskac hook claude --event=post-edit [--format=json]
                                           Check what Claude Code just edited,
                                           reading its hook input on standard
                                           input; exit 2 hands diagnostics back
    oskac hook claude --event=stop [--format=json] [targets...]
                                           Run the release check and the tests
                                           before Claude Code may finish; exit
                                           2 blocks it with what failed
    oskac cache info [--format=json]       Report the build cache, which check,
                                           run, test and build reuse unchanged
                                           results from: its folder, entries,
                                           size, limit, hits and misses
    oskac cache clean [--format=json]      Empty the build cache
    oskac self info [--format=json]        Report this installation: version,
                                           platform, executable, Oska home and
                                           install receipt
    oskac self uninstall [--format=json]   Remove this managed install, as the
                                           installer's uninstall does
    oskac self check [--format=json] [--no-identify]
                                           Ask oskalang.dev whether a newer
                                           release exists, verified against the
                                           pinned release key, and whether this
                                           binary is the published one; the
                                           only command that uses the network
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
";

/// The flags `oskac` takes, whichever command they are given to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    /// `--format=json`: machine-readable output. Any other `--format=`
    /// means the default, human-readable output.
    json: bool,
    /// `--release`: `oskac check` runs the release check, rejecting `dbg`,
    /// `todo` and unused authority, and `oskac fix` checks as it does.
    release: bool,
    /// `--integration`: `oskac test` runs `#[integration]` tests too.
    integration: bool,
    /// `--interpret`: `oskac run` and `oskac test` use the interpreter
    /// on a host with a native backend too (`native.rs`).
    interpret: bool,
    /// `--dry-run`: `oskac fix` prints the diff it would write, and
    /// writes nothing.
    dry_run: bool,
    /// `--placeholders`: `oskac fix` applies `placeholders` fixes too.
    placeholders: bool,
    /// `--no-cache`: `oskac check`, `run`, `test` and `build` neither
    /// read nor write the build cache (`cache.rs`).
    no_cache: bool,
    /// `--lib`: `oskac new` writes a library, `src/lib.osk` and no `main`.
    lib: bool,
    /// `--no-agent-files`: `oskac new` leaves out `AGENTS.md` and
    /// `CLAUDE.md`.
    no_agent_files: bool,
    /// `--no-identify`: `oskac self check` does not compare this binary with
    /// the release's signed checksums.
    no_identify: bool,
    /// `--update`: `oskac test` writes new and changed snapshots
    /// (`oska_run::snapshots`) instead of failing on them.
    update: bool,
}

/// A command line, parsed: the command and its operands, the flags, the
/// output path `-o` gives, the target platform `--target` names, and the
/// arguments after `--`, which belong to the program being run.
#[derive(Debug, Default, PartialEq, Eq)]
struct CommandLine {
    positional: Vec<String>,
    flags: Flags,
    output: Option<String>,
    /// `--baseline=<file>`: `oskac check` compares authority with it.
    baseline: Option<String>,
    /// `--update-baseline=<file>`: `oskac check` writes it.
    update_baseline: Option<String>,
    /// `--grant=<file>`: `oskac check` and `oskac run` compare authority
    /// with it.
    grant: Option<String>,
    /// The name `--event=` gives, for `oskac hook`.
    event: Option<String>,
    target: Option<String>,
    /// `--version <X.Y.Z>`, for `oskac release`; for any other command
    /// `--version` is the command that prints `oskac`'s own.
    version: Option<String>,
    /// `--key <file>`: the SSH signing key, for `oskac release`.
    key: Option<String>,
    /// `--filter <substring>`: `oskac test` runs only the tests whose
    /// names contain it.
    filter: Option<String>,
    /// Why `--filter` is missing its substring, empty or repeated: a
    /// usage error.
    filter_error: Option<&'static str>,
    /// The run limits (`--max-steps=<n>`, `--timeout=<ms>`,
    /// `--max-output=<bytes>`, `--max-memory=<bytes>`), which only `oskac
    /// run` takes.
    limits: oska_check::limits::Limits,
    /// Why a limit flag is malformed or repeated: a usage error.
    limit_error: Option<String>,
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
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            if let Some(taken) = limits::take_flag(&mut line.limits, &arg) {
                if let Err(error) = taken {
                    line.limit_error.get_or_insert(error);
                }
                continue;
            }
            match arg.as_str() {
                "-o" => match args.next() {
                    Some(output) => line.output = Some(output),
                    // `-o` with no path: the usage error says what is wrong.
                    None => line.positional.push(arg),
                },
                "--target" => match args.next() {
                    Some(target) => line.target = Some(target),
                    None => line.positional.push(arg),
                },
                "--version" if line.positional.first().is_some_and(|c| c == "release") => {
                    match args.next() {
                        Some(version) => line.version = Some(version),
                        None => line.positional.push(arg),
                    }
                }
                version if version.starts_with("--version=") => {
                    line.version = Some(version["--version=".len()..].to_string());
                }
                "--key" => match args.next() {
                    Some(key) => line.key = Some(key),
                    None => line.positional.push(arg),
                },
                key if key.starts_with("--key=") => {
                    line.key = Some(key["--key=".len()..].to_string());
                }
                // A flag after `--filter` is not its substring.
                "--filter" => match args.next_if(|next| !next.starts_with("--")) {
                    Some(filter) => line.take_filter(filter),
                    None => line.refuse_filter("--filter needs a substring after it"),
                },
                filter if filter.starts_with("--filter=") => {
                    line.take_filter(filter["--filter=".len()..].to_string());
                }
                target if target.starts_with("--target=") => {
                    line.target = target.strip_prefix("--target=").map(str::to_string);
                }
                "--release" => line.flags.release = true,
                "--integration" => line.flags.integration = true,
                "--interpret" => line.flags.interpret = true,
                "--dry-run" => line.flags.dry_run = true,
                "--placeholders" => line.flags.placeholders = true,
                "--no-cache" => line.flags.no_cache = true,
                "--lib" => line.flags.lib = true,
                "--no-agent-files" => line.flags.no_agent_files = true,
                "--update" => line.flags.update = true,
                "--no-identify" => line.flags.no_identify = true,
                event if event.starts_with("--event=") => {
                    line.event = Some(event["--event=".len()..].to_string());
                }
                format if format.starts_with("--format=") => {
                    line.flags.json = format == "--format=json";
                }
                // A baseline flag given twice is recorded with an empty
                // file name, which `main` refuses as a usage error.
                flag if flag.starts_with("--baseline=") => {
                    line.baseline =
                        value_unless_repeated(&line.baseline, &flag["--baseline=".len()..]);
                }
                flag if flag.starts_with("--grant=") => {
                    line.grant = value_unless_repeated(&line.grant, &flag["--grant=".len()..]);
                }
                flag if flag.starts_with("--update-baseline=") => {
                    line.update_baseline = value_unless_repeated(
                        &line.update_baseline,
                        &flag["--update-baseline=".len()..],
                    );
                }
                _ => line.positional.push(arg),
            }
        }
        line
    }
}

impl CommandLine {
    /// Record `filter`, the substring `--filter` gives, unless it is
    /// empty or a filter was given before.
    fn take_filter(&mut self, filter: String) {
        if self.filter.is_some() {
            self.refuse_filter("--filter is given more than once");
        } else if filter.is_empty() {
            self.refuse_filter("--filter takes a substring that is not empty");
        } else {
            self.filter = Some(filter);
        }
    }

    /// Record why `--filter` is a usage error, if it is not one already.
    fn refuse_filter(&mut self, error: &'static str) {
        self.filter_error.get_or_insert(error);
    }
}

/// The value of a flag that takes a file, `value`, or an empty one when
/// the flag was given before (`previous`).
fn value_unless_repeated(previous: &Option<String>, value: &str) -> Option<String> {
    match previous {
        Some(_) => Some(String::new()),
        None => Some(value.to_string()),
    }
}

fn main() -> ExitCode {
    crash::install();
    let line = CommandLine::parse(std::env::args().skip(1).collect());
    let flags = line.flags;
    let positional: Vec<&str> = line.positional.iter().map(String::as_str).collect();
    if let Some(error) = &line.limit_error {
        eprintln!("error: {error}");
        return ExitCode::from(2);
    }
    // Only `run` takes a limit: `check` is never mistaken for a run.
    if !line.limits.is_empty() && positional.first() != Some(&"run") {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    let gate = match (
        line.baseline.as_deref(),
        line.update_baseline.as_deref(),
        line.grant.as_deref(),
    ) {
        (None, None, None) => None,
        (Some(file), None, None) if !file.is_empty() => Some(Gate::Baseline(file)),
        (None, Some(file), None) if !file.is_empty() => Some(Gate::UpdateBaseline(file)),
        (None, None, Some(file)) if !file.is_empty() => Some(Gate::Grant(file)),
        // More than one, or an empty file name.
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Only `check` takes a baseline; `check` and `run` take a grant.
    let takes_gate = match gate {
        None => true,
        Some(Gate::Grant(_)) => matches!(positional.first(), Some(&("check" | "run"))),
        Some(_) => positional.first() == Some(&"check"),
    };
    if !takes_gate {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    let run = RunOptions {
        limits: line.limits,
        // The grant `run` compares with, before it runs anything.
        grant: match gate {
            Some(Gate::Grant(file)) => Some(file),
            _ => None,
        },
    };
    if flags.no_cache
        && !matches!(
            positional.first(),
            Some(&("check" | "run" | "test" | "build" | "release"))
        )
    {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    // Only `new` lays out a library or leaves out the agent files.
    if (flags.lib || flags.no_agent_files) && positional.first() != Some(&"new") {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    // Only `release` has a version and a key.
    if (line.version.is_some() || line.key.is_some()) && positional.first() != Some(&"release") {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    if let Some(error) = line.filter_error {
        if flags.json {
            outln!(
                "{{\"ok\":false,\"error\":\"usage\",\"message\":\"{}\"}}",
                oska_syntax::json_escape(error)
            );
        } else {
            eprintln!("error: {error}");
        }
        return ExitCode::from(2);
    }
    // Only `self check` skips the identity check.
    if flags.no_identify && positional.as_slice() != ["self", "check"] {
        if flags.json {
            outln!(
                "{{\"ok\":false,\"error\":\"usage\",\"message\":\"--no-identify belongs to `oskac self check` alone\"}}"
            );
        } else {
            eprint!("{USAGE}");
        }
        return ExitCode::from(2);
    }
    // Only `test` takes a filter or writes snapshots.
    if (line.filter.is_some() || flags.update) && positional.first() != Some(&"test") {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    let filter = line.filter.as_deref();
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
        // `self` takes no operand, -o, --target or program arguments.
        ["self", rest @ ..] => {
            let bare =
                line.output.is_none() && line.target.is_none() && line.program_args.is_empty();
            match rest {
                ["info"] if bare => self_cmd::info(flags.json),
                ["uninstall"] if bare => self_cmd::uninstall(flags.json),
                ["check"] if bare => self_check::check(flags.json, !flags.no_identify),
                _ => self_cmd::usage(flags.json),
            }
        }
        // `cache` takes no operand, -o, --target or program arguments.
        ["cache", rest @ ..] => {
            let bare =
                line.output.is_none() && line.target.is_none() && line.program_args.is_empty();
            match rest {
                ["info"] if bare => cache::info(flags.json),
                ["clean"] if bare => cache::clean(flags.json),
                _ => cache::usage(),
            }
        }
        // Only `build` has a target platform: `run` and `test` use the
        // host's.
        [command, ..] if line.target.is_some() && *command != "build" => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        // Only `check` and `run` read a module from standard input.
        ["lex" | "parse" | "test" | "build" | "fix", stdin::DASH] => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        ["check", stdin::DASH] => stdin::check(flags, gate),
        ["run", stdin::DASH] => stdin::run(flags, run, &line.program_args),
        ["lex", path] => lex_file(path, flags),
        ["parse", path] => parse_file(path, flags),
        ["check", rest @ ..] if rest.len() <= 1 => on_target(
            Command::Check(gate),
            rest.first(),
            flags,
            &line.program_args,
        ),
        ["run", rest @ ..] if rest.len() <= 1 => {
            on_target(Command::Run(run), rest.first(), flags, &line.program_args)
        }
        ["test", rest @ ..] if rest.len() <= 1 => on_target(
            Command::Test(filter),
            rest.first(),
            flags,
            &line.program_args,
        ),
        ["build", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            let target = match build::TargetChoice::named(line.target.as_deref()) {
                Ok(target) => target,
                Err(refusal) => return build::refuse(&[refusal], flags.json, ExitCode::from(2)),
            };
            let options = build::Options {
                output: line.output.as_deref(),
                target,
            };
            on_target(Command::Build(options), rest.first(), flags, &[])
        }
        ["release", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            let options = match release::options(
                line.output.as_deref(),
                line.version.as_deref(),
                line.key.as_deref(),
            ) {
                Ok(options) => options,
                Err((refusal, code)) => return release::refuse(&[refusal], flags.json, code),
            };
            on_target(Command::Release(options), rest.first(), flags, &[])
        }
        ["fix", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            fix::command(rest.first().copied(), flags)
        }
        ["explain", code] => explain_code(code, flags),
        ["doc", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            doc_command(rest.first().copied(), flags)
        }
        ["new", name] if line.output.is_none() && line.program_args.is_empty() => {
            let options = new::Options {
                lib: flags.lib,
                no_agent_files: flags.no_agent_files,
            };
            new::command(name, options, flags.json)
        }
        ["new", ..] => new::usage(flags.json),
        ["hook", "claude", targets @ ..] if line.program_args.is_empty() => {
            hook::run(line.event.as_deref(), targets, flags.json)
        }
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

/// What `oskac run` runs a program under: its run limits, and the
/// authority grant (`--grant=<file>`) it must stay within, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunOptions<'a> {
    limits: oska_check::limits::Limits,
    grant: Option<&'a str>,
}

/// The commands that take a source file or a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command<'a> {
    /// `oskac check`, with its authority gate, if any.
    Check(Option<Gate<'a>>),
    /// `oskac run`, under its limits and within its grant.
    Run(RunOptions<'a>),
    /// `oskac test`, with its `--filter`, if any.
    Test(Option<&'a str>),
    /// `oskac build`, with the output path `-o` gives and its target platforms.
    Build(build::Options<'a>),
    /// `oskac release`, with its output directory, version and key.
    Release(release::Options<'a>),
}

impl Command<'_> {
    fn name(self) -> &'static str {
        match self {
            Command::Check(_) => "check",
            Command::Run(..) => "run",
            Command::Test(_) => "test",
            Command::Build(_) => "build",
            Command::Release(_) => "release",
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

    /// `frame` of a panic's call stack, whose span is an offset into the
    /// program, placed as [`Places::locate`] places a diagnostic's frames.
    fn locate_frame(&self, frame: Frame) -> Frame {
        match self {
            Places::Located => frame,
            Places::File { path, src } => frame.locate(path, src),
            Places::Package { locator, dir, json } => {
                let mut located = locator.locate_frame(frame);
                if !*json {
                    frame_from_here(dir, &mut located);
                }
                located
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
    if let Some(frames) = &mut diagnostic.frames {
        for frame in &mut frames.frames {
            frame_from_here(dir, frame);
        }
    }
    diagnostic
}

/// `frame`, located in a package in `dir`, with its path written from
/// the current directory.
fn frame_from_here(dir: &str, frame: &mut Frame) {
    if let Some(location) = &mut frame.location {
        location.path = package::from_here(dir, &location.path);
    }
}

/// A native executable's panic reports name places as `oskac` reports
/// them: placed by [`Places`].
impl oska_native::Locate for Places<'_> {
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        Places::locate(self, diagnostic)
    }

    fn locate_frame(&self, frame: Frame) -> Frame {
        Places::locate_frame(self, frame)
    }

    fn places(&self) -> bool {
        !matches!(self, Places::Located)
    }
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
/// A result with no diagnostics is reused from the build cache, `store`,
/// when its inputs are unchanged; with a baseline or a grant it is always checked.
fn check_file(
    path: &str,
    flags: Flags,
    gate: Option<Gate>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    check_source(path, &src, flags, gate, store)
}

/// [`check_file`] on `src`, the text of the module `path` names.
fn check_source(
    path: &str,
    src: &str,
    Flags { json, release, .. }: Flags,
    gate: Option<Gate>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let check = || oska_check::check_file(path, src, oska_check::CheckOptions { release });
    match gate {
        Some(_) => finish_check(&check(), json, path, gate),
        None => cache::Cached::new(store, cache::file_sources(path, src))
            .check(&check_flags(json, release), || {
                render_report(&check(), json, path)
            })
            .emit(),
    }
}

/// The flags that change what `oskac check` prints, for its cache key.
fn check_flags(json: bool, release: bool) -> [&'static [u8]; 2] {
    [
        if json { b"json" } else { b"text" },
        if release { b"release" } else { b"debug" },
    ]
}

/// Print a check report, then compare it with a baseline or write one
/// when `gate` says so, and return the exit code.
fn finish_check(
    report: &oska_check::CheckReport,
    json: bool,
    path: &str,
    gate: Option<Gate>,
) -> ExitCode {
    match gate {
        Some(gate) => authority_gate::check(report, json, path, gate),
        None => print_report(report, json, path),
    }
}

/// Print a check report as `oskac check` does, and return its exit code.
/// `path` names the unlocated entries' file in the human report.
fn print_report(report: &oska_check::CheckReport, json: bool, path: &str) -> ExitCode {
    render_report(report, json, path).emit()
}

/// What [`print_report`] prints, and the exit code.
fn render_report(report: &oska_check::CheckReport, json: bool, path: &str) -> cache::Printed {
    let verdict = verdict::Verdict::of(report, None);
    let (stdout, stderr) = if json {
        (verdict::check_json(report, None, verdict), String::new())
    } else {
        (
            report.authority_to_human(path),
            diagnostics_text(&report.diagnostics),
        )
    };
    cache::Printed {
        stdout,
        stderr,
        code: verdict.exit_code(),
    }
}

/// Print diagnostics to stderr, each after its location,
/// `path:line:col`, and with its fix, as `oskac check` does.
fn print_diagnostics(diagnostics: &[Diagnostic]) {
    eprint!("{}", diagnostics_text(diagnostics));
}

/// What [`print_diagnostics`] prints.
fn diagnostics_text(diagnostics: &[Diagnostic]) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for diagnostic in diagnostics {
        let _ = match &diagnostic.location {
            Some(location) => writeln!(text, "{location}: {diagnostic}"),
            None => writeln!(text, "{diagnostic}"),
        };
        if let Some(fix) = &diagnostic.fix {
            let _ = match fix.applicability {
                Applicability::Machine => writeln!(text, "    fix: {}", fix.message),
                other => writeln!(text, "    fix ({}): {}", other.name(), fix.message),
            };
        }
        // A panic's call stack, under its line.
        if let Some(frames) = &diagnostic.frames {
            text.push_str(&frames.human());
        }
    }
    text
}

/// `oskac run`: check the program and, if the check reports no
/// diagnostic, run it on the real OS with `args` after the program path.
/// A refused program's diagnostics, or the panic that stopped it, go to
/// stderr as `oskac check` prints diagnostics, or one JSON object per
/// line with `--format=json`. The exit code is the program's: 101 after
/// a panic (see `docs/runtime.md`).
fn run_file(
    path: &str,
    flags: Flags,
    run: RunOptions,
    args: &[String],
    store: Option<&cache::Store>,
) -> ExitCode {
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    run_source(path, &src, flags, run, args, store, false)
}

/// [`run_file`] on `src`, the text of the module `path` names; with
/// `stdin_closed`, every read of the program's standard input is end of
/// file at once. The program is checked once, and that one check decides
/// both the grant and what runs.
fn run_source(
    path: &str,
    src: &str,
    flags: Flags,
    RunOptions { limits, grant }: RunOptions,
    args: &[String],
    store: Option<&cache::Store>,
    stdin_closed: bool,
) -> ExitCode {
    let json = flags.json;
    let cached = cache::Cached::new(store, cache::file_sources(path, src));
    let places = Places::File { path, src };
    let checked = oska_check::check_file(path, src, oska_check::CheckOptions { release: false });
    let program = match &checked.program {
        Some(program) if checked.diagnostics.is_empty() => program,
        _ => {
            let outcome = oska_run::RunOutcome {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                panic: None,
                diagnostics: checked.diagnostics,
            };
            return finish_run(outcome, json, &places);
        }
    };
    if let Some(code) = grant.and_then(|file| authority_gate::refuse_run(&checked, json, file)) {
        return code;
    }
    if let Some(target) = native::native_target(flags) {
        match native::run(
            program,
            target,
            &places,
            native::Invocation {
                argv0: path,
                args,
                json,
                limits,
                stdin_closed,
            },
            &cached,
        ) {
            native::Run::Ran(code) => return code,
            native::Run::Refused(refusal) => {
                if let Some(refusal) = native::gate(refusal, oska_run::refusal(program), &places) {
                    return refuse_unsupported(Gated::Run, refusal, json);
                }
            }
        }
    }
    let mut argv = vec![path.to_string()];
    argv.extend_from_slice(args);
    let mut host = if stdin_closed {
        oska_run::RealHost::without_stdin().with_dns_test_hooks(DNS_TEST_HOOKS)
    } else {
        oska_run::RealHost::new().with_dns_test_hooks(DNS_TEST_HOOKS)
    };
    let outcome = oska_run::run_program_with_limits(program, &mut host, &argv, limits);
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
fn test_file(
    path: &str,
    flags: Flags,
    filter: Option<&str>,
    store: Option<&cache::Store>,
) -> ExitCode {
    let Flags {
        json, integration, ..
    } = flags;
    let Some(src) = read_source(path) else {
        return ExitCode::FAILURE;
    };
    let cached = cache::Cached::new(store, cache::file_sources(path, &src));
    let options = oska_run::TestOptions {
        integration,
        filter,
    };
    let places = Places::File { path, src: &src };
    if let Some(target) = native::native_target(flags) {
        let checked = oska_check::check_module(&src);
        if let Some(program) = checked
            .program
            .as_ref()
            .filter(|_| checked.diagnostics.is_empty())
        {
            match native_test::test_program(program, target, options, json, &cached) {
                native_test::Tested::Report(mut report) => {
                    settle_snapshots(&mut report, path, &places, flags);
                    return finish_test(report, json, &places);
                }
                native_test::Tested::Ended(code) => return code,
                native_test::Tested::Refused(refusal) => {
                    if let Some(refusal) =
                        native::gate(refusal, oska_run::test_refusal(program), &places)
                    {
                        return refuse_unsupported(Gated::Test, refusal, json);
                    }
                }
            }
        }
    }
    let mut report = oska_run::test(
        &src,
        &mut oska_run::RealHost::new().with_dns_test_hooks(DNS_TEST_HOOKS),
        options,
    );
    settle_snapshots(&mut report, path, &places, flags);
    finish_test(report, json, &places)
}

/// Compare the snapshots the tests of `report` recorded with their files,
/// or with `--update` write the new and changed ones
/// (`oska_run::snapshots::settle`). `target` is the single file or the
/// package directory as given. The human report names each snapshot
/// written from the current directory.
fn settle_snapshots(
    report: &mut oska_run::TestReport,
    target: &str,
    places: &Places,
    flags: Flags,
) {
    let dir = match places {
        Places::Package { locator, .. } => {
            let file_of = |span| package_test_file(locator, span);
            settle_in(report, target, &file_of, flags.update);
            target.to_string()
        }
        _ => {
            let (dir, stem) = file_snapshots(target);
            settle_in(report, &dir, &|_| Some(stem.clone()), flags.update);
            dir
        }
    };
    if !flags.json {
        for written in report.snapshots.iter_mut().flatten() {
            written.path = package::from_here(&dir, &written.path);
        }
    }
}

/// [`oska_run::snapshots::settle`] under the directory `dir`, the current
/// one when it is empty.
pub(crate) fn settle_in(
    report: &mut oska_run::TestReport,
    dir: &str,
    file_of: &dyn Fn(Span) -> Option<String>,
    update: bool,
) {
    let dir = if dir.is_empty() { "." } else { dir };
    oska_run::snapshots::settle(report, std::path::Path::new(dir), file_of, update);
}

/// Where the snapshots of the single file `path` go: its directory, as
/// `path` names it, and its `<test-file>`, its name without `.osk`.
pub(crate) fn file_snapshots(path: &str) -> (String, String) {
    let path = std::path::Path::new(path);
    let dir = path
        .parent()
        .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned());
    let stem = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    (dir, stem)
}

/// The `<test-file>` of a package's snapshots for the test at `span`: its
/// file under `src/` without `.osk`, such as `lib` or `util/text`, or
/// `None` when the span is in no file of the package under `src/`.
pub(crate) fn package_test_file(locator: &oska_check::Locator, span: Span) -> Option<String> {
    let located = locator.locate(Diagnostic::new(oska_run::codes::SNAPSHOT_FAILED, "", span));
    let path = located.location?.path;
    let module = path.strip_prefix("src/")?.strip_suffix(".osk")?;
    (!module.is_empty()).then(|| module.to_string())
}

/// What the parity gate stopped: a run or the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gated {
    Run,
    Test,
}

/// The parity gate's refusal (#181): `oskac run` or `oskac test` of a
/// program the native backend cannot compile yet (`refusal`, `E0390`,
/// already located) reports it as `oskac build` does, as an error with
/// exit code 2, and runs nothing. `--interpret` still runs it.
fn refuse_unsupported(gated: Gated, refusal: Diagnostic, json: bool) -> ExitCode {
    let code = oska_run::UNSUPPORTED_EXIT_CODE;
    match gated {
        // A test report's own exit code is 2 only for the interpreter's
        // codes, so the gate's is given here.
        Gated::Test => {
            let report = oska_run::TestReport {
                tests: Vec::new(),
                diagnostics: vec![refusal],
                filtered: None,
                snapshots: None,
            };
            let _ = finish_test(report, json, &Places::Located);
            ExitCode::from(u8::try_from(code).unwrap_or(2))
        }
        Gated::Run => finish_run(
            oska_run::RunOutcome {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: code,
                panic: None,
                diagnostics: vec![refusal],
            },
            json,
            &Places::Located,
        ),
    }
}

/// Print a test report and return its exit code. `places` names the file
/// of each failure and refusal, as for [`finish_run`], and each test's
/// module in a package. A `--filter` that left out every test is an
/// error, so a typo is never a passing run.
fn finish_test(mut report: oska_run::TestReport, json: bool, places: &Places) -> ExitCode {
    if let (Some(filtered), true) = (&report.filtered, report.tests.is_empty()) {
        return no_test_matches(filtered, json);
    }
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

/// The error of a `--filter` that matched none of the program's tests,
/// as text on stderr or as JSON on stdout. A program with no tests at
/// all is told apart, so the filter is not blamed for it.
fn no_test_matches(filtered: &oska_run::Filtered, json: bool) -> ExitCode {
    let oska_run::Filtered {
        substring,
        left_out,
    } = filtered;
    let shown = oska_run::escape_control(substring);
    let (code, message) = if *left_out == 0 {
        (
            "no_tests",
            format!("the program has no tests, so the filter `{shown}` matches none"),
        )
    } else {
        (
            "no_test_matches",
            format!("no test name contains `{shown}`; {left_out} tests filtered out"),
        )
    };
    if json {
        outln!(
            "{{\"ok\":false,\"error\":\"{code}\",\"message\":\"{}\",\"filter\":\"{}\",\"filtered\":{left_out}}}",
            oska_syntax::json_escape(&message),
            oska_syntax::json_escape(substring)
        );
    } else {
        eprintln!("error: {message}");
    }
    ExitCode::FAILURE
}

// ---- Packages ------------------------------------------------------------------

/// `oskac check`, `run` or `test` on `path`: a package directory (the
/// current one with no path), or a file, which keeps its single-file
/// behaviour, with a hint when it is part of a package.
fn on_target(command: Command, path: Option<&&str>, flags: Flags, args: &[String]) -> ExitCode {
    let store = cache::for_command(flags.no_cache);
    let store = store.as_ref();
    match package::target(path.copied()) {
        package::Target::File(file) => {
            let code = match command {
                Command::Check(gate) => check_file(&file, flags, gate, store),
                Command::Run(run) => run_file(&file, flags, run, args, store),
                Command::Test(filter) => test_file(&file, flags, filter, store),
                Command::Build(options) => {
                    build::file(&file, flags, build::Goal::Build(options), store)
                }
                Command::Release(options) => {
                    build::file(&file, flags, build::Goal::Release(options), store)
                }
            };
            hint_package(&file, command.name());
            code
        }
        package::Target::Dir(dir) => on_package(command, &dir, flags, args, store),
    }
}

/// `command` on the package in `dir`. `oskac run` refuses a library, a
/// package without `src/main.osk`.
fn on_package(
    command: Command,
    dir: &str,
    flags: Flags,
    args: &[String],
    store: Option<&cache::Store>,
) -> ExitCode {
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
    let options = oska_check::CheckOptions {
        release: matches!(command, Command::Check(_)) && flags.release,
    };
    let cached = cache::Cached::new(store, cache::package_sources(dir, &package, &refused));
    if let Command::Check(gate) = command {
        let check = || {
            let report = package::check(&package, &refused, options);
            if json {
                report
            } else {
                report_from_here(dir, report)
            }
        };
        return match gate {
            Some(_) => finish_check(&check(), json, dir, gate),
            None => cached
                .check(&check_flags(json, flags.release), || {
                    render_report(&check(), json, dir)
                })
                .emit(),
        };
    }
    let report = if matches!(
        command,
        Command::Run(..) | Command::Build(_) | Command::Release(_)
    ) {
        package::check_to_run(&package, &refused, dir, options)
    } else {
        package::check(&package, &refused, options)
    };
    let Some(program) = &report.program else {
        return refuse(command, report.diagnostics, json, dir);
    };
    let places = Places::Package {
        locator: oska_check::Locator::new(&package),
        dir,
        json,
    };
    match command {
        Command::Build(options) => {
            return build::package(
                program,
                &places,
                dir,
                build::Goal::Build(options),
                json,
                &cached,
            );
        }
        Command::Release(options) => {
            return build::package(
                program,
                &places,
                dir,
                build::Goal::Release(options),
                json,
                &cached,
            );
        }
        _ => {}
    }
    let mut host = oska_run::RealHost::new().with_dns_test_hooks(DNS_TEST_HOOKS);
    if let Command::Run(RunOptions { limits, grant }) = command {
        // The package's one check decides both the grant and what runs.
        if let Some(code) = grant.and_then(|file| authority_gate::refuse_run(&report, json, file)) {
            return code;
        }
        if let Some(target) = native::native_target(flags) {
            match native::run(
                program,
                target,
                &places,
                native::Invocation {
                    argv0: dir,
                    args,
                    json,
                    limits,
                    stdin_closed: false,
                },
                &cached,
            ) {
                native::Run::Ran(code) => return code,
                native::Run::Refused(refusal) => {
                    if let Some(refusal) =
                        native::gate(refusal, oska_run::refusal(program), &places)
                    {
                        return refuse_unsupported(Gated::Run, refusal, json);
                    }
                }
            }
        }
        let mut argv = vec![dir.to_string()];
        argv.extend_from_slice(args);
        let outcome = oska_run::run_program_with_limits(program, &mut host, &argv, limits);
        finish_run(outcome, json, &places)
    } else {
        let filter = match command {
            Command::Test(filter) => filter,
            _ => None,
        };
        let options = oska_run::TestOptions {
            integration: flags.integration,
            filter,
        };
        if let Some(target) = native::native_target(flags) {
            match native_test::test_program(program, target, options, json, &cached) {
                native_test::Tested::Report(mut tests) => {
                    settle_snapshots(&mut tests, dir, &places, flags);
                    return finish_test(tests, json, &places);
                }
                native_test::Tested::Ended(code) => return code,
                native_test::Tested::Refused(refusal) => {
                    if let Some(refusal) =
                        native::gate(refusal, oska_run::test_refusal(program), &places)
                    {
                        return refuse_unsupported(Gated::Test, refusal, json);
                    }
                }
            }
        }
        let mut tests = oska_run::test_program(program, &mut host, options);
        settle_snapshots(&mut tests, dir, &places, flags);
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
        Command::Check(gate) => {
            let report = oska_check::CheckReport {
                diagnostics,
                ..oska_check::CheckReport::default()
            };
            finish_check(&report, json, dir, gate)
        }
        Command::Run(..) => {
            let outcome = oska_run::RunOutcome {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                panic: None,
                diagnostics,
            };
            finish_run(outcome, json, &places)
        }
        Command::Test(_) => {
            let report = oska_run::TestReport {
                tests: Vec::new(),
                diagnostics,
                filtered: None,
                snapshots: None,
            };
            finish_test(report, json, &places)
        }
        Command::Build(_) => build::refuse(&diagnostics, json, ExitCode::FAILURE),
        Command::Release(_) => release::refuse(&diagnostics, json, ExitCode::FAILURE),
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

/// `oskac doc [filter]`: the API reference, or the part `filter` selects.
/// An unknown module or item is a usage error (exit code 2).
fn doc_command(filter: Option<&str>, Flags { json, .. }: Flags) -> ExitCode {
    let entries = doc::entries();
    let entries = match filter {
        None => entries,
        Some(filter) => match doc::select(entries, filter) {
            Some(selected) => selected,
            None => {
                let message = format!(
                    "the reference has no module or item named `{filter}`; \
                     run `oskac doc` for the whole list"
                );
                if json {
                    outln!(
                        "{{\"error\":\"unknown item\",\"item\":\"{}\"}}",
                        oska_syntax::json_escape(filter)
                    );
                } else {
                    eprintln!("error: {message}");
                }
                return ExitCode::from(2);
            }
        },
    };
    if json {
        out!("{}", doc::to_json(&entries));
    } else {
        out!("{}", doc::to_human(&entries));
    }
    ExitCode::SUCCESS
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
        let line = parse(&[
            "--integration",
            "test",
            "f.osk",
            "--format=json",
            "--no-cache",
        ]);
        assert_eq!(line.positional, ["test", "f.osk"]);
        assert_eq!(
            line.flags,
            Flags {
                json: true,
                release: false,
                integration: true,
                interpret: false,
                dry_run: false,
                placeholders: false,
                no_cache: true,
                lib: false,
                no_agent_files: false,
                update: false,
                no_identify: false,
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
    fn target_takes_the_next_argument_or_the_text_after_an_equals_sign() {
        let line = parse(&["build", "--target", "linux-aarch64", "f.osk"]);
        assert_eq!(line.positional, ["build", "f.osk"]);
        assert_eq!(line.target.as_deref(), Some("linux-aarch64"));
        let line = parse(&["build", "f.osk", "--target=all"]);
        assert_eq!(line.target.as_deref(), Some("all"));
        // With nothing after it, `--target` is left positional: a usage error.
        let line = parse(&["build", "f.osk", "--target"]);
        assert_eq!(line.positional, ["build", "f.osk", "--target"]);
        assert_eq!(line.target, None);
    }

    #[test]
    fn another_format_means_human_output_and_an_unknown_flag_is_positional() {
        let line = parse(&["check", "f.osk", "--format=text", "--nope"]);
        assert!(!line.flags.json);
        assert_eq!(line.positional, ["check", "f.osk", "--nope"]);
    }
}
