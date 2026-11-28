// SPDX-License-Identifier: Apache-2.0
//! The command line: the usage text, the flags and their parser.

pub(crate) const USAGE: &str = "\
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
                                           binary is the published one; with
                                           self update, the only commands that
                                           use the network
    oskac self update [--version X [--allow-downgrade] | --rollback]
                      [--keep-previous] [--format=json]
                                           Replace this managed install with
                                           the latest verified release, or
                                           with release X; an older release
                                           needs --allow-downgrade. --rollback
                                           goes back one version.
                                           --keep-previous keeps the replaced
                                           binary for this update only
    oskac --version [--format=json]        Print the version
    oskac --help                           Print this help
";

/// The flags `oskac` takes, whichever command they are given to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Flags {
    /// `--format=json`: machine-readable output. Any other `--format=`
    /// means the default, human-readable output.
    pub(crate) json: bool,
    /// `--release`: `oskac check` runs the release check, rejecting `dbg`,
    /// `todo` and unused authority, and `oskac fix` checks as it does.
    pub(crate) release: bool,
    /// `--integration`: `oskac test` runs `#[integration]` tests too.
    pub(crate) integration: bool,
    /// `--interpret`: `oskac run` and `oskac test` use the interpreter
    /// on a host with a native backend too (`native.rs`).
    pub(crate) interpret: bool,
    /// `--dry-run`: `oskac fix` prints the diff it would write, and
    /// writes nothing.
    pub(crate) dry_run: bool,
    /// `--placeholders`: `oskac fix` applies `placeholders` fixes too.
    pub(crate) placeholders: bool,
    /// `--no-cache`: `oskac check`, `run`, `test` and `build` neither
    /// read nor write the build cache (`cache.rs`).
    pub(crate) no_cache: bool,
    /// `--lib`: `oskac new` writes a library, `src/lib.osk` and no `main`.
    pub(crate) lib: bool,
    /// `--no-agent-files`: `oskac new` leaves out `AGENTS.md` and
    /// `CLAUDE.md`.
    pub(crate) no_agent_files: bool,
    /// `--update`: `oskac test` writes new and changed snapshots
    /// (`oska_run::snapshots`) instead of failing on them.
    pub(crate) update: bool,
    /// `--no-identify`: `oskac self check` does not compare this binary with
    /// the release's signed checksums.
    pub(crate) no_identify: bool,
    /// `--allow-downgrade`: `oskac self update --version X` may install an
    /// older release.
    pub(crate) allow_downgrade: bool,
    /// `--rollback`: `oskac self update` goes back to the previous version.
    pub(crate) rollback: bool,
    /// `--keep-previous`: `oskac self update` keeps the binary it replaces.
    pub(crate) keep_previous: bool,
}

/// A command line, parsed: the command and its operands, the flags, the
/// output path `-o` gives, the target platform `--target` names, and the
/// arguments after `--`, which belong to the program being run.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CommandLine {
    pub(crate) positional: Vec<String>,
    pub(crate) flags: Flags,
    pub(crate) output: Option<String>,
    /// `--baseline=<file>`: `oskac check` compares authority with it.
    pub(crate) baseline: Option<String>,
    /// `--update-baseline=<file>`: `oskac check` writes it.
    pub(crate) update_baseline: Option<String>,
    /// `--grant=<file>`: `oskac check` and `oskac run` compare authority
    /// with it.
    pub(crate) grant: Option<String>,
    /// The name `--event=` gives, for `oskac hook`.
    pub(crate) event: Option<String>,
    pub(crate) target: Option<String>,
    /// `--version <X.Y.Z>`, for `oskac release`; for any other command
    /// `--version` is the command that prints `oskac`'s own.
    pub(crate) version: Option<String>,
    /// `--key <file>`: the SSH signing key, for `oskac release`.
    pub(crate) key: Option<String>,
    /// `--filter <substring>`: `oskac test` runs only the tests whose
    /// names contain it.
    pub(crate) filter: Option<String>,
    /// Why `--filter` is missing its substring, empty or repeated: a
    /// usage error.
    pub(crate) filter_error: Option<&'static str>,
    /// The run limits (`--max-steps=<n>`, `--timeout=<ms>`,
    /// `--max-output=<bytes>`, `--max-memory=<bytes>`), which only `oskac
    /// run` takes.
    pub(crate) limits: oska_check::limits::Limits,
    /// Why a limit flag is malformed or repeated: a usage error.
    pub(crate) limit_error: Option<String>,
    pub(crate) program_args: Vec<String>,
}

impl CommandLine {
    /// Parse `args`, without the executable's own name. A flag may come
    /// anywhere before `--`; anything else is positional.
    pub(crate) fn parse(mut args: Vec<String>) -> CommandLine {
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
            if let Some(taken) = crate::limits::take_flag(&mut line.limits, &arg) {
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
                "--version"
                    if line.positional.first().is_some_and(|c| c == "release")
                        || line.positional.as_slice() == ["self", "update"] =>
                {
                    match args.next_if(|next| !next.starts_with("--")) {
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
                "--allow-downgrade" => line.flags.allow_downgrade = true,
                "--rollback" => line.flags.rollback = true,
                "--keep-previous" => line.flags.keep_previous = true,
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
pub(crate) fn value_unless_repeated(previous: &Option<String>, value: &str) -> Option<String> {
    match previous {
        Some(_) => Some(String::new()),
        None => Some(value.to_string()),
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
                allow_downgrade: false,
                rollback: false,
                keep_previous: false,
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
