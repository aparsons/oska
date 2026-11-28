// SPDX-License-Identifier: Apache-2.0
//! `oskac run`: check a program, then run it.

use crate::authority_gate;
use crate::cache;
use crate::check::print_diagnostics;
use crate::cli::Flags;
use crate::places::Places;
use crate::source::read_source;
use crate::test::finish_test;
use crate::{native, DNS_TEST_HOOKS};
use oska_syntax::Diagnostic;
use std::process::ExitCode;

/// What `oskac run` runs a program under: its run limits, and the
/// authority grant (`--grant=<file>`) it must stay within, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunOptions<'a> {
    pub(crate) limits: oska_check::limits::Limits,
    pub(crate) grant: Option<&'a str>,
}

/// `oskac run`: check the program and, if the check reports no
/// diagnostic, run it on the real OS with `args` after the program path.
/// A refused program's diagnostics, or the panic that stopped it, go to
/// stderr as `oskac check` prints diagnostics, or one JSON object per
/// line with `--format=json`. The exit code is the program's: 101 after
/// a panic (see `docs/runtime.md`).
pub(crate) fn run_file(
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
pub(crate) fn run_source(
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
pub(crate) fn finish_run(outcome: oska_run::RunOutcome, json: bool, places: &Places) -> ExitCode {
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

/// What the parity gate stopped: a run or the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gated {
    Run,
    Test,
}

/// The parity gate's refusal (#181): `oskac run` or `oskac test` of a
/// program the native backend cannot compile yet (`refusal`, `E0390`,
/// already located) reports it as `oskac build` does, as an error with
/// exit code 2, and runs nothing. `--interpret` still runs it.
pub(crate) fn refuse_unsupported(gated: Gated, refusal: Diagnostic, json: bool) -> ExitCode {
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
