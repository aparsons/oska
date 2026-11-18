//! `analyse`: how an agent worked, read from a recorded transcript. The
//! transcript becomes neutral events (see [`crate::transcript`]); this
//! module turns the events into:
//!
//! - **iterations**: each `oskac check` result the agent received, from a
//!   command it ran or from the check-on-edit hook (a hook that says nothing
//!   was clean, but the transcript cannot show it, so only a hook that
//!   reported diagnostics counts);
//! - a **diagnostic timeline**: the codes of each iteration, and for each
//!   diagnostic the iteration it appeared in and the one it went away in;
//! - **fix outcomes**: whether a fix was offered, applied and helped. These
//!   are heuristics. A fix is applied when an edit to its file, between its
//!   check and the next, inserts every replacement it offered (a deletion is
//!   an edit that shortens the text), or when the agent ran `oskac fix` on
//!   it, which applies machine fixes only. It helped when the same code in
//!   the same file is gone at the next check;
//! - **lookups**: the `oskac explain` codes and the site pages requested;
//! - **escapes**: tool calls with a path outside the workspace;
//! - a **false claim**: a run that ended with a final message while the
//!   grade, given with `--functional`, fails the functional checks.
//!
//! Diagnostics are read from `oskac check`'s JSON, from the hook's JSON, or
//! from `oskac check`'s text; unknown fields are ignored.

use crate::json::pretty;
use crate::transcript::{self, Call, Event, Stop};
use oska_syntax::{parse_json, JsonValue};
use std::collections::HashMap;
use std::process::ExitCode;

pub fn run(workspace: Option<&str>, functional: Option<&str>, transcript: &str) -> ExitCode {
    match analyse_file(workspace, functional, transcript) {
        Ok(report) => {
            print!("{}", pretty(&report));
            ExitCode::SUCCESS
        }
        Err(message) => {
            let report = JsonValue::Object(vec![
                field("schema", num(1)),
                field("errors", JsonValue::Array(vec![JsonValue::Str(message)])),
            ]);
            print!("{}", pretty(&report));
            ExitCode::from(2)
        }
    }
}

fn analyse_file(
    workspace: Option<&str>,
    functional: Option<&str>,
    path: &str,
) -> Result<JsonValue, String> {
    let functional = match functional {
        None => None,
        Some("pass") => Some(true),
        Some("fail") => Some(false),
        Some(other) => {
            return Err(format!(
                "`--functional` takes `pass` or `fail`, not `{other}`"
            ))
        }
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let events = transcript::claude_stream_json(&text).map_err(|e| format!("{path}: {e}"))?;
    analyse(&events, workspace, functional)
}

/// A diagnostic the agent was shown.
struct Diag {
    code: String,
    path: Option<String>,
    fix: Option<Fix>,
}

/// The fix a diagnostic carried.
struct Fix {
    /// `machine`, `maybe` or whatever the checker called it.
    applicability: String,
    /// What its edits insert; empty when the output did not say.
    replacements: Vec<String>,
    /// The file its edits change, when not the diagnostic's.
    path: Option<String>,
}

/// One check result the agent received.
struct Check {
    /// The event it came in.
    at: usize,
    source: &'static str,
    diags: Vec<Diag>,
}

struct EditAt {
    at: usize,
    path: String,
    removed: String,
    inserted: String,
}

struct FixRunAt {
    at: usize,
    /// The file or directory it ran on, normalised; `None` for the whole
    /// workspace.
    target: Option<String>,
}

/// A diagnostic followed from the iteration it appeared in.
struct Occurrence {
    code: String,
    path: Option<String>,
    first: usize,
    resolved: Option<usize>,
    present: usize,
    fix: Option<FixOutcome>,
}

struct FixOutcome {
    applicability: String,
    applied: bool,
    /// `None` while unknown: applied, but no check came after.
    helped: Option<bool>,
    step_unknown: bool,
}

fn analyse(
    events: &[Event],
    workspace: Option<&str>,
    functional: Option<bool>,
) -> Result<JsonValue, String> {
    let init_cwd = events.iter().find_map(|e| match e {
        Event::Init { cwd } => cwd.as_deref(),
        _ => None,
    });
    let workspace = workspace.or(init_cwd).ok_or_else(|| {
        "the transcript names no workspace; give it with `--workspace <dir>`".to_string()
    })?;
    let ws = normalize(workspace, "/");

    let mut calls: HashMap<&str, &Call> = HashMap::new();
    let mut checks: Vec<Check> = Vec::new();
    let mut edits: Vec<EditAt> = Vec::new();
    let mut fix_runs: Vec<FixRunAt> = Vec::new();
    let mut explain: Vec<String> = Vec::new();
    let mut site: Vec<String> = Vec::new();
    let mut escapes: Vec<(String, String)> = Vec::new();
    let mut usage = None;
    let mut last = None;

    for (at, event) in events.iter().enumerate() {
        match event {
            Event::ToolCall { id, call } => {
                calls.insert(id, call);
                for path in call.paths() {
                    if is_outside(path, &ws) {
                        push_unique(&mut escapes, (call.tool().to_string(), path.to_string()));
                    }
                }
                match call {
                    Call::Edit {
                        path,
                        removed,
                        inserted,
                    } => edits.push(EditAt {
                        at,
                        path: normalize(path, &ws),
                        removed: removed.clone(),
                        inserted: inserted.clone(),
                    }),
                    Call::Fetch(url) if !url.is_empty() => push_distinct(&mut site, url),
                    Call::Command(command) => {
                        for path in command_paths(command) {
                            if is_outside(&path, &ws) {
                                push_unique(&mut escapes, ("Bash".to_string(), path));
                            }
                        }
                        for url in command_urls(command) {
                            push_distinct(&mut site, &url);
                        }
                        for invocation in oskac_invocations(command) {
                            let first = invocation.args.first();
                            match invocation.sub.as_str() {
                                "explain" => {
                                    if let Some(code) = first {
                                        push_distinct(&mut explain, code);
                                    }
                                }
                                "fix" => fix_runs.push(FixRunAt {
                                    at,
                                    target: first.map(|t| normalize(t, &ws)).filter(|t| *t != ws),
                                }),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::ToolResult {
                id,
                output,
                is_error,
            } => {
                let Some(Call::Command(command)) = calls.get(id.as_str()) else {
                    continue;
                };
                if oskac_invocations(command).iter().any(|i| i.sub == "check") {
                    if let Some(diags) = parse_output(output, *is_error) {
                        checks.push(Check {
                            at,
                            source: "command",
                            diags,
                        });
                    }
                }
            }
            Event::Hook { output } => {
                if let Some(diags) = parse_output(output, true).filter(|d| !d.is_empty()) {
                    checks.push(Check {
                        at,
                        source: "hook",
                        diags,
                    });
                }
            }
            Event::Usage { .. } => usage = Some(event),
            Event::Final { text, stop } => last = Some((text, *stop)),
            Event::Init { .. } => {}
        }
    }

    let occurrences = follow(&checks, &edits, &fix_runs, &ws);
    let to_clean = checks
        .iter()
        .position(|c| c.diags.is_empty())
        .map(|i| i + 1);
    let stop = match last {
        Some((_, Stop::Finished)) => "finished",
        Some((_, Stop::TurnLimit)) => "turn_limit",
        Some((_, Stop::Error)) => "error",
        None => "cut_off",
    };
    let false_claim = match (functional, last) {
        (None, _) => JsonValue::Null,
        (Some(false), Some((_, Stop::Finished))) => JsonValue::Bool(true),
        (Some(_), _) => JsonValue::Bool(false),
    };

    Ok(JsonValue::Object(vec![
        field("schema", num(1)),
        field("stop", text(stop)),
        field("iterations", num(checks.len() as u64)),
        field(
            "iterations_to_clean",
            to_clean.map_or(JsonValue::Null, |n| num(n as u64)),
        ),
        field("timeline", timeline(&checks)),
        field(
            "diagnostics",
            JsonValue::Array(occurrences.iter().map(occurrence_json).collect()),
        ),
        field("fixes", fixes_json(&occurrences)),
        field(
            "lookups",
            JsonValue::Object(vec![
                field("explain", strings(&explain)),
                field("site", strings(&site)),
            ]),
        ),
        field(
            "escapes",
            JsonValue::Array(
                escapes
                    .iter()
                    .map(|(tool, path)| {
                        JsonValue::Object(vec![
                            field("tool", text(tool)),
                            field("path", text(path)),
                        ])
                    })
                    .collect(),
            ),
        ),
        field("usage", usage_json(usage)),
        field("false_claim", false_claim),
    ]))
}

/// Follows each diagnostic through the checks.
fn follow(checks: &[Check], edits: &[EditAt], fix_runs: &[FixRunAt], ws: &str) -> Vec<Occurrence> {
    let mut all: Vec<Occurrence> = Vec::new();
    // Open occurrences: (code, path) to index into `all`.
    let mut open: HashMap<(String, String), usize> = HashMap::new();
    for (i, check) in checks.iter().enumerate() {
        let key = |d: &Diag| {
            (
                d.code.clone(),
                d.path
                    .as_deref()
                    .map(|p| normalize(p, ws))
                    .unwrap_or_default(),
            )
        };
        let mut present: Vec<((String, String), &Diag)> = Vec::new();
        for diag in &check.diags {
            let k = key(diag);
            match present.iter_mut().find(|(other, _)| *other == k) {
                Some(slot) => {
                    if slot.1.fix.is_none() {
                        slot.1 = diag;
                    }
                }
                None => present.push((k, diag)),
            }
        }
        // What went away at this check.
        let gone: Vec<(String, String)> = open
            .keys()
            .filter(|k| !present.iter().any(|(p, _)| p == *k))
            .cloned()
            .collect();
        for k in gone {
            if let Some(index) = open.remove(&k) {
                all[index].resolved = Some(i + 1);
            }
        }
        let next = checks.get(i + 1);
        let window_end = next.map_or(usize::MAX, |n| n.at);
        for (k, diag) in present {
            let index = *open.entry(k.clone()).or_insert_with(|| {
                all.push(Occurrence {
                    code: diag.code.clone(),
                    path: diag.path.as_deref().map(|p| display(p, ws)),
                    first: i + 1,
                    resolved: None,
                    present: 0,
                    fix: None,
                });
                all.len() - 1
            });
            all[index].present += 1;
            let Some(fix) = &diag.fix else { continue };
            let file = fix.path.as_deref().or(diag.path.as_deref());
            let in_window = |at: usize| at > check.at && at < window_end;
            let applied = applied(
                fix,
                file.map(|f| normalize(f, ws)).as_deref(),
                &edits.iter().filter(|e| in_window(e.at)).collect::<Vec<_>>(),
                &fix_runs
                    .iter()
                    .filter(|r| in_window(r.at))
                    .collect::<Vec<_>>(),
            );
            let outcome = all[index].fix.get_or_insert_with(|| FixOutcome {
                applicability: fix.applicability.clone(),
                applied: false,
                helped: Some(false),
                step_unknown: false,
            });
            if applied {
                outcome.applied = true;
                match next {
                    None => outcome.step_unknown = true,
                    Some(n) => {
                        let still = n.diags.iter().any(|d| key(d) == k);
                        if !still {
                            outcome.helped = Some(true);
                        }
                    }
                }
            }
        }
    }
    for occurrence in &mut all {
        if let Some(fix) = &mut occurrence.fix {
            if !fix.applied {
                fix.helped = Some(false);
            } else if fix.helped != Some(true) && fix.step_unknown {
                fix.helped = None;
            }
        }
    }
    all
}

/// Whether an edit or an `oskac fix` run, between two checks, applied `fix`
/// to `file`.
fn applied(fix: &Fix, file: Option<&str>, edits: &[&EditAt], runs: &[&FixRunAt]) -> bool {
    let covers = |target: &Option<String>| match (target, file) {
        (None, _) | (_, None) => true,
        (Some(t), Some(f)) => f == t || f.starts_with(&format!("{t}/")),
    };
    if fix.applicability == "machine" && runs.iter().any(|r| covers(&r.target)) {
        return true;
    }
    let same_file = |e: &&&EditAt| file.map_or(true, |f| same_path(&e.path, f));
    let here: Vec<&&EditAt> = edits.iter().filter(same_file).collect();
    if here.is_empty() {
        return false;
    }
    fix.replacements.iter().all(|r| {
        here.iter().any(|e| {
            if r.is_empty() {
                e.inserted.len() < e.removed.len()
            } else {
                e.inserted.contains(r.as_str())
            }
        })
    })
}

/// Two normalised paths name the same file, allowing one to be written
/// relative to a package inside the workspace.
fn same_path(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

fn timeline(checks: &[Check]) -> JsonValue {
    JsonValue::Array(
        checks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let codes: Vec<String> = c.diags.iter().map(|d| d.code.clone()).collect();
                JsonValue::Object(vec![
                    field("iteration", num(i as u64 + 1)),
                    field("source", text(c.source)),
                    field("codes", strings(&codes)),
                ])
            })
            .collect(),
    )
}

fn occurrence_json(o: &Occurrence) -> JsonValue {
    let mut fields = vec![field("code", text(&o.code))];
    if let Some(path) = &o.path {
        fields.push(field("path", text(path)));
    }
    fields.push(field("first_iteration", num(o.first as u64)));
    fields.push(field(
        "resolved_iteration",
        o.resolved.map_or(JsonValue::Null, |n| num(n as u64)),
    ));
    fields.push(field("iterations_present", num(o.present as u64)));
    fields.push(field(
        "fix",
        match &o.fix {
            None => JsonValue::Null,
            Some(f) => JsonValue::Object(vec![
                field("applicability", text(&f.applicability)),
                field("applied", JsonValue::Bool(f.applied)),
                field("helped", f.helped.map_or(JsonValue::Null, JsonValue::Bool)),
            ]),
        },
    ));
    JsonValue::Object(fields)
}

fn fixes_json(occurrences: &[Occurrence]) -> JsonValue {
    let fixes: Vec<&FixOutcome> = occurrences.iter().filter_map(|o| o.fix.as_ref()).collect();
    let machine: Vec<&&FixOutcome> = fixes
        .iter()
        .filter(|f| f.applicability == "machine")
        .collect();
    let count = |list: &[&&FixOutcome], f: fn(&FixOutcome) -> bool| {
        num(list.iter().filter(|x| f(x)).count() as u64)
    };
    JsonValue::Object(vec![
        field("offered", num(fixes.len() as u64)),
        field(
            "applied",
            num(fixes.iter().filter(|f| f.applied).count() as u64),
        ),
        field(
            "helped",
            num(fixes.iter().filter(|f| f.helped == Some(true)).count() as u64),
        ),
        field("machine_offered", num(machine.len() as u64)),
        field("machine_applied", count(&machine, |f| f.applied)),
        field(
            "machine_helped",
            count(&machine, |f| f.helped == Some(true)),
        ),
    ])
}

fn usage_json(usage: Option<&Event>) -> JsonValue {
    let Some(Event::Usage {
        turns,
        input_tokens,
        output_tokens,
        cost,
    }) = usage
    else {
        return JsonValue::Null;
    };
    let opt = |v: &Option<String>| v.clone().map_or(JsonValue::Null, JsonValue::Number);
    JsonValue::Object(vec![
        field("turns", opt(turns)),
        field("input_tokens", opt(input_tokens)),
        field("output_tokens", opt(output_tokens)),
        field("cost_usd", opt(cost)),
    ])
}

// Reading what a check printed.

/// The diagnostics in the output of a check or a hook, or `None` when it is
/// not a check result: a failed command that printed no diagnostics.
fn parse_output(output: &str, is_error: bool) -> Option<Vec<Diag>> {
    let body = match output.strip_prefix("Exit code") {
        Some(rest) => rest.split_once('\n').map_or("", |(_, tail)| tail),
        None => output,
    };
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        if let Some(list) = parse_json(trimmed)
            .ok()
            .as_ref()
            .and_then(|v| v.get("diagnostics"))
            .and_then(JsonValue::as_array)
        {
            return Some(list.iter().filter_map(json_diag).collect());
        }
    }
    let diags = text_diags(body);
    if !diags.is_empty() || !is_error {
        Some(diags)
    } else {
        None
    }
}

fn json_diag(value: &JsonValue) -> Option<Diag> {
    let code = value.get("code")?.as_str()?.to_string();
    let path = value
        .get("path")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    let fix = value.get("fix").map(|fix| Fix {
        applicability: fix
            .get("applicability")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown")
            .to_string(),
        replacements: fix
            .get("edits")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| e.get("replacement").and_then(JsonValue::as_str))
            .map(str::to_string)
            .collect(),
        path: fix
            .get("path")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
    });
    Some(Diag { code, path, fix })
}

/// Diagnostics in `oskac check`'s text: `path:line:col: error[CODE]: ...`,
/// then, when it has one, `    fix: ...` or `    fix (maybe): ...`.
fn text_diags(body: &str) -> Vec<Diag> {
    let mut diags: Vec<Diag> = Vec::new();
    for line in body.lines() {
        let marker = [": error[", ": warning[", ": note["]
            .iter()
            .filter_map(|m| line.find(m).map(|p| (p, m.len())))
            .min();
        if let Some((at, len)) = marker {
            let Some(code) = line[at + len..].split(']').next() else {
                continue;
            };
            let location = &line[..at];
            let mut parts = location.rsplitn(3, ':');
            let (col, row, path) = (parts.next(), parts.next(), parts.next());
            let path = match (col.and_then(|c| c.parse::<u32>().ok()), row, path) {
                (Some(_), Some(r), Some(p)) if r.parse::<u32>().is_ok() => p,
                _ => location,
            };
            diags.push(Diag {
                code: code.to_string(),
                path: Some(path.to_string()),
                fix: None,
            });
        } else if let Some(rest) = line.trim_start().strip_prefix("fix") {
            let applicability = match rest.strip_prefix(" (").and_then(|r| r.split(')').next()) {
                Some(kind) => kind.to_string(),
                None => "machine".to_string(),
            };
            // `replace with `text``: the one replacement the text names.
            let replacements = rest
                .split_once("replace with `")
                .and_then(|(_, tail)| tail.split('`').next())
                .map(|r| vec![r.to_string()])
                .unwrap_or_default();
            if let Some(last) = diags.last_mut().filter(|d| d.fix.is_none()) {
                last.fix = Some(Fix {
                    applicability,
                    replacements,
                    path: None,
                });
            }
        }
    }
    diags
}

// Reading shell commands.

struct Invocation {
    sub: String,
    args: Vec<String>,
}

fn tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c| c == '"' || c == '\'')
                .trim_end_matches(';')
                .to_string()
        })
        .collect()
}

fn is_oskac(token: &str) -> bool {
    let name = token.rsplit(['/', '\\']).next().unwrap_or(token);
    name == "oskac" || name == "oskac.exe"
}

/// Each `oskac <subcommand> <args>` in a command line, without its flags.
fn oskac_invocations(command: &str) -> Vec<Invocation> {
    let mut found = Vec::new();
    let mut segment: Vec<String> = Vec::new();
    let mut flush = |segment: &mut Vec<String>| {
        if let Some(at) = segment.iter().position(|t| is_oskac(t)) {
            if let Some(sub) = segment.get(at + 1) {
                found.push(Invocation {
                    sub: sub.clone(),
                    args: segment[at + 2..]
                        .iter()
                        .filter(|a| !a.starts_with('-'))
                        .cloned()
                        .collect(),
                });
            }
        }
        segment.clear();
    };
    for token in tokens(command) {
        if matches!(token.as_str(), "&&" | "||" | "|" | "") {
            flush(&mut segment);
        } else {
            segment.push(token);
        }
    }
    flush(&mut segment);
    found
}

/// Path-like words in a command line: absolute, home-relative, or climbing
/// out with `..`. The `oskac` being run and `/dev/null` are not paths the
/// agent chose to reach.
fn command_paths(command: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in tokens(command) {
        let mut word = token.as_str();
        if let Some(at) = word.rfind(['>', '<']) {
            word = &word[at + 1..];
        }
        if word.starts_with("--") {
            word = word.split_once('=').map_or("", |(_, value)| value);
        }
        let word = word.trim_end_matches([')', '`']);
        let looks_like_path = word.starts_with('/')
            || word.starts_with('~')
            || word == ".."
            || word.starts_with("../")
            || is_drive(word);
        if looks_like_path
            && !word.contains("://")
            && word != "/dev/null"
            && !is_oskac(word)
            && word != "/"
        {
            paths.push(word.to_string());
        }
    }
    paths
}

fn command_urls(command: &str) -> Vec<String> {
    tokens(command)
        .into_iter()
        .filter(|t| t.starts_with("http://") || t.starts_with("https://"))
        .collect()
}

// Paths.

fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// `path` as a rooted path with no `.` or `..`, a relative one taken from
/// `base`.
fn normalize(path: &str, base: &str) -> String {
    let path = path.replace('\\', "/");
    let joined = if path.starts_with('/') || is_drive(&path) {
        path
    } else {
        format!("{base}/{path}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

fn is_outside(path: &str, ws: &str) -> bool {
    if path.starts_with('~') {
        return true;
    }
    let full = normalize(path, ws);
    !(full == ws || full.starts_with(&format!("{ws}/")))
}

/// A path as the report shows it: relative to the workspace when inside.
fn display(path: &str, ws: &str) -> String {
    let full = normalize(path, ws);
    match full.strip_prefix(&format!("{ws}/")) {
        Some(rest) => rest.to_string(),
        None => path.replace('\\', "/"),
    }
}

// JSON helpers.

fn field(key: &str, value: JsonValue) -> (String, JsonValue) {
    (key.to_string(), value)
}

fn num(n: u64) -> JsonValue {
    JsonValue::Number(n.to_string())
}

fn text(s: &str) -> JsonValue {
    JsonValue::Str(s.to_string())
}

fn strings(items: &[String]) -> JsonValue {
    JsonValue::Array(items.iter().map(|s| text(s)).collect())
}

fn push_distinct(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|x| x == item) {
        list.push(item.to_string());
    }
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}
