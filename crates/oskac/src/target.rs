// SPDX-License-Identifier: Apache-2.0
//! The commands that take a file or a package: `check`, `run`, `test`,
//! `build` and `release` on it.

use crate::authority_gate::Gate;
use crate::check::{check_file, check_flags, finish_check, render_report};
use crate::cli::Flags;
use crate::places::{from_here, report_from_here, Places};
use crate::run::{finish_run, refuse_unsupported, run_file, Gated, RunOptions};
use crate::test::{finish_test, settle_snapshots, test_file};
use crate::{authority_gate, build, cache, native, native_test, package, release, DNS_TEST_HOOKS};
use oska_syntax::Diagnostic;
use std::process::ExitCode;

/// The commands that take a source file or a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command<'a> {
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

// ---- Packages ------------------------------------------------------------------

/// `oskac check`, `run` or `test` on `path`: a package directory (the
/// current one with no path), or a file, which keeps its single-file
/// behaviour, with a hint when it is part of a package.
pub(crate) fn on_target(
    command: Command,
    path: Option<&&str>,
    flags: Flags,
    args: &[String],
) -> ExitCode {
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
pub(crate) fn on_package(
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
pub(crate) fn refuse(
    command: Command,
    diagnostics: Vec<Diagnostic>,
    json: bool,
    dir: &str,
) -> ExitCode {
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
pub(crate) fn hint_package(file: &str, command: &str) {
    if let Some(dir) = package::package_containing(file) {
        eprintln!(
            "hint: `{file}` is part of the package in `{dir}` and was handled alone; \
             run `oskac {command} {dir}` to {command} the package"
        );
    }
}
