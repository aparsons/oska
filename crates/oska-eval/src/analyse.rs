// SPDX-License-Identifier: Apache-2.0
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
//! - **fix outcomes**: whether a fix was offered, applied, and whether its
//!   diagnostic was gone at the next check. These are heuristics. A fix is
//!   applied when, between its check and the next, an edit to its file
//!   makes the change each of the fix's edits offers (a transcript shows an
//!   edit's old and new text, not its position, so "the same span and text"
//!   means: the new text holds the replacement, and the text grew or shrank
//!   by exactly what the fix's span and replacement say; a whole-file write
//!   can only show the replacement is present, and a fix whose edits the
//!   output did not give is applied only by `oskac fix`), or when the agent
//!   ran `oskac fix` on it, which applies machine fixes only.
//!   `gone_next_check` says only that the next check shows one fewer
//!   diagnostic with that code in that file; it does not say the fix did it.
//!   Diagnostics with one code in one file are told apart by their order in
//!   the check, so two of them are never merged;
//! - **lookups**: the `oskac explain` codes and the site pages requested;
//! - **escapes**: tool calls and shell words that name a path outside the
//!   workspace (`outside`) or that cannot be resolved without running
//!   anything (`unresolved`: possibly outside). See the limits below;
//! - **finished_despite_failing**: a run that ended with a final message
//!   while the grade, given with `--functional`, fails the functional
//!   checks. It does not read the message, so it is not a claim of success.
//!
//! # Limits of the escape check
//!
//! It reads text, never the file system. Symlinks are not resolved, so a
//! link inside the workspace that points out is not seen. Shell words are
//! normalised against the workspace, not against a directory an earlier
//! `cd` moved to (which can only over-report). It fails closed: inside is
//! the proven case, never the default. A shell word with an expansion
//! character (`$`, a backtick, `{ } * ? [ ( ) ! < > | ; &`) is reported as
//! `unresolved` unless it is a glob that plainly stays inside (`src/*.osk`);
//! so are variables other than `HOME`, command substitutions, brace
//! expansion, a glob component that can match `..` (`.*`, `[.][.]`), `sh -c`
//! and `eval` strings, and any glob in the target of `cd` or `git -C`. A
//! `Glob` or `Grep` pattern is unresolved when it has braces and names
//! paths, or has a component that can match `..`. A script the agent wrote
//! and then ran, and a path a program builds itself, are beyond it. A `Grep`
//! pattern is a regular expression but is read as a path when it has a
//! separator, so it can over-report.
//!
//! Diagnostics are read from `oskac check`'s JSON, from the hook's JSON, or
//! from `oskac check`'s text; unknown fields are ignored.

use crate::json::pretty;
use crate::transcript::{self, Call, Change, Event, Stop};
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
    /// Its edits; empty when the output did not say.
    edits: Vec<FixEdit>,
    /// The file its edits change, when not the diagnostic's.
    path: Option<String>,
}

/// One edit of a fix.
struct FixEdit {
    /// The bytes it replaces, when the output gave a span.
    span: Option<(u64, u64)>,
    replacement: String,
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
    changes: Vec<Change>,
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
    gone_next_check: Option<bool>,
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
    let mut escapes: Vec<(String, String, Status)> = Vec::new();
    let mut usage = None;
    let mut last = None;

    for (at, event) in events.iter().enumerate() {
        match event {
            Event::ToolCall { id, call } => {
                calls.insert(id, call);
                for path in call.paths() {
                    if let Some(status) = verdict(path, &ws, Kind::Path) {
                        push_unique(
                            &mut escapes,
                            (call.tool().to_string(), path.to_string(), status),
                        );
                    }
                }
                for pattern in call.patterns() {
                    if let Some(status) = verdict(pattern, &ws, Kind::Pattern) {
                        push_unique(
                            &mut escapes,
                            (call.tool().to_string(), pattern.to_string(), status),
                        );
                    }
                }
                match call {
                    Call::Edit { path, changes } => edits.push(EditAt {
                        at,
                        path: normalize(path, &ws),
                        changes: changes.clone(),
                    }),
                    Call::Fetch(url) if !url.is_empty() => push_distinct(&mut site, url),
                    Call::Command(command) => {
                        for (path, status) in command_escapes(command, &ws) {
                            push_unique(&mut escapes, ("Bash".to_string(), path, status));
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
    let finished_despite_failing = match (functional, last) {
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
                    .map(|(tool, path, status)| {
                        JsonValue::Object(vec![
                            field("tool", text(tool)),
                            field("path", text(path)),
                            field("status", text(status.name())),
                        ])
                    })
                    .collect(),
            ),
        ),
        field("usage", usage_json(usage)),
        field("finished_despite_failing", finished_despite_failing),
    ]))
}

/// A diagnostic's identity across checks: code, normalised path, and its
/// order among those with the same code and path.
type Key = (String, String, usize);

fn file_key(diag: &Diag, ws: &str) -> String {
    diag.path
        .as_deref()
        .map(|p| normalize(p, ws))
        .unwrap_or_default()
}

/// Follows each diagnostic through the checks.
fn follow(checks: &[Check], edits: &[EditAt], fix_runs: &[FixRunAt], ws: &str) -> Vec<Occurrence> {
    let mut all: Vec<Occurrence> = Vec::new();
    // Open occurrences: (code, path) to index into `all`.
    let mut open: HashMap<Key, usize> = HashMap::new();
    for (i, check) in checks.iter().enumerate() {
        // Diagnostics with one code in one file are told apart by their
        // order in the check; none is merged into another.
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        let mut present: Vec<(Key, &Diag)> = Vec::new();
        for diag in &check.diags {
            let base = (diag.code.clone(), file_key(diag, ws));
            let ordinal = counts.entry(base.clone()).or_insert(0);
            present.push(((base.0, base.1, *ordinal), diag));
            *ordinal += 1;
        }
        // What went away at this check.
        let gone: Vec<Key> = open
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
                gone_next_check: Some(false),
                step_unknown: false,
            });
            if applied {
                outcome.applied = true;
                match next {
                    None => outcome.step_unknown = true,
                    Some(n) => {
                        let same = n
                            .diags
                            .iter()
                            .filter(|d| d.code == k.0 && file_key(d, ws) == k.1)
                            .count();
                        if same <= k.2 {
                            outcome.gone_next_check = Some(true);
                        }
                    }
                }
            }
        }
    }
    for occurrence in &mut all {
        if let Some(fix) = &mut occurrence.fix {
            if !fix.applied {
                fix.gone_next_check = Some(false);
            } else if fix.gone_next_check != Some(true) && fix.step_unknown {
                fix.gone_next_check = None;
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
    let same_file = |e: &&&EditAt| file.is_none_or(|f| same_path(&e.path, f));
    let here: Vec<&&EditAt> = edits.iter().filter(same_file).collect();
    if here.is_empty() {
        return false;
    }
    // A fix whose edits the output did not give is applied only by
    // `oskac fix`.
    !fix.edits.is_empty()
        && fix.edits.iter().all(|fe| {
            here.iter()
                .any(|e| e.changes.iter().any(|c| matches(fe, c)))
        })
}

/// Whether `change` makes the change `fix` offers: its new text holds the
/// replacement and, for an edit that shows its old text, the text grew or
/// shrank by what the span and replacement say.
fn matches(fix: &FixEdit, change: &Change) -> bool {
    let holds = fix.replacement.is_empty() || change.inserted.contains(&fix.replacement);
    match (&change.removed, fix.span) {
        (None, _) => !fix.replacement.is_empty() && holds,
        (Some(removed), Some((start, end))) => {
            let span = end.saturating_sub(start) as i64;
            let delta = change.inserted.len() as i64 - removed.len() as i64;
            holds
                && removed.len() as i64 >= span
                && delta == fix.replacement.len() as i64 - span
                && (span > 0 || !fix.replacement.is_empty())
        }
        (Some(removed), None) => {
            !fix.replacement.is_empty() && holds && *removed != change.inserted
        }
    }
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
                field(
                    "gone_next_check",
                    f.gone_next_check.map_or(JsonValue::Null, JsonValue::Bool),
                ),
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
            "gone_next_check",
            num(fixes
                .iter()
                .filter(|f| f.gone_next_check == Some(true))
                .count() as u64),
        ),
        field("machine_offered", num(machine.len() as u64)),
        field("machine_applied", count(&machine, |f| f.applied)),
        field(
            "machine_gone_next_check",
            count(&machine, |f| f.gone_next_check == Some(true)),
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
        edits: fix
            .get("edits")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| {
                let replacement = e.get("replacement").and_then(JsonValue::as_str)?;
                let span = e.get("span").and_then(|sp| {
                    let at = |k: &str| match sp.get(k) {
                        Some(JsonValue::Number(n)) => n.parse::<u64>().ok(),
                        _ => None,
                    };
                    Some((at("start")?, at("end")?))
                });
                Some(FixEdit {
                    span,
                    replacement: replacement.to_string(),
                })
            })
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
            // `replace with `text``: the one edit the text names.
            let edits = rest
                .split_once("replace with `")
                .and_then(|(_, tail)| tail.split('`').next())
                .map(|r| {
                    vec![FixEdit {
                        span: None,
                        replacement: r.to_string(),
                    }]
                })
                .unwrap_or_default();
            if let Some(last) = diags.last_mut().filter(|d| d.fix.is_none()) {
                last.fix = Some(Fix {
                    applicability,
                    edits,
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

/// The words of a command line, as a shell splits them: quotes group a
/// word (a quoted path may hold spaces) and are removed, and `\ ` keeps a
/// space in a word. Each `;`, `&`, `|` or newline outside quotes ends the
/// command: it becomes one empty word. Nothing is expanded.
fn tokens(command: &str) -> Vec<String> {
    fn end(words: &mut Vec<String>, word: &mut String, started: &mut bool) {
        if *started {
            words.push(std::mem::take(word));
            *started = false;
        }
    }
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') if matches!(chars.peek(), Some(' ' | '"' | '\'')) => {
                word.push(chars.next().unwrap_or(' '));
                started = true;
            }
            (None, ';' | '&' | '|' | '\n') => {
                end(&mut words, &mut word, &mut started);
                if words.last().is_some_and(|w| !w.is_empty()) {
                    words.push(String::new());
                }
            }
            (None, c) if c.is_whitespace() => end(&mut words, &mut word, &mut started),
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    end(&mut words, &mut word, &mut started);
    words
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
        if token.is_empty() {
            flush(&mut segment);
        } else {
            segment.push(token);
        }
    }
    flush(&mut segment);
    found
}

/// Where a path stands against the workspace, when not inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Outside,
    /// Not decidable from the text: possibly outside.
    Unresolved,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Outside => "outside",
            Status::Unresolved => "unresolved, possibly outside",
        }
    }
}

const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "dash", "ksh", "eval"];
const WRAPPERS: [&str; 7] = ["sudo", "env", "command", "exec", "time", "nohup", "xargs"];

/// Words in a command line that reach outside the workspace, or may. The
/// `oskac` being run and `/dev/null` are not paths the agent chose to reach.
fn command_escapes(command: &str, ws: &str) -> Vec<(String, Status)> {
    let mut found: Vec<(String, Status)> = Vec::new();
    let words = tokens(command);
    for segment in words.split(|w| w.is_empty()) {
        // The program: past `VAR=value` words and wrappers.
        let mut start = 0;
        while let Some(w) = segment.get(start) {
            let assignment = w.split_once('=').is_some_and(|(k, _)| {
                !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_')
            });
            if assignment || WRAPPERS.contains(&base_name(w)) {
                start += 1;
            } else {
                break;
            }
        }
        let Some(program) = segment.get(start).map(|w| base_name(w)) else {
            continue;
        };
        let args = &segment[start + 1..];
        // Words before the program (`VAR=value`, wrappers) and the program
        // itself are read like any other word.
        let before = &segment[..=start];
        // Words taken as a directory the command moves to.
        let mut targets: Vec<usize> = Vec::new();
        if SHELLS.contains(&program) {
            let script = if program == "eval" {
                Some(args.join(" ")).filter(|s| !s.is_empty())
            } else {
                args.iter()
                    .position(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
                    .and_then(|i| args.get(i + 1).cloned())
            };
            if let Some(script) = script {
                push_unique(&mut found, (script, Status::Unresolved));
            }
        } else if matches!(program, "cd" | "pushd" | "chdir") {
            match args.iter().position(|a| !a.starts_with('-') || a == "-") {
                Some(i) => targets.push(i),
                None => push_unique(&mut found, ("~".to_string(), Status::Outside)),
            }
        } else if program == "git" {
            targets = (0..args.len().saturating_sub(1))
                .filter(|&i| args[i] == "-C")
                .map(|i| i + 1)
                .collect();
        }
        let words = before.iter().map(|w| (w, false)).chain(
            args.iter()
                .enumerate()
                .map(|(i, w)| (w, targets.contains(&i))),
        );
        for (arg, strict) in words {
            let kind = if strict { Kind::Target } else { Kind::Word };
            for value in path_values(arg) {
                if skip_word(&value) {
                    continue;
                }
                if let Some(status) = verdict(&value, ws, kind) {
                    push_unique(&mut found, (value, status));
                }
            }
        }
    }
    found
}

fn base_name(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
}

fn skip_word(word: &str) -> bool {
    word.contains("://") || word == "/dev/null" || word == "/" || is_oskac(word)
}

/// The parts of `arg` that may be paths: the word, what follows a
/// redirection, what follows `=` (`--out=x`, `VAR=x`), and what follows a
/// short flag's letter (`-o/abs`).
fn path_values(arg: &str) -> Vec<String> {
    let mut word = arg;
    if let Some(at) = word.rfind(['>', '<']) {
        word = &word[at + 1..];
    }
    let mut values = Vec::new();
    if !word.starts_with("--") {
        values.push(word.to_string());
    }
    if let Some((key, value)) = word.split_once('=') {
        if key.starts_with("--") || key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            values.push(value.to_string());
        }
    }
    if word.starts_with('-') && !word.starts_with("--") && word.len() > 2 {
        let rest = word.get(2..).unwrap_or("");
        values.push(rest.strip_prefix('=').unwrap_or(rest).to_string());
    }
    for v in &mut values {
        *v = v.trim_end_matches([')', '`']).to_string();
    }
    values.retain(|v| !v.is_empty());
    values
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

/// What a word is, which decides what it may hold and still be inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A word of a shell command: the shell expands it.
    Word,
    /// A word that is the directory a command moves to (`cd`, `git -C`).
    Target,
    /// A path a tool was given: not expanded, but `$` and `~` are read.
    Path,
    /// A Glob or Grep pattern: a glob or a regular expression.
    Pattern,
}

/// Characters a shell expands or acts on. A shell word that holds one is
/// not inside unless it is a glob that plainly stays inside.
const EXPANSION: [char; 14] = [
    '$', '`', '{', '}', '*', '?', '[', '(', ')', '!', '<', '>', '|', ';',
];

/// Whether `path` is outside the workspace or cannot be told, and `None`
/// only when it is no path or plainly inside. Inside is the proven case:
/// anything with an expansion character is unresolved unless it is a glob
/// whose every component is a name, `*` or `?` among other letters.
fn verdict(path: &str, ws: &str, kind: Kind) -> Option<Status> {
    let after_home = |rest: &str| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_');
    if path.starts_with('~')
        || path.strip_prefix("$HOME").is_some_and(after_home)
        || path.starts_with("${HOME}")
    {
        return Some(Status::Outside);
    }
    let separator = path.contains(['/', '\\']);
    let shell = matches!(kind, Kind::Word | Kind::Target);
    let unresolved = match kind {
        Kind::Word | Kind::Target => path.contains(EXPANSION) || path.contains('&'),
        Kind::Path => path.contains(['$', '`']),
        // A pattern's braces choose between paths, so they are read only
        // where the pattern names paths.
        Kind::Pattern => {
            path.contains(['{', '}']) && (separator || path.contains("..") || path.contains('~'))
        }
    };
    let glob_only =
        |p: &str| !p.contains(['$', '`', '{', '}', '(', ')', '!', '<', '>', '|', ';', '&']);
    if unresolved && !(shell && glob_only(path) && !path.contains(['{', '}'])) {
        return Some(Status::Unresolved);
    }
    if kind == Kind::Target && (path == "-" || path.contains(['*', '?', '[', '{'])) {
        return Some(Status::Unresolved);
    }
    if kind == Kind::Pattern && !separator && path != ".." {
        return None;
    }
    // A component made of globs and dots can match `..`.
    let reaches_up = |component: &str| {
        let glob = component.contains(['*', '?']);
        let literal: Vec<char> = component.chars().filter(|c| !"*?".contains(*c)).collect();
        component.contains('[')
            || (glob && !literal.is_empty() && literal.iter().all(|c| *c == '.'))
    };
    if kind != Kind::Path && path.split(['/', '\\']).any(reaches_up) {
        return Some(Status::Unresolved);
    }
    if separator || path == ".." {
        let full = normalize(path, ws);
        if !(full == ws || full.starts_with(&format!("{ws}/"))) {
            return Some(Status::Outside);
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "/work/ws";

    fn escapes(command: &str) -> Vec<(String, Status)> {
        command_escapes(command, WS)
    }

    fn outside(command: &str, word: &str) {
        assert_eq!(
            escapes(command),
            vec![(word.to_string(), Status::Outside)],
            "{command}"
        );
    }

    fn unresolved(command: &str, word: &str) {
        assert_eq!(
            escapes(command),
            vec![(word.to_string(), Status::Unresolved)],
            "{command}"
        );
    }

    fn inside(command: &str) {
        assert_eq!(escapes(command), vec![], "{command}");
    }

    #[test]
    fn a_climb_hidden_in_the_middle_of_a_word_is_outside() {
        outside("cat a/../../x", "a/../../x");
        outside("cat src/../..", "src/../..");
        outside("cat ..", "..");
        outside("cat a\\..\\..\\x", "a\\..\\..\\x");
        inside("cat a/../b/x");
    }

    #[test]
    fn home_is_outside_in_each_spelling() {
        outside("cat $HOME/.ssh/id_rsa", "$HOME/.ssh/id_rsa");
        outside("cat ${HOME}/x", "${HOME}/x");
        outside("cat ~/x", "~/x");
        outside("cd", "~");
    }

    #[test]
    fn a_short_flag_path_is_read() {
        outside("gcc -o/tmp/a x.c", "/tmp/a");
        outside("gcc -I../inc x.c", "../inc");
        outside("gcc -o=/tmp/a x.c", "/tmp/a");
        inside("gcc -o out/a x.c");
    }

    #[test]
    fn a_quoted_path_with_spaces_is_one_word() {
        outside("cat \"../my dir/x\"", "../my dir/x");
        outside("cat '/opt/my dir/x'", "/opt/my dir/x");
        outside("cat ../my\\ dir/x", "../my dir/x");
        inside("cat \"my dir/x\"");
    }

    #[test]
    fn a_redirection_or_assignment_target_is_read() {
        outside("echo hi>/etc/x", "/etc/x");
        outside("echo hi 2>>../log", "../log");
        outside("dd of=/dev/sda", "/dev/sda");
        outside("cp x --target-directory=/tmp", "/tmp");
    }

    #[test]
    fn what_cannot_be_resolved_is_never_inside() {
        unresolved("cat $DIR/x", "$DIR/x");
        unresolved("cat $(pwd)/../x", "$(pwd)/../x");
        unresolved("bash -c 'cat /etc/passwd'", "cat /etc/passwd");
        unresolved("sh -c \"echo hi\"", "echo hi");
        unresolved("eval echo hi", "echo hi");
        unresolved("cd src/*", "src/*");
        unresolved("git -C ../*/x status", "../*/x");
        unresolved("cd $PROJECT", "$PROJECT");
        unresolved("cd -", "-");
    }

    #[test]
    fn a_command_after_a_separator_is_read_too() {
        outside("cd src; cat /etc/passwd", "/etc/passwd");
        outside("true && cat ../x | wc", "../x");
        inside("cd src && ls | wc -l");
    }

    #[test]
    fn git_c_into_the_workspace_is_inside() {
        inside("git -C src status");
        outside("git -C /elsewhere status", "/elsewhere");
    }

    #[test]
    fn tool_inputs_use_the_same_rules() {
        let path = |p| verdict(p, WS, Kind::Path);
        let pattern = |p| verdict(p, WS, Kind::Pattern);
        assert_eq!(path("a/../../x"), Some(Status::Outside));
        assert_eq!(path("$HOME/x"), Some(Status::Outside));
        assert_eq!(path("$DIR"), Some(Status::Unresolved));
        assert_eq!(path("src"), None);
        assert_eq!(pattern("../**/*.osk"), Some(Status::Outside));
        assert_eq!(pattern("/work/ws/**/*.osk"), None);
        assert_eq!(pattern("src/*.osk"), None);
    }

    #[test]
    fn a_pattern_that_may_reach_above_the_workspace_is_unresolved() {
        let pattern = |p| verdict(p, WS, Kind::Pattern);
        assert_eq!(pattern("{/etc,src}/*"), Some(Status::Unresolved));
        assert_eq!(pattern("{..,x}/y"), Some(Status::Unresolved));
        assert_eq!(pattern(".*/x"), Some(Status::Unresolved));
        assert_eq!(pattern("[.][.]/x"), Some(Status::Unresolved));
        // A regular expression with braces or dots names no path.
        assert_eq!(pattern("a{2}"), None);
        assert_eq!(pattern("fn main"), None);
        assert_eq!(pattern(".*"), None);
    }

    #[test]
    fn ordinary_commands_in_the_workspace_are_inside() {
        inside("cat src/main.osk");
        inside("oskac check .");
        inside("/opt/oska/bin/oskac check src");
        inside("ls src/*.osk");
        inside("cat ./a/../b.txt");
        inside("grep -rn \"fn main\" src");
        assert_eq!(verdict("fn main", WS, Kind::Pattern), None);
        assert_eq!(verdict("src", WS, Kind::Path), None);
    }

    #[test]
    fn a_bare_variable_or_substitution_is_unresolved() {
        unresolved("cat $F", "$F");
        unresolved("cat \"$(echo /etc/passwd)\"", "$(echo /etc/passwd");
        unresolved("cat `pwd`", "`pwd");
        unresolved("$PROG src", "$PROG");
    }

    #[test]
    fn a_brace_expansion_is_unresolved() {
        unresolved("cat {/etc/passwd,a}", "{/etc/passwd,a}");
        unresolved("cat {..,x}/y", "{..,x}/y");
        unresolved("ls --exclude={a,b} src", "{a,b}");
    }

    #[test]
    fn a_glob_that_can_match_dot_dot_is_unresolved() {
        unresolved("ls .*", ".*");
        unresolved("cat .*/x", ".*/x");
        unresolved("cat ..?/x", "..?/x");
        unresolved("cat [.][.]/x", "[.][.]/x");
        inside("cat .git*");
    }

    fn fix(span: Option<(u64, u64)>, replacement: &str) -> FixEdit {
        FixEdit {
            span,
            replacement: replacement.to_string(),
        }
    }

    fn change(removed: Option<&str>, inserted: &str) -> Change {
        Change {
            removed: removed.map(str::to_string),
            inserted: inserted.to_string(),
        }
    }

    #[test]
    fn an_edit_must_match_the_fix_span_and_text() {
        // Delete one byte.
        let delete = fix(Some((1, 2)), "");
        assert!(matches(&delete, &change(Some("a;"), "a")));
        assert!(
            !matches(&delete, &change(Some("abcd"), "ab")),
            "shrinks by 2"
        );
        assert!(!matches(&delete, &change(Some("a"), "a")));
        // Replace two bytes with `xyz`.
        let replace = fix(Some((4, 6)), "xyz");
        assert!(matches(&replace, &change(Some("ab"), "xyz")), "grows by 1");
        assert!(
            !matches(&replace, &change(Some("ab"), "xyw")),
            "not the text"
        );
        assert!(
            !matches(&replace, &change(Some("ab"), "axyzb")),
            "grows by 3"
        );
        // Without a span, only the text can be checked.
        let text = fix(None, "xyz");
        assert!(matches(&text, &change(Some("ab"), "axyzb")));
        assert!(!matches(&text, &change(Some("ab"), "ab")));
        // A deletion with no span cannot be shown, so it is not applied.
        assert!(!matches(&fix(None, ""), &change(Some("ab"), "a")));
        // A whole-file write shows the text, not a deletion.
        assert!(matches(&text, &change(None, "..xyz..")));
        assert!(!matches(&delete, &change(None, "short")));
    }
}
