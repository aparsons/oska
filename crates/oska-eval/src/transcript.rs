//! Transcripts: an adapter per agent CLI that turns a recorded transcript
//! into neutral [`Event`]s, so the analysis never sees an agent's own
//! format. v1 has one adapter, [`claude_stream_json`], for Claude Code's
//! `--output-format stream-json`: one JSON object per line.
//!
//! The adapter reads only the fields it needs and ignores the rest, so a
//! field an agent CLI adds later does not break an old transcript. A line
//! that is not a JSON object is an error, never a silently empty result.

use oska_syntax::{parse_json, JsonValue};

/// Why a run ended, as the agent CLI reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Finished,
    TurnLimit,
    Error,
}

/// What the agent asked a tool to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// A shell command.
    Command(String),
    /// An edit of one file, as one or more changes.
    Edit { path: String, changes: Vec<Change> },
    /// A fetch of a web page.
    Fetch(String),
    /// Any other tool, with the file paths it was given and, for Glob and
    /// Grep, its search patterns.
    Other {
        tool: String,
        paths: Vec<String>,
        patterns: Vec<String>,
    },
}

/// One thing that happened in a run, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The agent started in `cwd`.
    Init { cwd: Option<String> },
    /// A tool call, to be matched with its result by `id`.
    ToolCall { id: String, call: Call },
    /// What a tool call returned.
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
    /// What a hook told the agent after an edit.
    Hook { output: String },
    /// What the run used, as the agent CLI reported it. Numbers are kept as
    /// written.
    Usage {
        turns: Option<String>,
        input_tokens: Option<String>,
        output_tokens: Option<String>,
        cost: Option<String>,
    },
    /// The agent's last message, and why it stopped.
    Final { text: String, stop: Stop },
}

/// One replacement inside an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The text it replaced; `None` for a whole-file write, where the
    /// transcript does not show what was there.
    pub removed: Option<String>,
    pub inserted: String,
}

impl Call {
    /// The file paths the call names: what it reads, edits or searches.
    /// Shell commands are searched by the analysis, not here.
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Call::Edit { path, .. } => vec![path],
            Call::Other { paths, .. } => paths.iter().map(String::as_str).collect(),
            Call::Command(_) | Call::Fetch(_) => Vec::new(),
        }
    }

    /// The search patterns of a Glob or Grep call.
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            Call::Other { patterns, .. } => patterns.iter().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// The tool's name, for reports.
    pub fn tool(&self) -> &str {
        match self {
            Call::Command(_) => "Bash",
            Call::Edit { .. } => "Edit",
            Call::Fetch(_) => "WebFetch",
            Call::Other { tool, .. } => tool,
        }
    }
}

/// The events of a Claude Code `stream-json` transcript.
pub fn claude_stream_json(text: &str) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let number = index + 1;
        let value = parse_json(line).map_err(|e| format!("line {number} is not JSON: {e}"))?;
        // `get` reads the first of two equal keys; a line must not be able
        // to show one tool input to this reader and another to the agent.
        if let Some(key) = value.duplicate_key() {
            return Err(format!("line {number}: the key `{key}` appears twice"));
        }
        if !matches!(value, JsonValue::Object(_)) {
            return Err(format!("line {number} is not a JSON object"));
        }
        let Some(kind) = value.get("type").and_then(JsonValue::as_str) else {
            return Err(format!("line {number} has no `type`"));
        };
        match kind {
            "system" => system(&value, &mut events),
            "assistant" => assistant(&value, number, &mut events)?,
            "user" => user(&value, &mut events),
            "result" => result(&value, &mut events),
            _ => {}
        }
    }
    Ok(events)
}

fn system(value: &JsonValue, events: &mut Vec<Event>) {
    match value.get("subtype").and_then(JsonValue::as_str) {
        Some("init") => events.push(Event::Init {
            cwd: string(value, "cwd"),
        }),
        Some("hook_response") => {
            let post_tool =
                value.get("hook_event").and_then(JsonValue::as_str) == Some("PostToolUse");
            if let (true, Some(output)) = (post_tool, string(value, "stdout")) {
                if !output.trim().is_empty() {
                    events.push(Event::Hook { output });
                }
            }
        }
        _ => {}
    }
}

fn assistant(value: &JsonValue, number: usize, events: &mut Vec<Event>) -> Result<(), String> {
    let blocks = content(value);
    for block in blocks {
        if block.get("type").and_then(JsonValue::as_str) != Some("tool_use") {
            continue;
        }
        let (Some(id), Some(name)) = (string(block, "id"), string(block, "name")) else {
            return Err(format!("line {number}: a tool call has no `id` or `name`"));
        };
        let input = block.get("input").unwrap_or(&JsonValue::Null);
        events.push(Event::ToolCall {
            id,
            call: call(&name, input),
        });
    }
    Ok(())
}

fn user(value: &JsonValue, events: &mut Vec<Event>) {
    for block in content(value) {
        if block.get("type").and_then(JsonValue::as_str) != Some("tool_result") {
            continue;
        }
        let Some(id) = string(block, "tool_use_id") else {
            continue;
        };
        let output = match block.get("content") {
            Some(JsonValue::Str(s)) => s.clone(),
            Some(JsonValue::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(JsonValue::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        let is_error = block.get("is_error").and_then(JsonValue::as_bool) == Some(true);
        events.push(Event::ToolResult {
            id,
            output,
            is_error,
        });
    }
}

fn result(value: &JsonValue, events: &mut Vec<Event>) {
    let usage = value.get("usage");
    let tokens = |key: &str| usage.and_then(|u| number(u, key));
    events.push(Event::Usage {
        turns: number(value, "num_turns"),
        input_tokens: tokens("input_tokens"),
        output_tokens: tokens("output_tokens"),
        cost: number(value, "total_cost_usd"),
    });
    let stop = match value.get("subtype").and_then(JsonValue::as_str) {
        Some("success") => Stop::Finished,
        Some("error_max_turns") => Stop::TurnLimit,
        _ => Stop::Error,
    };
    events.push(Event::Final {
        text: string(value, "result").unwrap_or_default(),
        stop,
    });
}

/// The content blocks of a message, or none for a message whose content is
/// plain text.
fn content(value: &JsonValue) -> &[JsonValue] {
    value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(JsonValue::as_array)
        .unwrap_or(&[])
}

/// Maps a Claude Code tool to the neutral call.
fn call(name: &str, input: &JsonValue) -> Call {
    let path = |key: &str| string(input, key);
    match name {
        "Bash" => Call::Command(string(input, "command").unwrap_or_default()),
        "WebFetch" => Call::Fetch(string(input, "url").unwrap_or_default()),
        "Edit" => Call::Edit {
            path: path("file_path").unwrap_or_default(),
            changes: vec![Change {
                removed: Some(string(input, "old_string").unwrap_or_default()),
                inserted: string(input, "new_string").unwrap_or_default(),
            }],
        },
        "Write" => Call::Edit {
            path: path("file_path").unwrap_or_default(),
            changes: vec![Change {
                removed: None,
                inserted: string(input, "content").unwrap_or_default(),
            }],
        },
        "MultiEdit" => Call::Edit {
            path: path("file_path").unwrap_or_default(),
            changes: input
                .get("edits")
                .and_then(JsonValue::as_array)
                .unwrap_or(&[])
                .iter()
                .map(|e| Change {
                    removed: Some(string(e, "old_string").unwrap_or_default()),
                    inserted: string(e, "new_string").unwrap_or_default(),
                })
                .collect(),
        },
        _ => Call::Other {
            tool: name.to_string(),
            paths: ["file_path", "path", "notebook_path"]
                .iter()
                .filter_map(|key| path(key))
                .collect(),
            // Glob and Grep reach files through `pattern` too.
            patterns: if matches!(name, "Glob" | "Grep") {
                path("pattern").into_iter().collect()
            } else {
                Vec::new()
            },
        },
    }
}

fn string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn number(value: &JsonValue, key: &str) -> Option<String> {
    match value.get(key) {
        Some(JsonValue::Number(n)) => Some(n.clone()),
        _ => None,
    }
}
