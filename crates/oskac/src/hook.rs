//! `oskac hook`: the commands an AI agent's hooks call, so the check on
//! every edit runs inside `oskac` and works the same on every platform,
//! with no shell script and no regular expression (spec #185).
//!
//! The logic of an event is a function of the event, its target and what
//! the check found, and it returns a [`Verdict`]: what to print and the
//! exit status. `claude` is only the adapter for Claude Code's input; a
//! later plugin, another agent's hook format or an MCP tool would adapt
//! its own input to the same [`Edit`] and call the same function.
//!
//! A hook reads its input and the program's files. It writes no file,
//! starts no program and opens no network connection, so enabling it
//! gives the agent no new authority.
//!
//! The exit status is 0 for nothing to report, 2 to hand feedback back
//! (Claude Code shows standard error to the agent), and 1 with one line
//! for malformed input, an unreadable file or an unknown event: Claude
//! Code treats 1 as non-blocking, so a broken hook never wedges an agent.

mod claude;
mod post_edit;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// The events a hook is called for. A new event adds a variant, its
/// logic beside [`post_edit`], and an arm in [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    /// After the agent edited a file.
    PostEdit,
}

impl Event {
    /// The event `--event=<name>` names.
    fn named(name: &str) -> Option<Event> {
        match name {
            "post-edit" => Some(Event::PostEdit),
            _ => None,
        }
    }
}

/// An edit of one file, as any agent's hook input says it: the file
/// (absolute when the input gave the project's directory) and the
/// project's directory, which the paths printed are written from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    file: PathBuf,
    cwd: Option<PathBuf>,
}

/// What a hook does in the end: the text for standard error and the exit
/// status.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Verdict {
    stderr: String,
    status: u8,
}

impl Verdict {
    /// Nothing to report.
    fn quiet() -> Verdict {
        Verdict {
            stderr: String::new(),
            status: 0,
        }
    }

    /// Feedback for the agent: Claude Code shows it, and for a Stop hook
    /// it blocks.
    fn feedback(stderr: String) -> Verdict {
        Verdict { stderr, status: 2 }
    }

    /// The hook could not do its work: one line, and a status that blocks
    /// nothing.
    fn failed(reason: &str) -> Verdict {
        let one_line: String = reason
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        Verdict {
            stderr: format!("error: {one_line}\n"),
            status: 1,
        }
    }
}

/// `oskac hook claude --event=<event>`: read Claude Code's hook JSON from
/// standard input, decide, and print what the verdict says to standard
/// error.
pub fn run(event: Option<&str>) -> ExitCode {
    let verdict = match event {
        None => Verdict::failed("`oskac hook claude` needs --event=post-edit"),
        Some(name) => match Event::named(name) {
            Some(event) => on_event(event),
            None => Verdict::failed(&format!(
                "unknown hook event `{name}`; the event is post-edit"
            )),
        },
    };
    // A closed standard error leaves nothing to say it to.
    let _ = std::io::stderr().write_all(verdict.stderr.as_bytes());
    ExitCode::from(verdict.status)
}

fn on_event(event: Event) -> Verdict {
    let input = match read_input() {
        Ok(input) => input,
        Err(reason) => return Verdict::failed(&reason),
    };
    match event {
        Event::PostEdit => match claude::post_edit_input(&input) {
            Ok(edit) => post_edit::verdict_for(&edit),
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
