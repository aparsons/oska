//! `oskac new` (spec #367, ticket #375): lay out a package, with the
//! agent-setup files, that passes the release check and its tests as
//! written. Each test runs `oskac new` in its own scratch directory, then
//! the commands an agent runs next on what it wrote.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::portable::{literal_path, output_of, Scratch};
use common::Json;

fn oskac_in(dir: &Path, args: &[&str]) -> Output {
    output_of(
        Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(args)
            .current_dir(dir)
            .env("OSKA_CACHE", "off"),
    )
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `oskac check --format=json` reported no diagnostic.
fn assert_no_diagnostics(check: &Output) {
    let report = Json::parse(&stdout(check));
    assert_eq!(
        report.get("diagnostics"),
        Some(&Json::Arr(Vec::new())),
        "{}",
        stdout(check)
    );
}

/// Every file under `dir`, as `/`-separated paths relative to it, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("readable directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let relative = path.strip_prefix(root).expect("under the root");
                let parts: Vec<String> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(parts.join("/"));
            }
        }
    }
    let mut found = Vec::new();
    if dir.exists() {
        walk(dir, dir, &mut found);
    }
    found.sort();
    found
}

#[test]
fn new_writes_a_package_with_the_agent_files_that_runs() {
    let scratch = Scratch::new("oskac-new", "program");
    let made = oskac_in(&scratch, &["new", "demo"]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    let demo = scratch.join("demo");
    assert_eq!(
        files_under(&demo),
        ["AGENTS.md", "CLAUDE.md", "oska.toml", "src/main.osk"]
    );
    let manifest = std::fs::read_to_string(demo.join("oska.toml")).expect("manifest");
    assert_eq!(
        manifest,
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"
    );
    let run = oskac_in(&demo, &["run"]);
    assert_eq!(run.status.code(), Some(0), "{}", stderr(&run));
    assert_eq!(stdout(&run), "Hello, world!\n");
}

#[test]
fn a_new_program_passes_the_release_check_and_its_tests() {
    let scratch = Scratch::new("oskac-new", "release");
    assert_eq!(oskac_in(&scratch, &["new", "demo"]).status.code(), Some(0));
    let demo = scratch.join("demo");
    let check = oskac_in(&demo, &["check", "--release", "--format=json"]);
    assert_eq!(check.status.code(), Some(0), "{}", stdout(&check));
    assert_no_diagnostics(&check);
    let test = oskac_in(&demo, &["test"]);
    assert_eq!(test.status.code(), Some(0), "{}", stdout(&test));
    assert!(
        stdout(&test).contains("2 tests: 2 passed, 0 failed"),
        "{}",
        stdout(&test)
    );
}

#[test]
fn lib_writes_a_library_with_no_main_that_passes_the_release_check_and_its_tests() {
    let scratch = Scratch::new("oskac-new", "lib");
    let made = oskac_in(&scratch, &["new", "shapes", "--lib"]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    let shapes = scratch.join("shapes");
    assert_eq!(
        files_under(&shapes),
        ["AGENTS.md", "CLAUDE.md", "oska.toml", "src/lib.osk"]
    );
    let lib = std::fs::read_to_string(shapes.join("src/lib.osk")).expect("lib");
    assert!(!lib.contains("fn main"), "{lib}");
    let check = oskac_in(&shapes, &["check", "--release", "--format=json"]);
    assert_eq!(check.status.code(), Some(0), "{}", stdout(&check));
    assert_no_diagnostics(&check);
    let test = oskac_in(&shapes, &["test"]);
    assert_eq!(test.status.code(), Some(0), "{}", stdout(&test));
    assert!(
        stdout(&test).contains(": 1 passed, 0 failed"),
        "{}",
        stdout(&test)
    );
}

#[test]
fn no_agent_files_writes_neither_agent_file() {
    let scratch = Scratch::new("oskac-new", "bare");
    let made = oskac_in(&scratch, &["new", "demo", "--no-agent-files"]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    assert_eq!(
        files_under(&scratch.join("demo")),
        ["oska.toml", "src/main.osk"]
    );
}

#[test]
fn a_name_that_is_not_a_package_name_is_refused_and_nothing_is_written() {
    for name in ["Demo", "my-app", "2fast", "a__b", "../escape", "a/b", ""] {
        let scratch = Scratch::new("oskac-new", "bad-name");
        let refused = oskac_in(&scratch, &["new", name, "--format=json"]);
        assert_eq!(
            refused.status.code(),
            Some(1),
            "{name:?}: {}",
            stdout(&refused)
        );
        let report = Json::parse(&stdout(&refused));
        assert_eq!(report.get("ok"), Some(&Json::Bool(false)), "{name:?}");
        assert_eq!(
            report.get("error"),
            Some(&Json::Str("invalid_name".to_string())),
            "{name:?}"
        );
        assert_eq!(files_under(&scratch), Vec::<String>::new(), "{name:?}");
        assert!(!scratch.parent().expect("parent").join("escape").exists());
    }
}

#[test]
fn a_directory_that_is_not_empty_is_refused_and_nothing_is_written() {
    let scratch = Scratch::new("oskac-new", "not-empty");
    let demo = scratch.join("demo");
    std::fs::create_dir(&demo).expect("mkdir");
    std::fs::write(demo.join("notes.txt"), "mine\n").expect("write");
    let refused = oskac_in(&scratch, &["new", "demo"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("is not empty"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(files_under(&demo), ["notes.txt"]);
    assert_eq!(
        std::fs::read_to_string(demo.join("notes.txt")).expect("read"),
        "mine\n"
    );

    // A file in the way is refused the same way.
    let file = Scratch::new("oskac-new", "a-file");
    std::fs::write(file.join("demo"), "mine\n").expect("write");
    let refused = oskac_in(&file, &["new", "demo", "--format=json"]);
    assert_eq!(refused.status.code(), Some(1));
    let report = Json::parse(&stdout(&refused));
    assert_eq!(report.get("error"), Some(&Json::Str("exists".to_string())));
    assert_eq!(
        std::fs::read_to_string(file.join("demo")).expect("read"),
        "mine\n"
    );
}

#[test]
fn an_empty_directory_is_filled_in() {
    let scratch = Scratch::new("oskac-new", "empty");
    std::fs::create_dir(scratch.join("demo")).expect("mkdir");
    let made = oskac_in(&scratch, &["new", "demo", "--no-agent-files"]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    assert_eq!(
        files_under(&scratch.join("demo")),
        ["oska.toml", "src/main.osk"]
    );
}

#[test]
fn format_json_lists_the_files_written() {
    let scratch = Scratch::new("oskac-new", "json");
    let made = oskac_in(&scratch, &["new", "shapes", "--lib", "--format=json"]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    assert_eq!(
        stdout(&made),
        "{\"ok\":true,\"directory\":\"shapes\",\"files\":[\"oska.toml\",\"src/lib.osk\",\"AGENTS.md\",\"CLAUDE.md\"]}\n"
    );
}

#[test]
fn the_agent_files_are_the_published_agent_setup_templates_byte_for_byte() {
    let scratch = Scratch::new("oskac-new", "templates");
    assert_eq!(oskac_in(&scratch, &["new", "demo"]).status.code(), Some(0));
    let published = literal_path(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["..", "..", "site", "agent-setup"],
    );
    for (written, template) in [
        ("AGENTS.md", "AGENTS.template.md"),
        ("CLAUDE.md", "CLAUDE.template.md"),
    ] {
        assert_eq!(
            std::fs::read(scratch.join("demo").join(written)).expect("written"),
            std::fs::read(published.join(template)).expect("published"),
            "{written} should be {template} byte for byte"
        );
    }
}

#[test]
fn the_new_flags_belong_to_new_alone() {
    let scratch = Scratch::new("oskac-new", "usage");
    for args in [
        &["check", "--lib"][..],
        &["test", "--no-agent-files"],
        &["new"],
        &["new", "a", "b"],
    ] {
        let refused = oskac_in(&scratch, args);
        assert_eq!(refused.status.code(), Some(2), "{args:?}");
    }
    assert_eq!(files_under(&scratch), Vec::<String>::new());
}

#[test]
fn a_new_with_no_name_or_two_names_is_a_usage_error_as_json_under_format_json() {
    let scratch = Scratch::new("oskac-new", "usage-json");
    for args in [
        &["new", "--format=json"][..],
        &["new", "a", "b", "--format=json"],
    ] {
        let refused = oskac_in(&scratch, args);
        assert_eq!(refused.status.code(), Some(2), "{args:?}");
        let report = Json::parse(&stdout(&refused));
        assert_eq!(report.get("ok"), Some(&Json::Bool(false)), "{args:?}");
        assert_eq!(
            report.get("error"),
            Some(&Json::Str("usage".to_string())),
            "{args:?}"
        );
    }
    assert_eq!(files_under(&scratch), Vec::<String>::new());
}

/// A symlink at `at` to the directory `target`, or `false` where this OS
/// or user may not make one: Windows without the privilege or developer
/// mode.
fn symlink_dir(target: &Path, at: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, at);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    match made {
        Ok(()) => true,
        Err(err) => {
            eprintln!("skipped: cannot make a symlink here: {err}");
            false
        }
    }
}

#[test]
fn a_symlink_in_the_way_is_refused_and_nothing_is_written_through_it() {
    let scratch = Scratch::new("oskac-new", "symlink");
    let elsewhere = scratch.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("mkdir");
    if !symlink_dir(&elsewhere, &scratch.join("demo")) {
        return;
    }
    let refused = oskac_in(&scratch, &["new", "demo", "--format=json"]);
    assert_eq!(refused.status.code(), Some(1), "{}", stdout(&refused));
    let report = Json::parse(&stdout(&refused));
    assert_eq!(report.get("error"), Some(&Json::Str("exists".to_string())));
    assert_eq!(files_under(&elsewhere), Vec::<String>::new());
}
