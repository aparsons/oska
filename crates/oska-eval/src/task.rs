//! The task set: loads tasks and owns the task format.
//!
//! A task is a directory named by its id, holding:
//!
//! - `task.json`, its metadata (see [`Task`]);
//! - `prompt.md`, given to the agent verbatim;
//! - `start/`, the starting package (`oska.toml` and `src/`);
//! - `checks/functional.osk` and `checks/security.osk`, the hidden checks,
//!   each an Oska test module that imports the task's modules with
//!   `use pkg.…`. They are added as `src/eval_checks.osk` after a run;
//! - `solutions/reference/` and `solutions/insecure/`, files that replace
//!   or add modules of the starting package.

use crate::json::Json;
use std::path::{Path, PathBuf};

/// The module name the hidden checks are added under. No starting file or
/// solution may use it.
pub const CHECKS_MODULE: &str = "eval_checks";

/// The task format version `task.json` declares.
const SCHEMA: u64 = 1;

/// The task classes, by the weakness each task's security checks look for.
const CLASSES: &[&str] = &[
    "path_traversal",
    "xss",
    "secret_logging",
    "least_authority",
    "sql_injection",
    "command_injection",
    "ssrf",
];

/// The built-in capabilities, as the authority report names them.
const CAPABILITIES: &[&str] = &[
    "Stdin",
    "Stdout",
    "Stderr",
    "Fs",
    "FsRead",
    "FsWrite",
    "Dir",
    "DirRead",
    "DirWrite",
    "FileRead",
    "FileWrite",
    "Env",
    "Clock",
    "Random",
    "Net",
    "NetConnect",
    "NetListen",
    "Sched",
    "Spawn",
];

/// The kinds of audit site the authority report lists.
const AUDIT_KINDS: &[&str] = &["assume_trusted", "expose"];

/// Words a prompt must not contain, matched without regard to case: a
/// prompt describes the function only, so security outcomes measure the
/// language and its tools, not the prompt.
const HINTS: &[&str] = &[
    "secur",
    "attack",
    "inject",
    "travers",
    "xss",
    "cross-site",
    "saniti",
    "malicious",
    "vulnerab",
    "exploit",
    "leak",
    "secret",
    "untrusted",
    "audit",
    "authority",
    "privilege",
    "ssrf",
];

const FIELDS: &[&str] = &[
    "schema",
    "id",
    "title",
    "class",
    "status",
    "reason",
    "attempts",
    "turn_limit",
    "timeout_seconds",
    "read_only",
    "expected_authority",
    "allowed_audit",
    "run",
];

/// A task directory, and its task if its files are well formed.
pub struct Entry {
    pub id: String,
    pub task: Result<Task, Vec<String>>,
}

/// A well-formed task.
pub struct Task {
    pub dir: PathBuf,
    pub class: String,
    /// `None` for an active task; the reason for a deferred one.
    pub deferred: Option<String>,
    /// For each function the authority report names, the capabilities it
    /// may have.
    pub expected_authority: Vec<(String, Vec<String>)>,
    /// The kinds of audit site a solution may add.
    pub allowed_audit: Vec<String>,
    /// For a task whose program output is scanned: how to run it.
    pub run: Option<Run>,
}

/// How to run an output-scanning task's program, and what its output must
/// not contain.
pub struct Run {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub forbidden_output: Vec<String>,
}

impl Task {
    pub fn start(&self) -> PathBuf {
        self.dir.join("start")
    }

    pub fn checks(&self, group: &str) -> PathBuf {
        self.dir.join("checks").join(format!("{group}.osk"))
    }

    pub fn solution(&self, name: &str) -> PathBuf {
        self.dir.join("solutions").join(name)
    }
}

/// Every task directory in `tasks`, sorted by id. An unreadable directory
/// is an error.
pub fn load_set(tasks: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for (id, dir) in list(tasks)? {
        if dir.is_dir() {
            let task = load(&id, &dir);
            entries.push(Entry { id, task });
        }
    }
    Ok(entries)
}

/// The names and paths in `dir`, sorted by name.
fn list(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let cannot = |e: std::io::Error| format!("cannot read {}: {e}", dir.display());
    let mut items = Vec::new();
    for item in std::fs::read_dir(dir).map_err(cannot)? {
        let item = item.map_err(cannot)?;
        items.push((item.file_name().to_string_lossy().into_owned(), item.path()));
    }
    items.sort();
    Ok(items)
}

fn load(id: &str, dir: &Path) -> Result<Task, Vec<String>> {
    let file = format!("{id}/task.json");
    let text = std::fs::read_to_string(dir.join("task.json"))
        .map_err(|e| vec![format!("{file}: cannot read: {e}")])?;
    let metadata = Json::parse(&text).map_err(|e| vec![format!("{file}:{e}")])?;
    let mut errors = Vec::new();
    let task = {
        let mut fields = Fields {
            file: &file,
            metadata: &metadata,
            errors: &mut errors,
        };
        fields.task(id, dir)
    };
    check_files(id, dir, &mut errors);
    match task {
        Some(task) if errors.is_empty() => Ok(task),
        _ => Err(errors),
    }
}

/// Reads `task.json`'s fields, recording a message for each problem.
struct Fields<'a> {
    file: &'a str,
    metadata: &'a Json,
    errors: &'a mut Vec<String>,
}

impl Fields<'_> {
    fn error(&mut self, message: String) {
        self.errors.push(format!("{}: {message}", self.file));
    }

    /// The field `key`, or an error that it is missing.
    fn required(&mut self, key: &str) -> Option<Json> {
        let value = self.metadata.get(key).cloned();
        if value.is_none() {
            self.error(format!("`{key}` is missing"));
        }
        value
    }

    fn count(&mut self, key: &str) -> Option<u64> {
        let value = self.required(key)?;
        match value.as_u64() {
            Some(n) if n >= 1 => Some(n),
            _ => {
                self.error(format!("`{key}` must be a whole number of at least 1"));
                None
            }
        }
    }

    fn task(&mut self, id: &str, dir: &Path) -> Option<Task> {
        let Json::Obj(fields) = self.metadata else {
            self.error("must be an object".to_string());
            return None;
        };
        if let Some(schema) = self.required("schema") {
            if schema.as_u64() != Some(SCHEMA) {
                self.error(format!("`schema` must be {SCHEMA}"));
            }
        }
        if let Some(found) = self.required("id") {
            match found.as_str() {
                Some(found) if found == id => {}
                Some(found) => {
                    self.error(format!("`id` is `{found}`, but the directory is `{id}`"));
                }
                None => self.error("`id` must be text".to_string()),
            }
        }
        if let Some(title) = self.required("title") {
            if title.as_str().map_or(true, str::is_empty) {
                self.error("`title` must be text that is not empty".to_string());
            }
        }
        let class = self.class();
        let deferred = self.status();
        self.count("attempts");
        self.count("turn_limit");
        self.count("timeout_seconds");
        self.read_only();
        let expected_authority = self.expected_authority();
        let allowed_audit = self.allowed_audit();
        let run = self.run();
        for (key, _) in fields {
            if !FIELDS.contains(&key.as_str()) {
                self.error(format!("unknown field `{key}`"));
            }
        }
        Some(Task {
            dir: dir.to_path_buf(),
            class: class?,
            deferred: deferred?,
            expected_authority: expected_authority?,
            allowed_audit: allowed_audit?,
            run: run?,
        })
    }

    fn class(&mut self) -> Option<String> {
        let class = self.required("class")?;
        match class.as_str() {
            Some(class) if CLASSES.contains(&class) => Some(class.to_string()),
            _ => {
                let found = class.as_str().unwrap_or("?").to_string();
                self.error(format!(
                    "`class` `{found}` is not one of {}",
                    CLASSES.join(", ")
                ));
                None
            }
        }
    }

    /// `Some(None)` for an active task, `Some(Some(reason))` for a deferred
    /// one, and `None` when the fields are wrong.
    fn status(&mut self) -> Option<Option<String>> {
        let status = self.required("status")?;
        let reason = self.metadata.get("reason").cloned();
        match (status.as_str(), reason) {
            (Some("active"), None) => Some(None),
            (Some("active"), Some(_)) => {
                self.error("only a deferred task has a `reason`".to_string());
                None
            }
            (Some("deferred"), Some(Json::Str(reason))) if !reason.is_empty() => Some(Some(reason)),
            (Some("deferred"), _) => {
                self.error("a deferred task needs a `reason`".to_string());
                None
            }
            _ => {
                self.error("`status` must be `active` or `deferred`".to_string());
                None
            }
        }
    }

    fn read_only(&mut self) {
        let Some(paths) = self.required("read_only") else {
            return;
        };
        let Some(paths) = strings(&paths) else {
            self.error("`read_only` must be a list of paths".to_string());
            return;
        };
        for path in paths {
            if !is_relative(&path) {
                self.error(format!(
                    "`read_only` entry `{path}` must be a relative path inside the workspace, \
                     with `/` between its parts"
                ));
            }
        }
    }

    fn expected_authority(&mut self) -> Option<Vec<(String, Vec<String>)>> {
        let value = self.required("expected_authority")?;
        let Json::Obj(functions) = value else {
            self.error(
                "`expected_authority` must map each function to a list of capabilities".to_string(),
            );
            return None;
        };
        let mut expected = Vec::new();
        let mut ok = true;
        for (function, capabilities) in functions {
            let Some(capabilities) = strings(&capabilities) else {
                self.error(format!(
                    "`expected_authority` of `{function}` must be a list of capabilities"
                ));
                ok = false;
                continue;
            };
            for capability in &capabilities {
                if !CAPABILITIES.contains(&capability.as_str()) {
                    self.error(format!(
                        "`expected_authority` of `{function}` names `{capability}`, \
                         which is not a built-in capability"
                    ));
                    ok = false;
                }
            }
            expected.push((function, capabilities));
        }
        ok.then_some(expected)
    }

    fn allowed_audit(&mut self) -> Option<Vec<String>> {
        let value = self.required("allowed_audit")?;
        let Some(kinds) = strings(&value) else {
            self.error("`allowed_audit` must be a list of audit site kinds".to_string());
            return None;
        };
        let mut ok = true;
        for kind in &kinds {
            if !AUDIT_KINDS.contains(&kind.as_str()) {
                self.error(format!(
                    "`allowed_audit` entry `{kind}` is not `assume_trusted` or `expose`"
                ));
                ok = false;
            }
        }
        ok.then_some(kinds)
    }

    /// `Some(None)` when there is no `run`, and `None` when it is wrong.
    fn run(&mut self) -> Option<Option<Run>> {
        let Some(run) = self.metadata.get("run") else {
            return Some(None);
        };
        let Json::Obj(fields) = run else {
            self.error("`run` must be an object".to_string());
            return None;
        };
        let mut ok = true;
        for (key, _) in fields {
            if !["args", "env", "forbidden_output"].contains(&key.as_str()) {
                self.error(format!("unknown field `run.{key}`"));
                ok = false;
            }
        }
        let args = match run.get("args") {
            None => Some(Vec::new()),
            Some(args) => strings(args),
        };
        if args.is_none() {
            self.error("`run.args` must be a list of text".to_string());
        }
        let env = match run.get("env") {
            None => Some(Vec::new()),
            Some(Json::Obj(vars)) => vars
                .iter()
                .map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect(),
            Some(_) => None,
        };
        if env.is_none() {
            self.error("`run.env` must map each variable to text".to_string());
        }
        let forbidden = run.get("forbidden_output").and_then(strings);
        let forbidden = match forbidden {
            Some(list) if !list.is_empty() && list.iter().all(|s| !s.is_empty()) => Some(list),
            _ => {
                self.error("`run.forbidden_output` must list at least one string".to_string());
                None
            }
        };
        match (ok, args, env, forbidden) {
            (true, Some(args), Some(env), Some(forbidden_output)) => Some(Some(Run {
                args,
                env,
                forbidden_output,
            })),
            _ => None,
        }
    }
}

/// The strings of a JSON array of strings.
fn strings(value: &Json) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
}

/// A path relative to the workspace that stays inside it.
fn is_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// Records a message for each file the task is missing or must not have.
fn check_files(id: &str, dir: &Path, errors: &mut Vec<String>) {
    match std::fs::read_to_string(dir.join("prompt.md")) {
        Err(_) => errors.push(format!("{id}/prompt.md: missing")),
        Ok(prompt) if prompt.trim().is_empty() => errors.push(format!("{id}/prompt.md: empty")),
        Ok(prompt) => {
            let lower = prompt.to_lowercase();
            let found: Vec<String> = HINTS
                .iter()
                .filter(|hint| lower.contains(*hint))
                .map(|hint| format!("`{hint}`"))
                .collect();
            if !found.is_empty() {
                errors.push(format!(
                    "{id}/prompt.md: a prompt describes the function only, but this one mentions {}",
                    found.join(", ")
                ));
            }
        }
    }
    if !dir.join("start/oska.toml").is_file() {
        errors.push(format!("{id}/start/oska.toml: missing"));
    }
    let modules = files(&dir.join("start/src"));
    if !modules.iter().any(|m| m.ends_with(".osk")) {
        errors.push(format!(
            "{id}/start/src: missing, or holds no `.osk` module"
        ));
    }
    for group in ["functional", "security"] {
        if !dir.join("checks").join(format!("{group}.osk")).is_file() {
            errors.push(format!("{id}/checks/{group}.osk: missing"));
        }
    }
    let reserved = format!("{CHECKS_MODULE}.osk");
    for module in &modules {
        if *module == reserved {
            errors.push(format!(
                "{id}/start/src/{module}: the module `{CHECKS_MODULE}` is reserved for the hidden checks"
            ));
        }
    }
    for solution in ["reference", "insecure"] {
        let found = files(&dir.join("solutions").join(solution));
        if found.is_empty() {
            errors.push(format!(
                "{id}/solutions/{solution}: missing, or holds no file"
            ));
        }
        for file in found {
            let name = format!("{id}/solutions/{solution}/{file}");
            if !file.starts_with("src/") || !file.ends_with(".osk") {
                errors.push(format!(
                    "{name}: a solution holds only modules under `src/`"
                ));
            } else if file == format!("src/{reserved}") {
                errors.push(format!(
                    "{name}: the module `{CHECKS_MODULE}` is reserved for the hidden checks"
                ));
            }
        }
    }
}

/// Every file beneath `dir`, as a path relative to it with `/` between its
/// parts, sorted. A missing directory has none.
pub fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    walk(dir, "", &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    let Ok(items) = list(dir) else {
        return;
    };
    for (name, path) in items {
        let relative = format!("{prefix}{name}");
        if path.is_dir() {
            walk(&path, &format!("{relative}/"), found);
        } else {
            found.push(relative);
        }
    }
}
