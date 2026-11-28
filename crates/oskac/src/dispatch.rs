// SPDX-License-Identifier: Apache-2.0
//! Check the command line as a whole and call the subcommand it names.

use crate::authority_gate::Gate;
use crate::cli::{CommandLine, USAGE};
use crate::run::RunOptions;
use crate::target::{on_target, Command};
use crate::{build, cache, doc, explain, fix, hook, lex, new, parse, release, self_cmd, stdin};
use std::process::ExitCode;

pub(crate) fn dispatch(line: CommandLine) -> ExitCode {
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
        ["lex", path] => lex::command(path, flags),
        ["parse", path] => parse::command(path, flags),
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
        ["explain", code] => explain::command(code, flags),
        ["doc", rest @ ..] if rest.len() <= 1 && line.program_args.is_empty() => {
            doc::command(rest.first().copied(), flags)
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
