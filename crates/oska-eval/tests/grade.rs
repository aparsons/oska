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
    let mut kept: Vec<String> = std::fs::read_dir(run.dir())
        .expect("the run directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    kept.sort();
    assert_eq!(kept, ["prompt.md", "workspace"], "nothing the grade trusts");
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
fn a_run_directory_without_a_workspace_is_bad_input() {
    let run = Scratch::new();
    let outcome = harness(&["grade", "--oskac", &oskac(), TASKS, "greeting", run.str()]);
    assert_eq!(outcome.code, 2, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("no workspace"),
        "{}",
        outcome.stdout
    );
}

/// The agent can write anywhere in the run directory, so a record kept
/// beside the workspace could be forged to hide an edit. Read-only files are
/// judged against the task's own starting package instead.
#[test]
fn a_record_forged_beside_the_workspace_hides_no_edit() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let toml = run.workspace().join("oska.toml");
    std::fs::write(
        &toml,
        "[package]\nname = \"renamed\"\nversion = \"9.9.9\"\n",
    )
    .expect("the agent's edit");
    // A snapshot of the workspace as it is now, where the old harness kept one.
    std::fs::write(
        run.dir().join("snapshot.json"),
        "{\"schema\": 1, \"files\": []}\n",
    )
    .expect("the forged record");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("oska.toml was changed"),
        "{}",
        outcome.stdout
    );
}

/// A tampered read-only file is reported, and the package is graded with
/// the task's own copy of it, so the tampering cannot sway `passed` or
/// `secure`. Here a broken manifest would fail every check if graded.
#[test]
fn a_tampered_read_only_file_is_graded_as_the_task_wrote_it() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    std::fs::write(run.workspace().join("oska.toml"), "not a manifest [[[")
        .expect("the agent's edit");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_restored.json", &outcome.stdout);
}

#[test]
fn a_removed_read_only_file_is_restored_to_grade() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    std::fs::remove_file(run.workspace().join("oska.toml")).expect("the agent's removal");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"clean\": false"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("oska.toml was changed"),
        "{}",
        outcome.stdout
    );
}

/// A link in the workspace could pull a file from outside it into the
/// graded copy. It is never followed: it is left out and reported.
#[cfg(unix)]
#[test]
fn a_link_to_a_file_outside_the_workspace_is_not_followed() {
    let run = Prepared::new("greeting");
    let outside = Scratch::new();
    let elsewhere = outside.path().join("page.osk");
    std::fs::copy(
        solution("greeting", "reference").join("src/page.osk"),
        &elsewhere,
    )
    .expect("a file outside the workspace");
    let page = run.workspace().join("src/page.osk");
    std::fs::remove_file(&page).expect("the starting module");
    std::os::unix::fs::symlink(&elsewhere, &page).expect("the agent's link");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("grade_link.json", &outcome.stdout);
}

/// A link that loops back up the tree would make a walk that follows links
/// run forever. Grading ends, and reports the link.
#[cfg(unix)]
#[test]
fn a_link_cycle_does_not_hang_grading() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    std::os::unix::fs::symlink("..", run.workspace().join("src/loop")).expect("the agent's link");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("src/loop is a link"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
}

/// The workspace itself swapped for a link to another package.
#[cfg(unix)]
#[test]
fn a_workspace_that_is_a_link_is_not_followed() {
    let run = Prepared::new("greeting");
    let outside = Scratch::new();
    copy_tree(&run.workspace(), outside.path());
    copy_tree(&solution("greeting", "reference"), outside.path());
    std::fs::remove_dir_all(run.workspace()).expect("the workspace");
    std::os::unix::fs::symlink(outside.path(), run.workspace()).expect("the agent's link");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("workspace/ is a link"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": false"),
        "{}",
        outcome.stdout
    );
}

/// An agent still running while it is graded keeps planting the hidden
/// checks module and editing a read-only file. Whatever the copy caught,
/// the report must agree with itself: tampering is judged on the copy that
/// is graded, not on the live workspace a moment later.
#[test]
fn an_agent_still_writing_cannot_split_what_is_graded_from_what_is_checked() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let workspace = run.workspace();
    let stop = AtomicBool::new(false);
    let outcomes = std::thread::scope(|scope| {
        scope.spawn(|| {
            let module = workspace.join("src/eval_checks.osk");
            let toml = workspace.join("oska.toml");
            let original = std::fs::read(&toml).expect("the manifest");
            while !stop.load(Ordering::Relaxed) {
                let _ = std::fs::write(&module, "test \"x\" {\n    t.assert(true)\n}\n");
                let _ = std::fs::write(&toml, "[package]\nname = \"other\"\nversion = \"1.0.0\"\n");
                std::thread::yield_now();
                let _ = std::fs::remove_file(&module);
                let _ = std::fs::write(&toml, &original);
                std::thread::yield_now();
            }
        });
        let outcomes: Vec<_> = (0..3).map(|_| run.grade("greeting")).collect();
        stop.store(true, Ordering::Relaxed);
        outcomes
    });
    for outcome in outcomes {
        assert!(
            matches!(outcome.code, 0 | 1),
            "{}{}",
            outcome.stdout,
            outcome.stderr
        );
        let changed = |file: &str| {
            let list = outcome.stdout.split("\"checks\"").next().unwrap_or("");
            list.contains(&format!("\"{file}\""))
        };
        assert_eq!(
            changed("src/eval_checks.osk"),
            outcome.stdout.contains("eval_checks.osk was added"),
            "{}",
            outcome.stdout
        );
        assert_eq!(
            changed("oska.toml"),
            outcome.stdout.contains("oska.toml was changed"),
            "{}",
            outcome.stdout
        );
    }
}

/// A pipe would make a copy that opens it wait for a writer forever.
/// Grading ends, and reports it.
#[cfg(unix)]
#[test]
fn a_pipe_in_the_workspace_does_not_hang_grading() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let made = std::process::Command::new("mkfifo")
        .arg(run.workspace().join("src/pipe.osk"))
        .status()
        .expect("mkfifo should run");
    assert!(made.success());
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome
            .stdout
            .contains("src/pipe.osk is a link or not a regular file"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
}

/// A read-only file hard-linked to a file outside, then changed through the
/// outside name: the copy sees the change, reports it, and grades the
/// task's file. Grading writes nothing through the link.
#[test]
fn a_read_only_file_changed_through_a_hard_link_is_caught() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let outside = Scratch::new();
    let alias = outside.path().join("manifest");
    let toml = run.workspace().join("oska.toml");
    std::fs::hard_link(&toml, &alias).expect("the agent's hard link");
    std::fs::write(&alias, "not a manifest [[[").expect("the edit through it");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("oska.toml was changed"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&alias).expect("the outside name"),
        "not a manifest [[["
    );
}

/// A directory where a read-only file was: the file is restored over it.
#[test]
fn a_directory_in_a_read_only_files_place_is_cleared() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let toml = run.workspace().join("oska.toml");
    std::fs::remove_file(&toml).expect("the manifest");
    std::fs::create_dir(&toml).expect("a directory in its place");
    std::fs::write(toml.join("x"), "x").expect("a file in it");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("oska.toml was changed"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
}

/// A read-only file renamed only in case still opens on a file system that
/// ignores case, but it is not the task's file: it is caught everywhere.
#[test]
fn a_read_only_file_renamed_only_in_case_is_caught() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let workspace = run.workspace();
    std::fs::rename(workspace.join("oska.toml"), workspace.join("OSKA.toml"))
        .expect("the agent's rename");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stdout.contains("oska.toml was changed"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"OSKA.toml\""),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
}

/// The hidden checks' name taken by a directory, or in another case.
#[test]
fn a_checks_module_planted_as_a_directory_or_in_another_case_is_caught() {
    let run = Prepared::new("greeting");
    run.agent_writes(&solution("greeting", "reference"));
    let src = run.workspace().join("src");
    std::fs::create_dir(src.join("eval_checks")).expect("a directory");
    std::fs::write(src.join("eval_checks/more.osk"), "fn more() {}\n").expect("a module");
    std::fs::write(src.join("Eval_Checks.osk"), "fn other() {}\n").expect("a module");
    let outcome = run.grade("greeting");
    assert_eq!(outcome.code, 1, "{}{}", outcome.stdout, outcome.stderr);
    for planted in ["src/eval_checks was added", "src/Eval_Checks.osk was added"] {
        assert!(
            outcome.stdout.contains(planted),
            "{planted}: {}",
            outcome.stdout
        );
    }
    assert!(
        outcome.stdout.contains("\"passed\": true"),
        "{}",
        outcome.stdout
    );
}
