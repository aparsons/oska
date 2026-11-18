//! `validate`: checks every task in a task set, offline, so a broken task
//! is caught before it costs a paid run.
//!
//! Every task's files must be well formed. An active task's hidden checks
//! then run against its two solutions: the reference solution must pass
//! every check, and the insecure solution must pass every functional check
//! and fail at least one security check. A deferred task's sinks do not run
//! yet, so each solution need only check with each group of hidden checks.
//! Every task's starting package must check.

use crate::grader::Oskac;
use crate::task::{self, Task};
use crate::workspace::Workspace;
use oska_syntax::JsonValue;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// One check of one solution, and how it came out.
struct Check {
    group: &'static str,
    name: String,
    /// `None` if it passed, or why it failed.
    failure: Option<String>,
    /// It is one of the task's hidden tests, not a check the harness makes.
    test: bool,
}

impl Check {
    fn new(group: &'static str, name: &str, failure: Option<String>) -> Check {
        Check {
            group,
            name: name.to_string(),
            failure,
            test: false,
        }
    }

    fn test(group: &'static str, name: &str, failure: Option<String>) -> Check {
        Check {
            test: true,
            ..Check::new(group, name, failure)
        }
    }

    fn json(&self) -> JsonValue {
        let mut fields = vec![
            field("group", JsonValue::Str(self.group.to_string())),
            field("check", JsonValue::Str(self.name.clone())),
            field(
                "outcome",
                JsonValue::Str(
                    if self.failure.is_some() {
                        "failed"
                    } else {
                        "passed"
                    }
                    .to_string(),
                ),
            ),
        ];
        if let Some(failure) = &self.failure {
            fields.push(field("detail", JsonValue::Str(failure.clone())));
        }
        JsonValue::Object(fields)
    }
}

/// What validating one well-formed task found.
struct Validated {
    reference: Vec<Check>,
    insecure: Vec<Check>,
    failures: Vec<String>,
}

pub fn run(oskac: &str, tasks: &str) -> ExitCode {
    let entries = match task::load_set(Path::new(tasks)) {
        Ok(entries) => entries,
        Err(message) => {
            // Bad input is reported the way every other kind is: a report
            // on stdout, exit 2.
            let report = JsonValue::Object(vec![
                field("schema", JsonValue::Number("1".to_string())),
                field("valid", JsonValue::Bool(false)),
                field("errors", strings(&[message])),
                field("tasks", JsonValue::Array(Vec::new())),
            ]);
            print!("{}", crate::json::pretty(&report));
            return ExitCode::from(2);
        }
    };
    let oskac = Oskac::new(oskac);
    let results = validate_all(&oskac, &entries);

    let mut bad_input = false;
    let mut failed = false;
    let mut reports = Vec::new();
    for (entry, result) in entries.iter().zip(results) {
        let mut fields = vec![field("id", JsonValue::Str(entry.id.clone()))];
        match (&entry.task, result) {
            (Ok(task), Some(Ok(validated))) => {
                fields.push(field("class", JsonValue::Str(task.class.clone())));
                match &task.deferred {
                    None => fields.push(field("status", JsonValue::Str("active".to_string()))),
                    Some(reason) => {
                        fields.push(field("status", JsonValue::Str("deferred".to_string())));
                        fields.push(field("reason", JsonValue::Str(reason.clone())));
                    }
                }
                fields.push(field("reference", checks(&validated.reference)));
                fields.push(field("insecure", checks(&validated.insecure)));
                failed |= !validated.failures.is_empty();
                fields.push(field("failures", strings(&validated.failures)));
            }
            (Err(errors), _) => {
                bad_input = true;
                fields.push(field("errors", strings(errors)));
            }
            (Ok(_), Some(Err(error))) => {
                bad_input = true;
                fields.push(field(
                    "errors",
                    strings(&[format!("{}: {error}", entry.id)]),
                ));
            }
            (Ok(_), None) => {}
        }
        reports.push(JsonValue::Object(fields));
    }
    let report = JsonValue::Object(vec![
        field("schema", JsonValue::Number("1".to_string())),
        field("valid", JsonValue::Bool(!bad_input && !failed)),
        field("tasks", JsonValue::Array(reports)),
    ]);
    print!("{}", crate::json::pretty(&report));
    if bad_input {
        ExitCode::from(2)
    } else if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Validates every well-formed task, on at most `available_parallelism`
/// threads (each task runs several `oskac` processes), and returns the
/// outcomes in the entries' order.
fn validate_all(oskac: &Oskac, entries: &[task::Entry]) -> Vec<Option<Result<Validated, String>>> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(entries.len().max(1));
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(entry) = entries.get(index) else {
                    break;
                };
                let outcome = entry.task.as_ref().ok().map(|task| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(oskac, task)))
                        .unwrap_or_else(|_| Err("validating this task panicked".to_string()))
                });
                if let Ok(mut done) = done.lock() {
                    done.push((index, outcome));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_default();
    done.sort_by_key(|(index, _)| *index);
    done.into_iter().map(|(_, outcome)| outcome).collect()
}

fn validate(oskac: &Oskac, task: &Task) -> Result<Validated, String> {
    let reference = solution_checks(oskac, task, "reference")?;
    let insecure = solution_checks(oskac, task, "insecure")?;
    let mut failures = Vec::new();
    let start = Workspace::new()?;
    start.copy_in(&task.start())?;
    if let Some(diagnostic) = oskac.check(start.path())?.diagnostics.into_iter().next() {
        failures.push(format!("the starting package does not check: {diagnostic}"));
    }
    for check in reference.iter().filter(|c| c.failure.is_some()) {
        failures.push(format!(
            "the reference solution fails the {} check `{}`",
            check.group, check.name
        ));
    }
    if task.deferred.is_some() {
        for check in insecure.iter().filter(|c| c.failure.is_some()) {
            failures.push(format!(
                "the insecure solution fails the {} check `{}`",
                check.group, check.name
            ));
        }
    } else {
        if !reference.iter().any(|c| c.test && c.group == "functional") {
            failures.push("the functional checks run no test".to_string());
        }
        for check in insecure.iter().filter(|c| c.group == "functional") {
            if check.failure.is_some() {
                failures.push(format!(
                    "the insecure solution fails the functional check `{}`",
                    check.name
                ));
            }
        }
        if !insecure
            .iter()
            .any(|c| c.group == "security" && c.failure.is_some())
        {
            failures.push(
                "the insecure solution passes every security check, so the checks cannot \
                 tell it from the reference solution"
                    .to_string(),
            );
        }
    }
    Ok(Validated {
        reference,
        insecure,
        failures,
    })
}

/// Every check of the solution `name`, in a workspace holding the starting
/// package with the solution's files over it.
fn solution_checks(oskac: &Oskac, task: &Task, name: &str) -> Result<Vec<Check>, String> {
    let workspace = Workspace::new()?;
    workspace.copy_in(&task.start())?;
    workspace.copy_in(&task.solution(name))?;
    let dir = workspace.path();
    let mut checks = Vec::new();

    if task.deferred.is_some() {
        for group in ["functional", "security"] {
            workspace.set_checks(Some(&task.checks(group)))?;
            let report = oskac.check(dir)?;
            checks.push(Check::new(
                group,
                "the package checks",
                report.diagnostics.into_iter().next(),
            ));
        }
        return Ok(checks);
    }

    for group in ["functional", "security"] {
        workspace.set_checks(Some(&task.checks(group)))?;
        let run = oskac.test(dir)?;
        if let Some(diagnostic) = run.diagnostics.into_iter().next() {
            checks.push(Check::new(group, "the package checks", Some(diagnostic)));
        }
        for (test, failure) in run.tests {
            checks.push(Check::test(group, &test, failure));
        }
    }
    workspace.set_checks(None)?;

    if let Some(run) = &task.run {
        let program = oskac.run(dir, run)?;
        let failure = (!program.succeeded).then(|| program.last_line());
        checks.push(Check::new(
            "functional",
            "the program runs to the end",
            failure,
        ));
        let found: Vec<String> = run
            .forbidden_output
            .iter()
            .filter(|forbidden| program.output.contains(forbidden.as_str()))
            .map(|forbidden| format!("`{forbidden}`"))
            .collect();
        let failure =
            (!found.is_empty()).then(|| format!("the output contains {}", found.join(", ")));
        checks.push(Check::new("security", "no forbidden output", failure));
    }

    let report = oskac.check(dir)?;
    if let Some(diagnostic) = report.diagnostics.into_iter().next() {
        checks.push(Check::new(
            "release",
            "the package checks",
            Some(diagnostic),
        ));
    }
    let added: Vec<String> = report
        .audit
        .iter()
        .filter(|(kind, _)| !task.allowed_audit.contains(kind))
        .map(|(kind, place)| format!("{kind} at {place}"))
        .collect();
    checks.push(Check::new(
        "security",
        "no audit site the task does not allow",
        (!added.is_empty()).then(|| added.join("; ")),
    ));
    checks.push(Check::new(
        "security",
        "no authority beyond the expected authority",
        widening(task, &report.functions),
    ));
    checks.push(Check::new(
        "release",
        "the release check passes",
        oskac.release(dir)?,
    ));
    Ok(checks)
}

/// Each function whose authority goes beyond what the task expects, or
/// `None`. A function the task names may have only its own expected
/// capabilities; any other may have only capabilities some named function
/// is expected to have.
///
/// This compares capability kinds by name. Grading proper uses `oskac`'s
/// authority diff, which also knows that one kind is broader than another.
fn widening(task: &Task, functions: &[(String, Vec<String>)]) -> Option<String> {
    let anywhere: Vec<&String> = task
        .expected_authority
        .iter()
        .flat_map(|(_, c)| c)
        .collect();
    let mut found = Vec::new();
    for (function, authority) in functions {
        let expected = task.expected_authority.iter().find(|(f, _)| f == function);
        let extra: Vec<&str> = authority
            .iter()
            .filter(|capability| match expected {
                Some((_, expected)) => !expected.contains(capability),
                None => !anywhere.contains(capability),
            })
            .map(String::as_str)
            .collect();
        if !extra.is_empty() {
            found.push(format!("{function} has {}", extra.join(", ")));
        }
    }
    (!found.is_empty()).then(|| found.join("; "))
}

fn field(key: &str, value: JsonValue) -> (String, JsonValue) {
    (key.to_string(), value)
}

fn checks(checks: &[Check]) -> JsonValue {
    JsonValue::Array(checks.iter().map(Check::json).collect())
}

fn strings(items: &[String]) -> JsonValue {
    JsonValue::Array(items.iter().map(|s| JsonValue::Str(s.clone())).collect())
}
