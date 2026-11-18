//! Runs the `oskac` under test on a workspace, through its command line
//! only, and reads what it reports: the same outputs an agent sees.
//!
//! An `Err` means `oskac` could not be run or printed something that is
//! not its JSON report: the harness cannot trust a result, so it stops with
//! bad input rather than guess.

use crate::task::{Run, CHECKS_MODULE};
use oska_syntax::{parse_json, JsonValue};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub struct Oskac<'a> {
    /// The path as given, for messages.
    path: &'a str,
    /// The program to start: the path made absolute, since each command
    /// runs in a workspace, or a bare name to find on `PATH`.
    program: PathBuf,
}

/// What `oskac test` reported for the hidden checks.
pub struct TestRun {
    /// The checker's diagnostics, as `CODE: message`. No test ran if there
    /// are any.
    pub diagnostics: Vec<String>,
    /// Each hidden check: its name, and `None` if it passed or the failure
    /// as `CODE: message`. Tests in other modules are left out.
    pub tests: Vec<(String, Option<String>)>,
}

/// What `oskac check --format=json` reported.
pub struct CheckReport {
    pub diagnostics: Vec<String>,
    /// Each audit site's kind, and where it is.
    pub audit: Vec<(String, String)>,
}

impl Oskac<'_> {
    pub fn new(path: &str) -> Oskac<'_> {
        let given = Path::new(path);
        let program = match std::env::current_dir() {
            Ok(here) if given.components().count() > 1 => here.join(given),
            _ => given.to_path_buf(),
        };
        Oskac { path, program }
    }

    fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn output(&self, mut command: Command, what: &str) -> Result<Output, String> {
        command
            .output()
            .map_err(|e| format!("cannot run `{} {what}`: {e}", self.path))
    }

    fn json(&self, dir: &Path, args: &[&str]) -> Result<JsonValue, String> {
        let what = args.join(" ");
        let output = self.output(self.command(dir, args), &what)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_json(&stdout).map_err(|e| {
            format!(
                "`oskac {what}` did not print its JSON report ({e}); it wrote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
        })
    }

    /// Runs the package's tests and reads the hidden checks' outcomes.
    pub fn test(&self, dir: &Path) -> Result<TestRun, String> {
        let report = self.json(dir, &["test", ".", "--format=json"])?;
        let malformed = || "`oskac test` printed a report this harness cannot read".to_string();
        let mut tests = Vec::new();
        for test in report
            .get("tests")
            .and_then(JsonValue::as_array)
            .ok_or_else(malformed)?
        {
            if test.get("module").and_then(JsonValue::as_str) != Some(CHECKS_MODULE) {
                continue;
            }
            let name = test
                .get("name")
                .and_then(JsonValue::as_str)
                .ok_or_else(malformed)?;
            let failure = match test.get("status").and_then(JsonValue::as_str) {
                Some("passed") => None,
                Some(_) => Some(test.get("failure").map_or_else(
                    || "did not pass".to_string(),
                    |failure| match failure {
                        JsonValue::Null => "did not pass".to_string(),
                        failure => diagnostic(failure),
                    },
                )),
                None => return Err(malformed()),
            };
            tests.push((name.to_string(), failure));
        }
        Ok(TestRun {
            diagnostics: diagnostics(&report).ok_or_else(malformed)?,
            tests,
        })
    }

    /// Checks the package and reads its authority report.
    pub fn check(&self, dir: &Path) -> Result<CheckReport, String> {
        let report = self.json(dir, &["check", ".", "--format=json"])?;
        let malformed = || "`oskac check` printed a report this harness cannot read".to_string();
        let mut audit = Vec::new();
        for site in report
            .get("audit")
            .and_then(JsonValue::as_array)
            .ok_or_else(malformed)?
        {
            let kind = site
                .get("kind")
                .and_then(JsonValue::as_str)
                .ok_or_else(malformed)?;
            audit.push((kind.to_string(), place(site)));
        }
        Ok(CheckReport {
            diagnostics: diagnostics(&report).ok_or_else(malformed)?,
            audit,
        })
    }

    /// How the package's authority widened against the authority baseline
    /// in `baseline`, as `oskac check --baseline` reports it: one line for
    /// each function that gained authority, became impure or is new and
    /// reaches a kind no baseline function does. Empty when it did not
    /// widen. Audit sites are not included. The package must check.
    pub fn widenings(&self, dir: &Path, baseline: &Path) -> Result<Vec<String>, String> {
        let file = baseline.to_string_lossy();
        let report = self.json(
            dir,
            &["check", ".", &format!("--baseline={file}"), "--format=json"],
        )?;
        let malformed =
            || "`oskac check --baseline` printed a report this harness cannot read".to_string();
        let diff = report.get("authority_diff").ok_or_else(malformed)?;
        let list = |key: &str| {
            diff.get(key)
                .and_then(JsonValue::as_array)
                .ok_or_else(malformed)
        };
        let text = |entry: &JsonValue, key: &str| {
            entry
                .get(key)
                .and_then(JsonValue::as_str)
                .map(str::to_string)
                .ok_or_else(malformed)
        };
        let names = |entry: &JsonValue, key: &str| {
            entry
                .get(key)
                .and_then(JsonValue::as_array)
                .ok_or_else(malformed)?
                .iter()
                .map(|n| n.as_str().map(str::to_string).ok_or_else(malformed))
                .collect::<Result<Vec<_>, _>>()
        };
        let mut found = Vec::new();
        let mut widened = Vec::new();
        for change in list("widened")? {
            let function = text(change, "function")?;
            found.push(format!(
                "{function} has {}",
                names(change, "gained")?.join(", ")
            ));
            widened.push(function);
        }
        for entry in list("became_impure")? {
            let function = text(entry, "function")?;
            if !widened.contains(&function) {
                found.push(format!("{function} is no longer pure"));
            }
        }
        for added in list("added")? {
            let kinds = names(added, "new_kinds")?;
            if !kinds.is_empty() {
                found.push(format!(
                    "{} is new and reaches {}",
                    text(added, "function")?,
                    kinds.join(", ")
                ));
            }
        }
        Ok(found)
    }

    /// The release check: `None` if it passes, or its first diagnostic.
    pub fn release(&self, dir: &Path) -> Result<Option<String>, String> {
        let report = self.json(dir, &["check", ".", "--release", "--format=json"])?;
        let found = diagnostics(&report)
            .ok_or("`oskac check --release` printed a report this harness cannot read")?;
        Ok(found.into_iter().next())
    }

    /// Runs the package's program with `run`'s arguments and environment,
    /// with nothing on standard input.
    pub fn run(&self, dir: &Path, run: &Run) -> Result<ProgramRun, String> {
        let mut args = vec!["run", ".", "--"];
        args.extend(run.args.iter().map(String::as_str));
        let mut command = self.command(dir, &args);
        command.envs(run.env.iter().map(|(k, v)| (k, v)));
        let output = self.output(command, "run")?;
        Ok(ProgramRun {
            succeeded: output.status.success(),
            output: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

/// How a program run through `oskac run` ended.
pub struct ProgramRun {
    /// It exited with 0.
    pub succeeded: bool,
    /// Everything it, and `oskac`, wrote to standard output and standard
    /// error.
    pub output: String,
}

impl ProgramRun {
    /// The last line of the output, which says why a run failed.
    pub fn last_line(&self) -> String {
        self.output.lines().last().unwrap_or("").to_string()
    }
}

/// A report's diagnostics, each as `CODE: message`.
fn diagnostics(report: &JsonValue) -> Option<Vec<String>> {
    Some(
        report
            .get("diagnostics")?
            .as_array()?
            .iter()
            .map(diagnostic)
            .collect(),
    )
}

fn diagnostic(found: &JsonValue) -> String {
    let code = found.get("code").and_then(JsonValue::as_str).unwrap_or("?");
    let message = found
        .get("message")
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    format!("{code}: {message}")
}

/// Where a report entry is: `path bytes start..end`.
fn place(entry: &JsonValue) -> String {
    let path = entry.get("path").and_then(JsonValue::as_str).unwrap_or("?");
    let span = entry.get("span");
    let at = |key: &str| {
        span.and_then(|s| s.get(key))
            .and_then(JsonValue::as_u64)
            .map_or_else(|| "?".to_string(), |n| n.to_string())
    };
    format!("{path} bytes {}..{}", at("start"), at("end"))
}
