//! A batch config, and `plan`: the runs a batch will make and what they
//! may cost, worked out before anything runs.
//!
//! A batch config is a JSON file the maintainer writes, named by the
//! batch's id (`<batch>.json`). It says which model runs, on which tasks,
//! in which variants (each switching the agent's access to the Oska site,
//! the check-on-edit hook and `oskac fix`), the per-run estimate and cap,
//! and the budget the driver stops at. Each task's attempts, turn limit and timeout come from its
//! `task.json`, so a batch cannot run a task differently from how the task
//! set declares it. The driver copies the config into the batch directory
//! as `batch.json`, and `summarise` carries it into the summary.

use crate::json::{self, decimal, field, num, pretty, text};
use crate::record::is_name;
use crate::task;
use oska_syntax::JsonValue;
use std::path::Path;
use std::process::ExitCode;

/// The batch config format version.
const SCHEMA: u64 = 1;

/// The fields a batch config holds, and may hold only.
const FIELDS: &[&str] = &[
    "schema",
    "model",
    "tasks",
    "variants",
    "budget_usd",
    "estimate_usd_per_run",
    "cap_usd_per_run",
    "estimate_tokens_per_run",
];

/// The fields of a variant.
const VARIANT_FIELDS: &[&str] = &["name", "web", "hook", "fix"];

/// A well-formed batch config.
pub struct Config {
    pub tasks: Vec<String>,
    pub variants: Vec<Variant>,
    pub budget: f64,
    pub estimate: f64,
    /// The most one run may cost: the agent CLI stops a run there, and a
    /// run whose cost is unknown counts as costing this much.
    pub cap: f64,
    pub tokens: u64,
    /// The config as JSON, its fields in the documented order.
    pub json: JsonValue,
}

/// One variant of a batch: its name, whether the agent may fetch the Oska
/// site, whether the check-on-edit hook runs after each edit, and whether
/// the agent may run `oskac fix`.
pub struct Variant {
    pub name: String,
    pub web: bool,
    pub hook: bool,
    pub fix: bool,
}

/// The batch config at `path`. It is checked on its own: whether its tasks
/// exist is for [`plan`].
pub fn read(path: &Path) -> Result<Config, String> {
    let name = path.display().to_string();
    let source = std::fs::read_to_string(path).map_err(|e| format!("cannot read {name}: {e}"))?;
    let value = json::read(&source).map_err(|e| format!("{name}: {e}"))?;
    parse(&value).map_err(|e| format!("{name}: {e}"))
}

fn parse(value: &JsonValue) -> Result<Config, String> {
    if let Some(key) = value.duplicate_key() {
        return Err(format!("the key `{key}` appears twice"));
    }
    let JsonValue::Object(fields) = value else {
        return Err("a batch config must be an object".to_string());
    };
    if let Some((key, _)) = fields.iter().find(|(k, _)| !FIELDS.contains(&k.as_str())) {
        return Err(format!("unknown field `{key}`"));
    }
    let get = |key: &str| value.get(key).ok_or_else(|| format!("`{key}` is missing"));
    if get("schema")?.as_u64() != Some(SCHEMA) {
        return Err(format!("`schema` must be {SCHEMA}"));
    }
    let model = match get("model")?.as_str() {
        Some(m) if !m.is_empty() && !m.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            m.to_string()
        }
        _ => return Err("`model` must be text with no space".to_string()),
    };

    let mut tasks: Vec<String> = Vec::new();
    for item in get("tasks")?.as_array().unwrap_or_default() {
        let id = item.as_str().ok_or("`tasks` must list task ids, as text")?;
        if tasks.iter().any(|t| t == id) {
            return Err(format!("the task `{id}` is listed twice"));
        }
        tasks.push(id.to_string());
    }
    if tasks.is_empty() {
        return Err("`tasks` must list at least one task id".to_string());
    }

    let mut variants: Vec<Variant> = Vec::new();
    for item in get("variants")?.as_array().unwrap_or_default() {
        variants.push(variant(item)?);
        let last = &variants[variants.len() - 1].name;
        if variants[..variants.len() - 1]
            .iter()
            .any(|v| &v.name == last)
        {
            return Err(format!("the variant `{last}` is listed twice"));
        }
    }
    if variants.is_empty() {
        return Err("`variants` must list at least one variant".to_string());
    }

    let budget = amount(get("budget_usd")?)
        .filter(|b| *b > 0.0)
        .ok_or("`budget_usd` must be a number above 0")?;
    let estimate = amount(get("estimate_usd_per_run")?)
        .ok_or("`estimate_usd_per_run` must be a number of at least 0")?;
    let cap = amount(get("cap_usd_per_run")?)
        .filter(|c| *c > 0.0)
        .ok_or("`cap_usd_per_run` must be a number above 0")?;
    if cap < estimate {
        return Err("`cap_usd_per_run` must be at least `estimate_usd_per_run`".to_string());
    }
    let tokens = get("estimate_tokens_per_run")?
        .as_u64()
        .ok_or("`estimate_tokens_per_run` must be a whole number")?;

    let json = JsonValue::Object(vec![
        field("schema", num(SCHEMA)),
        field("model", text(&model)),
        field("tasks", json::strings(&tasks)),
        field(
            "variants",
            JsonValue::Array(
                variants
                    .iter()
                    .map(|v| {
                        JsonValue::Object(vec![
                            field("name", text(&v.name)),
                            field("web", JsonValue::Bool(v.web)),
                            field("hook", JsonValue::Bool(v.hook)),
                            field("fix", JsonValue::Bool(v.fix)),
                        ])
                    })
                    .collect(),
            ),
        ),
        field("budget_usd", decimal(budget)),
        field("estimate_usd_per_run", decimal(estimate)),
        field("cap_usd_per_run", decimal(cap)),
        field("estimate_tokens_per_run", num(tokens)),
    ]);
    Ok(Config {
        tasks,
        variants,
        budget,
        estimate,
        cap,
        tokens,
        json,
    })
}

fn variant(item: &JsonValue) -> Result<Variant, String> {
    let JsonValue::Object(fields) = item else {
        return Err(
            "each variant must be an object with `name`, `web`, `hook` and `fix`".to_string(),
        );
    };
    if let Some((key, _)) = fields
        .iter()
        .find(|(k, _)| !VARIANT_FIELDS.contains(&k.as_str()))
    {
        return Err(format!("a variant has the unknown field `{key}`"));
    }
    let name = match item.get("name").and_then(JsonValue::as_str) {
        Some(name) if is_name(name) => name.to_string(),
        Some(name) => {
            return Err(format!(
                "the variant `{name}` must be named with lower-case letters, digits, `-` and `_`"
            ))
        }
        None => return Err("each variant must have a `name`, as text".to_string()),
    };
    let switch = |key: &str| {
        item.get(key)
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| format!("the variant `{name}` must say `{key}`: true or false"))
    };
    Ok(Variant {
        web: switch("web")?,
        hook: switch("hook")?,
        fix: switch("fix")?,
        name,
    })
}

/// A non-negative, finite amount of US dollars.
fn amount(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(n) => n.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0),
        _ => None,
    }
}

/// The batch id a config file names: its file name without `.json`.
pub fn batch_id(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.strip_suffix(".json") {
        Some(id) if is_name(id) => Ok(id.to_string()),
        _ => Err(format!(
            "`{name}` is not a batch id followed by `.json`: a batch id is lower-case letters, digits, `-` and `_`"
        )),
    }
}

/// `oska-eval plan <tasks-dir> <config>`.
pub fn run(tasks: &str, config: &str) -> ExitCode {
    match plan(Path::new(tasks), Path::new(config)) {
        Ok((plan, within)) => {
            print!("{}", pretty(&plan));
            if within {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(message) => json::bad_input(&[message]),
    }
}

/// The plan, and whether its estimate is within the budget.
fn plan(tasks: &Path, path: &Path) -> Result<(JsonValue, bool), String> {
    let id = batch_id(path)?;
    let config = read(path)?;
    let mut runs = Vec::new();
    let (mut turns, mut seconds) = (0, 0);
    for task_id in &config.tasks {
        let task = task::load_active(tasks, task_id)?;
        for variant in &config.variants {
            for attempt in 1..=task.attempts {
                turns += task.turn_limit;
                seconds += task.timeout_seconds;
                runs.push(JsonValue::Object(vec![
                    field("task", text(task_id)),
                    field("variant", text(&variant.name)),
                    field("web", JsonValue::Bool(variant.web)),
                    field("hook", JsonValue::Bool(variant.hook)),
                    field("fix", JsonValue::Bool(variant.fix)),
                    field("attempt", num(attempt)),
                    field("turn_limit", num(task.turn_limit)),
                    field("timeout_seconds", num(task.timeout_seconds)),
                    field(
                        "record",
                        text(&format!("{task_id}-{}-{attempt}.json", variant.name)),
                    ),
                ]));
            }
        }
    }
    let cost = config.estimate * runs.len() as f64;
    let within = cost <= config.budget;
    let plan = JsonValue::Object(vec![
        field("schema", num(SCHEMA)),
        field("batch", text(&id)),
        field("config", config.json.clone()),
        field(
            "estimate",
            JsonValue::Object(vec![
                field("runs", num(runs.len() as u64)),
                field("turns_at_most", num(turns)),
                field("seconds_at_most", num(seconds)),
                field("tokens", num(config.tokens * runs.len() as u64)),
                field("cost_usd", decimal(cost)),
                field("cost_usd_at_most", decimal(config.cap * runs.len() as f64)),
                field("budget_usd", decimal(config.budget)),
                field("within_budget", JsonValue::Bool(within)),
            ]),
        ),
        field("runs", JsonValue::Array(runs)),
    ]);
    Ok((plan, within))
}
