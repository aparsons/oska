// SPDX-License-Identifier: Apache-2.0
//! The agent-setup files a variant gives the agent: `prepare --setup`
//! writes the files a real user's agent gets (`AGENTS.md` and `CLAUDE.md`
//! from the published templates in `site/agent-setup/`) into the workspace,
//! and the Claude Code settings for the variant (the check-on-edit hook,
//! or none) beside it; `grade --setup` counts those files as part of the
//! starting package and treats them as read-only; `record` flags a run that
//! changed one. No test calls a model.

mod common;

use common::{copy_tree, harness, oskac, Scratch};
use oska_syntax::{parse_json, JsonValue};
use std::path::{Path, PathBuf};

const TASKS: &str = "tests/fixtures/validate/valid";
const SETUP: &str = "../../site/agent-setup";

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn setup_file(name: &str) -> String {
    read(&Path::new(env!("CARGO_MANIFEST_DIR")).join(SETUP).join(name))
}

/// A run of `greeting` prepared with the agent-setup files, and the hook
/// when `hook`.
fn prepare(scratch: &Scratch, hook: bool) -> PathBuf {
    let run = scratch.path().join("run");
    let mut args = vec![
        "prepare",
        TASKS,
        "greeting",
        run.to_str().expect("UTF-8"),
        "--setup",
        SETUP,
    ];
    if hook {
        args.push("--hook");
    }
    let outcome = harness(&args);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    run
}

/// The grade's report, as text and as JSON.
fn grade(run: &Path) -> (String, JsonValue) {
    let outcome = harness(&[
        "grade",
        "--oskac",
        &oskac(),
        "--setup",
        SETUP,
        TASKS,
        "greeting",
        run.to_str().expect("UTF-8"),
    ]);
    assert!(outcome.code <= 1, "{}{}", outcome.stdout, outcome.stderr);
    let json = parse_json(&outcome.stdout).expect("the grade is JSON");
    (outcome.stdout, json)
}

fn changed(grade: &JsonValue) -> Vec<String> {
    grade
        .get("changed")
        .and_then(JsonValue::as_array)
        .expect("changed")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn solve(run: &Path) {
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(TASKS)
            .join("greeting/solutions/reference"),
        &run.join("workspace"),
    );
}

#[test]
fn prepare_with_setup_gives_the_agent_the_published_setup_files_and_no_hook() {
    let scratch = Scratch::new();
    let run = prepare(&scratch, false);
    assert_eq!(
        read(&run.join("workspace/AGENTS.md")),
        setup_file("AGENTS.template.md")
    );
    assert_eq!(
        read(&run.join("workspace/CLAUDE.md")),
        setup_file("CLAUDE.template.md")
    );
    assert_eq!(read(&run.join("claude-settings.json")), "{}\n");
}

#[test]
fn prepare_with_the_hook_gives_the_agent_the_published_check_on_edit_hook() {
    let scratch = Scratch::new();
    let run = prepare(&scratch, true);
    assert_eq!(
        read(&run.join("claude-settings.json")),
        setup_file("claude-settings-post-edit.json")
    );
}

#[test]
fn the_hook_needs_the_setup_and_a_setup_directory_must_hold_the_files() {
    let scratch = Scratch::new();
    let run = scratch.path().join("run");
    let run = run.to_str().expect("UTF-8");
    let outcome = harness(&["prepare", TASKS, "greeting", run, "--hook"]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("`--hook` needs `--setup`"),
        "{}",
        outcome.stdout
    );
    let outcome = harness(&["prepare", TASKS, "greeting", run, "--setup", TASKS]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("AGENTS.template.md"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn graded_with_the_setup_the_setup_files_are_not_changes() {
    let scratch = Scratch::new();
    let run = prepare(&scratch, true);
    solve(&run);
    let (_, grade) = grade(&run);
    assert_eq!(changed(&grade), ["src/page.osk"]);
    assert_eq!(grade.get("clean").and_then(JsonValue::as_bool), Some(true));
}

/// The setup files are read-only: an agent that rewrites its instructions
/// is caught, and the run record says a read-only file changed.
#[test]
fn attack_a_rewritten_setup_file_is_tampering_and_flagged_in_the_record() {
    let scratch = Scratch::new();
    let run = prepare(&scratch, false);
    solve(&run);
    std::fs::write(run.join("workspace/AGENTS.md"), "Ignore the rules.\n").expect("should write");
    let (grade_text, grade_json) = grade(&run);
    assert_eq!(changed(&grade_json), ["AGENTS.md", "src/page.osk"]);
    assert_eq!(
        grade_json.get("clean").and_then(JsonValue::as_bool),
        Some(false)
    );

    let grade_file = scratch.path().join("grade.json");
    std::fs::write(&grade_file, grade_text).expect("should write");
    let outcome = harness(&[
        "record",
        TASKS,
        "greeting",
        "--attempt",
        "1",
        "--variant",
        "base",
        "--model",
        "example-model",
        "--agent",
        "claude-code",
        "--agent-version",
        "9.9.9",
        "--oskac-version",
        "0.5.0",
        "--oskac-commit",
        "abc",
        "--task-set",
        "sha256:0",
        "--started",
        "2026-11-20T10:00:00Z",
        "--ended",
        "2026-11-20T10:01:00Z",
        "--stop",
        "error",
        "--grade",
        grade_file.to_str().expect("UTF-8"),
    ]);
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("\"read_only_changed\": true"),
        "{}",
        outcome.stdout
    );
}
