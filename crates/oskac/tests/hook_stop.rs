// SPDX-License-Identifier: Apache-2.0
//! `oskac hook claude --event=stop` (spec #185, ticket #195): when Claude
//! Code's agent tries to finish, run the release check and the tests on
//! each target, and block it while any fails.
//!
//! Each test runs the real binary with Claude Code's input on standard
//! input, as the published settings do, and asserts what the agent and the
//! user see: the exit status, standard output and standard error.

mod common;

use std::path::Path;
use std::process::Command;

use common::hook::{hook, scratch, snapshot, symlink_file, write_all, Hooked, MANIFEST};
use common::portable::json_str;
use common::Json;

fn string(json: Option<&Json>) -> &str {
    match json {
        Some(Json::Str(s)) => s,
        other => panic!("{other:?} is not a string"),
    }
}

// ---- The Stop gate -------------------------------------------------------------------------------

/// `oskac hook claude --event=stop [targets...]`: Claude Code's Stop-hook
/// JSON on standard input, run in `cwd`.
fn stop(targets: &[&str], cwd: &Path) -> Hooked {
    let mut args = vec!["hook", "claude", "--event=stop"];
    args.extend_from_slice(targets);
    hook(&args, &stop_input(cwd, false), cwd)
}

/// The JSON Claude Code gives a Stop hook in the project at `cwd`;
/// `active` is whether the agent is already continuing after a block.
fn stop_input(cwd: &Path, active: bool) -> String {
    format!(
        "{{\"session_id\":\"s\",\"cwd\":{},\"hook_event_name\":\"Stop\",\
         \"stop_hook_active\":{active}}}",
        json_str(&cwd.to_string_lossy())
    )
}

/// A program that checks for release, with one test that passes.
const PASSING: &str = "\
fn double(n: Int) -> Int {
    n * 2
}

test \"double doubles\" {
    t.assert_eq(double(2), 4)
}
";

/// A program that checks for release, with one test that passes and one
/// that fails.
const FAILING: &str = "\
fn double(n: Int) -> Int {
    n * 2
}

test \"double doubles\" {
    t.assert_eq(double(2), 4)
}

test \"double is wrong here\" {
    t.assert_eq(double(3), 7)
}
";

/// A program that passes `oskac check` and its tests, and fails
/// `oskac check --release`: it leaves a `todo()`.
const UNFINISHED: &str = "\
fn later() -> Int {
    todo()
}

test \"nothing\" {
    t.assert_eq(1, 1)
}
";

/// A program that passes `oskac check` and its tests, and fails
/// `oskac check --release` with unused authority (`E0296`): `greet` is
/// given `out` and never uses it.
const UNUSED_AUTHORITY: &str = "\
fn greet(out: Stdout, name: String) -> String {
    \"hello {name}\"
}

test \"greets\" {
    t.assert_eq(greet(t.stdout, \"a\"), \"hello a\")
}
";

/// A program that does not check.
const BROKEN: &str = "fn main() { let x: Int = \"a\" }\n";

#[test]
fn the_stop_gate_is_quiet_and_exit_status_0_when_the_checks_and_the_tests_pass() {
    let dir = scratch("stop-pass");
    write_all(&dir, &[("app.osk", PASSING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(
        (hooked.code, hooked.stdout.as_str(), hooked.stderr.as_str()),
        (Some(0), "", "")
    );
}

#[test]
fn the_stop_gate_blocks_with_exit_status_2_and_names_a_failed_test() {
    let dir = scratch("stop-fail");
    write_all(&dir, &[("app.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.stdout, "");
    let failure = hooked
        .stderr
        .lines()
        .find(|l| l.contains("double is wrong here"))
        .unwrap_or_else(|| panic!("the failed test is named: {}", hooked.stderr));
    assert!(
        failure.starts_with("test double is wrong here: app.osk:"),
        "{failure}"
    );
    assert!(
        failure.contains("error[P0011]: ") && failure.contains("actual 6, expected 7"),
        "{failure}"
    );
    assert!(
        !hooked.stderr.contains("double doubles"),
        "a passing test is not mentioned: {}",
        hooked.stderr
    );
    // On every attempt: Claude Code's `stop_hook_active` lets nothing through.
    let again = hook(
        &["hook", "claude", "--event=stop"],
        &stop_input(&dir, true),
        &dir,
    );
    assert_eq!(again.code, Some(2), "{}", again.stderr);
    assert_eq!(again.stderr, hooked.stderr);
}

#[test]
fn the_stop_gate_runs_the_release_check_and_blocks_on_a_todo() {
    let dir = scratch("stop-release");
    write_all(&dir, &[("app.osk", UNFINISHED)]);
    let plain = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["check", "app.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert_eq!(plain.status.code(), Some(0), "the plain check is clean");
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .lines()
            .any(|l| l.starts_with("app.osk:2:") && l.contains("todo()")),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_blocks_on_unused_authority() {
    let dir = scratch("stop-unused-authority");
    write_all(&dir, &[("app.osk", UNUSED_AUTHORITY)]);
    let plain = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "app.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert_eq!(
        plain.status.code(),
        Some(0),
        "the plain check and the tests pass: {}",
        String::from_utf8_lossy(&plain.stdout)
    );
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .lines()
            .any(|l| l.starts_with("app.osk:1:") && l.contains("error[E0296]")),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_checks_a_package_and_names_its_files_from_the_project() {
    let dir = scratch("stop-package");
    write_all(
        &dir,
        &[
            ("app/oska.toml", MANIFEST),
            ("app/src/main.osk", "fn main() {}\n"),
            ("app/src/util.osk", FAILING),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .contains("test util: double is wrong here: app/src/util.osk:"),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_with_no_targets_checks_the_package_of_the_project_directory() {
    let dir = scratch("stop-root-package");
    write_all(
        &dir,
        &[
            ("oska.toml", MANIFEST),
            ("src/main.osk", "fn main() {}\n"),
            // Not the package's business: it is not under `src/`.
            ("broken.osk", BROKEN),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    write_all(&dir, &[("src/util.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .contains("test util: double is wrong here: src/util.osk:"),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_finds_every_package_and_standalone_file_beneath_the_project() {
    let dir = scratch("stop-discover");
    write_all(
        &dir,
        &[
            ("one/oska.toml", MANIFEST),
            ("one/src/main.osk", "fn main() {}\n"),
            ("tools/two/oska.toml", MANIFEST),
            ("tools/two/src/main.osk", "fn main() {}\n"),
            ("tools/script.osk", PASSING),
            ("top.osk", PASSING),
        ],
    );
    assert_eq!(stop(&[], &dir).code, Some(0));
    // Each one is found: break each in turn and the gate blocks, naming it.
    for (file, broken) in [
        ("one/src/main.osk", BROKEN),
        ("tools/two/src/main.osk", BROKEN),
        ("tools/script.osk", FAILING),
        ("top.osk", FAILING),
    ] {
        let original = std::fs::read(dir.join(file)).expect("readable");
        write_all(&dir, &[(file, broken)]);
        let hooked = stop(&[], &dir);
        assert_eq!(hooked.code, Some(2), "{file}: {}", hooked.stderr);
        assert!(hooked.stderr.contains(file), "{file}: {}", hooked.stderr);
        std::fs::write(dir.join(file), original).expect("should restore");
    }
}

#[test]
fn the_stop_gate_skips_hidden_directories_and_does_not_follow_symlinks() {
    let dir = scratch("stop-skip");
    write_all(
        &dir,
        &[
            ("ok.osk", PASSING),
            (".hidden/bad.osk", FAILING),
            ("sub/.git/bad.osk", FAILING),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    #[cfg(unix)]
    {
        let outside = scratch("stop-skip-outside");
        write_all(&outside, &[("bad.osk", FAILING)]);
        std::os::unix::fs::symlink(&*outside, dir.join("link")).expect("should link");
        std::os::unix::fs::symlink(outside.join("bad.osk"), dir.join("bad.osk"))
            .expect("should link");
        let hooked = stop(&[], &dir);
        assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    }
}

#[test]
fn the_stop_gate_checks_only_the_named_targets_resolved_from_the_project() {
    let dir = scratch("stop-named");
    write_all(
        &dir,
        &[
            ("good.osk", PASSING),
            ("fixtures/broken.osk", BROKEN),
            ("pkg/oska.toml", MANIFEST),
            ("pkg/src/main.osk", "fn main() {}\n"),
            ("pkg/src/util.osk", FAILING),
        ],
    );
    // The broken fixture is not named, so it is not checked.
    let hooked = stop(&["good.osk"], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // Run from elsewhere: a target is a path from the project's directory.
    let elsewhere = scratch("stop-named-elsewhere");
    let hooked = hook(
        &["hook", "claude", "--event=stop", "good.osk"],
        &stop_input(&dir, false),
        &elsewhere,
    );
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // A named package, and a named file, are each checked.
    let hooked = stop(&["good.osk", "pkg"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("pkg/src/util.osk:"),
        "{}",
        hooked.stderr
    );
    let hooked = stop(&["fixtures/broken.osk"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("fixtures/broken.osk:1:"),
        "{}",
        hooked.stderr
    );
}

#[test]
fn with_format_json_the_gate_blocks_with_one_json_object() {
    let dir = scratch("stop-json");
    write_all(
        &dir,
        &[
            ("app.osk", FAILING),
            ("todo.osk", UNFINISHED),
            ("ok.osk", PASSING),
        ],
    );
    let args = [
        "hook",
        "claude",
        "--event=stop",
        "--format=json",
        "app.osk",
        "todo.osk",
        "ok.osk",
        "missing.osk",
    ];
    let hooked = hook(&args, &stop_input(&dir, false), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.lines().len(), 1, "one line: {}", hooked.stderr);
    let message = Json::parse(&hooked.stderr);
    let Some(Json::Arr(targets)) = message.get("targets") else {
        panic!("a targets list: {}", hooked.stderr);
    };
    // Only the targets that block, each with what blocks it.
    let names: Vec<&str> = targets.iter().map(|t| string(t.get("target"))).collect();
    assert_eq!(names, ["app.osk", "todo.osk"]);
    let Some(Json::Arr(failed)) = targets[0].get("failed_tests") else {
        panic!("a failed_tests list: {}", hooked.stderr);
    };
    assert_eq!(failed.len(), 1);
    assert_eq!(string(failed[0].get("name")), "double is wrong here");
    let failure = failed[0].get("failure").expect("the failure");
    assert_eq!(string(failure.get("code")), "P0011");
    assert_eq!(string(failure.get("path")), "app.osk");
    assert!(matches!(failure.get("line"), Some(Json::Num(_))));
    let Some(Json::Arr(check)) = targets[1].get("diagnostics") else {
        panic!("a diagnostics list: {}", hooked.stderr);
    };
    assert_eq!(string(check[0].get("path")), "todo.osk");
    assert_eq!(check[0].get("line"), Some(&Json::Num(2)));
    assert_eq!(string(check[0].get("severity")), "error");
    // What could not be read is named too.
    let Some(Json::Arr(errors)) = message.get("errors") else {
        panic!("an errors list: {}", hooked.stderr);
    };
    assert_eq!(errors.len(), 1);
    assert!(string(errors.first()).contains("missing.osk"));
}

#[test]
fn a_named_symlinked_file_is_refused_with_e0411_and_never_read() {
    let dir = scratch("stop-named-symlink");
    write_all(&dir, &[("outside/ok.osk", PASSING)]);
    if !symlink_file(&dir.join("outside/ok.osk"), &dir.join("link.osk")) {
        return;
    }
    let hooked = stop(&["link.osk"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.starts_with("link.osk:1:1: error[E0411]: "),
        "{}",
        hooked.stderr
    );
}

#[test]
fn a_named_target_that_is_not_there_blocks_and_the_others_are_still_checked() {
    let dir = scratch("stop-named-missing");
    write_all(&dir, &[("good.osk", PASSING), ("bad.osk", FAILING)]);
    // Never let through: a gate that cannot check what it was told to
    // check has not shown that the work is done.
    let hooked = stop(&["missing.osk"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .lines()
            .iter()
            .any(|l| l.starts_with("error: ") && l.contains("missing.osk")),
        "{}",
        hooked.stderr
    );
    // The targets around it are each checked, and each says what it found.
    let hooked = stop(&["missing.osk", "bad.osk", "good.osk"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .contains("test double is wrong here: bad.osk:"),
        "{}",
        hooked.stderr
    );
    assert!(hooked.stderr.contains("error: "), "{}", hooked.stderr);
}

/// A directory nobody may list, removed again on drop, so the scratch
/// directory around it can be.
#[cfg(unix)]
struct Locked(std::path::PathBuf);

#[cfg(unix)]
impl Locked {
    /// `dir`, made unreadable, or `None` when this user can read it
    /// anyway, as root can.
    fn new(dir: std::path::PathBuf) -> Option<Locked> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(&dir).expect("should create");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000))
            .expect("should lock");
        let locked = Locked(dir);
        std::fs::read_dir(&locked.0).is_err().then_some(locked)
    }
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_blocks_and_every_other_target_is_still_checked() {
    let dir = scratch("stop-unreadable");
    write_all(&dir, &[("app/a.osk", UNFINISHED)]);
    let Some(_locked) = Locked::new(dir.join("locked")) else {
        println!("skipped: this user can read a directory with mode 000");
        return;
    };
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("app/a.osk:2:") && l.contains("todo()")),
        "the readable target is still checked: {}",
        hooked.stderr
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("error: ") && l.contains("locked")),
        "the unreadable directory is named: {}",
        hooked.stderr
    );
    // Even when everything that could be read is clean.
    write_all(&dir, &[("app/a.osk", PASSING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(hooked.stderr.contains("locked"), "{}", hooked.stderr);
}

#[test]
fn the_stop_gate_does_not_run_integration_tests() {
    let dir = scratch("stop-integration");
    write_all(
        &dir,
        &[(
            "app.osk",
            "#[integration]\ntest \"fails on the real os\" {\n    t.assert(false)\n}\n\n\
             test \"passes\" {\n    t.assert(true)\n}\n",
        )],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

/// A program with a test that uses something the interpreter cannot run
/// yet (`E0380`): `oskac test` ends with status 2 on it.
const UNRUNNABLE: &str = "\
test \"uses a constructor as a value\" {
    let mk = Some
    let x: Option<Int> = mk(1)
}
";

#[test]
fn the_stop_gate_does_not_block_on_what_the_interpreter_cannot_run_and_tells_the_user() {
    let dir = scratch("stop-unrunnable");
    write_all(&dir, &[("app.osk", UNRUNNABLE)]);
    let plain = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "--interpret", "app.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert_eq!(plain.status.code(), Some(2), "{plain:?}");
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(0), "{}", hooked.stderr);
    assert_eq!(hooked.stderr, "", "the agent is told nothing");
    // The user is told, through Claude Code's user-visible message.
    let output = Json::parse(&hooked.stdout);
    let message = string(output.get("systemMessage"));
    assert!(message.contains("app.osk"), "{message}");
    assert!(message.contains("E0380"), "{message}");
    // Another target that fails still blocks.
    write_all(&dir, &[("other.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
}

#[test]
fn the_stop_gate_writes_nothing_and_a_test_cannot_reach_the_real_filesystem() {
    let dir = scratch("stop-readonly");
    write_all(
        &dir,
        &[
            ("app.osk", FAILING),
            (
                "writes.osk",
                "test \"writes\" {\n    \
                 let _ = t.fs.restrict_to(\"/\").write_only().write(Path.new(\"out.txt\"), \"x\")\n}\n",
            ),
        ],
    );
    let before = snapshot(&dir);
    stop(&[], &dir);
    assert_eq!(snapshot(&dir), before);
    // The write test runs (and passes) on its own.
    let alone = stop(&["writes.osk"], &dir);
    assert_eq!((alone.code, alone.stderr.as_str()), (Some(0), ""));
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn the_stop_gate_with_malformed_input_is_one_line_and_exit_status_1() {
    let dir = scratch("stop-malformed");
    for input in ["", "not json", "[]", "{\"cwd\":7}", "{} {}"] {
        let hooked = hook(&["hook", "claude", "--event=stop"], input, &dir);
        assert_eq!(hooked.code, Some(1), "{input}: {}", hooked.stderr);
        assert!(hooked.stderr.starts_with("error: "), "{input}");
        assert_eq!(hooked.lines().len(), 1, "{input}: {}", hooked.stderr);
    }
}
