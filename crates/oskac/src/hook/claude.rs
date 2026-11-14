//! Claude Code's hook input, as JSON on standard input: the adapter from
//! it to the hook's own [`Edit`] or [`Finish`], and from the hook's
//! [`Verdict`] to what Claude Code reads on standard output. It is read with the toolchain's JSON
//! reader, so a path with an escaped quote or a Unicode escape comes out
//! as written, and input of an unexpected shape is an error, not a guess.

use std::path::PathBuf;

use oska_syntax::{json_escape, parse_json, JsonValue};

use super::{Edit, Finish, Verdict};

/// The project a Stop hook's input reports: `cwd`, its directory.
pub fn stop_input(input: &str) -> Result<Finish, String> {
    let json = parse_json(input).map_err(|e| format!("the hook input is not JSON: {e}"))?;
    if !matches!(json, JsonValue::Object(_)) {
        return Err("the hook input is not a JSON object".to_string());
    }
    let cwd = match json.get("cwd") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::Str(cwd)) if !cwd.is_empty() => Some(PathBuf::from(cwd)),
        Some(_) => return Err("the hook input's `cwd` is not a directory name".to_string()),
    };
    Ok(Finish { cwd })
}

/// What Claude Code reads on standard output: with exit status 0, a JSON
/// object whose `systemMessage` it shows the user and not the agent.
/// Nothing when the verdict has no note for the user.
pub fn output(verdict: &Verdict) -> String {
    match &verdict.notice {
        Some(notice) => format!("{{\"systemMessage\":\"{}\"}}\n", json_escape(notice)),
        None => String::new(),
    }
}

/// The edit a PostToolUse hook's input reports: `tool_input.file_path`,
/// resolved against `cwd`, the project's directory, when it is relative.
pub fn post_edit_input(input: &str) -> Result<Edit, String> {
    let json = parse_json(input).map_err(|e| format!("the hook input is not JSON: {e}"))?;
    let cwd = match json.get("cwd") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::Str(cwd)) if !cwd.is_empty() => Some(PathBuf::from(cwd)),
        Some(_) => return Err("the hook input's `cwd` is not a directory name".to_string()),
    };
    let file = match json.get("tool_input").and_then(|t| t.get("file_path")) {
        Some(JsonValue::Str(file)) if !file.is_empty() => PathBuf::from(file),
        Some(_) => return Err("the hook input's `tool_input.file_path` is not a path".to_string()),
        None => return Err("the hook input has no `tool_input.file_path`".to_string()),
    };
    let file = match &cwd {
        Some(cwd) => cwd.join(file),
        None => file,
    };
    Ok(Edit { file, cwd })
}
