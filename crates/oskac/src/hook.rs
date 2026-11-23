// SPDX-License-Identifier: Apache-2.0
//! `oskac hook`: the commands an AI agent's hooks call, so the check on
//! every edit runs inside `oskac` and works the same on every platform,
//! with no shell script and no regular expression (spec #185).
//!
//! The logic of an event is a function of the event, its target and what
//! the check found, and it returns a [`Verdict`]: what to print and the
//! exit status. `claude` is only the adapter for Claude Code's input; a
//! later plugin, another agent's hook format or an MCP tool would adapt
//! its own input to the same [`Edited`] and call the same function.
//!
//! A hook reads its input and the program's files. It writes no file,
//! starts no program and opens no network connection, so enabling it
//! gives the agent no new authority.
//!
//! The exit status is 0 for nothing to report, 2 to hand feedback back
//! (Claude Code shows standard error to the agent; for the Stop gate, 2
//! also blocks the agent from finishing), and 1 with one line for
//! malformed input or an unknown event, and, after an edit, a file that
//! cannot be read: Claude Code treats 1 as non-blocking, so a broken hook
//! never wedges an agent. The Stop gate blocks with 2 on a target it
//! cannot read, so it never lets the agent finish on what it could not
//! check. With `--format=json` the feedback is one JSON object instead of
//! text, with each diagnostic in the shape `oskac check --format=json`
//! gives it.

mod claude;
mod post_edit;
mod stop;
mod target;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// The events a hook is called for. A new event adds a variant, its
/// logic beside [`post_edit`] and [`stop`], and an arm in [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    /// After the agent edited a file.
    PostEdit,
    /// When the agent tries to finish.
    Stop,
}

impl Event {
    /// The event `--event=<name>` names.
    fn named(name: &str) -> Option<Event> {
        match name {
            "post-edit" => Some(Event::PostEdit),
            "stop" => Some(Event::Stop),
            _ => None,
        }
    }
}

/// How the feedback is written: as text, a line per diagnostic, or, with
/// `--format=json`, as one JSON object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

/// An edit of one file, as any agent's hook input says it: the file
/// (absolute when the input gave the project's directory) and the
/// project's directory, which the paths printed are written from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edited {
    file: PathBuf,
    cwd: Option<PathBuf>,
}

/// The project an agent is finishing work in, as any agent's hook input
/// says it: its directory, which named targets are paths from and the
/// paths printed are written from. Without one, the hook's own.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Finish {
    cwd: Option<PathBuf>,
}

/// What a hook does in the end: the text for standard error, the exit
/// status, and a note for the user, which the agent is not shown.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Verdict {
    stderr: String,
    status: u8,
    /// A note for the user, on a hook that ends with 0: the agent's
    /// adapter says it where the user, and not the agent, reads it.
    notice: Option<String>,
}

impl Verdict {
    /// Nothing to report.
    fn quiet() -> Verdict {
        Verdict {
            stderr: String::new(),
            status: 0,
            notice: None,
        }
    }

    /// Nothing for the agent, and a note for the user.
    fn noting(notice: String) -> Verdict {
        Verdict {
            notice: Some(notice),
            ..Verdict::quiet()
        }
    }

    /// Feedback for the agent: Claude Code shows it, and for a Stop hook
    /// it blocks.
    fn feedback(stderr: String) -> Verdict {
        Verdict {
            stderr,
            status: 2,
            notice: None,
        }
    }

    /// The hook could not do its work: one line, and a status that blocks
    /// nothing.
    fn failed(reason: &str) -> Verdict {
        Verdict {
            stderr: format!("error: {}\n", target::one_line(reason)),
            status: 1,
            notice: None,
        }
    }
}

/// `oskac hook claude --event=<event> [--format=json] [targets...]`: read
/// Claude Code's hook JSON from standard input, decide, and print what the
/// verdict says: its text to standard error, as JSON when `json` says so,
/// and its note for the user to standard output. Only the Stop gate takes
/// `targets`.
pub fn run(event: Option<&str>, targets: &[&str], json: bool) -> ExitCode {
    let format = if json { Format::Json } else { Format::Text };
    let verdict = match event {
        None => Verdict::failed("`oskac hook claude` needs --event=post-edit or --event=stop"),
        Some(name) => match Event::named(name) {
            Some(Event::PostEdit) if !targets.is_empty() => {
                Verdict::failed("--event=post-edit takes no targets")
            }
            Some(event) => on_event(event, targets, format),
            None => Verdict::failed(&format!(
                "unknown hook event `{name}`; the events are post-edit and stop"
            )),
        },
    };
    // A closed standard error or output leaves nothing to say it to.
    let _ = std::io::stderr().write_all(verdict.stderr.as_bytes());
    let _ = std::io::stdout().write_all(claude::output(&verdict).as_bytes());
    ExitCode::from(verdict.status)
}

fn on_event(event: Event, targets: &[&str], format: Format) -> Verdict {
    let input = match read_input() {
        Ok(input) => input,
        Err(reason) => return Verdict::failed(&reason),
    };
    match event {
        Event::PostEdit => match claude::post_edit_input(&input) {
            Ok(edited) => post_edit::verdict_for(&edited, format),
            Err(reason) => Verdict::failed(&reason),
        },
        Event::Stop => match claude::stop_input(&input) {
            Ok(finish) => stop::verdict_for(&finish, targets, format),
            Err(reason) => Verdict::failed(&reason),
        },
    }
}

/// The hook's standard input, as text.
fn read_input() -> Result<String, String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read the hook input: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "the hook input is not UTF-8 text".to_string())
}
