//! `oska-eval prepare` and `oska-eval grade`: a task becomes a fresh
//! workspace, an agent's changes are simulated by laying fixture files over
//! it, and the grade comes from `oskac`'s own outputs. Each case's report is
//! compared with `tests/golden/grade_<case>.json`. Refresh the golden files
//! with `UPDATE_GOLDEN=1 cargo test -p oska-eval --test grade` and review
//! the diff.

mod common;

use common::{assert_golden, copy_tree, harness, oskac, Outcome, Scratch};
use std::path::{Path, PathBuf};

const TASKS: &str = "tests/fixtures/validate/valid";

fn fixture(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/grade")
        .join(case)
}

fn solution(task: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(TASKS)
        .join(task)
        .join("solutions")
        .join(name)
}

/// A prepared run of `task`: the run directory, with the agent's workspace.
struct Prepared {
    run: Scratch,
}

impl Prepared {
    fn new(task: &str) -> Prepared {
        let run = Scratch::new();
        let dir = run.path().join("run");
        let outcome = harness(&["prepare", TASKS, task, dir.to_str().expect("UTF-8 path")]);
        assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
        Prepared { run }
    }

    fn dir(&self) -> PathBuf {
        self.run.path().join("run")
    }

    fn workspace(&self) -> PathBuf {
        self.dir().join("workspace")
    }

    /// What the agent did: lays the files of `from` over the workspace.
    fn agent_writes(&self, from: &Path) -> &Prepared {
        copy_tree(from, &self.workspace());
        self
    }

    fn grade(&self, task: &str) -> Outcome {
        harness(&[
            "grade",
            "--oskac",
            &oskac(),
            TASKS,
            task,
            self.dir().to_str().expect("UTF-8 path"),
        ])
    }
}

#[test]
fn prepare_makes_a_workspace_that_holds_only_what_the_agent_may_see() {
    let run = Prepared::new("greeting");
    let workspace = run.workspace();
    assert!(workspace.join("oska.toml").is_file());
    assert!(workspace.join("src/page.osk").is_file());
    assert!(!workspace.join("src/eval_checks.osk").exists());
    assert!(!workspace.join("checks").exists());
    assert!(!workspace.join("solutions").exists());
    let prompt = std::fs::read_to_string(run.dir().join("prompt.md")).expect("the prompt");
    let given = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(TASKS)
            .join("greeting/prompt.md"),
    )
    .expect("the task's prompt");
    assert_eq!(prompt, given, "the prompt is given verbatim");
    assert!(run.dir().join("snapshot.json").is_file());
}

#[test]
fn prepare_refuses_a_run_directory_that_is_not_fresh() {
    let run = Scratch::new();
    std::fs::write(run.path().join("old.txt"), "x").expect("a file");
    let outcome = harness(&["prepare", TASKS, "greeting", run.str()]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("not empty"), "{}", outcome.stdout);
}

#[test]
fn prepare_refuses_a_run_directory_inside_the_task_set() {
    let outcome = harness(&[
        "prepare",
        TASKS,
        "greeting",
        "tests/fixtures/validate/valid/greeting/run",
    ]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("inside"), "{}", outcome.stdout);
}

#[test]
fn prepare_refuses_a_task_that_is_deferred() {
    let run = Scratch::new();
    let dir = run.path().join("run");
    let outcome = harness(&["prepare", TASKS, "fetch", dir.to_str().expect("UTF-8")]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("deferred"), "{}", outcome.stdout);
}

#[test]
fn a_correct_workspace_passes() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_pass.json", &outcome.stdout);
}

#[test]
fn grading_leaves_the_agents_workspace_as_it_was() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    run.grade("greeting");
    assert!(!run.workspace().join("src/eval_checks.osk").exists());
}

#[test]
fn a_workspace_that_works_wrongly_fails_the_functional_checks() {
    let run = Prepared::new("greeting");
    run.agent_writes(&fixture("wrong"));
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_functional.json", &outcome.stdout);
}

#[test]
fn a_workspace_an_attack_breaks_fails_the_security_checks() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "insecure"));
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_security.json", &outcome.stdout);
}

#[test]
fn a_workspace_that_widens_authority_is_judged_by_oskacs_authority_diff() {
    let run = Prepared::new("greeting");
    run.agent_writes(&fixture("wide"));
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_widening.json", &outcome.stdout);
}

#[test]
fn a_changed_read_only_file_and_a_planted_checks_module_are_flagged() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    run.agent_writes(&fixture("tampered"));
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_tampered.json", &outcome.stdout);
}

#[test]
fn a_workspace_that_does_not_check_is_graded_not_refused() {
    let run = Prepared::new("greeting");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_unfinished.json", &outcome.stdout);
}

#[test]
fn an_oskac_that_cannot_run_is_bad_input() {
    let run = Prepared::new("greeting");
    let outcome = harness(&[
        "grade",
        "--oskac",
        "tests/no-such-oskac",
        TASKS,
        "greeting",
        run.dir().to_str().expect("UTF-8"),
    ]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("cannot run"), "{}", outcome.stdout);
}

#[test]
fn a_run_directory_without_a_snapshot_is_bad_input() {
    let run = Scratch::new();
    let outcome = harness(&["grade", "--oskac", &oskac(), TASKS, "greeting", run.str()]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("snapshot"), "{}", outcome.stdout);
}
