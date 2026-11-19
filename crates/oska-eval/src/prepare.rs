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
//!
//! With `--setup <dir>` (the repo's `site/agent-setup/`), the workspace also
//! gets the agent-setup files a real user's agent gets, `AGENTS.md` and
//! `CLAUDE.md` from the published templates, and the run directory gets
//! `claude-settings.json`, the Claude Code settings for the variant: the
//! published check-on-edit hook with `--hook`, else none. The setup files
//! are read-only for the agent: `grade --setup` treats them so.

use crate::json::{field, pretty};
use crate::task;
use crate::workspace::Workspace;
use oska_syntax::JsonValue;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The agent-setup files a workspace gets with `--setup`, and the template
/// each is written from.
pub const SETUP_FILES: &[(&str, &str)] = &[
    ("AGENTS.md", "AGENTS.template.md"),
    ("CLAUDE.md", "CLAUDE.template.md"),
];

/// The settings file of a run, beside its workspace, and the template of
/// the check-on-edit hook's settings.
const SETTINGS: &str = "claude-settings.json";
const HOOK_SETTINGS: &str = "claude-settings-post-edit.json";

/// The agent-setup files in `setup`, as (workspace path, contents).
pub fn setup_files(setup: &Path) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    SETUP_FILES
        .iter()
        .map(|(file, template)| {
            let path = setup.join(template);
            std::fs::read(&path).map(|text| (*file, text)).map_err(|e| {
                format!(
                    "cannot read the agent-setup template {}: {e}",
                    path.display()
                )
            })
        })
        .collect()
}

/// `oska-eval prepare <tasks-dir> <task-id> <run-dir> [--setup <dir> [--hook]]`.
pub fn run(tasks: &str, id: &str, run_dir: &str, flags: &[&str]) -> ExitCode {
    let options = match flags {
        [] => Ok((None, false)),
        ["--setup", setup] => Ok((Some(*setup), false)),
        ["--setup", setup, "--hook"] | ["--hook", "--setup", setup] => Ok((Some(*setup), true)),
        _ if flags.contains(&"--hook") && !flags.contains(&"--setup") => {
            Err("`--hook` needs `--setup`".to_string())
        }
        _ => Err(format!("unknown flags: {}", flags.join(" "))),
    };
    let result = options.and_then(|(setup, hook)| prepare(tasks, id, run_dir, setup, hook));
    match result {
        Ok(report) => {
            print!("{}", pretty(&report));
            ExitCode::SUCCESS
        }
        Err(message) => crate::json::bad_input(&[message]),
    }
}

fn prepare(
    tasks: &str,
    id: &str,
    run_dir: &str,
    setup: Option<&str>,
    hook: bool,
) -> Result<JsonValue, String> {
    let task = task::load_active(Path::new(tasks), id)?;
    let run_dir = absolute(Path::new(run_dir))?;
    let setup = match setup {
        Some(dir) => {
            let dir = Path::new(dir);
            let settings = if hook {
                let path = dir.join(HOOK_SETTINGS);
                std::fs::read(&path)
                    .map_err(|e| format!("cannot read the hook settings {}: {e}", path.display()))?
            } else {
                b"{}\n".to_vec()
            };
            Some((setup_files(dir)?, settings))
        }
        None => None,
    };
    fresh(Path::new(tasks), &run_dir)?;

    let workspace = Workspace::kept(&run_dir.join("workspace"))?;
    workspace.copy_in(&task.start())?;
    let prompt = task.dir.join("prompt.md");
    std::fs::copy(&prompt, run_dir.join("prompt.md"))
        .map_err(|e| format!("cannot copy {}: {e}", prompt.display()))?;
    if let Some((files, settings)) = &setup {
        for (file, text) in files {
            let path = workspace.path().join(file);
            std::fs::write(&path, text)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        let path = run_dir.join(SETTINGS);
        std::fs::write(&path, settings)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }

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
