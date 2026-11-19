//! `record`: one run's record. It joins the run's grade and analysis with
//! what produced them (the task, attempt and variant, the model, the agent
//! CLI and its version, the `oskac` version and commit, and the task set's
//! digest), how and when the run stopped, and the flags a reader filters
//! on.
//!
//! A record is meant to be committed and published, so it holds no raw
//! transcript text (an analysis holds none) and no local path: the run's
//! workspace becomes `<workspace>` and the home directory `~`, wherever
//! either starts a path in any text of the record.

use crate::json::{self, field, num, pretty, text};
use crate::task;
use oska_syntax::JsonValue;
use std::path::Path;
use std::process::ExitCode;

/// The run record format version.
pub const SCHEMA: u64 = 1;

/// The flags `record` takes, each with a value. The first group is
/// required.
const REQUIRED: &[&str] = &[
    "--attempt",
    "--variant",
    "--model",
    "--agent",
    "--agent-version",
    "--oskac-version",
    "--oskac-commit",
    "--task-set",
    "--started",
    "--ended",
];
const OPTIONAL: &[&str] = &["--stop", "--workspace", "--grade", "--analysis"];

/// Why a run stopped, as the driver knows it and a transcript cannot say.
const DRIVER_STOPS: &[&str] = &["timeout", "budget", "error"];

/// The fields of an analysis a record keeps. Its `usage` moves up to the
/// record, its `stop` gives way to the record's, and its
/// `finished_despite_failing` becomes the `false_claim` flag.
const ANALYSIS_KEPT: &[&str] = &[
    "iterations",
    "iterations_to_clean",
    "timeline",
    "diagnostics",
    "fixes",
    "lookups",
    "escapes",
];

/// The fields of a grade a record keeps: all but `schema` and `task`.
const GRADE_KEPT: &[&str] = &["passed", "secure", "clean", "changed", "checks"];

/// `oska-eval record <tasks-dir> <task-id> --flag value ...`.
pub fn run(tasks: &str, id: &str, flags: &[&str]) -> ExitCode {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok());
    match record(tasks, id, flags, home.as_deref()) {
        Ok(report) => {
            print!("{}", pretty(&report));
            ExitCode::SUCCESS
        }
        Err(message) => json::bad_input(&[message]),
    }
}

/// The flags, in order, as (flag, value) pairs; an unknown flag or one
/// without a value is an error.
fn parse_flags<'a>(flags: &[&'a str]) -> Result<Vec<(&'a str, &'a str)>, String> {
    let mut found = Vec::new();
    let mut rest = flags;
    while let [flag, tail @ ..] = rest {
        if !REQUIRED.contains(flag) && !OPTIONAL.contains(flag) {
            return Err(format!("unknown flag `{flag}`"));
        }
        let [value, tail @ ..] = tail else {
            return Err(format!("`{flag}` needs a value"));
        };
        if found.iter().any(|(f, _)| f == flag) {
            return Err(format!("`{flag}` is given twice"));
        }
        found.push((*flag, *value));
        rest = tail;
    }
    if let Some(missing) = REQUIRED
        .iter()
        .find(|f| !found.iter().any(|(g, _)| g == *f))
    {
        return Err(format!("missing `{missing}`"));
    }
    Ok(found)
}

fn record(tasks: &str, id: &str, flags: &[&str], home: Option<&str>) -> Result<JsonValue, String> {
    let flags = parse_flags(flags)?;
    let get = |name: &str| flags.iter().find(|(f, _)| *f == name).map(|(_, v)| *v);
    let required = |name: &str| get(name).unwrap_or_default();

    let task = task::load_active(Path::new(tasks), id)?;
    let attempt = match required("--attempt").parse::<u64>() {
        Ok(n) if n >= 1 => n,
        _ => return Err("`--attempt` must be a whole number of at least 1".to_string()),
    };
    if attempt > task.attempts {
        return Err(format!(
            "attempt {attempt} is beyond the task's {} attempts",
            task.attempts
        ));
    }
    let variant = required("--variant");
    if !is_name(variant) {
        return Err(format!(
            "`--variant` `{variant}` must be lower-case letters, digits, `-` and `_`"
        ));
    }
    for name in [
        "--model",
        "--agent",
        "--agent-version",
        "--oskac-version",
        "--oskac-commit",
        "--task-set",
    ] {
        let value = required(name);
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("`{name}` must be text with no space"));
        }
    }
    let (started, ended) = (required("--started"), required("--ended"));
    for (name, time) in [("--started", started), ("--ended", ended)] {
        if !is_utc_time(time) {
            return Err(format!(
                "`{name}` `{time}` must be a UTC time such as 2026-11-20T10:00:00Z"
            ));
        }
    }
    if ended < started {
        return Err("`--ended` is before `--started`".to_string());
    }

    let grade = get("--grade").map(read_json).transpose()?;
    if let Some(grade) = &grade {
        check_grade(grade, id)?;
    }
    let analysis = get("--analysis").map(read_json).transpose()?;
    if let Some(analysis) = &analysis {
        check_analysis(analysis)?;
    }

    let stop = match (get("--stop"), &analysis) {
        (Some(stop), _) if DRIVER_STOPS.contains(&stop) => stop.to_string(),
        (Some(stop), _) => {
            return Err(format!(
                "`--stop` `{stop}` must be one of {}",
                DRIVER_STOPS.join(", ")
            ))
        }
        (None, Some(analysis)) => match analysis.get("stop").and_then(JsonValue::as_str) {
            Some(stop @ ("finished" | "turn_limit")) => stop.to_string(),
            _ => "error".to_string(),
        },
        (None, None) => {
            return Err(
                "neither a stop reason nor an analysis: give `--stop` or `--analysis`".to_string(),
            )
        }
    };

    let passed = grade
        .as_ref()
        .and_then(|g| g.get("passed"))
        .and_then(JsonValue::as_bool);
    let escape = analysis
        .as_ref()
        .and_then(|a| a.get("escapes"))
        .and_then(JsonValue::as_array)
        .is_some_and(|e| !e.is_empty());
    let read_only_changed = grade
        .as_ref()
        .and_then(|g| g.get("changed"))
        .and_then(JsonValue::as_array)
        .is_some_and(|changed| {
            changed.iter().filter_map(JsonValue::as_str).any(|path| {
                task.read_only.iter().any(|r| r == path)
                    || crate::prepare::SETUP_FILES.iter().any(|(f, _)| *f == path)
            })
        });
    let usage = analysis
        .as_ref()
        .and_then(|a| a.get("usage"))
        .cloned()
        .unwrap_or(JsonValue::Null);

    let mut record = JsonValue::Object(vec![
        field("schema", num(SCHEMA)),
        field("task", text(id)),
        field("class", text(&task.class)),
        field("attempt", num(attempt)),
        field("attempts", num(task.attempts)),
        field("variant", text(variant)),
        field("model", text(required("--model"))),
        field(
            "agent",
            JsonValue::Object(vec![
                field("name", text(required("--agent"))),
                field("version", text(required("--agent-version"))),
            ]),
        ),
        field(
            "oskac",
            JsonValue::Object(vec![
                field("version", text(required("--oskac-version"))),
                field("commit", text(required("--oskac-commit"))),
            ]),
        ),
        field("task_set_digest", text(required("--task-set"))),
        field("started", text(started)),
        field("ended", text(ended)),
        field("stop", text(&stop)),
        field("usage", usage),
        field(
            "flags",
            JsonValue::Object(vec![
                field("escape", JsonValue::Bool(escape)),
                field(
                    "false_claim",
                    JsonValue::Bool(stop == "finished" && passed == Some(false)),
                ),
                field("read_only_changed", JsonValue::Bool(read_only_changed)),
            ]),
        ),
        field("grade", keep(grade.as_ref(), GRADE_KEPT)),
        field("analysis", keep(analysis.as_ref(), ANALYSIS_KEPT)),
    ]);
    if let Some(workspace) = get("--workspace") {
        scrub(&mut record, workspace, "<workspace>");
    }
    if let Some(home) = home.filter(|h| h.len() > 1) {
        scrub(&mut record, home, "~");
    }
    Ok(record)
}

/// A variant's name: lower-case letters, digits, `-` and `_`.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A time such as `2026-11-20T10:00:00Z`: UTC, to the second, so that
/// times sort as text.
pub fn is_utc_time(time: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:ddZ";
    time.len() == shape.len()
        && time.chars().zip(shape.chars()).all(|(c, s)| match s {
            'd' => c.is_ascii_digit(),
            s => c == s,
        })
}

fn read_json(path: &str) -> Result<JsonValue, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    json::read(&text).map_err(|e| format!("{path}: {e}"))
}

fn check_grade(grade: &JsonValue, id: &str) -> Result<(), String> {
    if grade.get("schema").and_then(JsonValue::as_u64) != Some(1) {
        return Err("the grade is not a grade of schema 1".to_string());
    }
    match grade.get("task").and_then(JsonValue::as_str) {
        Some(task) if task == id => {}
        Some(task) => return Err(format!("the grade is of the task `{task}`, not `{id}`")),
        None => return Err("the grade names no task".to_string()),
    }
    for key in ["passed", "secure", "clean"] {
        if grade.get(key).and_then(JsonValue::as_bool).is_none() {
            return Err(format!("the grade's `{key}` must be true or false"));
        }
    }
    for key in ["changed", "checks"] {
        if grade.get(key).and_then(JsonValue::as_array).is_none() {
            return Err(format!("the grade's `{key}` must be a list"));
        }
    }
    Ok(())
}

fn check_analysis(analysis: &JsonValue) -> Result<(), String> {
    if analysis.get("schema").and_then(JsonValue::as_u64) != Some(1) {
        return Err("the analysis is not an analysis of schema 1".to_string());
    }
    for key in ANALYSIS_KEPT.iter().chain(&["stop", "usage"]) {
        if analysis.get(key).is_none() {
            return Err(format!("the analysis has no `{key}`"));
        }
    }
    Ok(())
}

/// The fields of `value` named in `keys`, in that order, or null.
fn keep(value: Option<&JsonValue>, keys: &[&str]) -> JsonValue {
    let Some(value) = value else {
        return JsonValue::Null;
    };
    JsonValue::Object(
        keys.iter()
            .filter_map(|key| value.get(key).map(|v| field(key, v.clone())))
            .collect(),
    )
}

/// Every text in `value` with `path`, where it starts a path (at the start
/// of the text or after a space, quote or `=`, and followed by a separator
/// or the end), replaced by `placeholder`.
fn scrub(value: &mut JsonValue, path: &str, placeholder: &str) {
    match value {
        JsonValue::Str(s) => *s = replace_path(s, path, placeholder),
        JsonValue::Array(items) => items.iter_mut().for_each(|v| scrub(v, path, placeholder)),
        JsonValue::Object(fields) => fields
            .iter_mut()
            .for_each(|(_, v)| scrub(v, path, placeholder)),
        _ => {}
    }
}

fn replace_path(text: &str, path: &str, placeholder: &str) -> String {
    let path = path.trim_end_matches(['/', '\\']);
    if path.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(path) {
        let before = if at > 0 {
            rest[..at].chars().last()
        } else {
            out.chars().last()
        };
        let after = rest[at + path.len()..].chars().next();
        let starts = before.map_or(true, |c| c.is_whitespace() || "\"'=:(".contains(c));
        let ends = after.map_or(true, |c| c == '/' || c == '\\' || c.is_whitespace());
        out.push_str(&rest[..at]);
        out.push_str(if starts && ends { placeholder } else { path });
        rest = &rest[at + path.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::replace_path;

    #[test]
    fn a_path_is_replaced_only_where_it_is_a_whole_path() {
        let home = "/home/user";
        assert_eq!(replace_path("/home/user/.ssh", home, "~"), "~/.ssh");
        assert_eq!(replace_path("cat /home/user", home, "~"), "cat ~");
        assert_eq!(
            replace_path("/home/username/x", home, "~"),
            "/home/username/x"
        );
        assert_eq!(replace_path("/x/home/user/y", home, "~"), "/x/home/user/y");
    }
}
