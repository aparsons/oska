//! `prepare`: materialises a task into a fresh run directory.
//!
//! The run directory holds `workspace/` (the starting package, the only
//! place the agent works) and `prompt.md` (the task's prompt, verbatim).
//! Nothing the grade relies on is kept there: the agent can write anywhere
//! in the run directory, so `grade` compares the workspace with the task's
//! own `start/`, never with a record beside it. It must be new, and
//! outside the repo and the task set, so the agent cannot read the hidden
//! checks or the solutions. The hidden checks are added only when the run
//! is graded.

use crate::json::{field, pretty};
use crate::task;
use crate::workspace::Workspace;
use oska_syntax::JsonValue;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `oska-eval prepare <tasks-dir> <task-id> <run-dir>`.
pub fn run(tasks: &str, id: &str, run_dir: &str) -> ExitCode {
    match prepare(tasks, id, run_dir) {
        Ok(report) => {
            print!("{}", pretty(&report));
            ExitCode::SUCCESS
        }
        Err(message) => crate::json::bad_input(&[message]),
    }
}

fn prepare(tasks: &str, id: &str, run_dir: &str) -> Result<JsonValue, String> {
    let task = task::load_active(Path::new(tasks), id)?;
    let run_dir = absolute(Path::new(run_dir))?;
    fresh(Path::new(tasks), &run_dir)?;

    let workspace = Workspace::kept(&run_dir.join("workspace"))?;
    workspace.copy_in(&task.start())?;
    let prompt = task.dir.join("prompt.md");
    std::fs::copy(&prompt, run_dir.join("prompt.md"))
        .map_err(|e| format!("cannot copy {}: {e}", prompt.display()))?;

    Ok(JsonValue::Object(vec![
        field("schema", JsonValue::Number("1".to_string())),
        field("task", JsonValue::Str(id.to_string())),
        field(
            "workspace",
            JsonValue::Str(workspace.path().display().to_string()),
        ),
        field(
            "prompt",
            JsonValue::Str(run_dir.join("prompt.md").display().to_string()),
        ),
    ]))
}

/// `run_dir` must be missing or empty, and outside the task set and the
/// repo that holds it.
fn fresh(tasks: &Path, run_dir: &Path) -> Result<(), String> {
    if let Ok(mut items) = std::fs::read_dir(run_dir) {
        if items.next().is_some() {
            return Err(format!(
                "the run directory {} is not empty: a run needs a fresh one",
                run_dir.display()
            ));
        }
    } else if run_dir.exists() {
        return Err(format!("{} is not a directory", run_dir.display()));
    }
    let tasks = tasks
        .canonicalize()
        .map_err(|e| format!("cannot read {}: {e}", tasks.display()))?;
    let run = resolve(run_dir);
    let repo = tasks
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(&tasks);
    if run.starts_with(repo) {
        return Err(format!(
            "the run directory {} is inside {}: an agent there could read the hidden checks \
             and the solutions",
            run_dir.display(),
            repo.display()
        ));
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|here| here.join(path))
        .map_err(|e| format!("cannot find the current directory: {e}"))
}

/// `path` with links followed as far as it exists, so a run directory that
/// reaches the repo through a link is still found inside it.
fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut here = path;
    loop {
        if let Ok(real) = here.canonicalize() {
            return rest.iter().rev().fold(real, |dir, part| dir.join(part));
        }
        match (here.parent(), here.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                here = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}
