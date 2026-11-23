// SPDX-License-Identifier: Apache-2.0
//! `validate`: checks every task in a task set, offline, so a broken task
//! is caught before it costs a paid run.
//!
//! Every task's files must be well formed. An active task's hidden checks
//! then run against its two solutions: the reference solution must pass
//! every check, and the insecure solution must pass every functional check
//! and fail at least one security check. A deferred task's sinks do not run
//! yet, so each solution need only check with each group of hidden checks.
//! Every task's starting package must check.

use crate::grade::{checks, checks_json, Check};
use crate::grader::Oskac;
use crate::json::{field, strings};
use crate::task::{self, Task};
use crate::workspace::Workspace;
use oska_syntax::JsonValue;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// What validating one well-formed task found.
struct Validated {
    reference: Vec<Check>,
    insecure: Vec<Check>,
    failures: Vec<String>,
}

pub fn run(oskac: &str, tasks: &str) -> ExitCode {
    let entries = match task::load_set(Path::new(tasks)) {
        Ok(entries) => entries,
        Err(message) => return crate::json::bad_input(&[message]),
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
                fields.push(field("reference", checks_json(&validated.reference)));
                fields.push(field("insecure", checks_json(&validated.insecure)));
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
    let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    map_parallel(entries, workers, |entry| {
        entry.task.as_ref().ok().map(|task| validate(oskac, task))
    })
    .into_iter()
    .map(|outcome| match outcome {
        Ok(outcome) => outcome,
        Err(panic) => Some(Err(panic)),
    })
    .collect()
}

/// `work` over every item, on at most `workers` threads, with the results
/// in the items' order. An item whose `work` panics gives an `Err`, and the
/// other items still run.
fn map_parallel<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<Result<R, String>> {
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1).min(items.len()) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else {
                    break;
                };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(item)))
                    .map_err(|_| "validating this task panicked".to_string());
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
    for check in reference.iter().filter(|c| c.failed()) {
        failures.push(format!(
            "the reference solution fails the {} check `{}`",
            check.group, check.name
        ));
    }
    if task.deferred.is_some() {
        for check in insecure.iter().filter(|c| c.failed()) {
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
            if check.failed() {
                failures.push(format!(
                    "the insecure solution fails the functional check `{}`",
                    check.name
                ));
            }
        }
        if !insecure.iter().any(|c| c.group == "security" && c.failed()) {
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
    checks(oskac, task, &workspace)
}

#[cfg(test)]
mod tests {
    use super::map_parallel;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn results_come_back_in_the_items_order() {
        let items: Vec<u64> = (0..20).collect();
        // Earlier items take longer, so they finish last.
        let results = map_parallel(&items, 4, |n| {
            std::thread::sleep(Duration::from_millis(20 - n));
            n * 2
        });
        let doubled: Vec<u64> = results.into_iter().map(|r| r.expect("no panic")).collect();
        assert_eq!(doubled, (0..20).map(|n| n * 2).collect::<Vec<_>>());
    }

    /// Each item waits, up to a deadline, until as many are running as the
    /// cap allows, then stays a little longer, so threads past the cap would
    /// be caught running too. Run serially, no item ever sees the others and
    /// the most at once is 1; with no cap, it is more than 3. Only exactly
    /// the cap passes.
    #[test]
    fn as_many_as_the_cap_run_at_once_and_no_more() {
        const CAP: usize = 3;
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let items = [0; 12];
        map_parallel(&items, CAP, |_| {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(5);
            while running.load(Ordering::SeqCst) < CAP && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(20));
            most.fetch_max(running.load(Ordering::SeqCst), Ordering::SeqCst);
            running.fetch_sub(1, Ordering::SeqCst);
        });
        assert_eq!(most.load(Ordering::SeqCst), CAP);
    }

    #[test]
    fn a_panic_is_that_items_error_and_the_rest_still_run() {
        let items = [1, 2, 3];
        let results = map_parallel(&items, 2, |n| {
            assert!(*n != 2, "boom");
            *n
        });
        assert_eq!(results[0], Ok(1));
        assert!(results[1].is_err());
        assert_eq!(results[2], Ok(3));
    }

    #[test]
    fn no_items_and_no_workers_are_fine() {
        assert!(map_parallel(&[] as &[u8], 4, |n| *n).is_empty());
        assert_eq!(map_parallel(&[7], 0, |n| *n), vec![Ok(7)]);
    }
}
