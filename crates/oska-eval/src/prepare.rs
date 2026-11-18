//! `prepare`: materialises a task into a fresh run directory and snapshots
//! it, so a finished run can be graded and tampering found.
//!
//! The run directory holds `workspace/` (the starting package, the only
//! place the agent works), `prompt.md` (the task's prompt, verbatim) and
//! `snapshot.json` (a digest of every starting file). It must be new, and
//! outside the repo and the task set, so the agent cannot read the hidden
//! checks or the solutions. The hidden checks are added only when the run
//! is graded.

use crate::json::{field, pretty};
use crate::task;
use crate::workspace::Workspace;
use oska_syntax::{parse_json, JsonValue};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The snapshot format version.
const SCHEMA: u64 = 1;

/// A digest of every file in a workspace when it was prepared. It finds a
/// file the agent changed, added or removed; it is not a defence against
/// an agent that wants to forge it, because the agent never learns where
/// the snapshot is kept: it lives beside the workspace, not in it.
pub struct Snapshot {
    files: Vec<(String, String)>,
}

impl Snapshot {
    fn of(dir: &Path) -> Result<Snapshot, String> {
        let mut files = Vec::new();
        for file in task::files(dir) {
            files.push((file.clone(), digest(&dir.join(&file))?));
        }
        Ok(Snapshot { files })
    }

    /// The files of `dir` that differ from the snapshot, are not in it, or
    /// are missing, sorted.
    pub fn changes(&self, dir: &Path) -> Vec<String> {
        let now = task::files(dir);
        let mut changed: Vec<String> = self
            .files
            .iter()
            .filter(|(file, was)| digest(&dir.join(file)).ok().as_ref() != Some(was))
            .map(|(file, _)| file.clone())
            .collect();
        changed.extend(
            now.into_iter()
                .filter(|file| !self.files.iter().any(|(f, _)| f == file)),
        );
        changed.sort();
        changed
    }

    fn json(&self) -> JsonValue {
        JsonValue::Object(vec![
            field("schema", JsonValue::Number(SCHEMA.to_string())),
            field(
                "files",
                JsonValue::Array(
                    self.files
                        .iter()
                        .map(|(file, digest)| {
                            JsonValue::Object(vec![
                                field("path", JsonValue::Str(file.clone())),
                                field("digest", JsonValue::Str(digest.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// The snapshot `prepare` wrote in `run_dir`.
pub fn read_snapshot(run_dir: &Path) -> Result<Snapshot, String> {
    let file = run_dir.join("snapshot.json");
    let bad = |why: String| format!("the snapshot {}: {why}", file.display());
    let text = std::fs::read_to_string(&file).map_err(|e| bad(format!("cannot read: {e}")))?;
    let json = parse_json(&text).map_err(bad)?;
    let malformed = || bad("is not a snapshot this harness wrote".to_string());
    if json.get("schema").and_then(JsonValue::as_u64) != Some(SCHEMA) {
        return Err(malformed());
    }
    let mut files = Vec::new();
    for entry in json
        .get("files")
        .and_then(JsonValue::as_array)
        .ok_or_else(malformed)?
    {
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        };
        files.push((
            text("path").ok_or_else(malformed)?,
            text("digest").ok_or_else(malformed)?,
        ));
    }
    Ok(Snapshot { files })
}

/// FNV-1a over the file's bytes, in hex: enough to see that a file changed.
fn digest(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Ok(format!("{hash:016x}"))
}

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
    let snapshot = Snapshot::of(workspace.path())?;
    std::fs::write(run_dir.join("snapshot.json"), pretty(&snapshot.json()))
        .map_err(|e| format!("cannot write the snapshot: {e}"))?;

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
